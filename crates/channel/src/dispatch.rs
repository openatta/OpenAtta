//! Message dispatch loop
//!
//! Receives messages from channel listeners and routes them through
//! the policy → debounce → session → handler pipeline.

use std::collections::HashMap;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::debounce::SimpleDebouncer;
use crate::handler::ChannelMessageHandler;
use crate::persistence::MessageStore;
use crate::policy::{MessagePolicy, PolicyDecision};
use crate::session::SessionRouter;
use crate::traits::{Channel, ChannelMessage, SendMessage};

/// Process a single incoming channel message via a handler.
pub async fn process_channel_message(
    msg: &ChannelMessage,
    channel: &dyn Channel,
    handler: &dyn ChannelMessageHandler,
) {
    handler.handle(msg, channel).await;
}

/// Send a message with exponential backoff retry
pub async fn send_with_retry(
    channel: &dyn Channel,
    msg: SendMessage,
    max_retries: u32,
) -> Result<(), atta_types::AttaError> {
    let mut last_err = None;
    for attempt in 0..=max_retries {
        if attempt > 0 {
            let delay = std::time::Duration::from_millis(500 * (1 << (attempt - 1)));
            warn!(
                attempt,
                delay_ms = delay.as_millis() as u64,
                "retrying channel send"
            );
            tokio::time::sleep(delay).await;
        }
        match channel.send(msg.clone()).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                warn!(attempt, error = %e, "channel send failed");
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| atta_types::AttaError::Channel("send failed".to_string())))
}

/// Pipeline configuration for the dispatch loop.
#[derive(Default)]
pub struct DispatchPipeline {
    /// Message policy chain (dedup, mention, access control, etc.)
    pub policy: Option<Arc<dyn MessagePolicy>>,
    /// Session router for session-aware dispatch
    pub session_router: Option<Arc<SessionRouter>>,
    /// Message debouncer for rapid-fire message aggregation
    pub debouncer: Option<Arc<SimpleDebouncer>>,
    /// Message store for persistence before processing
    pub message_store: Option<Arc<dyn MessageStore>>,
}

/// Run the main message dispatch loop with the full processing pipeline.
///
/// Pipeline order:
/// 1. Persist message (if message store configured)
/// 2. Evaluate policy chain (dedup, mention filter, access control)
/// 3. Debounce (if debouncer configured — aggregate rapid messages)
/// 4. Resolve session (if session router configured)
/// 5. Check ACP takeover (skip agent if human operator active)
/// 6. Dispatch to handler
pub async fn run_message_dispatch_loop(
    rx: tokio::sync::mpsc::Receiver<ChannelMessage>,
    channels: HashMap<String, Arc<dyn Channel>>,
    handler: Arc<dyn ChannelMessageHandler>,
    cancel: CancellationToken,
) {
    // Default pipeline (no policy, no debounce, no persistence)
    run_pipeline_dispatch_loop(rx, channels, handler, DispatchPipeline::default(), cancel).await;
}

