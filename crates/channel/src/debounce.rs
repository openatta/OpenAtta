//! Message debouncer — aggregates rapid messages from the same sender.
//!
//! When a user sends multiple short messages in quick succession, the debouncer
//! waits for a configurable quiet window before emitting a single combined
//! message. This prevents spawning multiple expensive Agent invocations for
//! what is logically a single user turn.

use std::collections::HashMap;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::RwLock;
use tracing::debug;

use crate::traits::ChannelMessage;

/// Build a session key for debouncing (channel + sender + optional group).
pub fn debounce_key(msg: &ChannelMessage) -> String {
    match &msg.group_id {
        Some(gid) => format!("{}:{}:{}", msg.channel, gid, msg.sender),
        None => format!("{}:{}", msg.channel, msg.sender),
    }
}

/// A simpler message aggregation buffer that collects messages and flushes
/// them on demand or when a threshold is met.
///
/// This version is designed to be used with a periodic flush timer in the
/// dispatch loop rather than spawning per-key deadline tasks.
pub struct SimpleDebouncer {
    /// Quiet window
    window: Duration,
    /// Max accumulated chars before force-flush
    max_chars: usize,
    /// Pending batches: key → (first_msg, fragments, last_activity)
    pending: RwLock<HashMap<String, SimpleBatch>>,
}

struct SimpleBatch {
    first_msg: ChannelMessage,
    fragments: Vec<String>,
    char_count: usize,
    last_activity: std::time::Instant,
}

impl SimpleDebouncer {
    /// Create a new simple debouncer.
    pub fn new(window: Duration, max_chars: usize) -> Self {
        Self {
            window,
            max_chars,
            pending: RwLock::new(HashMap::new()),
        }
    }

    /// Accept a message. Returns `Some(combined)` if force-flush threshold met.
    pub async fn accept(&self, msg: ChannelMessage) -> Option<ChannelMessage> {
        let key = debounce_key(&msg);
        let mut pending = self.pending.write().await;

        if let Some(batch) = pending.get_mut(&key) {
            batch.char_count += msg.content.len();
            batch.fragments.push(msg.content);
            batch.last_activity = std::time::Instant::now();

            if batch.char_count >= self.max_chars {
                debug!(key, chars = batch.char_count, "debounce: force flush");
                let combined = self.combine(batch);
                pending.remove(&key);
                return Some(combined);
            }
            None
        } else {
            let char_count = msg.content.len();
            pending.insert(
                key,
                SimpleBatch {
                    first_msg: msg.clone(),
                    fragments: vec![msg.content],
                    char_count,
                    last_activity: std::time::Instant::now(),
                },
            );
            None
        }
    }

    /// Flush all batches that have been idle longer than the window.
    /// Returns the combined messages.
    pub async fn flush_expired(&self) -> Vec<ChannelMessage> {
        let now = std::time::Instant::now();
        let mut pending = self.pending.write().await;
        let mut results = Vec::new();
        let mut expired_keys = Vec::new();

        for (key, batch) in pending.iter() {
            if now.duration_since(batch.last_activity) >= self.window {
                expired_keys.push(key.clone());
            }
        }

        for key in expired_keys {
            if let Some(batch) = pending.remove(&key) {
                results.push(self.combine(&batch));
            }
        }

        results
    }

    /// Flush all pending batches regardless of timing.
    pub async fn flush_all(&self) -> Vec<ChannelMessage> {
        let mut pending = self.pending.write().await;
        pending
            .drain()
            .map(|(_, batch)| self.combine(&batch))
            .collect()
    }

    /// Number of pending batches.
    pub async fn pending_count(&self) -> usize {
        self.pending.read().await.len()
    }

    /// The debounce window duration.
    pub fn window(&self) -> Duration {
        self.window
    }

