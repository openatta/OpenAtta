//! Message persistence — stores inbound messages before processing.
//!
//! Provides a safety net for messages: if agent processing fails, the message
//! is not lost and can be retried.

use std::collections::VecDeque;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use atta_types::AttaError;

use crate::traits::ChannelMessage;

/// Processing status of a persisted message
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    /// Message received but not yet processed
    Pending,
    /// Currently being processed
    Processing,
    /// Successfully processed
    Completed,
    /// Processing failed (may be retried)
    Failed,
    /// Held for human operator (ACP takeover) — not retryable
    Held,
}

/// A persisted message record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedMessage {
    /// Unique record ID
    pub record_id: String,
    /// The original channel message
    pub message: ChannelMessage,
    /// Current processing status
    pub status: MessageStatus,
    /// Number of processing attempts
    pub attempts: u32,
    /// Last error message (if failed)
    pub last_error: Option<String>,
    /// When the message was received
    pub received_at: DateTime<Utc>,
    /// When the message was last updated
    pub updated_at: DateTime<Utc>,
}

/// Trait for persisting channel messages.
#[async_trait]
pub trait MessageStore: Send + Sync + 'static {
    /// Persist a new inbound message. Returns the record ID.
    async fn save(&self, msg: &ChannelMessage) -> Result<String, AttaError>;

    /// Mark a message as processing.
    async fn mark_processing(&self, record_id: &str) -> Result<(), AttaError>;

    /// Mark a message as completed.
    async fn mark_completed(&self, record_id: &str) -> Result<(), AttaError>;

    /// Mark a message as failed with an error.
    async fn mark_failed(&self, record_id: &str, error: &str) -> Result<(), AttaError>;

    /// Mark a message as held for human operator (ACP takeover).
    ///
    /// Held messages are excluded from retry and cleanup until the takeover
    /// is cleared, at which point they should be marked back to `Pending`.
    async fn mark_held(&self, record_id: &str) -> Result<(), AttaError>;

    /// Get messages that are pending or failed (for retry).
    /// Excludes messages in `Held` status.
    async fn get_retryable(&self, limit: usize) -> Result<Vec<PersistedMessage>, AttaError>;

    /// Get a message by record ID.
    async fn get(&self, record_id: &str) -> Result<Option<PersistedMessage>, AttaError>;

    /// Delete old completed messages (cleanup).
    async fn cleanup(&self, older_than: DateTime<Utc>) -> Result<usize, AttaError>;

    /// Release all held messages back to Pending status (used when ACP takeover ends).
    ///
    /// Returns the number of messages that were released.
    async fn release_all_held(&self) -> Result<usize, AttaError>;
}

// ---------------------------------------------------------------------------
// InMemoryMessageStore — for testing and Desktop mode
// ---------------------------------------------------------------------------

/// In-memory implementation of [`MessageStore`].
///
/// Messages are stored in a bounded deque; oldest are evicted when capacity
/// is reached.
pub struct InMemoryMessageStore {
    messages: RwLock<VecDeque<PersistedMessage>>,
    capacity: usize,
}

impl InMemoryMessageStore {
    /// Create a new in-memory store with the given capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            messages: RwLock::new(VecDeque::with_capacity(capacity)),
            capacity,
        }
    }
}

impl Default for InMemoryMessageStore {
    fn default() -> Self {
        Self::new(10_000)
    }
}

#[async_trait]
impl MessageStore for InMemoryMessageStore {
    async fn save(&self, msg: &ChannelMessage) -> Result<String, AttaError> {
        let record_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();

        let record = PersistedMessage {
            record_id: record_id.clone(),
            message: msg.clone(),
            status: MessageStatus::Pending,
            attempts: 0,
            last_error: None,
            received_at: now,
            updated_at: now,
        };

        let mut messages = self.messages.write().await;
        if messages.len() >= self.capacity {
            messages.pop_front();
        }
        messages.push_back(record);

        Ok(record_id)
    }

