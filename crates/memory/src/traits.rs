//! 记忆存储 trait 与相关类型
//!
//! [`MemoryStore`] 定义记忆的持久化与检索接口。
//! 不同后端（SQLite FTS、向量数据库等）实现此 trait 即可接入。

use atta_types::{AttaError, MemoryType};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::mmr::MmrParams;
use crate::rrf::RrfParams;
use crate::temporal_decay::DecayParams;

/// 记忆条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// 唯一标识
    pub id: Uuid,
    /// 记忆类型
    pub memory_type: MemoryType,
    /// 记忆内容（文本）
    pub content: String,
    /// 嵌入向量（可选，由 EmbeddingProvider 生成）
    pub embedding: Option<Vec<f32>>,
    /// 附加元数据
    pub metadata: MemoryMetadata,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 最近访问时间
    pub last_accessed: DateTime<Utc>,
    /// 访问次数
    pub access_count: u32,
}

/// 记忆元数据
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryMetadata {
    /// 关联 Task ID
    pub task_id: Option<Uuid>,
    /// 关联 Skill ID
    pub skill_id: Option<String>,
    /// 标签
    pub tags: Vec<String>,
    /// 来源
    pub source: Option<String>,
    /// 相关性分数（由检索时设置）
    pub relevance_score: Option<f32>,
}

/// 搜索选项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchOptions {
    /// 返回结果数量上限
    pub limit: usize,
    /// 按记忆类型过滤（可多选）
    pub memory_types: Option<Vec<MemoryType>>,
    /// 最低相关性分数（0.0 ~ 1.0）
    pub min_score: Option<f32>,
    /// 按标签过滤
    pub tags: Option<Vec<String>>,
    /// 时间范围过滤
    pub time_range: Option<(DateTime<Utc>, DateTime<Utc>)>,
    /// 按关联 Task ID 过滤
    pub task_id: Option<Uuid>,
    /// 按关联 Skill ID 过滤
    pub skill_id: Option<String>,
    /// 分页偏移量
    pub offset: Option<usize>,
    /// Custom RRF fusion parameters
    pub rrf_params: Option<RrfParams>,
    /// MMR diversity reranking parameters
    pub mmr: Option<MmrParams>,
    /// Temporal decay parameters
    pub decay: Option<DecayParams>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            limit: 10,
            memory_types: None,
            min_score: None,
            tags: None,
            time_range: None,
            task_id: None,
            skill_id: None,
            offset: None,
            rrf_params: None,
            mmr: None,
            decay: None,
        }
    }
}

/// 匹配来源
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchSource {
    /// 向量相似度匹配
    Vector,
    /// 全文搜索匹配
    FullText,
    /// 混合匹配（RRF 融合）
    Hybrid,
}

/// 搜索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// 匹配的记忆条目
    pub entry: MemoryEntry,
    /// 相关性分数（0.0 ~ 1.0）
    pub score: f32,
    /// 匹配来源
    pub match_source: MatchSource,
}

/// 记忆存储 trait
///
/// 定义记忆的增删查接口。向量检索、FTS 等不同后端
/// 实现此 trait 即可被 Agent 使用。
#[async_trait::async_trait]
pub trait MemoryStore: Send + Sync + 'static {
    /// 存储一条记忆
    async fn store(&self, entry: MemoryEntry) -> Result<(), AttaError>;

    /// 搜索记忆（语义 / 全文）
    async fn search(
        &self,
        query: &str,
        options: &SearchOptions,
    ) -> Result<Vec<SearchResult>, AttaError>;

    /// 按 ID 获取单条记忆
    async fn get(&self, id: &Uuid) -> Result<Option<MemoryEntry>, AttaError>;

    /// 按 ID 删除单条记忆
    async fn delete(&self, id: &Uuid) -> Result<(), AttaError>;

    /// 清理指定时间前的记忆
    async fn cleanup(&self, before: DateTime<Utc>) -> Result<usize, AttaError>;
}