    fn combine(&self, batch: &SimpleBatch) -> ChannelMessage {
        let combined_content = batch.fragments.join("\n");
        let mut msg = batch.first_msg.clone();
        // Generate a new unique ID so dedup policy won't reject the combined message
        msg.id = uuid::Uuid::new_v4().to_string();
        msg.content = combined_content;
        msg.timestamp = Utc::now();
        msg
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::ChatType;

    fn test_msg(sender: &str, content: &str) -> ChannelMessage {
        ChannelMessage {
            id: uuid::Uuid::new_v4().to_string(),
            sender: sender.to_string(),
            content: content.to_string(),
            channel: "test".to_string(),
            reply_target: None,
            timestamp: Utc::now(),
            thread_ts: None,
            metadata: serde_json::json!({}),
            chat_type: ChatType::Dm,
            bot_mentioned: false,
            group_id: None,
        }
    }

    #[tokio::test]
    async fn test_simple_debouncer_single_message() {
        let debouncer = SimpleDebouncer::new(Duration::from_millis(100), 4000);
        let msg = test_msg("user1", "hello");

        // First message — buffered, no immediate output
        assert!(debouncer.accept(msg).await.is_none());
        assert_eq!(debouncer.pending_count().await, 1);

        // Wait for window to expire
        tokio::time::sleep(Duration::from_millis(150)).await;

        let flushed = debouncer.flush_expired().await;
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].content, "hello");
    }

    #[tokio::test]
    async fn test_simple_debouncer_combines_messages() {
        let debouncer = SimpleDebouncer::new(Duration::from_millis(200), 4000);

        debouncer.accept(test_msg("user1", "hello")).await;
        debouncer.accept(test_msg("user1", "world")).await;
        debouncer.accept(test_msg("user1", "!")).await;

        // Not yet expired
        let flushed = debouncer.flush_expired().await;
        assert!(flushed.is_empty());

        // Wait and flush
        tokio::time::sleep(Duration::from_millis(250)).await;
        let flushed = debouncer.flush_expired().await;
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].content, "hello\nworld\n!");
    }

    #[tokio::test]
    async fn test_simple_debouncer_different_senders() {
        let debouncer = SimpleDebouncer::new(Duration::from_millis(100), 4000);

        debouncer.accept(test_msg("user1", "a")).await;
        debouncer.accept(test_msg("user2", "b")).await;

        assert_eq!(debouncer.pending_count().await, 2);

        tokio::time::sleep(Duration::from_millis(150)).await;
        let flushed = debouncer.flush_expired().await;
        assert_eq!(flushed.len(), 2);
    }

    #[tokio::test]
    async fn test_simple_debouncer_force_flush_on_max_chars() {
        let debouncer = SimpleDebouncer::new(Duration::from_secs(60), 10);

        assert!(debouncer.accept(test_msg("user1", "12345")).await.is_none());
        let result = debouncer.accept(test_msg("user1", "67890")).await;

        // Should force-flush at 10 chars
        assert!(result.is_some());
        assert_eq!(result.unwrap().content, "12345\n67890");
        assert_eq!(debouncer.pending_count().await, 0);
    }

    #[tokio::test]
    async fn test_simple_debouncer_flush_all() {
        let debouncer = SimpleDebouncer::new(Duration::from_secs(60), 4000);

        debouncer.accept(test_msg("user1", "a")).await;
        debouncer.accept(test_msg("user2", "b")).await;

        let flushed = debouncer.flush_all().await;
        assert_eq!(flushed.len(), 2);
        assert_eq!(debouncer.pending_count().await, 0);
    }

    #[test]
    fn test_debounce_key() {
        let dm = ChannelMessage {
            id: "1".to_string(),
            sender: "user1".to_string(),
            content: "".to_string(),
            channel: "tg".to_string(),
            reply_target: None,
            timestamp: Utc::now(),
            thread_ts: None,
            metadata: serde_json::json!({}),
            chat_type: ChatType::Dm,
            bot_mentioned: false,
            group_id: None,
        };
        assert_eq!(debounce_key(&dm), "tg:user1");

        let group = ChannelMessage {
            group_id: Some("g1".to_string()),
            ..dm
        };
        assert_eq!(debounce_key(&group), "tg:g1:user1");
    }
}
