//! Persistent AuditStore implementation
//!
//! Enterprise-grade audit logging backed by SQLite.
//! Uses the `audit_log` table from 001_init.sql.
//! Append-only: no UPDATE or DELETE operations.

use chrono::{DateTime, Utc};
use sqlx::sqlite::SqlitePool;
use sqlx::Row;
use tracing::debug;
use uuid::Uuid;

use atta_types::error::AuditError;
use atta_types::AttaError;

use crate::{AuditEntry, AuditFilter, AuditOutcome, AuditSink};

/// Whitelist of allowed filter column names for audit queries.
///
/// All dynamic WHERE clauses MUST reference only columns in this list.
/// This prevents SQL injection even if the query builder is refactored
/// to accept arbitrary field names in the future.
const ALLOWED_FILTER_FIELDS: &[&str] = &[
    "actor_id",
    "action",
    "resource_type",
    "resource_id",
    "correlation_id",
    "timestamp",
];

/// Persistent audit store backed by SQLite
///
/// Writes audit entries to the `audit_log` table. All writes are append-only;
/// no UPDATE or DELETE operations are performed, ensuring tamper-evidence.
pub struct AuditStore {
    pool: SqlitePool,
}

impl AuditStore {
    /// Create a new AuditStore with the given SQLite connection pool
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl AuditSink for AuditStore {
    async fn record(&self, entry: &AuditEntry) -> Result<(), AttaError> {
        debug!(
            id = %entry.id,
            actor = %entry.actor.id,
            action = %entry.action,
            "recording audit entry"
        );

        let outcome_str = match &entry.outcome {
            AuditOutcome::Success => "success".to_string(),
            AuditOutcome::Denied => "denied".to_string(),
            AuditOutcome::Failed { error } => format!("failed:{error}"),
        };

        let actor_type = format!("{:?}", entry.actor.actor_type).to_lowercase();
        let resource_type = format!("{:?}", entry.resource.entity_type).to_lowercase();
        let resource_id = entry.resource.id.to_string();
        let detail = serde_json::to_string(&entry.detail).unwrap_or_default();

        sqlx::query(
            r#"INSERT INTO audit_log (id, timestamp, actor_type, actor_id, action, resource_type, resource_id, correlation_id, outcome, detail)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"#,
        )
        .bind(entry.id.to_string())
        .bind(entry.timestamp.to_rfc3339())
        .bind(&actor_type)
        .bind(&entry.actor.id)
        .bind(&entry.action)
        .bind(&resource_type)
        .bind(&resource_id)
        .bind(entry.correlation_id.to_string())
        .bind(&outcome_str)
        .bind(&detail)
        .execute(&self.pool)
        .await
        .map_err(|e| AuditError::RecordFailed(e.into()))?;

        Ok(())
    }

    async fn record_batch(&self, entries: &[AuditEntry]) -> Result<(), AttaError> {
        // Use a transaction for batch writes
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| AuditError::RecordFailed(e.into()))?;

        for entry in entries {
            let outcome_str = match &entry.outcome {
                AuditOutcome::Success => "success".to_string(),
                AuditOutcome::Denied => "denied".to_string(),
                AuditOutcome::Failed { error } => format!("failed:{error}"),
            };

            let actor_type = format!("{:?}", entry.actor.actor_type).to_lowercase();
            let resource_type = format!("{:?}", entry.resource.entity_type).to_lowercase();
            let resource_id = entry.resource.id.to_string();
            let detail = serde_json::to_string(&entry.detail).unwrap_or_default();

            sqlx::query(
                r#"INSERT INTO audit_log (id, timestamp, actor_type, actor_id, action, resource_type, resource_id, correlation_id, outcome, detail)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"#,
            )
            .bind(entry.id.to_string())
            .bind(entry.timestamp.to_rfc3339())
            .bind(&actor_type)
            .bind(&entry.actor.id)
            .bind(&entry.action)
            .bind(&resource_type)
            .bind(&resource_id)
            .bind(entry.correlation_id.to_string())
            .bind(&outcome_str)
            .bind(&detail)
            .execute(&mut *tx)
            .await
            .map_err(|e| AuditError::RecordFailed(e.into()))?;
        }

        tx.commit()
            .await
            .map_err(|e| AuditError::RecordFailed(e.into()))?;

        Ok(())
    }