    async fn mark_processing(&self, record_id: &str) -> Result<(), AttaError> {
        let mut messages = self.messages.write().await;
        if let Some(record) = messages.iter_mut().find(|r| r.record_id == record_id) {
            record.status = MessageStatus::Processing;
            record.attempts += 1;
            record.updated_at = Utc::now();
            Ok(())
        } else {
            Err(AttaError::NotFound {
                entity_type: "message".to_string(),
                id: record_id.to_string(),
            })
        }
    }

    async fn mark_completed(&self, record_id: &str) -> Result<(), AttaError> {
        let mut messages = self.messages.write().await;
        if let Some(record) = messages.iter_mut().find(|r| r.record_id == record_id) {
            record.status = MessageStatus::Completed;
            record.updated_at = Utc::now();
            Ok(())
        } else {
            Err(AttaError::NotFound {
                entity_type: "message".to_string(),
                id: record_id.to_string(),
            })
        }
    }

    async fn mark_failed(&self, record_id: &str, error: &str) -> Result<(), AttaError> {
        let mut messages = self.messages.write().await;
        if let Some(record) = messages.iter_mut().find(|r| r.record_id == record_id) {
            record.status = MessageStatus::Failed;
            record.last_error = Some(error.to_string());
            record.updated_at = Utc::now();
            Ok(())
        } else {
            Err(AttaError::NotFound {
                entity_type: "message".to_string(),
                id: record_id.to_string(),
            })
        }
    }

    async fn mark_held(&self, record_id: &str) -> Result<(), AttaError> {
        let mut messages = self.messages.write().await;
        if let Some(record) = messages.iter_mut().find(|r| r.record_id == record_id) {
            record.status = MessageStatus::Held;
            record.updated_at = Utc::now();
            Ok(())
        } else {
            Err(AttaError::NotFound {
                entity_type: "message".to_string(),
                id: record_id.to_string(),
            })
        }
    }

    async fn get_retryable(&self, limit: usize) -> Result<Vec<PersistedMessage>, AttaError> {
        let messages = self.messages.read().await;
        let results: Vec<_> = messages
            .iter()
            .filter(|r| {
                r.status == MessageStatus::Pending
                    || (r.status == MessageStatus::Failed && r.attempts < 3)
            })
            .take(limit)
            .cloned()
            .collect();
        Ok(results)
    }

    async fn get(&self, record_id: &str) -> Result<Option<PersistedMessage>, AttaError> {
        let messages = self.messages.read().await;
        Ok(messages.iter().find(|r| r.record_id == record_id).cloned())
    }

    async fn cleanup(&self, older_than: DateTime<Utc>) -> Result<usize, AttaError> {
        let mut messages = self.messages.write().await;
        let before = messages.len();
        messages.retain(|r| {
            !(r.status == MessageStatus::Completed && r.updated_at < older_than)
        });
        Ok(before - messages.len())
    }

