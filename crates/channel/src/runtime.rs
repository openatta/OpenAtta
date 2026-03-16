//! Channel runtime context and startup

use std::sync::Arc;
use std::time::Duration;

use atta_types::AttaError;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::debounce::SimpleDebouncer;
use crate::dispatch::{run_message_dispatch_loop, run_pipeline_dispatch_loop, DispatchPipeline};
use crate::handler::ChannelMessageHandler;
use crate::heartbeat::HeartbeatMonitor;
use crate::persistence::MessageStore;
use crate::policy::MessagePolicy;
use crate::session::SessionRouter;
use crate::supervisor::supervised_listener;
use crate::traits::{Channel, ChannelMessage};

/// Runtime context for channel operations
pub struct ChannelRuntimeContext {
    /// Available channels
    pub channels: Vec<Arc<dyn Channel>>,
    /// Message handler (typically an agent pipeline)
    pub handler: Arc<dyn ChannelMessageHandler>,
    /// Cancellation token for graceful shutdown
    pub cancel: CancellationToken,
    /// Message policy chain (dedup, mention, access control)
    pub policy: Option<Arc<dyn MessagePolicy>>,
    /// Session router for session-aware dispatch
    pub session_router: Option<Arc<SessionRouter>>,
    /// Message debouncer configuration: (window, max_chars)
    pub debounce_config: Option<(Duration, usize)>,
    /// Message persistence store
    pub message_store: Option<Arc<dyn MessageStore>>,
    /// Heartbeat interval (None = disabled)
    pub heartbeat_interval: Option<Duration>,
}

/// Start all channels and the message dispatch loop.
///
/// Spawns a supervised listener for each channel, an optional heartbeat
/// monitor, a message retry task, and a dispatch loop that processes
/// incoming messages through the full policy → debounce → session → handler
/// pipeline.
///
/// ## Message ordering
///
/// Messages are dispatched concurrently via `tokio::spawn`. This means
/// messages from the same sender may be processed out of order if one
/// agent invocation takes longer than another. For most chat use-cases
/// this is acceptable. If strict ordering is required, configure
/// `debounce_config` so rapid messages are aggregated into a single turn.
pub async fn start_channels(ctx: ChannelRuntimeContext) -> Result<(), AttaError> {
    let (msg_tx, msg_rx) = tokio::sync::mpsc::channel::<ChannelMessage>(256);

    info!(channels = ctx.channels.len(), "starting channel listeners");

    // Start supervised listeners for each channel
    for channel in &ctx.channels {
        let ch = Arc::clone(channel);
        let tx = msg_tx.clone();
        let token = ctx.cancel.clone();

        tokio::spawn(async move {
            supervised_listener(ch, tx, token).await;
        });
    }

    // Start heartbeat monitor if configured
    if let Some(interval) = ctx.heartbeat_interval {
        let channels = ctx.channels.clone();
        let cancel = ctx.cancel.clone();
        tokio::spawn(async move {
            let monitor = HeartbeatMonitor::new(channels, interval, cancel);
            monitor.run().await;
        });
        info!(interval_secs = interval.as_secs(), "heartbeat monitor started");
    }

    // Start message retry task if persistence is configured
    if let Some(ref store) = ctx.message_store {
        let store = Arc::clone(store);
        let cancel = ctx.cancel.clone();
        let retry_tx = msg_tx.clone();
        tokio::spawn(async move {
            run_retry_task(store, retry_tx, cancel).await;
        });
        info!("message retry task started");
    }

    // Start message cleanup task if persistence is configured
    if let Some(ref store) = ctx.message_store {
        let store = Arc::clone(store);
        let cancel = ctx.cancel.clone();
        tokio::spawn(async move {
            run_cleanup_task(store, cancel).await;
        });
        info!("message cleanup task started");
    }

    // Drop extra sender
    drop(msg_tx);

    // Build channel lookup for dispatch
    let channels: std::collections::HashMap<String, Arc<dyn Channel>> = ctx
        .channels
        .iter()
        .map(|ch| (ch.name().to_string(), Arc::clone(ch)))
        .collect();

    // Build debouncer if configured
    let debouncer = ctx.debounce_config.map(|(window, max_chars)| {
        info!(
            window_ms = window.as_millis() as u64,
            max_chars,
            "message debouncer enabled"
        );
        Arc::new(SimpleDebouncer::new(window, max_chars))
    });

    // Check if any pipeline features are enabled
    let has_pipeline = ctx.policy.is_some()
        || ctx.session_router.is_some()
        || debouncer.is_some()
        || ctx.message_store.is_some();

    if has_pipeline {
        let pipeline = DispatchPipeline {
            policy: ctx.policy,
            session_router: ctx.session_router,
            debouncer,
            message_store: ctx.message_store,
        };
        run_pipeline_dispatch_loop(msg_rx, channels, ctx.handler, pipeline, ctx.cancel).await;
    } else {
        // Fast path: no pipeline features — use simple dispatch
        run_message_dispatch_loop(msg_rx, channels, ctx.handler, ctx.cancel).await;
    }

    info!("all channel listeners stopped");
    Ok(())
}

/// Periodically re-dispatch messages that are stuck in pending or failed state.
///
/// Runs every 60 seconds, fetches up to 10 retryable messages, marks them as
/// processing, and re-injects them into the dispatch loop via `retry_tx`.
/// The dispatch loop will re-run policy → session → handler as usual.
async fn run_retry_task(
    store: Arc<dyn MessageStore>,
    retry_tx: tokio::sync::mpsc::Sender<ChannelMessage>,
    cancel: CancellationToken,
) {
    let interval = Duration::from_secs(60);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!("message retry task cancelled");
                return;
            }
            _ = tokio::time::sleep(interval) => {
                match store.get_retryable(10).await {
                    Ok(messages) if messages.is_empty() => {}
                    Ok(messages) => {
                        info!(count = messages.len(), "retrying failed/pending messages");
                        for record in &messages {
                            // Mark as processing before re-dispatch
                            if let Err(e) = store.mark_processing(&record.record_id).await {
                                error!(error = %e, record_id = %record.record_id, "failed to mark message for retry");
                                continue;
                            }
                            // Re-inject into dispatch loop
                            if let Err(e) = retry_tx.try_send(record.message.clone()) {
                                warn!(
                                    error = %e,
                                    record_id = %record.record_id,
                                    "failed to re-inject message for retry (channel full or closed)"
                                );
                                if let Err(e) = store.mark_failed(
                                    &record.record_id,
                                    &format!("retry re-inject failed: {e}"),
                                ).await {
                                    error!(error = %e, "failed to mark retry failure");
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "failed to fetch retryable messages");
                    }
                }
            }
        }
    }
}

/// Periodically clean up old completed messages from the store.
///
/// Runs every hour, removes completed messages older than 24 hours.
async fn run_cleanup_task(store: Arc<dyn MessageStore>, cancel: CancellationToken) {
    let interval = Duration::from_secs(3600);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!("message cleanup task cancelled");
                return;
            }
            _ = tokio::time::sleep(interval) => {
                let cutoff = chrono::Utc::now() - chrono::Duration::hours(24);
                match store.cleanup(cutoff).await {
                    Ok(count) if count > 0 => {
                        info!(cleaned = count, "cleaned up old completed messages");
                    }
                    Ok(_) => {}
                    Err(e) => {
                        error!(error = %e, "message cleanup failed");
                    }
                }
            }
        }
    }
}