    async fn query(&self, filter: &AuditFilter) -> Result<Vec<AuditEntry>, AttaError> {
        debug!(filter = ?filter, "querying audit log");

        /// Helper to validate a column name against the whitelist before
        /// including it in a dynamic WHERE clause.
        fn validated_field(field: &str) -> Result<&str, AttaError> {
            if ALLOWED_FILTER_FIELDS.contains(&field) {
                Ok(field)
            } else {
                Err(AttaError::Validation(format!(
                    "invalid audit filter field: {}",
                    field
                )))
            }
        }

        let mut conditions = Vec::new();
        let mut bind_values: Vec<String> = Vec::new();

        if let Some(ref actor_id) = filter.actor_id {
            let field = validated_field("actor_id")?;
            bind_values.push(actor_id.clone());
            conditions.push(format!("{} = ?{}", field, bind_values.len()));
        }
        if let Some(ref action) = filter.action {
            let field = validated_field("action")?;
            bind_values.push(action.clone());
            conditions.push(format!("{} = ?{}", field, bind_values.len()));
        }
        if let Some(ref resource_type) = filter.resource_type {
            let field = validated_field("resource_type")?;
            bind_values.push(resource_type.clone());
            conditions.push(format!("{} = ?{}", field, bind_values.len()));
        }
        if let Some(ref resource_id) = filter.resource_id {
            let field = validated_field("resource_id")?;
            bind_values.push(resource_id.clone());
            conditions.push(format!("{} = ?{}", field, bind_values.len()));
        }
        if let Some(ref correlation_id) = filter.correlation_id {
            let field = validated_field("correlation_id")?;
            bind_values.push(correlation_id.to_string());
            conditions.push(format!("{} = ?{}", field, bind_values.len()));
        }
        if let Some(ref from) = filter.from {
            let field = validated_field("timestamp")?;
            bind_values.push(from.to_rfc3339());
            conditions.push(format!("{} >= ?{}", field, bind_values.len()));
        }
        if let Some(ref to) = filter.to {
            let field = validated_field("timestamp")?;
            bind_values.push(to.to_rfc3339());
            conditions.push(format!("{} <= ?{}", field, bind_values.len()));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let limit = if filter.limit > 0 { filter.limit } else { 1000 };

        let sql = format!(
            "SELECT id, timestamp, actor_type, actor_id, action, resource_type, resource_id, correlation_id, outcome, detail FROM audit_log {} ORDER BY timestamp DESC LIMIT {} OFFSET {}",
            where_clause, limit, filter.offset
        );

        let mut query = sqlx::query(&sql);
        for val in &bind_values {
            query = query.bind(val);
        }

        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AuditError::QueryFailed(e.into()))?;

        let mut entries = Vec::with_capacity(rows.len());
        for row in rows {
            let id_str: String = row.get("id");
            let timestamp_str: String = row.get("timestamp");
            let actor_type_str: String = row.get("actor_type");
            let actor_id: String = row.get("actor_id");
            let action: String = row.get("action");
            let resource_type_str: String = row.get("resource_type");
            let resource_id: String = row.get("resource_id");
            let correlation_id_str: String = row.get("correlation_id");
            let outcome_str: String = row.get("outcome");
            let detail_str: String = row.get("detail");

            let id = Uuid::parse_str(&id_str).unwrap_or_default();
            let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let correlation_id = Uuid::parse_str(&correlation_id_str).unwrap_or_default();

            let actor_type = match actor_type_str.as_str() {
                "system" => atta_types::ActorType::System,
                "service" => atta_types::ActorType::Service,
                _ => atta_types::ActorType::User,
            };

            let actor = atta_types::Actor {
                actor_type,
                id: actor_id,
            };

            let entity_type = match resource_type_str.as_str() {
                "task" => atta_types::ResourceType::Task,
                "flow" => atta_types::ResourceType::Flow,
                "skill" => atta_types::ResourceType::Skill,
                "tool" => atta_types::ResourceType::Tool,
                "node" => atta_types::ResourceType::Node,
                "secret" => atta_types::ResourceType::Secret,
                "audit_log" => atta_types::ResourceType::AuditLog,
                "approval" => atta_types::ResourceType::Approval,
                "package" => atta_types::ResourceType::Package,
                "mcp" => atta_types::ResourceType::Mcp,
                _ => atta_types::ResourceType::Task, // fallback
            };

            let entity = atta_types::event::EntityRef {
                entity_type,
                id: resource_id,
            };

            let outcome = if outcome_str == "success" {
                AuditOutcome::Success
            } else if outcome_str == "denied" {
                AuditOutcome::Denied
            } else if let Some(error) = outcome_str.strip_prefix("failed:") {
                AuditOutcome::Failed {
                    error: error.to_string(),
                }
            } else {
                AuditOutcome::Failed { error: outcome_str }
            };

            let detail: serde_json::Value =
                serde_json::from_str(&detail_str).unwrap_or(serde_json::Value::Null);

            entries.push(AuditEntry {
                id,
                timestamp,
                actor,
                action,
                resource: entity,
                correlation_id,
                outcome,
                detail,
            });
        }

        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn setup_db() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();

        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS audit_log (
                id              TEXT PRIMARY KEY,
                timestamp       TEXT NOT NULL,
                actor_type      TEXT NOT NULL,
                actor_id        TEXT NOT NULL,
                action          TEXT NOT NULL,
                resource_type   TEXT NOT NULL,
                resource_id     TEXT,
                correlation_id  TEXT NOT NULL,
                outcome         TEXT NOT NULL,
                detail          TEXT NOT NULL DEFAULT '{}',
                client_ip       TEXT,
                user_agent      TEXT
            )"#,
        )
        .execute(&pool)
        .await
        .unwrap();

