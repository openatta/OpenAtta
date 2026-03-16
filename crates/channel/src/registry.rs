//! Channel registry — stores running channel instances for lookup

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::traits::Channel;

/// Registry of running channel instances.
///
/// Provides thread-safe access to channels by name.
pub struct ChannelRegistry {
    channels: RwLock<HashMap<String, Arc<dyn Channel>>>,
}

impl ChannelRegistry {
    /// Create an empty registry
    pub fn new() -> Self {
        Self {
            channels: RwLock::new(HashMap::new()),
        }
    }

    /// Insert a channel into the registry
    pub async fn insert(&self, name: String, channel: Arc<dyn Channel>) {
        self.channels.write().await.insert(name, channel);
    }

    /// Get a channel by name
    pub async fn get(&self, name: &str) -> Option<Arc<dyn Channel>> {
        self.channels.read().await.get(name).cloned()
    }

    /// List all channel names
    pub async fn list(&self) -> Vec<String> {
        self.channels.read().await.keys().cloned().collect()
    }

    /// Remove a channel by name, returning it if it existed
    pub async fn remove(&self, name: &str) -> Option<Arc<dyn Channel>> {
        self.channels.write().await.remove(name)
    }

    /// Number of registered channels
    pub async fn len(&self) -> usize {
        self.channels.read().await.len()
    }

    /// Whether the registry is empty
    pub async fn is_empty(&self) -> bool {
        self.channels.read().await.is_empty()
    }
}

impl Default for ChannelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{ChannelMessage, SendMessage};
    use atta_types::AttaError;

    struct DummyChannel {
        name: String,
    }

    #[async_trait::async_trait]
    impl Channel for DummyChannel {
        fn name(&self) -> &str {
            &self.name
        }
        async fn send(&self, _message: SendMessage) -> Result<(), AttaError> {
            Ok(())
        }
        async fn listen(
            &self,
            _tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> Result<(), AttaError> {
            std::future::pending::<()>().await;
            Ok(())
        }
        async fn health_check(&self) -> Result<(), AttaError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_insert_and_get() {
        let registry = ChannelRegistry::new();
        let ch: Arc<dyn Channel> = Arc::new(DummyChannel {
            name: "test".to_string(),
        });
        registry.insert("test".to_string(), ch).await;
        assert!(registry.get("test").await.is_some());
        assert!(registry.get("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn test_list() {
        let registry = ChannelRegistry::new();
        registry
            .insert(
                "a".to_string(),
                Arc::new(DummyChannel {
                    name: "a".to_string(),
                }),
            )
            .await;
        registry
            .insert(
                "b".to_string(),
                Arc::new(DummyChannel {
                    name: "b".to_string(),
                }),
            )
            .await;
        let mut names = registry.list().await;
        names.sort();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[tokio::test]
    async fn test_remove() {
        let registry = ChannelRegistry::new();
        registry
            .insert(
                "x".to_string(),
                Arc::new(DummyChannel {
                    name: "x".to_string(),
                }),
            )
            .await;
        assert_eq!(registry.len().await, 1);
        let removed = registry.remove("x").await;
        assert!(removed.is_some());
        assert!(registry.is_empty().await);
    }
}
