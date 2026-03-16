//! AuditSink trait 及审计相关类型
//!
//! 审计日志的核心抽象。Desktop 版使用 NoopAudit（仅 debug 日志），
//! Enterprise 版使用 AuditStore（持久化存储 + 查询）。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use atta_types::event::EntityRef;
use atta_types::{Actor, AttaError, EventEnvelope};

/// 审计结果枚举
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditOutcome {
    /// 操作成功
    Success,
    /// 操作被拒绝
    Denied,
    /// 操作失败
    Failed {
        /// 错误描述
        error: String,
    },
}

/// 审计条目
///
/// 记录一次操作的完整审计信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// 审计条目唯一 ID
    pub id: Uuid,
    /// 事件发生时间
    pub timestamp: DateTime<Utc>,
    /// 执行操作的主体
    pub actor: Actor,
    /// 执行的动作（完整事件类型字符串，如 "task.create"）
    pub action: String,
    /// 目标资源
    pub resource: EntityRef,
    /// 关联 ID（用于追踪同一 Flow/Task 的所有操作）
    pub correlation_id: Uuid,
    /// 操作结果
    pub outcome: AuditOutcome,
    /// 附加详情（JSON 格式的扩展信息）
    pub detail: serde_json::Value,
}

impl AuditEntry {
    /// 从 EventEnvelope 构造审计条目
    ///
    /// 调用方提供 action 字符串和审计结果。
    ///
    /// # Examples
    ///
    /// ```
    /// use atta_audit::{AuditEntry, AuditOutcome};
    /// use atta_types::{Actor, EntityRef, EventEnvelope};
    /// use uuid::Uuid;
    ///
    /// let event = EventEnvelope::new(
    ///     "atta.task.created",
    ///     EntityRef::flow("deploy"),
    ///     Actor::system(),
    ///     Uuid::new_v4(),
    ///     serde_json::json!({}),
    /// ).unwrap();
    /// let entry = AuditEntry::from_event(&event, "task.create", AuditOutcome::Success);
    /// assert_eq!(entry.action, "task.create");
    /// ```
    pub fn from_event(event: &EventEnvelope, action: &str, outcome: AuditOutcome) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp: event.occurred_at,
            actor: event.actor.clone(),
            action: action.to_string(),
            resource: event.entity.clone(),
            correlation_id: event.correlation_id,
            outcome,
            detail: event.payload.clone(),
        }
    }
}

/// 审计查询过滤器
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditFilter {
    /// 按操作者 ID 过滤
    pub actor_id: Option<String>,
    /// 按动作过滤（完整事件类型字符串）
    pub action: Option<String>,
    /// 按资源类型过滤
    pub resource_type: Option<String>,
    /// 按资源 ID 过滤
    pub resource_id: Option<String>,
    /// 按关联 ID 过滤
    pub correlation_id: Option<Uuid>,
    /// 时间范围：起始
    pub from: Option<DateTime<Utc>>,
    /// 时间范围：截止
    pub to: Option<DateTime<Utc>>,
    /// 最大返回条数
    #[serde(default)]
    pub limit: usize,
    /// 偏移量（分页）
    #[serde(default)]
    pub offset: usize,
}

/// 审计日志 sink trait
///
/// 所有审计实现必须实现此 trait。`record` 是核心方法，
/// `record_batch` 提供默认的逐条写入实现，`query` 用于查询审计记录。
///
/// # Examples
///
/// ```rust,no_run
/// use atta_audit::{AuditSink, AuditEntry, AuditFilter, AuditOutcome};
/// use atta_types::{Actor, EntityRef, EventEnvelope};
/// use uuid::Uuid;
///
/// # async fn example(sink: impl AuditSink) -> Result<(), atta_types::AttaError> {
/// let event = EventEnvelope::system_started("desktop").unwrap();
/// let entry = AuditEntry::from_event(&event, "system.start", AuditOutcome::Success);
/// sink.record(&entry).await?;
///
/// let results = sink.query(&AuditFilter::default()).await?;
/// # Ok(())
/// # }
/// ```
#[async_trait::async_trait]
pub trait AuditSink: Send + Sync + 'static {
    /// 记录单条审计条目
    async fn record(&self, entry: &AuditEntry) -> Result<(), AttaError>;

    /// 批量记录审计条目
    ///
    /// 默认实现逐条调用 `record`，实现方可覆盖以优化批量写入。
    async fn record_batch(&self, entries: &[AuditEntry]) -> Result<(), AttaError> {
        for entry in entries {
            self.record(entry).await?;
        }
        Ok(())
    }

    /// 查询审计记录
    ///
    /// 根据过滤条件检索审计条目，返回按时间降序排列的结果。
    async fn query(&self, filter: &AuditFilter) -> Result<Vec<AuditEntry>, AttaError>;
}