    async fn release_all_held(&self) -> Result<usize, AttaError> {
        let mut messages = self.messages.write().await;
        let mut count = 0;
        for record in messages.iter_mut() {
            if record.status == MessageStatus::Held {
                record.status = MessageStatus::Pending;
                record.updated_at = Utc::now();
                count += 1;
            }
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::ChatType;

    fn test_msg() -> ChannelMessage {
        ChannelMessage {
            id: "msg1".to_string(),
            sender: "user1".to_string(),
            content: "hello".to_string(),
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
    async fn test_save_and_get() {
        let store = InMemoryMessageStore::new(100);
        let msg = test_msg();
        let id = store.save(&msg).await.unwrap();

        let record = store.get(&id).await.unwrap().unwrap();
        assert_eq!(record.message.content, "hello");
        assert_eq!(record.status, MessageStatus::Pending);
        assert_eq!(record.attempts, 0);
    }

    #[tokio::test]
    async fn test_lifecycle() {
        let store = InMemoryMessageStore::new(100);
        let msg = test_msg();
        let id = store.save(&msg).await.unwrap();

        // Pending → Processing
        store.mark_processing(&id).await.unwrap();
        let r = store.get(&id).await.unwrap().unwrap();
        assert_eq!(r.status, MessageStatus::Processing);
        assert_eq!(r.attempts, 1);

        // Processing → Completed
        store.mark_completed(&id).await.unwrap();
        let r = store.get(&id).await.unwrap().unwrap();
        assert_eq!(r.status, MessageStatus::Completed);
    }

    #[tokio::test]
    async fn test_failed_and_retryable() {
        let store = InMemoryMessageStore::new(100);
        let msg = test_msg();
        let id = store.save(&msg).await.unwrap();

        store.mark_processing(&id).await.unwrap();
        store.mark_failed(&id, "timeout").await.unwrap();

        let retryable = store.get_retryable(10).await.unwrap();
        assert_eq!(retryable.len(), 1);
        assert_eq!(retryable[0].last_error.as_deref(), Some("timeout"));
    }

    #[tokio::test]
    async fn test_max_retry_attempts() {
        let store = InMemoryMessageStore::new(100);
        let msg = test_msg();
        let id = store.save(&msg).await.unwrap();

        // Fail 3 times
        for _ in 0..3 {
            store.mark_processing(&id).await.unwrap();
            store.mark_failed(&id, "err").await.unwrap();
        }

        // Should not be retryable after 3 attempts
        let retryable = store.get_retryable(10).await.unwrap();
        assert!(retryable.is_empty());
    }

    #[tokio::test]
    async fn test_capacity_eviction() {
        let store = InMemoryMessageStore::new(2);
        let msg = test_msg();

        let id1 = store.save(&msg).await.unwrap();
        let _id2 = store.save(&msg).await.unwrap();
        let _id3 = store.save(&msg).await.unwrap();

        // First message should be evicted
        assert!(store.get(&id1).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_cleanup() {
        let store = InMemoryMessageStore::new(100);
        let msg = test_msg();
        let id = store.save(&msg).await.unwrap();
        store.mark_completed(&id).await.unwrap();

        // Cleanup with future cutoff
        let cleaned = store
            .cleanup(Utc::now() + chrono::Duration::hours(1))
            .await
            .unwrap();
        assert_eq!(cleaned, 1);
        assert!(store.get(&id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_release_all_held() {
        let store = InMemoryMessageStore::new(100);
        let msg = test_msg();

        // Save three messages and mark two as Held
        let id1 = store.save(&msg).await.unwrap();
        let id2 = store.save(&msg).await.unwrap();
        let _id3 = store.save(&msg).await.unwrap();

        store.mark_held(&id1).await.unwrap();
        store.mark_held(&id2).await.unwrap();
        // id3 stays Pending

        // Verify they are Held
        assert_eq!(store.get(&id1).await.unwrap().unwrap().status, MessageStatus::Held);
        assert_eq!(store.get(&id2).await.unwrap().unwrap().status, MessageStatus::Held);

        // Held messages should not be retryable
        let retryable = store.get_retryable(10).await.unwrap();
        assert_eq!(retryable.len(), 1); // only id3

        // Release all held
        let released = store.release_all_held().await.unwrap();
        assert_eq!(released, 2);

        // All three should now be Pending and retryable
        assert_eq!(store.get(&id1).await.unwrap().unwrap().status, MessageStatus::Pending);
        assert_eq!(store.get(&id2).await.unwrap().unwrap().status, MessageStatus::Pending);
        let retryable = store.get_retryable(10).await.unwrap();
        assert_eq!(retryable.len(), 3);
    }

    #[tokio::test]
    async fn test_cleanup_skips_non_completed() {
        let store = InMemoryMessageStore::new(100);
        let msg = test_msg();
        let id = store.save(&msg).await.unwrap();
        // Still pending — should not be cleaned up

        let cleaned = store
            .cleanup(Utc::now() + chrono::Duration::hours(1))
            .await
            .unwrap();
        assert_eq!(cleaned, 0);
        assert!(store.get(&id).await.unwrap().is_some());
    }
}
