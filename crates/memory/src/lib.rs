//! AttaOS 记忆系统
//!
//! 提供 Agent 记忆的存储与检索能力：
//! - [`traits::MemoryStore`] — 记忆存储抽象 trait
//! - [`noop::NoopMemoryStore`] — 空实现
//! - [`sqlite::SqliteMemoryStore`] — SQLite + FTS5 + 向量搜索实现
//! - [`embedding`] — 嵌入提供者 trait 和工具函数
//! - [`embedding_cache`] — 嵌入缓存（SQLite + LRU）
//! - [`rrf`] — Reciprocal Rank Fusion 算法
//! - [`mmr`] — Maximal Marginal Relevance 多样性重排序
//! - [`temporal_decay`] — 时间衰减评分

pub mod embedding;
pub mod embedding_cache;
pub mod mmr;
pub mod noop;
pub mod rrf;
pub mod sqlite;
pub mod temporal_decay;
pub mod tokenizer;
pub mod traits;

#[cfg(feature = "fastembed")]
pub mod fastembed;

#[cfg(feature = "postgres")]
pub mod postgres;

pub use embedding::{EmbeddingProvider, NoopEmbeddingProvider};
pub use embedding_cache::CachedEmbeddingProvider;
pub use noop::NoopMemoryStore;
pub use sqlite::SqliteMemoryStore;
pub use traits::{MemoryEntry, MemoryStore, SearchOptions, SearchResult};

#[cfg(feature = "fastembed")]
pub use self::fastembed::FastEmbedProvider;

#[cfg(feature = "postgres")]
pub use postgres::PgMemoryStore;