/// Run the dispatch loop with a configured pipeline.
pub async fn run_pipeline_dispatch_loop(
    mut rx: tokio::sync::mpsc::Receiver<ChannelMessage>,
    channels: HashMap<String, Arc<dyn Channel>>,
    handler: Arc<dyn ChannelMessageHandler>,
    pipeline: DispatchPipeline,
    cancel: CancellationToken,
) {
    // Compute debounce tick interval
    let debounce_tick = pipeline
        .debouncer
        .as_ref()
        .map(|d| d.window())
        .unwrap_or(std::time::Duration::from_secs(3600)); // effectively disabled

    let mut debounce_interval = tokio::time::interval(debounce_tick);
    debounce_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // Skip the first tick
    debounce_interval.tick().await;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!("dispatch loop cancelled");
                // Flush remaining debounced messages
                if let Some(ref debouncer) = pipeline.debouncer {
                    let remaining = debouncer.flush_all().await;
                    for msg in remaining {
                        dispatch_single(&msg, &channels, &handler, &pipeline).await;
                    }
                }
                break;
            }
            _ = debounce_interval.tick() => {
                // Flush expired debounce batches
                if let Some(ref debouncer) = pipeline.debouncer {
                    let expired = debouncer.flush_expired().await;
                    for msg in expired {
                        dispatch_single(&msg, &channels, &handler, &pipeline).await;
                    }
                }
            }
            msg = rx.recv() => {
                let Some(msg) = msg else {
                    info!("all channel senders dropped, dispatch loop ending");
                    break;
                };

                // --- Step 1: Persist ---
                let record_id = if let Some(ref store) = pipeline.message_store {
                    match store.save(&msg).await {
                        Ok(id) => Some(id),
                        Err(e) => {
                            warn!(error = %e, "failed to persist message, processing anyway");
                            None
                        }
                    }
                } else {
                    None
                };

                // --- Step 2: Policy evaluation ---
                if let Some(ref policy) = pipeline.policy {
                    match policy.evaluate(&msg).await {
                        PolicyDecision::Allow => {}
                        PolicyDecision::Deny { reason } => {
                            debug!(
                                channel = msg.channel,
                                sender = msg.sender,
                                reason,
                                "message denied by policy"
                            );
                            // Mark as completed in store (no retry needed)
                            if let (Some(ref store), Some(ref id)) = (&pipeline.message_store, &record_id) {
                                if let Err(e) = store.mark_completed(id).await {
                                    error!(error = %e, record_id = %id, "failed to mark denied message as completed");
                                }
                            }
                            continue;
                        }
                        PolicyDecision::Buffer { key } => {
                            debug!(key, "message buffered for debounce");
                            // Will be handled by debouncer
                        }
                    }
                }

                // --- Step 3: Debounce ---
                if let Some(ref debouncer) = pipeline.debouncer {
                    if let Some(combined) = debouncer.accept(msg).await {
                        // Force-flushed (max chars) — dispatch immediately
                        dispatch_single(&combined, &channels, &handler, &pipeline).await;
                    }
                    // Otherwise buffered — will be flushed by the tick
                    continue;
                }

                // --- Steps 4-6: Session + ACP + Handler ---
                dispatch_to_handler(msg, record_id, &channels, &handler, &pipeline).await;
            }
        }
    }
}

