//! NoopAudit 审计实现
//!
//! Desktop 版默认使用的审计实现：通过 `tracing::debug` 记录，
//! 不持久化，查询始终返回空列表。

use atta_types::AttaError;

use crate::traits::{AuditEntry, AuditFilter, AuditSink};

/// 空操作审计实现
///
/// 适用于 Desktop 单机单用户场景。所有审计条目仅输出 debug 日志，
/// 不做持久化存储。查询始终返回空结果。
pub struct NoopAudit;

impl NoopAudit {
    /// 创建新的 NoopAudit 实例
    pub fn new() -> Self {
        Self
    }
}

impl Default for NoopAudit {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl AuditSink for NoopAudit {
    async fn record(&self, entry: &AuditEntry) -> Result<(), AttaError> {
        tracing::debug!(
            audit_id = %entry.id,
            actor_id = %entry.actor.id,
            actor_type = entry.actor.actor_type.as_str(),
            action = %entry.action,
            resource_type = entry.resource.entity_type.as_str(),
            resource_id = %entry.resource.id,
            correlation_id = %entry.correlation_id,
            outcome = ?entry.outcome,
            "NoopAudit: recording audit entry (not persisted)"
        );
        Ok(())
    }

    async fn record_batch(&self, entries: &[AuditEntry]) -> Result<(), AttaError> {
        tracing::debug!(
            count = entries.len(),
            "NoopAudit: recording batch audit entries (not persisted)"
        );
        for entry in entries {
            self.record(entry).await?;
        }
        Ok(())
    }

    async fn query(&self, filter: &AuditFilter) -> Result<Vec<AuditEntry>, AttaError> {
        tracing::debug!(
            actor_id = ?filter.actor_id,
            action = ?filter.action,
            resource_type = ?filter.resource_type,
            correlation_id = ?filter.correlation_id,
            limit = filter.limit,
            "NoopAudit: query returns empty (no persistent storage)"
        );
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::event::EntityRef;
    use atta_types::{Actor, ActorType, ResourceType};
    use chrono::Utc;
    use uuid::Uuid;

    use crate::traits::AuditOutcome;

    fn make_entry() -> AuditEntry {
        AuditEntry {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            actor: Actor {
                actor_type: ActorType::User,
                id: "test-user".to_string(),
            },
            action: "task.create".to_string(),
            resource: EntityRef::new(ResourceType::Task, "task-123"),
            correlation_id: Uuid::new_v4(),
            outcome: AuditOutcome::Success,
            detail: serde_json::json!({"key": "value"}),
        }
    }

    #[tokio::test]
    async fn test_noop_record() {
        let audit = NoopAudit::new();
        let entry = make_entry();
        let result = audit.record(&entry).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_noop_record_batch() {
        let audit = NoopAudit::new();
        let entries = vec![make_entry(), make_entry(), make_entry()];
        let result = audit.record_batch(&entries).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_noop_query_returns_empty() {
        let audit = NoopAudit::new();
        let filter = AuditFilter::default();
        let results = audit.query(&filter).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_noop_query_with_filter() {
        let audit = NoopAudit::new();
        let filter = AuditFilter {
            actor_id: Some("test-user".to_string()),
            action: Some("task.create".to_string()),
            resource_type: Some("task".to_string()),
            limit: 10,
            ..Default::default()
        };
        let results = audit.query(&filter).await.unwrap();
        assert!(results.is_empty());
    }
}
