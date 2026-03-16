//! Heartbeat monitor — periodic health probes for running channels.
//!
//! Runs `channel.health_check()` at a configurable interval and reports
//! unhealthy channels via the event bus or tracing.

use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::traits::Channel;

/// Status of a single heartbeat check
#[derive(Debug, Clone)]
pub struct HeartbeatResult {
    pub channel_name: String,
    pub healthy: bool,
    pub error: Option<String>,
    pub checked_at: chrono::DateTime<chrono::Utc>,
}

/// Callback invoked after each heartbeat round.
pub type HeartbeatCallback = Arc<dyn Fn(Vec<HeartbeatResult>) + Send + Sync>;

/// Monitors channel health at a fixed interval.
pub struct HeartbeatMonitor {
    /// Channels to monitor
    channels: Vec<Arc<dyn Channel>>,
    /// Check interval
    interval: Duration,
    /// Optional callback for results
    callback: Option<HeartbeatCallback>,
    /// Cancellation token
    cancel: CancellationToken,
}

impl HeartbeatMonitor {
    /// Create a new monitor.
    pub fn new(
        channels: Vec<Arc<dyn Channel>>,
        interval: Duration,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            channels,
            interval,
            callback: None,
            cancel,
        }
    }

    /// Set a callback for heartbeat results.
    pub fn with_callback(mut self, cb: HeartbeatCallback) -> Self {
        self.callback = Some(cb);
        self
    }

    /// Run the heartbeat loop (blocks until cancelled).
    pub async fn run(&self) {
        info!(
            channels = self.channels.len(),
            interval_secs = self.interval.as_secs(),
            "heartbeat monitor started"
        );

        loop {
            tokio::select! {
                _ = self.cancel.cancelled() => {
                    info!("heartbeat monitor cancelled");
                    return;
                }
                _ = tokio::time::sleep(self.interval) => {
                    let results = self.check_all().await;
                    self.report(&results);
                    if let Some(cb) = &self.callback {
                        cb(results);
                    }
                }
            }
        }
    }

    /// Run a single check round.
    pub async fn check_all(&self) -> Vec<HeartbeatResult> {
        let mut results = Vec::with_capacity(self.channels.len());

        for channel in &self.channels {
            let name = channel.name().to_string();
            let checked_at = chrono::Utc::now();

            match channel.health_check().await {
                Ok(()) => {
                    results.push(HeartbeatResult {
                        channel_name: name,
                        healthy: true,
                        error: None,
                        checked_at,
                    });
                }
                Err(e) => {
                    results.push(HeartbeatResult {
                        channel_name: name,
                        healthy: false,
                        error: Some(e.to_string()),
                        checked_at,
                    });
                }
            }
        }

        results
    }

    fn report(&self, results: &[HeartbeatResult]) {
        let unhealthy: Vec<_> = results.iter().filter(|r| !r.healthy).collect();
        if unhealthy.is_empty() {
            info!(channels = results.len(), "heartbeat: all channels healthy");
        } else {
            for r in &unhealthy {
                warn!(
                    channel = r.channel_name,
                    error = r.error.as_deref().unwrap_or("unknown"),
                    "heartbeat: channel unhealthy"
                );
            }
            error!(
                unhealthy = unhealthy.len(),
                total = results.len(),
                "heartbeat: some channels unhealthy"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{ChannelMessage, SendMessage};
    use atta_types::AttaError;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct HealthyChannel;

    #[async_trait::async_trait]
    impl Channel for HealthyChannel {
        fn name(&self) -> &str {
            "healthy"
        }
        async fn send(&self, _: SendMessage) -> Result<(), AttaError> {
            Ok(())
        }
        async fn listen(&self, _: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
            std::future::pending::<()>().await;
            Ok(())
        }
        async fn health_check(&self) -> Result<(), AttaError> {
            Ok(())
        }
    }

    struct UnhealthyChannel;

    #[async_trait::async_trait]
    impl Channel for UnhealthyChannel {
        fn name(&self) -> &str {
            "unhealthy"
        }
        async fn send(&self, _: SendMessage) -> Result<(), AttaError> {
            Ok(())
        }
        async fn listen(&self, _: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
            std::future::pending::<()>().await;
            Ok(())
        }
        async fn health_check(&self) -> Result<(), AttaError> {
            Err(AttaError::Channel("connection lost".to_string()))
        }
    }

    #[tokio::test]
    async fn test_check_all() {
        let cancel = CancellationToken::new();
        let monitor = HeartbeatMonitor::new(
            vec![
                Arc::new(HealthyChannel) as Arc<dyn Channel>,
                Arc::new(UnhealthyChannel),
            ],
            Duration::from_secs(60),
            cancel,
        );

        let results = monitor.check_all().await;
        assert_eq!(results.len(), 2);
        assert!(results[0].healthy);
        assert!(!results[1].healthy);
        assert!(results[1].error.is_some());
    }

    #[tokio::test]
    async fn test_heartbeat_with_callback() {
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = Arc::clone(&called);
        let cancel = CancellationToken::new();

        let monitor = HeartbeatMonitor::new(
            vec![Arc::new(HealthyChannel) as Arc<dyn Channel>],
            Duration::from_millis(50),
            cancel.clone(),
        )
        .with_callback(Arc::new(move |results| {
            assert_eq!(results.len(), 1);
            assert!(results[0].healthy);
            called_clone.store(true, Ordering::SeqCst);
        }));

        // Run for a short time
        let handle = tokio::spawn(async move {
            monitor.run().await;
        });

        tokio::time::sleep(Duration::from_millis(100)).await;
        cancel.cancel();
        let _ = handle.await;

        assert!(called.load(Ordering::SeqCst));
    }
}
