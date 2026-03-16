//! Caching wrapper for EmbeddingProvider
//!
//! Uses SQLite to persist embeddings keyed by content SHA-256 hash.
//! Implements LRU eviction when the cache exceeds `max_entries`.

use atta_types::AttaError;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use tracing::debug;

use crate::embedding::{bytes_to_embedding, embedding_to_bytes, EmbeddingProvider};

/// Caching wrapper around any [`EmbeddingProvider`]
pub struct CachedEmbeddingProvider {
    inner: Box<dyn EmbeddingProvider>,
    pool: SqlitePool,
    max_entries: usize,
}

impl CachedEmbeddingProvider {
    /// Create a new cache wrapping `inner`
    pub async fn new(
        inner: Box<dyn EmbeddingProvider>,
        pool: SqlitePool,
        max_entries: usize,
    ) -> Result<Self, AttaError> {
        // Create cache table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS embedding_cache (
                content_hash TEXT PRIMARY KEY,
                embedding    BLOB NOT NULL,
                created_at   TEXT NOT NULL,
                accessed_at  TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .map_err(|e| AttaError::Other(e.into()))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_cache_accessed ON embedding_cache(accessed_at)",
        )
        .execute(&pool)
        .await
        .map_err(|e| AttaError::Other(e.into()))?;

        Ok(Self {
            inner,
            pool,
            max_entries,
        })
    }

    /// Compute a truncated SHA-256 hash (first 16 hex chars) for cache key
    fn content_hash(text: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(text.as_bytes());
        let result = hasher.finalize();
        // First 8 bytes → 16 hex chars
        hex::encode(&result[..8])
    }

    /// Look up a cached embedding
    async fn cache_get(&self, hash: &str) -> Result<Option<Vec<f32>>, AttaError> {
        let now = chrono::Utc::now().to_rfc3339();
        // Update accessed_at and return embedding
        let row = sqlx::query_as::<_, (Vec<u8>,)>(
            "UPDATE embedding_cache SET accessed_at = ? WHERE content_hash = ? RETURNING embedding",
        )
        .bind(&now)
        .bind(hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AttaError::Other(e.into()))?;

        Ok(row.map(|(bytes,)| bytes_to_embedding(&bytes)))
    }

    /// Store an embedding in the cache, evicting LRU if needed
    async fn cache_put(&self, hash: &str, embedding: &[f32]) -> Result<(), AttaError> {
        let now = chrono::Utc::now().to_rfc3339();
        let bytes = embedding_to_bytes(embedding);

        sqlx::query(
            "INSERT INTO embedding_cache (content_hash, embedding, created_at, accessed_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(content_hash) DO UPDATE SET
                embedding = excluded.embedding,
                accessed_at = excluded.accessed_at",
        )
        .bind(hash)
        .bind(&bytes)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| AttaError::Other(e.into()))?;

        // Evict if over capacity
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM embedding_cache")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if count.0 as usize > self.max_entries {
            let overflow = count.0 as usize - self.max_entries;
            sqlx::query(
                "DELETE FROM embedding_cache WHERE content_hash IN (
                    SELECT content_hash FROM embedding_cache
                    ORDER BY accessed_at ASC LIMIT ?
                )",
            )
            .bind(overflow as i64)
            .execute(&self.pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;
            debug!(evicted = overflow, "embedding cache LRU eviction");
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl EmbeddingProvider for CachedEmbeddingProvider {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, AttaError> {
        let hash = Self::content_hash(text);

        // Cache hit
        if let Some(cached) = self.cache_get(&hash).await? {
            debug!(hash = %hash, "embedding cache hit");
            return Ok(cached);
        }

        // Cache miss — compute and store
        let embedding = self.inner.embed(text).await?;
        self.cache_put(&hash, &embedding).await?;
        debug!(hash = %hash, "embedding cache miss, stored");
        Ok(embedding)
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, AttaError> {
        let hashes: Vec<String> = texts.iter().map(|t| Self::content_hash(t)).collect();

        // Check cache for all
        let mut results: Vec<Option<Vec<f32>>> = Vec::with_capacity(texts.len());
        let mut miss_indices: Vec<usize> = Vec::new();
        let mut miss_texts: Vec<&str> = Vec::new();

        for (i, hash) in hashes.iter().enumerate() {
            match self.cache_get(hash).await? {
                Some(cached) => results.push(Some(cached)),
                None => {
                    results.push(None);
                    miss_indices.push(i);
                    miss_texts.push(texts[i]);
                }
            }
        }

        // Batch compute misses
        if !miss_texts.is_empty() {
            let computed = self.inner.embed_batch(&miss_texts).await?;
            for (idx, embedding) in miss_indices.into_iter().zip(computed.into_iter()) {
                self.cache_put(&hashes[idx], &embedding).await?;
                results[idx] = Some(embedding);
            }
        }

        Ok(results.into_iter().map(|r| r.unwrap_or_default()).collect())
    }

    fn dimensions(&self) -> usize {
        self.inner.dimensions()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeProvider;

    #[async_trait::async_trait]
    impl EmbeddingProvider for FakeProvider {
        async fn embed(&self, _text: &str) -> Result<Vec<f32>, AttaError> {
            Ok(vec![1.0, 2.0, 3.0])
        }

        fn dimensions(&self) -> usize {
            3
        }
    }

    #[tokio::test]
    async fn test_cache_hit_and_miss() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let cached = CachedEmbeddingProvider::new(Box::new(FakeProvider), pool, 100)
            .await
            .unwrap();

        // First call — cache miss
        let emb1 = cached.embed("hello").await.unwrap();
        assert_eq!(emb1, vec![1.0, 2.0, 3.0]);

        // Second call — cache hit (same result)
        let emb2 = cached.embed("hello").await.unwrap();
        assert_eq!(emb2, vec![1.0, 2.0, 3.0]);
    }

    #[tokio::test]
    async fn test_lru_eviction() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let cached = CachedEmbeddingProvider::new(Box::new(FakeProvider), pool.clone(), 3)
            .await
            .unwrap();

        // Fill cache beyond capacity
        for i in 0..5 {
            cached.embed(&format!("text_{i}")).await.unwrap();
        }

        // Check that cache size is capped
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM embedding_cache")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(count.0 <= 3);
    }

    #[test]
    fn test_content_hash_deterministic() {
        let h1 = CachedEmbeddingProvider::content_hash("hello world");
        let h2 = CachedEmbeddingProvider::content_hash("hello world");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 16); // 8 bytes → 16 hex chars
    }
}
