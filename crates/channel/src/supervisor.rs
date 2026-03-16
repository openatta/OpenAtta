//! Supervised channel listener
//!
//! Wraps a channel's `listen()` call with exponential backoff retry.
//! In push model, `listen()` blocks and pushes messages via `tx`.
//! On disconnect or error, retries with increasing delay up to 60 seconds.

use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::traits::{Channel, ChannelMessage};

/// Run a supervised listener for a channel (push model).
///
/// Calls `channel.listen(tx)` which blocks and pushes messages.
/// On return (normal disconnect) or error, retries with exponential backoff.
/// Stops when `cancel` is triggered.
pub async fn supervised_listener(
    channel: Arc<dyn Channel>,
    tx: tokio::sync::mpsc::Sender<ChannelMessage>,
    cancel: CancellationToken,
) {
    let name = channel.name().to_string();
    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(60);

    info!(channel = %name, "starting supervised listener");

    loop {
        if cancel.is_cancelled() {
            info!(channel = %name, "listener cancelled");
            return;
        }

        match channel.listen(tx.clone()).await {
            Ok(()) => {
                info!(channel = %name, "listener disconnected normally");
                // Reset backoff on clean disconnect
                backoff = Duration::from_secs(1);
            }
            Err(e) => {
                error!(
                    channel = %name,
                    error = %e,
                    backoff_secs = backoff.as_secs(),
                    "listener failed, retrying"
                );
            }
        }

        // Backoff before retry
        tokio::select! {
            _ = cancel.cancelled() => {
                info!(channel = %name, "listener cancelled during backoff");
                return;
            }
            _ = tokio::time::sleep(backoff) => {}
        }

        // Exponential backoff with cap
        backoff = (backoff * 2).min(max_backoff);
    }
}
