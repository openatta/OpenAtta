//! NoopMemoryStore — 空实现
//!
//! 所有操作均为空操作，搜索返回空结果。
//! 用于测试、Desktop 轻量模式或记忆系统未启用时的回退。

use atta_types::AttaError;
use chrono::{DateTime, Utc};
use tracing::debug;
use uuid::Uuid;

use crate::traits::{MemoryEntry, MemoryStore, SearchOptions, SearchResult};

/// 空操作记忆存储
///
/// 不执行任何持久化，所有查询返回空结果。
pub struct NoopMemoryStore;

impl NoopMemoryStore {
    /// 创建新的 NoopMemoryStore
    pub fn new() -> Self {
        Self
    }
}

impl Default for NoopMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl MemoryStore for NoopMemoryStore {
    async fn store(&self, entry: MemoryEntry) -> Result<(), AttaError> {
        debug!(id = %entry.id, "NoopMemoryStore::store — discarding");
        Ok(())
    }

    async fn search(
        &self,
        query: &str,
        _options: &SearchOptions,
    ) -> Result<Vec<SearchResult>, AttaError> {
        debug!(query, "NoopMemoryStore::search — returning empty");
        Ok(vec![])
    }

    async fn get(&self, id: &Uuid) -> Result<Option<MemoryEntry>, AttaError> {
        debug!(%id, "NoopMemoryStore::get — returning None");
        Ok(None)
    }

    async fn delete(&self, id: &Uuid) -> Result<(), AttaError> {
        debug!(%id, "NoopMemoryStore::delete — noop");
        Ok(())
    }

    async fn cleanup(&self, before: DateTime<Utc>) -> Result<usize, AttaError> {
        debug!(%before, "NoopMemoryStore::cleanup — noop");
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::MemoryMetadata;
    use atta_types::MemoryType;

    fn sample_entry() -> MemoryEntry {
        MemoryEntry {
            id: Uuid::new_v4(),
            memory_type: MemoryType::Knowledge,
            content: "Rust is a systems programming language.".to_string(),
            embedding: None,
            metadata: MemoryMetadata::default(),
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            access_count: 0,
        }
    }

    #[tokio::test]
    async fn test_store_succeeds() {
        let store = NoopMemoryStore::new();
        let result = store.store(sample_entry()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_search_returns_empty() {
        let store = NoopMemoryStore::new();
        let results = store
            .search("anything", &SearchOptions::default())
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_get_returns_none() {
        let store = NoopMemoryStore::new();
        let result = store.get(&Uuid::new_v4()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_delete_succeeds() {
        let store = NoopMemoryStore::new();
        let result = store.delete(&Uuid::new_v4()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_cleanup_returns_zero() {
        let store = NoopMemoryStore::new();
        let result = store.cleanup(Utc::now()).await.unwrap();
        assert_eq!(result, 0);
    }
}
