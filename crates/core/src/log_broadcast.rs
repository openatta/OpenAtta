//! Broadcast-based log streaming
//!
//! Provides a [`LogBroadcast`] that captures tracing events via a
//! [`BroadcastLayer`] and fans them out to SSE subscribers.
//! A bounded in-memory ring buffer keeps recent entries for the
//! `/api/v1/logs/recent` endpoint.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex as StdMutex};

use serde::Serialize;
use tokio::sync::broadcast;
use tracing::Subscriber;
use tracing_subscriber::Layer;

/// Maximum number of recent log entries kept in the ring buffer.
const MAX_RECENT: usize = 500;

/// Capacity of the broadcast channel (lagging receivers will lose entries).
const CHANNEL_SIZE: usize = 1024;

/// A single structured log entry.
#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
}

/// Shared log broadcaster.
///
/// Holds a `broadcast::Sender` for real-time streaming and a bounded
/// ring buffer of recent entries.  All operations are synchronous so that
/// the tracing [`BroadcastLayer`] can safely call from **any** thread
/// (including non-Tokio threads such as `sqlx-sqlite-worker`).
#[derive(Clone)]
pub struct LogBroadcast {
    tx: broadcast::Sender<LogEntry>,
    recent: Arc<StdMutex<VecDeque<LogEntry>>>,
}

impl LogBroadcast {
    /// Create a new `LogBroadcast`.
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(CHANNEL_SIZE);
        Self {
            tx,
            recent: Arc::new(StdMutex::new(VecDeque::with_capacity(MAX_RECENT))),
        }
    }

    /// Obtain a new broadcast receiver for SSE streaming.
    pub fn subscribe(&self) -> broadcast::Receiver<LogEntry> {
        self.tx.subscribe()
    }

    /// Push a log entry (synchronous — safe to call from any thread).
    pub fn push(&self, entry: LogEntry) {
        if let Ok(mut recent) = self.recent.lock() {
            if recent.len() >= MAX_RECENT {
                recent.pop_front();
            }
            recent.push_back(entry.clone());
        }
        // Ignore send errors — they just mean no active receivers.
        let _ = self.tx.send(entry);
    }

    /// Return the most recent `limit` entries (newest first).
    pub fn recent(&self, limit: usize) -> Vec<LogEntry> {
        match self.recent.lock() {
            Ok(recent) => recent.iter().rev().take(limit).cloned().collect(),
            Err(_) => Vec::new(),
        }
    }
}

impl Default for LogBroadcast {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// tracing Layer
// ---------------------------------------------------------------------------

/// A `tracing_subscriber::Layer` that feeds log events into a [`LogBroadcast`].
pub struct BroadcastLayer {
    broadcast: LogBroadcast,
}

impl BroadcastLayer {
    /// Create a new layer backed by the given broadcast.
    pub fn new(broadcast: LogBroadcast) -> Self {
        Self { broadcast }
    }
}

impl<S: Subscriber> Layer<S> for BroadcastLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let metadata = event.metadata();
        let level = metadata.level().to_string();
        let target = metadata.target().to_string();

        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        let entry = LogEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            level: level.to_lowercase(),
            target,
            message: visitor.message,
        };

        // Fully synchronous — safe to call from any thread.
        self.broadcast.push(entry);
    }
}

/// Field visitor that extracts the `message` field from a tracing event.
#[derive(Default)]
struct MessageVisitor {
    message: String,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
        }
    }
}
