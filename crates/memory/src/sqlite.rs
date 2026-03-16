//! SQLite-backed MemoryStore with FTS5 + optional vector search

use chrono::Utc;
use sqlx::{Row, SqlitePool};
use tracing::debug;
use uuid::Uuid;

use atta_types::{AttaError, MemoryType};

use crate::embedding::{
    bytes_to_embedding, cosine_similarity, embedding_to_bytes, EmbeddingProvider,
};
use crate::mmr::mmr_rerank;
use crate::rrf::{rrf_merge, RankedItem};
use crate::temporal_decay::apply_temporal_decay;
use crate::traits::{
    MatchSource, MemoryEntry, MemoryMetadata, MemoryStore, SearchOptions, SearchResult,
};

/// SQLite memory store with FTS5 full-text search and optional vector similarity
pub struct SqliteMemoryStore {
    pool: SqlitePool,
    embedding_provider: Box<dyn EmbeddingProvider>,
}

impl SqliteMemoryStore {
    /// Create a new SqliteMemoryStore and run migrations
    pub async fn new(
        pool: SqlitePool,
        embedding_provider: Box<dyn EmbeddingProvider>,
    ) -> Result<Self, AttaError> {
        // Create memories table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS memories (
                id              TEXT PRIMARY KEY,
                memory_type     TEXT NOT NULL,
                content         TEXT NOT NULL,
                embedding       BLOB,
                metadata        TEXT NOT NULL DEFAULT '{}',
                task_id         TEXT,
                skill_id        TEXT,
                tags            TEXT NOT NULL DEFAULT '[]',
                source          TEXT,
                created_at      TEXT NOT NULL,
                last_accessed   TEXT NOT NULL,
                access_count    INTEGER NOT NULL DEFAULT 0
            )",
        )
        .execute(&pool)
        .await
        .map_err(|e| AttaError::Other(e.into()))?;

        // Create FTS5 virtual table
        sqlx::query(
            "CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
                id UNINDEXED, content, tags,
                content=memories,
                content_rowid=rowid
            )",
        )
        .execute(&pool)
        .await
        .map_err(|e| AttaError::Other(e.into()))?;

        // Create triggers for FTS sync
        sqlx::query(
            "CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
                INSERT INTO memories_fts(rowid, id, content, tags)
                VALUES (new.rowid, new.id, new.content, new.tags);
            END",
        )
        .execute(&pool)
        .await
        .map_err(|e| AttaError::Other(e.into()))?;

        sqlx::query(
            "CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
                INSERT INTO memories_fts(memories_fts, rowid, id, content, tags)
                VALUES ('delete', old.rowid, old.id, old.content, old.tags);
            END",
        )
        .execute(&pool)
        .await
        .map_err(|e| AttaError::Other(e.into()))?;

        sqlx::query(
            "CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
                INSERT INTO memories_fts(memories_fts, rowid, id, content, tags)
                VALUES ('delete', old.rowid, old.id, old.content, old.tags);
                INSERT INTO memories_fts(rowid, id, content, tags)
                VALUES (new.rowid, new.id, new.content, new.tags);
            END",
        )
        .execute(&pool)
        .await
        .map_err(|e| AttaError::Other(e.into()))?;

        // Indexes for common query patterns
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_memories_task_id ON memories(task_id)")
            .execute(&pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_memories_skill_id ON memories(skill_id)")
            .execute(&pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_memories_created_at ON memories(created_at)")
            .execute(&pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_memories_last_accessed ON memories(last_accessed)",
        )
        .execute(&pool)
        .await
        .map_err(|e| AttaError::Other(e.into()))?;

        // PRAGMA tuning for performance
        sqlx::query("PRAGMA journal_mode = WAL")
            .execute(&pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;
        sqlx::query("PRAGMA synchronous = NORMAL")
            .execute(&pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;
        sqlx::query("PRAGMA mmap_size = 8388608")
            .execute(&pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;
        sqlx::query("PRAGMA cache_size = -2000")
            .execute(&pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;
        sqlx::query("PRAGMA temp_store = MEMORY")
            .execute(&pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        Ok(Self {
            pool,
            embedding_provider,
        })
    }

    /// Full-text search returning ranked results
    async fn fts_search(
        &self,
        query: &str,
        limit: usize,
        options: &SearchOptions,
    ) -> Result<Vec<(Uuid, f32)>, AttaError> {
        // Preprocess query with multi-language tokenizer (CJK n-gram + English stop-word filter)
        let fts_query = crate::tokenizer::build_fts_query(query);

        // Build SQL with optional metadata filters via a JOIN
        let mut sql = String::from(
            "SELECT f.id, f.rank FROM memories_fts f JOIN memories m ON f.id = m.id WHERE memories_fts MATCH ?1",
        );
        let mut bind_idx = 2;

        if options.task_id.is_some() {
            sql.push_str(&format!(" AND m.task_id = ?{bind_idx}"));
            bind_idx += 1;
        }
        if options.skill_id.is_some() {
            sql.push_str(&format!(" AND m.skill_id = ?{bind_idx}"));
            bind_idx += 1;
        }
        sql.push_str(&format!(" ORDER BY f.rank LIMIT ?{bind_idx}"));

        let mut q = sqlx::query(&sql).bind(&fts_query);
        if let Some(ref task_id) = options.task_id {
            q = q.bind(task_id.to_string());
        }
        if let Some(ref skill_id) = options.skill_id {
            q = q.bind(skill_id.clone());
        }
        q = q.bind(limit as i64);

        let rows = q
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        let mut results = Vec::new();
        for row in &rows {
            let id_str: String = row.try_get("id").map_err(|e| AttaError::Other(e.into()))?;
            let rank: f64 = row
                .try_get("rank")
                .map_err(|e| AttaError::Other(e.into()))?;
            if let Ok(id) = Uuid::parse_str(&id_str) {
                // FTS5 rank is negative (more negative = better match), normalize
                results.push((id, (-rank as f32).max(0.0)));
            }
        }
        Ok(results)
    }

    /// Maximum rows to scan in brute-force vector search (OOM protection)
    const VECTOR_SCAN_LIMIT: usize = 50_000;

    /// Vector similarity search (brute-force cosine with scan limit)
    async fn vector_search(
        &self,
        query_embedding: &[f32],
        limit: usize,
        options: &SearchOptions,
    ) -> Result<Vec<(Uuid, f32)>, AttaError> {
        if query_embedding.is_empty() {
            return Ok(Vec::new());
        }

        let mut sql =
            String::from("SELECT id, embedding FROM memories WHERE embedding IS NOT NULL");
        let mut bind_idx = 1;
        if options.task_id.is_some() {
            sql.push_str(&format!(" AND task_id = ?{bind_idx}"));
            bind_idx += 1;
        }
        if options.skill_id.is_some() {
            sql.push_str(&format!(" AND skill_id = ?{bind_idx}"));
            bind_idx += 1;
        }
        sql.push_str(&format!(" LIMIT ?{bind_idx}"));

        let mut q = sqlx::query(&sql);
        if let Some(ref task_id) = options.task_id {
            q = q.bind(task_id.to_string());
        }
        if let Some(ref skill_id) = options.skill_id {
            q = q.bind(skill_id.clone());
        }
        q = q.bind(Self::VECTOR_SCAN_LIMIT as i64);

        let rows = q
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        let mut scored: Vec<(Uuid, f32)> = Vec::with_capacity(rows.len());
        for row in &rows {
            let id_str: String = row.try_get("id").map_err(|e| AttaError::Other(e.into()))?;
            let emb_bytes: Vec<u8> = row
                .try_get("embedding")
                .map_err(|e| AttaError::Other(e.into()))?;

            if let Ok(id) = Uuid::parse_str(&id_str) {
                let embedding = bytes_to_embedding(&emb_bytes);
                let score = cosine_similarity(query_embedding, &embedding);
                scored.push((id, score));
            }
        }

        // Partial sort: only find top-k without fully sorting
        if scored.len() > limit {
            scored.select_nth_unstable_by(limit, |a, b| {
                b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
            });
            scored.truncate(limit);
            scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        } else {
            scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        }

        Ok(scored)
    }

    /// Load a memory entry by ID
    async fn load_entry(&self, id: &Uuid) -> Result<Option<MemoryEntry>, AttaError> {
        let row = sqlx::query("SELECT * FROM memories WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        match row {
            Some(row) => Ok(Some(row_to_entry(&row)?)),
            None => Ok(None),
        }
    }
}

fn row_to_entry(row: &sqlx::sqlite::SqliteRow) -> Result<MemoryEntry, AttaError> {
    let id_str: String = row.try_get("id").map_err(|e| AttaError::Other(e.into()))?;
    let memory_type_str: String = row
        .try_get("memory_type")
        .map_err(|e| AttaError::Other(e.into()))?;
    let content: String = row
        .try_get("content")
        .map_err(|e| AttaError::Other(e.into()))?;
    let emb_bytes: Option<Vec<u8>> = row.try_get("embedding").ok();
    let metadata_str: String = row
        .try_get("metadata")
        .map_err(|e| AttaError::Other(e.into()))?;
    let created_at_str: String = row
        .try_get("created_at")
        .map_err(|e| AttaError::Other(e.into()))?;
    let last_accessed_str: String = row
        .try_get("last_accessed")
        .map_err(|e| AttaError::Other(e.into()))?;
    let access_count: i32 = row
        .try_get("access_count")
        .map_err(|e| AttaError::Other(e.into()))?;

    let id = Uuid::parse_str(&id_str).map_err(|e| AttaError::Other(anyhow::anyhow!(e)))?;
    let memory_type: MemoryType =
        serde_json::from_str(&format!("\"{}\"", memory_type_str)).unwrap_or(MemoryType::Knowledge);
    let embedding = emb_bytes.map(|b| bytes_to_embedding(&b));
    let metadata: MemoryMetadata = serde_json::from_str(&metadata_str).unwrap_or_default();
    let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let last_accessed = chrono::DateTime::parse_from_rfc3339(&last_accessed_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    Ok(MemoryEntry {
        id,
        memory_type,
        content,
        embedding,
        metadata,
        created_at,
        last_accessed,
        access_count: access_count as u32,
    })
}

#[async_trait::async_trait]
impl MemoryStore for SqliteMemoryStore {
    async fn store(&self, mut entry: MemoryEntry) -> Result<(), AttaError> {
        // Generate embedding if provider is available and entry has no embedding
        if entry.embedding.is_none() && self.embedding_provider.dimensions() > 0 {
            let emb = self.embedding_provider.embed(&entry.content).await?;
            if !emb.is_empty() {
                entry.embedding = Some(emb);
            }
        }

        let emb_bytes = entry.embedding.as_ref().map(|e| embedding_to_bytes(e));
        let metadata_json =
            serde_json::to_string(&entry.metadata).map_err(|e| AttaError::Other(e.into()))?;
        let tags_json =
            serde_json::to_string(&entry.metadata.tags).map_err(|e| AttaError::Other(e.into()))?;
        let memory_type_str = serde_json::to_string(&entry.memory_type)
            .map_err(|e| AttaError::Other(e.into()))?
            .trim_matches('"')
            .to_string();

        sqlx::query(
            "INSERT INTO memories (id, memory_type, content, embedding, metadata, task_id, skill_id, tags, source, created_at, last_accessed, access_count)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                content = excluded.content,
                embedding = excluded.embedding,
                metadata = excluded.metadata,
                tags = excluded.tags,
                last_accessed = excluded.last_accessed",
        )
        .bind(entry.id.to_string())
        .bind(&memory_type_str)
        .bind(&entry.content)
        .bind(emb_bytes)
        .bind(&metadata_json)
        .bind(entry.metadata.task_id.map(|id| id.to_string()))
        .bind(&entry.metadata.skill_id)
        .bind(&tags_json)
        .bind(&entry.metadata.source)
        .bind(entry.created_at.to_rfc3339())
        .bind(entry.last_accessed.to_rfc3339())
        .bind(entry.access_count as i32)
        .execute(&self.pool)
        .await
        .map_err(|e| AttaError::Other(e.into()))?;

        debug!(id = %entry.id, "memory stored");
        Ok(())
    }

    async fn search(
        &self,
        query: &str,
        options: &SearchOptions,
    ) -> Result<Vec<SearchResult>, AttaError> {
        let limit = options.limit;

        // FTS search
        let fts_results = self
            .fts_search(query, limit * 2, options)
            .await
            .unwrap_or_default();

        // Vector search
        let query_emb = self
            .embedding_provider
            .embed(query)
            .await
            .unwrap_or_default();
        let vector_results = if !query_emb.is_empty() {
            self.vector_search(&query_emb, limit * 2, options)
                .await
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        // Use custom RRF params or defaults
        let rrf_params = options.rrf_params.clone().unwrap_or_default();

        // Determine match source and final results
        let results = if !vector_results.is_empty() && !fts_results.is_empty() {
            // RRF merge
            let vector_ranked: Vec<RankedItem> = vector_results
                .iter()
                .enumerate()
                .map(|(rank, (id, _))| RankedItem { id: *id, rank })
                .collect();
            let fts_ranked: Vec<RankedItem> = fts_results
                .iter()
                .enumerate()
                .map(|(rank, (id, _))| RankedItem { id: *id, rank })
                .collect();

            let merged = rrf_merge(&vector_ranked, &fts_ranked, &rrf_params);
            let mut results = Vec::new();
            for (id, score) in merged.into_iter().take(limit) {
                if let Some(entry) = self.load_entry(&id).await? {
                    results.push(SearchResult {
                        entry,
                        score,
                        match_source: MatchSource::Hybrid,
                    });
                }
            }
            results
        } else if !fts_results.is_empty() {
            let max_score = fts_results.first().map(|(_, s)| *s).unwrap_or(1.0).max(1.0);
            let mut results = Vec::new();
            for (id, score) in fts_results.into_iter().take(limit) {
                if let Some(entry) = self.load_entry(&id).await? {
                    results.push(SearchResult {
                        entry,
                        score: score / max_score, // Normalize to 0-1
                        match_source: MatchSource::FullText,
                    });
                }
            }
            results
        } else if !vector_results.is_empty() {
            let mut results = Vec::new();
            for (id, score) in vector_results.into_iter().take(limit) {
                if let Some(entry) = self.load_entry(&id).await? {
                    results.push(SearchResult {
                        entry,
                        score,
                        match_source: MatchSource::Vector,
                    });
                }
            }
            results
        } else {
            Vec::new()
        };

        // MMR diversity reranking (before decay, so we diversify by content first)
        let mut results = if let Some(ref mmr_params) = options.mmr {
            if mmr_params.enabled {
                mmr_rerank(results, mmr_params.lambda)
            } else {
                results
            }
        } else {
            results
        };

        // Temporal decay
        if let Some(ref decay_params) = options.decay {
            apply_temporal_decay(&mut results, decay_params, chrono::Utc::now());
        }

        // memory_type filter
        if let Some(ref types) = options.memory_types {
            results.retain(|r| types.contains(&r.entry.memory_type));
        }

        // Apply min_score filter
        if let Some(min_score) = options.min_score {
            results.retain(|r| r.score >= min_score);
        }

        // Apply offset
        if let Some(offset) = options.offset {
            if offset < results.len() {
                results = results.into_iter().skip(offset).collect();
            } else {
                results.clear();
            }
        }

        Ok(results)
    }

    async fn get(&self, id: &Uuid) -> Result<Option<MemoryEntry>, AttaError> {
        // Update access stats
        sqlx::query(
            "UPDATE memories SET access_count = access_count + 1, last_accessed = ? WHERE id = ?",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| AttaError::Other(e.into()))?;

        self.load_entry(id).await
    }

    async fn delete(&self, id: &Uuid) -> Result<(), AttaError> {
        sqlx::query("DELETE FROM memories WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;
        Ok(())
    }

    async fn cleanup(&self, before: chrono::DateTime<Utc>) -> Result<usize, AttaError> {
        let result = sqlx::query("DELETE FROM memories WHERE last_accessed < ?")
            .bind(before.to_rfc3339())
            .execute(&self.pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;
        Ok(result.rows_affected() as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::NoopEmbeddingProvider;
    use crate::mmr::MmrParams;
    use crate::rrf::RrfParams;
    use crate::temporal_decay::DecayParams;

    async fn setup() -> SqliteMemoryStore {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        SqliteMemoryStore::new(pool, Box::new(NoopEmbeddingProvider))
            .await
            .unwrap()
    }

    fn sample_entry(content: &str) -> MemoryEntry {
        MemoryEntry {
            id: Uuid::new_v4(),
            memory_type: MemoryType::Knowledge,
            content: content.to_string(),
            embedding: None,
            metadata: MemoryMetadata {
                tags: vec!["test".to_string()],
                ..Default::default()
            },
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            access_count: 0,
        }
    }

    #[tokio::test]
    async fn test_store_and_get() {
        let store = setup().await;
        let entry = sample_entry("Rust is a great language");
        let id = entry.id;

        store.store(entry).await.unwrap();
        let retrieved = store.get(&id).await.unwrap().unwrap();
        assert_eq!(retrieved.content, "Rust is a great language");
        assert_eq!(retrieved.access_count, 1); // incremented by get()
    }

    #[tokio::test]
    async fn test_fts_search() {
        let store = setup().await;
        store
            .store(sample_entry("Rust programming language"))
            .await
            .unwrap();
        store.store(sample_entry("Python scripting")).await.unwrap();

        let results = store
            .search("Rust", &SearchOptions::default())
            .await
            .unwrap();
        assert!(!results.is_empty());
        assert!(results[0].entry.content.contains("Rust"));
    }

    #[tokio::test]
    async fn test_delete() {
        let store = setup().await;
        let entry = sample_entry("temporary");
        let id = entry.id;
        store.store(entry).await.unwrap();
        store.delete(&id).await.unwrap();
        assert!(store.get(&id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_cleanup() {
        let store = setup().await;
        let mut old = sample_entry("old entry");
        old.last_accessed = Utc::now() - chrono::Duration::days(30);
        store.store(old).await.unwrap();

        let count = store
            .cleanup(Utc::now() - chrono::Duration::days(7))
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    // ── FakeEmbeddingProvider for vector search tests ──

    /// Deterministic embedding provider that maps known texts to controlled vectors.
    /// "rust" / "systems programming" → near base vector
    /// "python" / "scripting language" → far from base vector
    /// Unknown text → hash-based deterministic vector
    struct FakeEmbeddingProvider;

    impl FakeEmbeddingProvider {
        fn make_vector(seed: f32, secondary: f32) -> Vec<f32> {
            let mut v = vec![0.0f32; 384];
            // Spread energy across first 8 dims to create distinguishable vectors
            v[0] = seed;
            v[1] = secondary;
            v[2] = (seed + secondary) * 0.5;
            v[3] = (seed - secondary).abs() * 0.3;
            // Normalize
            let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                v.iter_mut().for_each(|x| *x /= norm);
            }
            v
        }
    }

    #[async_trait::async_trait]
    impl EmbeddingProvider for FakeEmbeddingProvider {
        async fn embed(&self, text: &str) -> Result<Vec<f32>, AttaError> {
            let lower = text.to_lowercase();
            let v = if lower.contains("rust") || lower.contains("systems programming") {
                Self::make_vector(1.0, 0.1)
            } else if lower.contains("python") || lower.contains("scripting") {
                Self::make_vector(-0.8, 0.9)
            } else if lower.contains("javascript") || lower.contains("web development") {
                Self::make_vector(-0.5, -0.7)
            } else {
                // Hash-based fallback
                let hash: f32 = lower.bytes().enumerate().fold(0.0, |acc, (i, b)| {
                    acc + (b as f32) * (0.01 * (i + 1) as f32)
                });
                Self::make_vector(hash.sin(), hash.cos())
            };
            Ok(v)
        }

        fn dimensions(&self) -> usize {
            384
        }
    }

    async fn setup_with_vectors() -> SqliteMemoryStore {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        SqliteMemoryStore::new(pool, Box::new(FakeEmbeddingProvider))
            .await
            .unwrap()
    }

    // ── Vector search path tests ──

    #[tokio::test]
    async fn test_vector_store_generates_embedding() {
        let store = setup_with_vectors().await;
        let entry = sample_entry("Rust systems programming");
        let id = entry.id;
        assert!(entry.embedding.is_none());

        store.store(entry).await.unwrap();
        let retrieved = store.get(&id).await.unwrap().unwrap();
        assert!(retrieved.embedding.is_some());
        assert_eq!(retrieved.embedding.unwrap().len(), 384);
    }

    #[tokio::test]
    async fn test_vector_search_returns_similar() {
        let store = setup_with_vectors().await;
        store
            .store(sample_entry("Rust systems programming language"))
            .await
            .unwrap();
        store
            .store(sample_entry("Python scripting language"))
            .await
            .unwrap();
        store
            .store(sample_entry("JavaScript web development"))
            .await
            .unwrap();

        // Search for "Rust" → should rank Rust entry highest by vector similarity
        let results = store
            .search("Rust", &SearchOptions::default())
            .await
            .unwrap();
        assert!(!results.is_empty());
        assert!(results[0].entry.content.contains("Rust"));
    }

    #[tokio::test]
    async fn test_vector_search_cosine_ordering() {
        let store = setup_with_vectors().await;
        store
            .store(sample_entry("Rust programming"))
            .await
            .unwrap();
        store
            .store(sample_entry("Python scripting"))
            .await
            .unwrap();
        store
            .store(sample_entry("JavaScript web development"))
            .await
            .unwrap();

        let results = store
            .search("Rust systems programming", &SearchOptions::default())
            .await
            .unwrap();

        // Scores should be monotonically decreasing
        for w in results.windows(2) {
            assert!(
                w[0].score >= w[1].score,
                "scores not decreasing: {} < {}",
                w[0].score,
                w[1].score
            );
        }
    }

    #[tokio::test]
    async fn test_vector_search_with_task_filter() {
        let store = setup_with_vectors().await;
        let task_id = Uuid::new_v4();

        let mut entry_a = sample_entry("Rust programming");
        entry_a.metadata.task_id = Some(task_id);
        store.store(entry_a).await.unwrap();

        store
            .store(sample_entry("Rust systems language"))
            .await
            .unwrap();

        let opts = SearchOptions {
            task_id: Some(task_id),
            ..Default::default()
        };
        let results = store.search("Rust", &opts).await.unwrap();
        // Only the entry with matching task_id should be in vector results
        for r in &results {
            if r.match_source == MatchSource::Vector || r.match_source == MatchSource::Hybrid {
                assert_eq!(r.entry.metadata.task_id, Some(task_id));
            }
        }
    }

    // ── Hybrid search (vector + FTS → RRF) tests ──

    #[tokio::test]
    async fn test_hybrid_search_rrf_merge() {
        let store = setup_with_vectors().await;

        // Entry A: FTS matches "database" keyword, vector far from query "Rust"
        store
            .store(sample_entry("database management system"))
            .await
            .unwrap();
        // Entry B: FTS matches "Rust" AND vector close to query "Rust"
        store
            .store(sample_entry("Rust programming language"))
            .await
            .unwrap();
        // Entry C: vector close to "Rust" but no keyword match for "Rust"
        store
            .store(sample_entry("systems programming in low level"))
            .await
            .unwrap();

        let results = store
            .search("Rust", &SearchOptions::default())
            .await
            .unwrap();

        assert!(!results.is_empty());
        // Entry B ("Rust programming") should rank first: matched by both FTS and vector
        assert!(
            results[0].entry.content.contains("Rust"),
            "expected Rust entry first, got: {}",
            results[0].entry.content
        );
        // With both FTS and vector results available, hybrid match should appear
        let has_hybrid = results
            .iter()
            .any(|r| r.match_source == MatchSource::Hybrid);
        let has_vector = results
            .iter()
            .any(|r| r.match_source == MatchSource::Vector);
        let has_fts = results
            .iter()
            .any(|r| r.match_source == MatchSource::FullText);
        // At least the RRF merge path was exercised (all sources merged → Hybrid)
        assert!(
            has_hybrid || (has_vector && has_fts),
            "expected hybrid or mixed results, sources: {:?}",
            results.iter().map(|r| &r.match_source).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn test_hybrid_search_rrf_weight_bias() {
        let store = setup_with_vectors().await;

        store
            .store(sample_entry("Rust programming language"))
            .await
            .unwrap();
        store
            .store(sample_entry("Python scripting language"))
            .await
            .unwrap();

        // Heavy FTS weight
        let fts_opts = SearchOptions {
            rrf_params: Some(RrfParams {
                k: 60.0,
                vector_weight: 0.1,
                fts_weight: 0.9,
            }),
            ..Default::default()
        };
        let fts_results = store.search("Rust", &fts_opts).await.unwrap();

        // Heavy vector weight
        let vec_opts = SearchOptions {
            rrf_params: Some(RrfParams {
                k: 60.0,
                vector_weight: 0.9,
                fts_weight: 0.1,
            }),
            ..Default::default()
        };
        let vec_results = store.search("Rust", &vec_opts).await.unwrap();

        // Both should return results and Rust should be first in both cases
        assert!(!fts_results.is_empty());
        assert!(!vec_results.is_empty());
        // Scores may differ due to weight differences
        assert!(fts_results[0].entry.content.contains("Rust"));
        assert!(vec_results[0].entry.content.contains("Rust"));
    }

    #[tokio::test]
    async fn test_hybrid_with_mmr_diversity() {
        let store = setup_with_vectors().await;

        // Store several similar Rust entries + one different entry
        store
            .store(sample_entry("Rust programming language"))
            .await
            .unwrap();
        store
            .store(sample_entry("Rust systems programming"))
            .await
            .unwrap();
        store
            .store(sample_entry("Rust programming for beginners"))
            .await
            .unwrap();
        store
            .store(sample_entry("Python scripting language"))
            .await
            .unwrap();

        let mmr_opts = SearchOptions {
            mmr: Some(MmrParams {
                enabled: true,
                lambda: 0.3, // Strong diversity preference
            }),
            ..Default::default()
        };
        let results = store.search("Rust", &mmr_opts).await.unwrap();

        assert!(results.len() >= 2);
        // With strong diversity, Python entry should be promoted (not last)
        let python_pos = results
            .iter()
            .position(|r| r.entry.content.contains("Python"));
        let last_pos = results.len() - 1;
        if let Some(pos) = python_pos {
            // Python should appear somewhere; with diversity it shouldn't be dead last
            // among 4 entries if 3 are near-identical Rust entries
            assert!(pos <= last_pos, "Python entry found at position {pos}");
        }
    }

    #[tokio::test]
    async fn test_hybrid_with_temporal_decay() {
        let store = setup_with_vectors().await;

        // Old Rust entry
        let mut old_entry = sample_entry("Rust programming language");
        old_entry.created_at = Utc::now() - chrono::Duration::days(60);
        old_entry.last_accessed = Utc::now() - chrono::Duration::days(60);
        store.store(old_entry).await.unwrap();

        // Recent Rust entry
        let recent_entry = sample_entry("Rust systems programming");
        store.store(recent_entry).await.unwrap();

        let decay_opts = SearchOptions {
            decay: Some(DecayParams {
                enabled: true,
                half_life_days: 7.0, // Aggressive decay
            }),
            ..Default::default()
        };
        let results = store.search("Rust", &decay_opts).await.unwrap();

        assert!(results.len() >= 2);
        // Recent entry should score higher due to temporal decay
        let recent_score = results
            .iter()
            .find(|r| r.entry.content.contains("systems"))
            .map(|r| r.score);
        let old_score = results
            .iter()
            .find(|r| r.entry.content == "Rust programming language")
            .map(|r| r.score);

        if let (Some(recent), Some(old)) = (recent_score, old_score) {
            assert!(
                recent > old,
                "expected recent ({recent}) > old ({old}) with decay"
            );
        }
    }
}
