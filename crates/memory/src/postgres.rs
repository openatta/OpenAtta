//! PostgreSQL-backed MemoryStore with pgvector + full-text search
//!
//! Enterprise backend using `pgvector` for vector similarity
//! and PostgreSQL `tsvector`/`tsquery` for full-text search.

use chrono::Utc;
use sqlx::{PgPool, Row};
use tracing::debug;
use uuid::Uuid;

use atta_types::{AttaError, MemoryType};

use crate::embedding::{bytes_to_embedding, embedding_to_bytes, EmbeddingProvider};
use crate::mmr::mmr_rerank;
use crate::rrf::{rrf_merge, RankedItem, RrfParams};
use crate::temporal_decay::apply_temporal_decay;
use crate::traits::{
    MatchSource, MemoryEntry, MemoryMetadata, MemoryStore, SearchOptions, SearchResult,
};

/// PostgreSQL memory store with pgvector and full-text search
pub struct PgMemoryStore {
    pool: PgPool,
    embedding_provider: Box<dyn EmbeddingProvider>,
}

impl PgMemoryStore {
    /// Create a new PgMemoryStore and run migrations
    pub async fn new(
        pool: PgPool,
        embedding_provider: Box<dyn EmbeddingProvider>,
    ) -> Result<Self, AttaError> {
        let dims = embedding_provider.dimensions();

        // Ensure pgvector extension is available
        sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
            .execute(&pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        // Create memories table with vector column
        let create_sql = format!(
            "CREATE TABLE IF NOT EXISTS memories (
                id              UUID PRIMARY KEY,
                memory_type     TEXT NOT NULL,
                content         TEXT NOT NULL,
                embedding       vector({dims}),
                metadata        JSONB NOT NULL DEFAULT '{{}}',
                task_id         UUID,
                skill_id        TEXT,
                tags            JSONB NOT NULL DEFAULT '[]',
                source          TEXT,
                content_tsv     tsvector GENERATED ALWAYS AS (to_tsvector('english', content)) STORED,
                created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
                last_accessed   TIMESTAMPTZ NOT NULL DEFAULT now(),
                access_count    INTEGER NOT NULL DEFAULT 0
            )"
        );
        sqlx::query(&create_sql)
            .execute(&pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        // Indexes
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_pg_memories_task_id ON memories(task_id)")
            .execute(&pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_pg_memories_skill_id ON memories(skill_id)")
            .execute(&pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_pg_memories_created_at ON memories(created_at)",
        )
        .execute(&pool)
        .await
        .map_err(|e| AttaError::Other(e.into()))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_pg_memories_content_tsv ON memories USING GIN(content_tsv)",
        )
        .execute(&pool)
        .await
        .map_err(|e| AttaError::Other(e.into()))?;

        // IVFFlat index for vector similarity (requires some data to train)
        let ivfflat_sql = format!(
            "CREATE INDEX IF NOT EXISTS idx_pg_memories_embedding ON memories USING ivfflat (embedding vector_cosine_ops) WITH (lists = 100)"
        );
        // This may fail if not enough rows exist yet; that's OK
        let _ = sqlx::query(&ivfflat_sql).execute(&pool).await;

        Ok(Self {
            pool,
            embedding_provider,
        })
    }

    /// Full-text search using PostgreSQL tsvector
    async fn fts_search(
        &self,
        query: &str,
        limit: usize,
        options: &SearchOptions,
    ) -> Result<Vec<(Uuid, f32)>, AttaError> {
        let mut sql = String::from(
            "SELECT id, ts_rank(content_tsv, plainto_tsquery('english', $1)) AS rank
             FROM memories
             WHERE content_tsv @@ plainto_tsquery('english', $1)",
        );
        let mut bind_idx = 2;

        if options.task_id.is_some() {
            sql.push_str(&format!(" AND task_id = ${bind_idx}"));
            bind_idx += 1;
        }
        if options.skill_id.is_some() {
            sql.push_str(&format!(" AND skill_id = ${bind_idx}"));
            bind_idx += 1;
        }
        sql.push_str(&format!(" ORDER BY rank DESC LIMIT ${bind_idx}"));

        let mut q = sqlx::query(&sql).bind(query);
        if let Some(ref task_id) = options.task_id {
            q = q.bind(*task_id);
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
            let id: Uuid = row.try_get("id").map_err(|e| AttaError::Other(e.into()))?;
            let rank: f32 = row
                .try_get("rank")
                .map_err(|e| AttaError::Other(e.into()))?;
            results.push((id, rank));
        }
        Ok(results)
    }

    /// Vector similarity search using pgvector cosine distance
    async fn vector_search(
        &self,
        query_embedding: &[f32],
        limit: usize,
        options: &SearchOptions,
    ) -> Result<Vec<(Uuid, f32)>, AttaError> {
        if query_embedding.is_empty() {
            return Ok(Vec::new());
        }

        // Format embedding as pgvector literal: '[1.0,2.0,3.0]'
        let emb_str = format!(
            "[{}]",
            query_embedding
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );

        let mut sql = format!(
            "SELECT id, 1 - (embedding <=> '{emb_str}'::vector) AS score
             FROM memories
             WHERE embedding IS NOT NULL"
        );
        let mut bind_idx = 1;

        if options.task_id.is_some() {
            sql.push_str(&format!(" AND task_id = ${bind_idx}"));
            bind_idx += 1;
        }
        if options.skill_id.is_some() {
            sql.push_str(&format!(" AND skill_id = ${bind_idx}"));
            bind_idx += 1;
        }
        sql.push_str(&format!(" ORDER BY score DESC LIMIT ${bind_idx}"));

        let mut q = sqlx::query(&sql);
        if let Some(ref task_id) = options.task_id {
            q = q.bind(*task_id);
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
            let id: Uuid = row.try_get("id").map_err(|e| AttaError::Other(e.into()))?;
            let score: f32 = row
                .try_get("score")
                .map_err(|e| AttaError::Other(e.into()))?;
            results.push((id, score));
        }
        Ok(results)
    }

    /// Load a memory entry by ID
    async fn load_entry(&self, id: &Uuid) -> Result<Option<MemoryEntry>, AttaError> {
        let row = sqlx::query(
            "SELECT id, memory_type, content, embedding, metadata, task_id, skill_id, tags, source, created_at, last_accessed, access_count
             FROM memories WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AttaError::Other(e.into()))?;

        match row {
            Some(row) => Ok(Some(pg_row_to_entry(&row)?)),
            None => Ok(None),
        }
    }
}

fn pg_row_to_entry(row: &sqlx::postgres::PgRow) -> Result<MemoryEntry, AttaError> {
    let id: Uuid = row.try_get("id").map_err(|e| AttaError::Other(e.into()))?;
    let memory_type_str: String = row
        .try_get("memory_type")
        .map_err(|e| AttaError::Other(e.into()))?;
    let content: String = row
        .try_get("content")
        .map_err(|e| AttaError::Other(e.into()))?;

    // pgvector returns Vec<f32> directly
    let embedding: Option<Vec<f32>> = row.try_get("embedding").ok();

    let metadata: serde_json::Value = row
        .try_get("metadata")
        .map_err(|e| AttaError::Other(e.into()))?;
    let created_at: chrono::DateTime<Utc> = row
        .try_get("created_at")
        .map_err(|e| AttaError::Other(e.into()))?;
    let last_accessed: chrono::DateTime<Utc> = row
        .try_get("last_accessed")
        .map_err(|e| AttaError::Other(e.into()))?;
    let access_count: i32 = row
        .try_get("access_count")
        .map_err(|e| AttaError::Other(e.into()))?;

    let memory_type: MemoryType =
        serde_json::from_str(&format!("\"{}\"", memory_type_str)).unwrap_or(MemoryType::Knowledge);
    let metadata: MemoryMetadata = serde_json::from_value(metadata).unwrap_or_default();

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
impl MemoryStore for PgMemoryStore {
    async fn store(&self, mut entry: MemoryEntry) -> Result<(), AttaError> {
        // Generate embedding if provider is available
        if entry.embedding.is_none() && self.embedding_provider.dimensions() > 0 {
            let emb = self.embedding_provider.embed(&entry.content).await?;
            if !emb.is_empty() {
                entry.embedding = Some(emb);
            }
        }

        let metadata_json =
            serde_json::to_value(&entry.metadata).map_err(|e| AttaError::Other(e.into()))?;
        let tags_json =
            serde_json::to_value(&entry.metadata.tags).map_err(|e| AttaError::Other(e.into()))?;
        let memory_type_str = serde_json::to_string(&entry.memory_type)
            .map_err(|e| AttaError::Other(e.into()))?
            .trim_matches('"')
            .to_string();

        // Format embedding for pgvector
        let emb_str = entry.embedding.as_ref().map(|e| {
            format!(
                "[{}]",
                e.iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        });

        if let Some(ref emb) = emb_str {
            sqlx::query(
                "INSERT INTO memories (id, memory_type, content, embedding, metadata, task_id, skill_id, tags, source, created_at, last_accessed, access_count)
                 VALUES ($1, $2, $3, $4::vector, $5, $6, $7, $8, $9, $10, $11, $12)
                 ON CONFLICT(id) DO UPDATE SET
                    content = EXCLUDED.content,
                    embedding = EXCLUDED.embedding,
                    metadata = EXCLUDED.metadata,
                    tags = EXCLUDED.tags,
                    last_accessed = EXCLUDED.last_accessed",
            )
            .bind(entry.id)
            .bind(&memory_type_str)
            .bind(&entry.content)
            .bind(emb)
            .bind(&metadata_json)
            .bind(entry.metadata.task_id)
            .bind(&entry.metadata.skill_id)
            .bind(&tags_json)
            .bind(&entry.metadata.source)
            .bind(entry.created_at)
            .bind(entry.last_accessed)
            .bind(entry.access_count as i32)
            .execute(&self.pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;
        } else {
            sqlx::query(
                "INSERT INTO memories (id, memory_type, content, metadata, task_id, skill_id, tags, source, created_at, last_accessed, access_count)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
                 ON CONFLICT(id) DO UPDATE SET
                    content = EXCLUDED.content,
                    metadata = EXCLUDED.metadata,
                    tags = EXCLUDED.tags,
                    last_accessed = EXCLUDED.last_accessed",
            )
            .bind(entry.id)
            .bind(&memory_type_str)
            .bind(&entry.content)
            .bind(&metadata_json)
            .bind(entry.metadata.task_id)
            .bind(&entry.metadata.skill_id)
            .bind(&tags_json)
            .bind(&entry.metadata.source)
            .bind(entry.created_at)
            .bind(entry.last_accessed)
            .bind(entry.access_count as i32)
            .execute(&self.pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;
        }

        debug!(id = %entry.id, "pg memory stored");
        Ok(())
    }

    async fn search(
        &self,
        query: &str,
        options: &SearchOptions,
    ) -> Result<Vec<SearchResult>, AttaError> {
        let limit = options.limit;
        let rrf_params = options.rrf_params.clone().unwrap_or_default();

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

        // Merge results
        let results = if !vector_results.is_empty() && !fts_results.is_empty() {
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
            let max_score = fts_results
                .first()
                .map(|(_, s)| *s)
                .unwrap_or(1.0)
                .max(1e-6);
            let mut results = Vec::new();
            for (id, score) in fts_results.into_iter().take(limit) {
                if let Some(entry) = self.load_entry(&id).await? {
                    results.push(SearchResult {
                        entry,
                        score: score / max_score,
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

        // MMR diversity reranking
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
            apply_temporal_decay(&mut results, decay_params, Utc::now());
        }

        // memory_type filter
        if let Some(ref types) = options.memory_types {
            results.retain(|r| types.contains(&r.entry.memory_type));
        }

        // min_score filter
        if let Some(min_score) = options.min_score {
            results.retain(|r| r.score >= min_score);
        }

        // offset
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
        sqlx::query(
            "UPDATE memories SET access_count = access_count + 1, last_accessed = now() WHERE id = $1",
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| AttaError::Other(e.into()))?;

        self.load_entry(id).await
    }

    async fn delete(&self, id: &Uuid) -> Result<(), AttaError> {
        sqlx::query("DELETE FROM memories WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;
        Ok(())
    }

    async fn cleanup(&self, before: chrono::DateTime<Utc>) -> Result<usize, AttaError> {
        let result = sqlx::query("DELETE FROM memories WHERE last_accessed < $1")
            .bind(before)
            .execute(&self.pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;
        Ok(result.rows_affected() as usize)
    }
}