/// Enrich a message with session metadata and spawn the handler task.
///
/// This is the shared path for both direct messages and debounced messages,
/// ensuring consistent session resolution, ACP checks, metadata enrichment,
/// and error handling.
async fn dispatch_to_handler(
    msg: ChannelMessage,
    record_id: Option<String>,
    channels: &HashMap<String, Arc<dyn Channel>>,
    handler: &Arc<dyn ChannelMessageHandler>,
    pipeline: &DispatchPipeline,
) {
    let ch = channels.get(&msg.channel).cloned();
    let handler = Arc::clone(handler);
    let session_router = pipeline.session_router.clone();
    let message_store = pipeline.message_store.clone();

    let channel_name = msg.channel.clone();
    let monitor_record_id = record_id.clone();
    let join_handle = tokio::spawn(async move {
        let Some(ref ch) = ch else {
            warn!(channel = %msg.channel, "no channel found for message");
            return;
        };

        // Resolve session
        if let Some(ref router) = session_router {
            let session = router.resolve(&msg).await;

            // Check ACP takeover — mark as held for operator pickup
            if session.is_takeover() {
                warn!(
                    session = session.key,
                    channel = msg.channel,
                    sender = msg.sender,
                    "message held — session in ACP takeover mode (awaiting operator pickup)"
                );
                // Mark as Held so the retry task skips it. When the takeover
                // is cleared, held messages can be released back to Pending.
                if let (Some(ref store), Some(ref id)) = (&message_store, &record_id) {
                    if let Err(e) = store.mark_held(id).await {
                        error!(error = %e, record_id = %id, "failed to mark message as held");
                    }
                }
                return;
            }

            // Check session send policy
            if session.config.send_policy == crate::session::SendPolicyState::Deny {
                debug!(session = session.key, "session send policy: deny");
                if let (Some(ref store), Some(ref id)) = (&message_store, &record_id) {
                    if let Err(e) = store.mark_completed(id).await {
                        error!(error = %e, record_id = %id, "failed to mark denied message as completed");
                    }
                }
                return;
            }

            // Enrich message metadata with session info
            let mut enriched = msg.clone();
            enriched.metadata["_session_key"] = serde_json::json!(session.key);
            if let Some(ref agent_id) = session.config.agent_id {
                enriched.metadata["_agent_id"] = serde_json::json!(agent_id);
            }
            if let Some(ref flow_id) = session.config.flow_id {
                enriched.metadata["_flow_id"] = serde_json::json!(flow_id);
            }

            // Mark processing
            if let (Some(ref store), Some(ref id)) = (&message_store, &record_id) {
                if let Err(e) = store.mark_processing(id).await {
                    error!(error = %e, record_id = %id, "failed to mark message as processing");
                }
            }

            handler.handle(&enriched, ch.as_ref()).await;

            // Mark completed
            if let (Some(ref store), Some(ref id)) = (&message_store, &record_id) {
                if let Err(e) = store.mark_completed(id).await {
                    error!(error = %e, record_id = %id, "failed to mark message as completed");
                }
            }
            return;
        }

        // No session router — direct dispatch
        if let (Some(ref store), Some(ref id)) = (&message_store, &record_id) {
            if let Err(e) = store.mark_processing(id).await {
                error!(error = %e, record_id = %id, "failed to mark message as processing");
            }
        }

        handler.handle(&msg, ch.as_ref()).await;

        if let (Some(ref store), Some(ref id)) = (&message_store, &record_id) {
            if let Err(e) = store.mark_completed(id).await {
                error!(error = %e, record_id = %id, "failed to mark message as completed");
            }
        }
    });

    // Monitor the spawned task for panics — mark message as Failed for retry
    let monitor_store = pipeline.message_store.clone();
    tokio::spawn(async move {
        if let Err(e) = join_handle.await {
            error!(
                channel = %channel_name,
                error = %e,
                "handler task panicked"
            );
            if let (Some(ref store), Some(ref id)) = (&monitor_store, &monitor_record_id) {
                let error_msg = format!("handler panicked: {e}");
                // Retry marking as failed up to 3 times
                for attempt in 0..3 {
                    match store.mark_failed(id, &error_msg).await {
                        Ok(()) => break,
                        Err(e) => {
                            tracing::warn!(
                                record_id = %id,
                                attempt = attempt + 1,
                                error = %e,
                                "failed to mark panicked message as failed, retrying"
                            );
                            tokio::time::sleep(std::time::Duration::from_millis(100 * (attempt as u64 + 1))).await;
                        }
                    }
                }
            }
        }
    });
}

/// Dispatch a single debounced message through the full pipeline.
///
/// Re-runs policy evaluation, persists the combined message, resolves session,
/// enriches metadata, and dispatches to the handler — ensuring debounced
/// messages go through the same path as direct messages.
async fn dispatch_single(
    msg: &ChannelMessage,
    channels: &HashMap<String, Arc<dyn Channel>>,
    handler: &Arc<dyn ChannelMessageHandler>,
    pipeline: &DispatchPipeline,
) {
    let Some(_ch) = channels.get(&msg.channel) else {
        warn!(channel = %msg.channel, "no channel found for debounced message");
        return;
    };

    // Re-run policy on the combined message (skip dedup since it's already aggregated)
    if let Some(ref policy) = pipeline.policy {
        match policy.evaluate(msg).await {
            PolicyDecision::Allow => {}
            PolicyDecision::Deny { reason } => {
                debug!(
                    channel = msg.channel,
                    sender = msg.sender,
                    reason,
                    "debounced message denied by policy"
                );
                return;
            }
            PolicyDecision::Buffer { .. } => {
                // Treat Buffer as Allow for debounced messages (already aggregated)
            }
        }
    }

    // Persist the combined message as a new record
    let record_id = if let Some(ref store) = pipeline.message_store {
        match store.save(msg).await {
            Ok(id) => Some(id),
            Err(e) => {
                warn!(error = %e, "failed to persist debounced message");
                None
            }
        }
    } else {
        None
    };

    dispatch_to_handler(msg.clone(), record_id, channels, handler, pipeline).await;
}