        pool
    }

    fn make_entry(action: &str) -> AuditEntry {
        AuditEntry {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            actor: atta_types::Actor::user("test-user"),
            action: action.to_string(),
            resource: atta_types::event::EntityRef {
                entity_type: atta_types::ResourceType::Task,
                id: Uuid::new_v4().to_string(),
            },
            correlation_id: Uuid::new_v4(),
            outcome: AuditOutcome::Success,
            detail: serde_json::json!({"key": "value"}),
        }
    }

    #[tokio::test]
    async fn test_record_and_query() {
        let pool = setup_db().await;
        let store = AuditStore::new(pool);

        let entry = make_entry("task.create");
        store.record(&entry).await.unwrap();

        let results = store.query(&AuditFilter::default()).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].action, "task.create");
    }

    #[tokio::test]
    async fn test_record_batch() {
        let pool = setup_db().await;
        let store = AuditStore::new(pool);

        let entries = vec![
            make_entry("task.create"),
            make_entry("task.update"),
            make_entry("flow.advance"),
        ];

        store.record_batch(&entries).await.unwrap();

        let results = store.query(&AuditFilter::default()).await.unwrap();
        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn test_query_filter_by_action() {
        let pool = setup_db().await;
        let store = AuditStore::new(pool);

        store.record(&make_entry("task.create")).await.unwrap();
        store.record(&make_entry("task.update")).await.unwrap();
        store.record(&make_entry("flow.advance")).await.unwrap();

        let filter = AuditFilter {
            action: Some("task.create".to_string()),
            ..Default::default()
        };
        let results = store.query(&filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].action, "task.create");
    }

    #[tokio::test]
    async fn test_query_filter_by_actor() {
        let pool = setup_db().await;
        let store = AuditStore::new(pool);

        store.record(&make_entry("task.create")).await.unwrap();

        let filter = AuditFilter {
            actor_id: Some("test-user".to_string()),
            ..Default::default()
        };
        let results = store.query(&filter).await.unwrap();
        assert_eq!(results.len(), 1);

        let filter = AuditFilter {
            actor_id: Some("nonexistent".to_string()),
            ..Default::default()
        };
        let results = store.query(&filter).await.unwrap();
        assert_eq!(results.len(), 0);
    }

    fn make_entry_at(action: &str, timestamp: DateTime<Utc>) -> AuditEntry {
        AuditEntry {
            id: Uuid::new_v4(),
            timestamp,
            actor: atta_types::Actor::user("test-user"),
            action: action.to_string(),
            resource: atta_types::event::EntityRef {
                entity_type: atta_types::ResourceType::Task,
                id: Uuid::new_v4().to_string(),
            },
            correlation_id: Uuid::new_v4(),
            outcome: AuditOutcome::Success,
            detail: serde_json::json!({"key": "value"}),
        }
    }

    #[tokio::test]
    async fn test_query_filter_by_correlation_id() {
        let pool = setup_db().await;
        let store = AuditStore::new(pool);

        let target_correlation_id = Uuid::new_v4();

        let mut entry1 = make_entry("task.create");
        entry1.correlation_id = target_correlation_id;

        let entry2 = make_entry("task.update");
        let entry3 = make_entry("flow.advance");

        store.record(&entry1).await.unwrap();
        store.record(&entry2).await.unwrap();
        store.record(&entry3).await.unwrap();

        let filter = AuditFilter {
            correlation_id: Some(target_correlation_id),
            ..Default::default()
        };
        let results = store.query(&filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].correlation_id, target_correlation_id);
    }

    #[tokio::test]
    async fn test_query_filter_by_time_range() {
        let pool = setup_db().await;
        let store = AuditStore::new(pool);

        let base = Utc::now();
        let t_minus_10 = base - chrono::Duration::seconds(10);
        let t_base = base;
        let t_plus_10 = base + chrono::Duration::seconds(10);

        let entry_old = make_entry_at("task.create", t_minus_10);
        let entry_mid = make_entry_at("task.update", t_base);
        let entry_new = make_entry_at("flow.advance", t_plus_10);

        store.record(&entry_old).await.unwrap();
        store.record(&entry_mid).await.unwrap();
        store.record(&entry_new).await.unwrap();

        // Filter to only include the middle entry
        let from = t_minus_10 + chrono::Duration::seconds(5);
        let to = t_plus_10 - chrono::Duration::seconds(5);

        let filter = AuditFilter {
            from: Some(from),
            to: Some(to),
            ..Default::default()
        };
        let results = store.query(&filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].action, "task.update");
    }

    #[tokio::test]
    async fn test_query_pagination_limit() {
        let pool = setup_db().await;
        let store = AuditStore::new(pool);

        for i in 0..5 {
            store
                .record(&make_entry(&format!("action.{i}")))
                .await
                .unwrap();
        }

        let filter = AuditFilter {
            limit: 2,
            ..Default::default()
        };
        let results = store.query(&filter).await.unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_query_pagination_offset() {
        let pool = setup_db().await;
        let store = AuditStore::new(pool);

        // Insert 5 entries with distinct, well-separated timestamps so DESC ordering is stable
        let base = Utc::now();
        for i in 0..5u64 {
            let ts = base + chrono::Duration::seconds(i as i64);
            let entry = make_entry_at(&format!("action.{i}"), ts);
            store.record(&entry).await.unwrap();
        }

        // With ORDER BY timestamp DESC the order is action.4, action.3, action.2, action.1, action.0
        // limit=2, offset=2 should return action.2 and action.1
        let filter = AuditFilter {
            limit: 2,
            offset: 2,
            ..Default::default()
        };
        let results = store.query(&filter).await.unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].action, "action.2");
        assert_eq!(results[1].action, "action.1");
    }

    #[tokio::test]
    async fn test_denied_outcome_roundtrip() {
        let pool = setup_db().await;
        let store = AuditStore::new(pool);

        let mut entry = make_entry("task.delete");
        entry.outcome = AuditOutcome::Denied;
        store.record(&entry).await.unwrap();

        let results = store.query(&AuditFilter::default()).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].outcome, AuditOutcome::Denied));
    }

    #[tokio::test]
    async fn test_failed_outcome_roundtrip() {
        let pool = setup_db().await;
        let store = AuditStore::new(pool);

        let mut entry = make_entry("task.execute");
        entry.outcome = AuditOutcome::Failed {
            error: "something broke".to_string(),
        };
        store.record(&entry).await.unwrap();

        let results = store.query(&AuditFilter::default()).await.unwrap();
        assert_eq!(results.len(), 1);
        match &results[0].outcome {
            AuditOutcome::Failed { error } => assert_eq!(error, "something broke"),
            other => panic!("expected Failed outcome, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_query_returns_desc_order() {
        let pool = setup_db().await;
        let store = AuditStore::new(pool);

        let base = Utc::now();
        let entry_first = make_entry_at("action.first", base);
        let entry_second = make_entry_at("action.second", base + chrono::Duration::seconds(1));
        let entry_third = make_entry_at("action.third", base + chrono::Duration::seconds(2));

        // Insert in chronological order
        store.record(&entry_first).await.unwrap();
        store.record(&entry_second).await.unwrap();
        store.record(&entry_third).await.unwrap();

        let results = store.query(&AuditFilter::default()).await.unwrap();
        assert_eq!(results.len(), 3);
        // Newest first
        assert_eq!(results[0].action, "action.third");
        assert_eq!(results[1].action, "action.second");
        assert_eq!(results[2].action, "action.first");
    }

    #[tokio::test]
    async fn test_query_combined_filters() {
        let pool = setup_db().await;
        let store = AuditStore::new(pool);

        // actor "alice" doing task.create
        let mut entry_alice_create = make_entry("task.create");
        entry_alice_create.actor = atta_types::Actor::user("alice");
        store.record(&entry_alice_create).await.unwrap();

        // actor "alice" doing task.update
        let mut entry_alice_update = make_entry("task.update");
        entry_alice_update.actor = atta_types::Actor::user("alice");
        store.record(&entry_alice_update).await.unwrap();

        // actor "bob" doing task.create
        let mut entry_bob_create = make_entry("task.create");
        entry_bob_create.actor = atta_types::Actor::user("bob");
        store.record(&entry_bob_create).await.unwrap();

        // Filter: action=task.create AND actor_id=alice — should return exactly 1
        let filter = AuditFilter {
            action: Some("task.create".to_string()),
            actor_id: Some("alice".to_string()),
            ..Default::default()
        };
        let results = store.query(&filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].action, "task.create");
        assert_eq!(results[0].actor.id, "alice");
    }

    #[tokio::test]
    async fn test_record_batch_empty() {
        let pool = setup_db().await;
        let store = AuditStore::new(pool);

        // Batch with zero entries should succeed without error
        store.record_batch(&[]).await.unwrap();

        let results = store.query(&AuditFilter::default()).await.unwrap();
        assert_eq!(results.len(), 0);
    }

    #[tokio::test]
    async fn test_query_filter_by_resource_type() {
        let pool = setup_db().await;
        let store = AuditStore::new(pool);

        // make_entry uses ResourceType::Task which serialises to "task"
        store.record(&make_entry("task.create")).await.unwrap();
        store.record(&make_entry("task.update")).await.unwrap();

        // A third entry with a manually overridden resource_type stored as "flow"
        let mut entry_flow = make_entry("flow.advance");
        entry_flow.resource.entity_type = atta_types::ResourceType::Flow;
        store.record(&entry_flow).await.unwrap();

        let filter = AuditFilter {
            resource_type: Some("task".to_string()),
            ..Default::default()
        };
        let results = store.query(&filter).await.unwrap();
        assert_eq!(results.len(), 2);
        for r in &results {
            assert!(r.action.starts_with("task."));
        }

        let filter = AuditFilter {
            resource_type: Some("flow".to_string()),
            ..Default::default()
        };
        let results = store.query(&filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].action, "flow.advance");
    }

    #[tokio::test]
    async fn test_resource_type_roundtrip() {
        let pool = setup_db().await;
        let store = AuditStore::new(pool);

        // Insert with Flow resource type
        let mut entry = make_entry("flow.advance");
        entry.resource.entity_type = atta_types::ResourceType::Flow;
        store.record(&entry).await.unwrap();

        // Insert with Skill resource type
        let mut entry2 = make_entry("skill.execute");
        entry2.resource.entity_type = atta_types::ResourceType::Skill;
        store.record(&entry2).await.unwrap();

        let results = store.query(&AuditFilter::default()).await.unwrap();
        assert_eq!(results.len(), 2);

        // Verify the resource types survived the roundtrip (not hardcoded to Task)
        let flow_entry = results.iter().find(|e| e.action == "flow.advance").unwrap();
        assert_eq!(flow_entry.resource.entity_type, atta_types::ResourceType::Flow);

        let skill_entry = results.iter().find(|e| e.action == "skill.execute").unwrap();
        assert_eq!(skill_entry.resource.entity_type, atta_types::ResourceType::Skill);
    }
}
