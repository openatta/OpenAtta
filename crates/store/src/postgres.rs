//! PostgreSQL 实现的 StateStore
//!
//! Enterprise 版存储后端。使用 sqlx::PgPool 管理连接池。
//! 与 SqliteStore 实现相同的 StateStore trait。

use chrono::{DateTime, Utc};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use tracing::{debug, info};
use uuid::Uuid;

use atta_types::approval::ApprovalContext;
use atta_types::auth::Role;
use atta_types::error::StoreError;
use atta_types::package::{PackageType, ServiceAccount};
use atta_types::usage::{ModelUsage, UsageDaily, UsageRecord, UsageSummary};
use atta_types::{
    Actor, ApprovalFilter, ApprovalRequest, ApprovalStatus, AttaError, CronJob, CronRun,
    CronRunStatus, FlowDef, FlowState, McpServerConfig, McpTransport, NodeCapacity, NodeInfo,
    NodeStatus, PackageRecord, PluginManifest, SkillDef, StateTransition, Task, TaskFilter,
    TaskStatus, ToolDef,
};

use crate::common::{json_merge, task_status_from_db, task_status_to_db};
use crate::{
    ApprovalStore, CronStore, FlowStore, McpStore, NodeStore, PackageStore, RbacStore,
    RegistryStore, RemoteAgentStore, ServiceAccountStore, StateStore, TaskStore, UsageStore,
};

/// PostgreSQL 状态存储
///
/// Enterprise 版的持久化实现。使用 PostgreSQL 提供强一致性和高并发支持。
pub struct PostgresStore {
    pool: PgPool,
}

impl PostgresStore {
    /// 连接数据库并运行 migrations
    pub async fn connect(url: &str) -> Result<Self, AttaError> {
        info!(url = %url, "connecting to PostgreSQL");

        let pool = PgPoolOptions::new()
            .max_connections(20)
            .connect(url)
            .await
            .map_err(|e| StoreError::Database(format!("failed to connect: {e}")))?;

        // Run incremental migrations
        sqlx::migrate!("../../migrations/postgres")
            .run(&pool)
            .await
            .map_err(|e| StoreError::Database(format!("migration failed: {e}")))?;

        Ok(Self { pool })
    }

    /// Create from an existing PgPool (useful for testing)
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }
}

// ── Helper functions ──

fn task_from_pg_row(row: &sqlx::postgres::PgRow) -> Result<Task, AttaError> {
    let id: String = row
        .try_get("id")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let flow_id: String = row
        .try_get("flow_id")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let current_state: String = row
        .try_get("current_state")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let status_str: String = row
        .try_get("status")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let error_msg: Option<String> = row.try_get("error_message").ok().flatten();
    let state_data_str: String = row
        .try_get("state_data")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let input_str: String = row
        .try_get("input")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let output_str: Option<String> = row.try_get("output").ok().flatten();
    let created_by_str: String = row
        .try_get("created_by")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let created_at: String = row
        .try_get("created_at")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let updated_at: String = row
        .try_get("updated_at")
        .map_err(|e| StoreError::Database(e.to_string()))?;

    let id =
        Uuid::parse_str(&id).map_err(|e| StoreError::Database(format!("invalid UUID: {e}")))?;
    let state_data: serde_json::Value = serde_json::from_str(&state_data_str)
        .map_err(|e| StoreError::Serialization(format!("state_data: {e}")))?;
    let input: serde_json::Value = serde_json::from_str(&input_str)
        .map_err(|e| StoreError::Serialization(format!("input: {e}")))?;
    let output = output_str
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .map_err(|e| StoreError::Serialization(format!("output: {e}")))?;
    let created_at = DateTime::parse_from_rfc3339(&created_at)
        .map_err(|e| StoreError::Database(format!("invalid created_at: {e}")))?
        .with_timezone(&Utc);
    let updated_at = DateTime::parse_from_rfc3339(&updated_at)
        .map_err(|e| StoreError::Database(format!("invalid updated_at: {e}")))?
        .with_timezone(&Utc);
    let created_by: Actor = serde_json::from_str(&created_by_str).unwrap_or_else(|e| {
        tracing::warn!(key = "created_by", error = %e, "failed to parse field, using fallback");
        Actor::user(created_by_str.clone())
    });

    Ok(Task {
        id,
        flow_id,
        current_state,
        state_data,
        input,
        output,
        status: task_status_from_db(&status_str, error_msg),
        created_by,
        created_at,
        updated_at,
        completed_at: {
            let s: Option<String> = row.try_get("completed_at").ok().flatten();
            s.and_then(|v| {
                DateTime::parse_from_rfc3339(&v)
                    .ok()
                    .map(|d| d.with_timezone(&Utc))
            })
        },
        version: row.try_get::<i64, _>("version").unwrap_or_else(|e| {
            tracing::warn!(key = "version", error = %e, "failed to read field, using fallback 0");
            0
        }) as u64,
    })
}

fn tool_from_pg_row(row: &sqlx::postgres::PgRow) -> Result<ToolDef, AttaError> {
    let name: String = row
        .try_get("name")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let description: String = row
        .try_get("description")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let plugin_name: Option<String> = row.try_get("plugin_name").ok().flatten();
    let mcp_server: Option<String> = row.try_get("mcp_server").ok().flatten();
    let risk_str: String = row
        .try_get("risk_level")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let params_str: String = row
        .try_get("parameters")
        .map_err(|e| StoreError::Database(e.to_string()))?;

    let risk_level = match risk_str.as_str() {
        "high" => atta_types::RiskLevel::High,
        "medium" => atta_types::RiskLevel::Medium,
        _ => atta_types::RiskLevel::Low,
    };
    let parameters: serde_json::Value = serde_json::from_str(&params_str).unwrap_or_else(|e| {
        tracing::warn!(key = "parameters", error = %e, "failed to parse field, using fallback");
        serde_json::Value::default()
    });

    let binding = if let Some(ref pn) = plugin_name {
        if let Some(handler) = pn.strip_prefix("builtin:") {
            atta_types::ToolBinding::Builtin {
                handler_name: handler.to_string(),
            }
        } else if let Some(handler) = pn.strip_prefix("native:") {
            atta_types::ToolBinding::Native {
                handler_name: handler.to_string(),
            }
        } else {
            // Legacy plugin tools stored as plain names — treat as native
            atta_types::ToolBinding::Native {
                handler_name: pn.clone(),
            }
        }
    } else if let Some(ref server) = mcp_server {
        atta_types::ToolBinding::Mcp {
            server_name: server.clone(),
        }
    } else {
        atta_types::ToolBinding::Native {
            handler_name: "unknown".to_string(),
        }
    };

    Ok(ToolDef {
        name,
        description,
        binding,
        risk_level,
        parameters,
    })
}

fn node_from_pg_row(row: &sqlx::postgres::PgRow) -> Result<NodeInfo, AttaError> {
    let id: String = row
        .try_get("id")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let hostname: String = row
        .try_get("hostname")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let labels_str: String = row
        .try_get("labels")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let status_str: String = row
        .try_get("status")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let total_memory: i64 = row
        .try_get("total_memory")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let available_memory: i64 = row
        .try_get("available_memory")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let running_agents: i32 = row
        .try_get("running_agents")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let max_concurrent: i32 = row
        .try_get("max_concurrent")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let last_heartbeat_str: String = row
        .try_get("last_heartbeat")
        .map_err(|e| StoreError::Database(e.to_string()))?;

    let labels: Vec<String> = serde_json::from_str(&labels_str).unwrap_or_default();
    let status = match status_str.as_str() {
        "online" => NodeStatus::Online,
        "offline" => NodeStatus::Offline,
        "draining" => NodeStatus::Draining,
        _ => NodeStatus::Online,
    };
    let last_heartbeat = DateTime::parse_from_rfc3339(&last_heartbeat_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    Ok(NodeInfo {
        id,
        hostname,
        labels,
        status,
        capacity: NodeCapacity {
            total_memory: total_memory as u64,
            available_memory: available_memory as u64,
            running_agents: running_agents as usize,
            max_concurrent: max_concurrent as usize,
        },
        last_heartbeat,
    })
}

fn approval_from_pg_row(row: &sqlx::postgres::PgRow) -> Result<ApprovalRequest, AttaError> {
    let id: String = row
        .try_get("id")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let task_id: String = row
        .try_get("task_id")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let requested_by_str: String = row
        .try_get("requested_by")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let approver_role_str: String = row
        .try_get("approver_role")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let status_str: String = row
        .try_get("status")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let context_str: String = row
        .try_get("context")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let resolved_by_str: Option<String> = row.try_get("resolved_by").ok().flatten();
    let resolved_at_str: Option<String> = row.try_get("resolved_at").ok().flatten();
    let timeout_at_str: String = row
        .try_get("timeout_at")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let created_at_str: String = row
        .try_get("created_at")
        .map_err(|e| StoreError::Database(e.to_string()))?;

    let id = Uuid::parse_str(&id).unwrap_or_default();
    let task_id = Uuid::parse_str(&task_id).unwrap_or_default();

    let requested_by: Actor =
        serde_json::from_str(&requested_by_str).unwrap_or_else(|_| Actor::user(requested_by_str));
    let approver_role: Role =
        serde_json::from_str(&format!("\"{}\"", approver_role_str)).unwrap_or(Role::Approver);
    let context: ApprovalContext =
        serde_json::from_str(&context_str).unwrap_or_else(|_| ApprovalContext {
            summary: String::new(),
            diff_summary: None,
            test_results: None,
            risk_assessment: String::new(),
            pending_tools: vec![],
        });
    let resolved_by: Option<Actor> =
        resolved_by_str.map(|s| serde_json::from_str(&s).unwrap_or_else(|_| Actor::user(s)));
    let resolved_at = resolved_at_str.and_then(|s| {
        DateTime::parse_from_rfc3339(&s)
            .ok()
            .map(|d| d.with_timezone(&Utc))
    });
    let timeout_at = DateTime::parse_from_rfc3339(&timeout_at_str)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    let status = match status_str.as_str() {
        "pending" => ApprovalStatus::Pending,
        "approved" => ApprovalStatus::Approved,
        "denied" => ApprovalStatus::Denied,
        "request_changes" => ApprovalStatus::RequestChanges,
        "expired" => ApprovalStatus::Expired,
        _ => ApprovalStatus::Pending,
    };

    // Compute timeout Duration from timeout_at - created_at
    let timeout_duration = (timeout_at - created_at)
        .to_std()
        .unwrap_or(std::time::Duration::from_secs(3600));

    Ok(ApprovalRequest {
        id,
        task_id,
        requested_by,
        approver_role,
        context,
        status,
        created_at,
        resolved_at,
        resolved_by,
        timeout: timeout_duration,
        timeout_at,
    })
}

// ── TaskStore ──

#[async_trait::async_trait]
impl TaskStore for PostgresStore {
    async fn create_task(&self, task: &Task) -> Result<(), AttaError> {
        let (status, error_msg) = task_status_to_db(&task.status);
        let id = task.id.to_string();
        let state_data = serde_json::to_string(&task.state_data)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let input = serde_json::to_string(&task.input)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let output = task
            .output
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let created_by = serde_json::to_string(&task.created_by)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let created_at = task.created_at.to_rfc3339();
        let updated_at = task.updated_at.to_rfc3339();

        sqlx::query(
            "INSERT INTO tasks (id, flow_id, current_state, status, error_message, state_data, input, output, created_by, created_at, updated_at, version)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)"
        )
        .bind(&id)
        .bind(&task.flow_id)
        .bind(&task.current_state)
        .bind(&status)
        .bind(&error_msg)
        .bind(&state_data)
        .bind(&input)
        .bind(&output)
        .bind(&created_by)
        .bind(&created_at)
        .bind(&updated_at)
        .bind(task.version as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("insert task: {e}")))?;

        Ok(())
    }

    async fn get_task(&self, id: &Uuid) -> Result<Option<Task>, AttaError> {
        let id_str = id.to_string();
        let row = sqlx::query("SELECT * FROM tasks WHERE id = $1")
            .bind(&id_str)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("get task: {e}")))?;

        match row {
            Some(row) => Ok(Some(task_from_pg_row(&row)?)),
            None => Ok(None),
        }
    }

    async fn update_task_status(&self, id: &Uuid, status: TaskStatus) -> Result<(), AttaError> {
        let (status_str, error_msg) = task_status_to_db(&status);
        let id_str = id.to_string();
        let now = Utc::now().to_rfc3339();
        let completed_at = if matches!(status, TaskStatus::Completed) {
            Some(now.clone())
        } else {
            None
        };

        sqlx::query(
            "UPDATE tasks SET status = $1, error_message = $2, updated_at = $3, completed_at = $4 WHERE id = $5"
        )
        .bind(&status_str)
        .bind(&error_msg)
        .bind(&now)
        .bind(&completed_at)
        .bind(&id_str)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("update task status: {e}")))?;

        Ok(())
    }

    async fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, AttaError> {
        const MAX_QUERY_LIMIT: usize = 1000;
        let limit = filter.limit.clamp(1, MAX_QUERY_LIMIT);
        let offset = filter.offset;

        let mut sql = String::from("SELECT * FROM tasks WHERE 1=1");
        let mut args: Vec<String> = Vec::new();

        if let Some(ref status) = filter.status {
            let (s, _) = task_status_to_db(status);
            args.push(s);
            sql.push_str(&format!(" AND status = ${}", args.len()));
        }
        if let Some(ref flow_id) = filter.flow_id {
            args.push(flow_id.clone());
            sql.push_str(&format!(" AND flow_id = ${}", args.len()));
        }
        if let Some(ref created_by) = filter.created_by {
            // Escape LIKE wildcards in user input
            let escaped = created_by
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            args.push(format!("%{escaped}%"));
            sql.push_str(&format!(" AND created_by LIKE ${} ESCAPE '\\'", args.len()));
        }

        let param_offset = args.len();
        sql.push_str(&format!(
            " ORDER BY created_at DESC LIMIT ${} OFFSET ${}",
            param_offset + 1,
            param_offset + 2
        ));

        let mut query = sqlx::query(&sql);
        for arg in &args {
            query = query.bind(arg);
        }
        query = query.bind(limit as i64);
        query = query.bind(offset as i64);

        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("list tasks: {e}")))?;

        let mut tasks = Vec::with_capacity(rows.len());
        for row in &rows {
            tasks.push(task_from_pg_row(row)?);
        }
        Ok(tasks)
    }

    async fn merge_task_state_data(
        &self,
        task_id: &Uuid,
        data: serde_json::Value,
    ) -> Result<(), AttaError> {
        let id_str = task_id.to_string();

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StoreError::Database(format!("begin tx: {e}")))?;

        let row = sqlx::query("SELECT state_data FROM tasks WHERE id = $1")
            .bind(&id_str)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let current_data = match row {
            Some(row) => {
                let data_str: String = row.get("state_data");
                serde_json::from_str::<serde_json::Value>(&data_str).unwrap_or_default()
            }
            None => {
                return Err(StoreError::NotFound {
                    entity_type: "Task".to_string(),
                    id: id_str,
                }
                .into());
            }
        };

        let merged = json_merge(current_data, data);
        let merged_str =
            serde_json::to_string(&merged).map_err(|e| StoreError::Serialization(e.to_string()))?;
        let now = Utc::now().to_rfc3339();

        sqlx::query("UPDATE tasks SET state_data = $1, updated_at = $2 WHERE id = $3")
            .bind(&merged_str)
            .bind(&now)
            .bind(&id_str)
            .execute(&mut *tx)
            .await
            .map_err(|e| StoreError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| StoreError::Database(format!("commit tx: {e}")))?;

        Ok(())
    }
}

// ── FlowStore ──

#[async_trait::async_trait]
impl FlowStore for PostgresStore {
    async fn save_flow_def(&self, flow: &FlowDef) -> Result<(), AttaError> {
        let definition =
            serde_json::to_string(flow).map_err(|e| StoreError::Serialization(e.to_string()))?;
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO flow_defs (id, version, name, description, definition, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (id) DO UPDATE SET version = $2, name = $3, description = $4, definition = $5, updated_at = $7"
        )
        .bind(&flow.id)
        .bind(&flow.version)
        .bind(flow.name.as_deref())
        .bind(flow.description.as_deref())
        .bind(&definition)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("save flow def: {e}")))?;

        Ok(())
    }

    async fn get_flow_def(&self, id: &str) -> Result<Option<FlowDef>, AttaError> {
        let row = sqlx::query("SELECT definition FROM flow_defs WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StoreError::Database(e.to_string()))?;

        match row {
            Some(row) => {
                let def_str: String = row.get("definition");
                let flow: FlowDef = serde_json::from_str(&def_str)
                    .map_err(|e| StoreError::Serialization(e.to_string()))?;
                Ok(Some(flow))
            }
            None => Ok(None),
        }
    }

    async fn save_flow_state(&self, task_id: &Uuid, state: &FlowState) -> Result<(), AttaError> {
        let task_id_str = task_id.to_string();
        let history = serde_json::to_string(&state.history)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let pending_approval = state
            .pending_approval
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO flow_states (task_id, current_state, history, retry_count, updated_at)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (task_id) DO UPDATE SET current_state = $2, history = $3, retry_count = $4, updated_at = $5"
        )
        .bind(&task_id_str)
        .bind(&state.current_state)
        .bind(&history)
        .bind(state.retry_count as i32)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(e.to_string()))?;

        let _ = pending_approval; // stored in history for now

        Ok(())
    }

    async fn get_flow_state(&self, task_id: &Uuid) -> Result<Option<FlowState>, AttaError> {
        let task_id_str = task_id.to_string();
        let row = sqlx::query(
            "SELECT current_state, history, retry_count FROM flow_states WHERE task_id = $1",
        )
        .bind(&task_id_str)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StoreError::Database(e.to_string()))?;

        match row {
            Some(row) => {
                let current_state: String = row.get("current_state");
                let history_str: String = row.get("history");
                let retry_count: i32 = row.get("retry_count");

                let history: Vec<StateTransition> = serde_json::from_str(&history_str)
                    .map_err(|e| StoreError::Serialization(e.to_string()))?;

                Ok(Some(FlowState {
                    task_id: *task_id,
                    current_state,
                    history,
                    pending_approval: None,
                    retry_count: retry_count as u32,
                }))
            }
            None => Ok(None),
        }
    }

    async fn list_flow_defs(&self) -> Result<Vec<FlowDef>, AttaError> {
        let rows = sqlx::query("SELECT definition FROM flow_defs ORDER BY id")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let mut flows = Vec::new();
        for row in rows {
            let def_str: String = row.get("definition");
            let flow: FlowDef = serde_json::from_str(&def_str)
                .map_err(|e| StoreError::Serialization(e.to_string()))?;
            flows.push(flow);
        }
        Ok(flows)
    }

    async fn list_skill_defs(&self) -> Result<Vec<SkillDef>, AttaError> {
        RegistryStore::list_skills(self).await
    }

    async fn create_task_with_flow(
        &self,
        task: &Task,
        flow_state: &FlowState,
    ) -> Result<(), AttaError> {
        // Verify flow definition exists
        let flow_exists = sqlx::query("SELECT 1 FROM flow_defs WHERE id = $1")
            .bind(&task.flow_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("check flow exists: {e}")))?;
        if flow_exists.is_none() {
            return Err(AttaError::FlowNotFound(task.flow_id.clone()));
        }

        TaskStore::create_task(self, task).await?;
        self.save_flow_state(&task.id, flow_state).await?;
        Ok(())
    }

    async fn advance_task(
        &self,
        task_id: &Uuid,
        new_status: TaskStatus,
        new_state: &str,
        transition: &StateTransition,
        expected_version: u64,
    ) -> Result<(), AttaError> {
        let id_str = task_id.to_string();
        let now = Utc::now().to_rfc3339();
        let new_version = expected_version + 1;
        let (status_str, error_msg) = task_status_to_db(&new_status);

        let mut tx = self.pool.begin().await.map_err(|e| StoreError::Database(format!("begin tx: {e}")))?;

        // Optimistic version check
        let result = sqlx::query(
            "UPDATE tasks SET status = $1, error_message = $2, current_state = $3, updated_at = $4, version = $5 WHERE id = $6 AND version = $7"
        )
        .bind(&status_str)
        .bind(&error_msg)
        .bind(new_state)
        .bind(&now)
        .bind(new_version as i64)
        .bind(&id_str)
        .bind(expected_version as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| StoreError::Database(e.to_string()))?;

        if result.rows_affected() == 0 {
            // Query actual version before rollback
            let actual_row = sqlx::query("SELECT version FROM tasks WHERE id = $1")
                .bind(&id_str)
                .fetch_optional(&mut *tx)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!("failed to query actual version during conflict: {e}");
                    None
                });
            let actual_version = actual_row
                .map(|r| r.get::<i64, _>("version") as u64)
                .unwrap_or(expected_version);

            if let Err(e) = tx.rollback().await {
                tracing::warn!(error = %e, "transaction rollback failed");
            }
            return Err(AttaError::Conflict {
                entity_type: "task".to_string(),
                id: id_str,
                expected: expected_version,
                actual: actual_version,
            });
        }

        // Inline get_flow_state within transaction
        let flow_row = sqlx::query(
            "SELECT current_state, history, retry_count FROM flow_states WHERE task_id = $1",
        )
        .bind(&id_str)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| StoreError::Database(e.to_string()))?;

        let mut flow_state = match flow_row {
            Some(row) => {
                let current_state: String = row.get("current_state");
                let history_str: String = row.get("history");
                let retry_count: i32 = row.get("retry_count");

                let history: Vec<StateTransition> = serde_json::from_str(&history_str)
                    .map_err(|e| StoreError::Serialization(e.to_string()))?;

                FlowState {
                    task_id: *task_id,
                    current_state,
                    history,
                    pending_approval: None,
                    retry_count: retry_count as u32,
                }
            }
            None => {
                if let Err(e) = tx.rollback().await {
                    tracing::warn!(error = %e, "transaction rollback failed");
                }
                return Err(StoreError::NotFound {
                    entity_type: "flow_state".to_string(),
                    id: id_str.clone(),
                }
                .into());
            }
        };

        flow_state.current_state = new_state.to_string();
        flow_state.history.push(transition.clone());

        // Inline save_flow_state within transaction
        let history = serde_json::to_string(&flow_state.history)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let save_now = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO flow_states (task_id, current_state, history, retry_count, updated_at)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (task_id) DO UPDATE SET current_state = $2, history = $3, retry_count = $4, updated_at = $5"
        )
        .bind(&id_str)
        .bind(&flow_state.current_state)
        .bind(&history)
        .bind(flow_state.retry_count as i32)
        .bind(&save_now)
        .execute(&mut *tx)
        .await
        .map_err(|e| StoreError::Database(e.to_string()))?;

        tx.commit().await.map_err(|e| StoreError::Database(format!("commit tx: {e}")))?;

        Ok(())
    }

    async fn delete_flow_def(&self, id: &str) -> Result<(), AttaError> {
        debug!(flow_id = %id, "deleting flow definition");

        let result = sqlx::query("DELETE FROM flow_defs WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("delete flow def: {e}")))?;

        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound {
                entity_type: "flow_def".to_string(),
                id: id.to_string(),
            }
            .into());
        }

        Ok(())
    }
}

// ── RegistryStore ──

#[async_trait::async_trait]
impl RegistryStore for PostgresStore {
    async fn register_plugin(&self, manifest: &PluginManifest) -> Result<(), AttaError> {
        let manifest_json = serde_json::to_string(manifest)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let permissions = serde_json::to_string(&manifest.permissions)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO plugins (name, version, description, author, organization, permissions, manifest, installed_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (name) DO UPDATE SET version = $2, description = $3, manifest = $7, updated_at = $9"
        )
        .bind(&manifest.name)
        .bind(&manifest.version)
        .bind(manifest.description.as_deref())
        .bind(manifest.author.as_deref())
        .bind(manifest.organization.as_deref())
        .bind(&permissions)
        .bind(&manifest_json)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(e.to_string()))?;

        Ok(())
    }

    async fn unregister_plugin(&self, name: &str) -> Result<(), AttaError> {
        sqlx::query("DELETE FROM plugins WHERE name = $1")
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Database(e.to_string()))?;
        Ok(())
    }

    async fn list_plugins(&self) -> Result<Vec<PluginManifest>, AttaError> {
        let rows = sqlx::query("SELECT manifest FROM plugins ORDER BY name")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let mut manifests = Vec::new();
        for row in rows {
            let manifest_str: String = row.get("manifest");
            let manifest: PluginManifest = serde_json::from_str(&manifest_str)
                .map_err(|e| StoreError::Serialization(e.to_string()))?;
            manifests.push(manifest);
        }
        Ok(manifests)
    }

    async fn register_tool(&self, tool: &ToolDef) -> Result<(), AttaError> {
        let params = serde_json::to_string(&tool.parameters)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let risk = format!("{:?}", tool.risk_level).to_lowercase();
        let now = Utc::now().to_rfc3339();

        let (plugin_name, mcp_server) = match &tool.binding {
            atta_types::ToolBinding::Mcp { server_name } => (None, Some(server_name.clone())),
            atta_types::ToolBinding::Builtin { handler_name } => {
                (Some(format!("builtin:{handler_name}")), None)
            }
            atta_types::ToolBinding::Native { handler_name } => {
                (Some(format!("native:{handler_name}")), None)
            }
        };

        sqlx::query(
            "INSERT INTO tool_defs (name, description, plugin_name, mcp_server, risk_level, parameters, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (name) DO UPDATE SET description = $2, plugin_name = $3, mcp_server = $4, risk_level = $5, parameters = $6"
        )
        .bind(&tool.name)
        .bind(&tool.description)
        .bind(plugin_name)
        .bind(mcp_server)
        .bind(&risk)
        .bind(&params)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(e.to_string()))?;

        Ok(())
    }

    async fn list_tools(&self) -> Result<Vec<ToolDef>, AttaError> {
        let rows = sqlx::query(
            "SELECT name, description, plugin_name, mcp_server, risk_level, parameters FROM tool_defs WHERE enabled = true ORDER BY name"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StoreError::Database(e.to_string()))?;

        let mut tools = Vec::new();
        for row in rows {
            tools.push(tool_from_pg_row(&row)?);
        }
        Ok(tools)
    }

    async fn register_skill(&self, skill: &SkillDef) -> Result<(), AttaError> {
        let definition =
            serde_json::to_string(skill).map_err(|e| StoreError::Serialization(e.to_string()))?;
        let tags = serde_json::to_string(&skill.tags)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let risk = format!("{:?}", skill.risk_level).to_lowercase();
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO skill_defs (id, version, name, definition, risk_level, tags, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (id) DO UPDATE SET version = $2, name = $3, definition = $4, risk_level = $5, tags = $6, updated_at = $8"
        )
        .bind(&skill.id)
        .bind(&skill.version)
        .bind(skill.name.as_deref())
        .bind(&definition)
        .bind(&risk)
        .bind(&tags)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(e.to_string()))?;

        Ok(())
    }

    async fn list_skills(&self) -> Result<Vec<SkillDef>, AttaError> {
        let rows = sqlx::query("SELECT definition FROM skill_defs ORDER BY id")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let mut skills = Vec::new();
        for row in rows {
            let def_str: String = row.get("definition");
            let skill: SkillDef = serde_json::from_str(&def_str)
                .map_err(|e| StoreError::Serialization(e.to_string()))?;
            skills.push(skill);
        }
        Ok(skills)
    }

    async fn get_tool(&self, name: &str) -> Result<Option<ToolDef>, AttaError> {
        let row = sqlx::query(
            "SELECT name, description, plugin_name, mcp_server, risk_level, parameters FROM tool_defs WHERE name = $1"
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StoreError::Database(e.to_string()))?;

        match row {
            Some(row) => Ok(Some(tool_from_pg_row(&row)?)),
            None => Ok(None),
        }
    }

    async fn get_skill(&self, id: &str) -> Result<Option<SkillDef>, AttaError> {
        let row = sqlx::query("SELECT definition FROM skill_defs WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StoreError::Database(e.to_string()))?;

        match row {
            Some(row) => {
                let def_str: String = row.get("definition");
                let skill: SkillDef = serde_json::from_str(&def_str)
                    .map_err(|e| StoreError::Serialization(e.to_string()))?;
                Ok(Some(skill))
            }
            None => Ok(None),
        }
    }

    async fn get_plugin(&self, name: &str) -> Result<Option<PluginManifest>, AttaError> {
        let row = sqlx::query("SELECT manifest FROM plugins WHERE name = $1")
            .bind(name)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StoreError::Database(e.to_string()))?;

        match row {
            Some(row) => {
                let manifest_str: String = row.get("manifest");
                let manifest: PluginManifest = serde_json::from_str(&manifest_str)
                    .map_err(|e| StoreError::Serialization(e.to_string()))?;
                Ok(Some(manifest))
            }
            None => Ok(None),
        }
    }

    async fn delete_skill(&self, id: &str) -> Result<(), AttaError> {
        debug!(skill_id = %id, "deleting skill");

        let result = sqlx::query("DELETE FROM skill_defs WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("delete skill: {e}")))?;

        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound {
                entity_type: "skill".to_string(),
                id: id.to_string(),
            }
            .into());
        }

        Ok(())
    }
}

// ── PackageStore ──

#[async_trait::async_trait]
impl PackageStore for PostgresStore {
    async fn register_package(&self, pkg: &PackageRecord) -> Result<(), AttaError> {
        let pkg_type = format!("{:?}", pkg.package_type).to_lowercase();
        let installed_at = pkg.installed_at.to_rfc3339();

        sqlx::query(
            "INSERT INTO packages (name, version, package_type, installed_at, installed_by)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (name, version) DO UPDATE SET installed_at = $4, installed_by = $5",
        )
        .bind(&pkg.name)
        .bind(&pkg.version)
        .bind(&pkg_type)
        .bind(&installed_at)
        .bind(&pkg.installed_by)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(e.to_string()))?;

        Ok(())
    }

    async fn get_package(&self, name: &str) -> Result<Option<PackageRecord>, AttaError> {
        let row = sqlx::query(
            "SELECT name, version, package_type, installed_at, installed_by FROM packages WHERE name = $1 ORDER BY installed_at DESC LIMIT 1"
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StoreError::Database(e.to_string()))?;

        match row {
            Some(row) => {
                let name: String = row.get("name");
                let version: String = row.get("version");
                let pkg_type_str: String = row.get("package_type");
                let installed_at_str: String = row.get("installed_at");
                let installed_by: String = row.get("installed_by");

                let package_type = match pkg_type_str.as_str() {
                    "plugin" => PackageType::Plugin,
                    "flow" => PackageType::Flow,
                    "skill" => PackageType::Skill,
                    "tool" => PackageType::Tool,
                    "mcp" => PackageType::Mcp,
                    _ => PackageType::Plugin,
                };
                let installed_at = DateTime::parse_from_rfc3339(&installed_at_str)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());

                Ok(Some(PackageRecord {
                    name,
                    version,
                    package_type,
                    installed_at,
                    installed_by,
                }))
            }
            None => Ok(None),
        }
    }
}

// ── ServiceAccountStore ──

#[async_trait::async_trait]
impl ServiceAccountStore for PostgresStore {
    async fn get_service_account_by_key(
        &self,
        api_key_hash: &str,
    ) -> Result<Option<ServiceAccount>, AttaError> {
        let row = sqlx::query(
            "SELECT id, name, api_key_hash, roles, created_at, enabled FROM service_accounts WHERE api_key_hash = $1"
        )
        .bind(api_key_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StoreError::Database(e.to_string()))?;

        match row {
            Some(row) => {
                let id_str: String = row.get("id");
                let name: String = row.get("name");
                let api_key_hash: String = row.get("api_key_hash");
                let roles_str: String = row.get("roles");
                let created_at_str: String = row.get("created_at");
                let enabled: bool = row.get("enabled");

                let id = Uuid::parse_str(&id_str).unwrap_or_default();
                let roles: Vec<Role> = serde_json::from_str(&roles_str).unwrap_or_default();
                let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());

                Ok(Some(ServiceAccount {
                    id,
                    name,
                    api_key_hash,
                    roles,
                    created_at,
                    enabled,
                }))
            }
            None => Ok(None),
        }
    }
}

// ── NodeStore ──

#[async_trait::async_trait]
impl NodeStore for PostgresStore {
    async fn upsert_node(&self, node_info: &NodeInfo) -> Result<(), AttaError> {
        let labels = serde_json::to_string(&node_info.labels)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let status = format!("{:?}", node_info.status).to_lowercase();
        let last_heartbeat = node_info.last_heartbeat.to_rfc3339();
        let registered_at = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO nodes (id, hostname, labels, status, total_memory, available_memory, running_agents, max_concurrent, last_heartbeat, registered_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (id) DO UPDATE SET hostname = $2, labels = $3, status = $4, total_memory = $5, available_memory = $6, running_agents = $7, max_concurrent = $8, last_heartbeat = $9"
        )
        .bind(&node_info.id)
        .bind(&node_info.hostname)
        .bind(&labels)
        .bind(&status)
        .bind(node_info.capacity.total_memory as i64)
        .bind(node_info.capacity.available_memory as i64)
        .bind(node_info.capacity.running_agents as i32)
        .bind(node_info.capacity.max_concurrent as i32)
        .bind(&last_heartbeat)
        .bind(&registered_at)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(e.to_string()))?;

        Ok(())
    }

    async fn get_node(&self, node_id: &str) -> Result<Option<NodeInfo>, AttaError> {
        let row = sqlx::query(
            "SELECT id, hostname, labels, status, total_memory, available_memory, running_agents, max_concurrent, last_heartbeat FROM nodes WHERE id = $1"
        )
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StoreError::Database(e.to_string()))?;

        match row {
            Some(row) => Ok(Some(node_from_pg_row(&row)?)),
            None => Ok(None),
        }
    }

    async fn list_nodes(&self) -> Result<Vec<NodeInfo>, AttaError> {
        let rows = sqlx::query(
            "SELECT id, hostname, labels, status, total_memory, available_memory, running_agents, max_concurrent, last_heartbeat FROM nodes ORDER BY id"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StoreError::Database(e.to_string()))?;

        let mut nodes = Vec::new();
        for row in rows {
            nodes.push(node_from_pg_row(&row)?);
        }
        Ok(nodes)
    }

    async fn list_nodes_after(&self, cutoff: DateTime<Utc>) -> Result<Vec<NodeInfo>, AttaError> {
        let cutoff_str = cutoff.to_rfc3339();
        let rows = sqlx::query(
            "SELECT id, hostname, labels, status, total_memory, available_memory, running_agents, max_concurrent, last_heartbeat FROM nodes WHERE last_heartbeat > $1 ORDER BY id"
        )
        .bind(&cutoff_str)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StoreError::Database(e.to_string()))?;

        let mut nodes = Vec::new();
        for row in rows {
            nodes.push(node_from_pg_row(&row)?);
        }
        Ok(nodes)
    }

    async fn update_node_status(&self, node_id: &str, status: NodeStatus) -> Result<(), AttaError> {
        let status_str = format!("{:?}", status).to_lowercase();

        sqlx::query("UPDATE nodes SET status = $1 WHERE id = $2")
            .bind(&status_str)
            .bind(node_id)
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Database(e.to_string()))?;

        Ok(())
    }
}

// ── ApprovalStore ──

#[async_trait::async_trait]
impl ApprovalStore for PostgresStore {
    async fn save_approval(&self, approval: &ApprovalRequest) -> Result<(), AttaError> {
        let context = serde_json::to_string(&approval.context)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let status = format!("{:?}", approval.status).to_lowercase();
        let requested_by = serde_json::to_string(&approval.requested_by)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let approver_role = serde_json::to_string(&approval.approver_role)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;

        sqlx::query(
            "INSERT INTO approvals (id, task_id, requested_by, approver_role, status, context, timeout_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (id) DO UPDATE SET status = $5"
        )
        .bind(approval.id.to_string())
        .bind(approval.task_id.to_string())
        .bind(&requested_by)
        .bind(&approver_role)
        .bind(&status)
        .bind(&context)
        .bind(approval.timeout_at.to_rfc3339())
        .bind(approval.created_at.to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(e.to_string()))?;

        Ok(())
    }

    async fn get_approval(&self, id: &Uuid) -> Result<Option<ApprovalRequest>, AttaError> {
        let row = sqlx::query(
            "SELECT id, task_id, requested_by, approver_role, status, context, resolved_by, resolved_at, timeout_at, created_at FROM approvals WHERE id = $1"
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StoreError::Database(e.to_string()))?;

        match row {
            Some(row) => Ok(Some(approval_from_pg_row(&row)?)),
            None => Ok(None),
        }
    }

    async fn list_approvals(
        &self,
        filter: &ApprovalFilter,
    ) -> Result<Vec<ApprovalRequest>, AttaError> {
        let mut sql = String::from(
            "SELECT id, task_id, requested_by, approver_role, status, context, resolved_by, resolved_at, timeout_at, created_at FROM approvals WHERE 1=1"
        );
        let mut args: Vec<String> = Vec::new();

        if let Some(ref status) = filter.status {
            let s = format!("{:?}", status).to_lowercase();
            args.push(s);
            sql.push_str(&format!(" AND status = ${}", args.len()));
        }
        if let Some(ref task_id) = filter.task_id {
            args.push(task_id.to_string());
            sql.push_str(&format!(" AND task_id = ${}", args.len()));
        }

        sql.push_str(" ORDER BY created_at DESC");

        let mut query = sqlx::query(&sql);
        for arg in &args {
            query = query.bind(arg);
        }

        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let mut approvals = Vec::new();
        for row in rows {
            approvals.push(approval_from_pg_row(&row)?);
        }
        Ok(approvals)
    }

    async fn update_approval_status(
        &self,
        id: &Uuid,
        status: ApprovalStatus,
        resolved_by: &Actor,
        comment: Option<&str>,
    ) -> Result<(), AttaError> {
        let status_str = format!("{:?}", status).to_lowercase();
        let now = Utc::now().to_rfc3339();
        let resolved_by_str = serde_json::to_string(resolved_by)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;

        sqlx::query(
            "UPDATE approvals SET status = $1, resolved_by = $2, resolved_at = $3, comment = $4 WHERE id = $5",
        )
        .bind(&status_str)
        .bind(&resolved_by_str)
        .bind(&now)
        .bind(comment)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(e.to_string()))?;

        Ok(())
    }
}

// ── McpStore ──

#[async_trait::async_trait]
impl McpStore for PostgresStore {
    async fn register_mcp(&self, mcp_def: &McpServerConfig) -> Result<(), AttaError> {
        let args = serde_json::to_string(&mcp_def.args)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let transport = match mcp_def.transport {
            McpTransport::Stdio => "stdio",
            McpTransport::Sse => "sse",
        };
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO mcp_servers (name, description, transport, url, command, args, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (name) DO UPDATE SET description = $2, transport = $3, url = $4, command = $5, args = $6"
        )
        .bind(&mcp_def.name)
        .bind(mcp_def.description.as_deref())
        .bind(transport)
        .bind(mcp_def.url.as_deref())
        .bind(mcp_def.command.as_deref())
        .bind(&args)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(e.to_string()))?;

        Ok(())
    }

    async fn list_mcp_servers(&self) -> Result<Vec<McpServerConfig>, AttaError> {
        let rows = sqlx::query(
            "SELECT name, description, transport, url, command, args FROM mcp_servers ORDER BY name"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StoreError::Database(e.to_string()))?;

        let mut servers = Vec::new();
        for row in rows {
            let name: String = row.get("name");
            let description: Option<String> = row.try_get("description").ok().flatten();
            let transport_str: String = row.get("transport");
            let url: Option<String> = row.try_get("url").ok().flatten();
            let command: Option<String> = row.try_get("command").ok().flatten();
            let args_str: String = row.get("args");

            let transport = match transport_str.as_str() {
                "sse" => McpTransport::Sse,
                _ => McpTransport::Stdio,
            };
            let args: Vec<String> = serde_json::from_str(&args_str).unwrap_or_default();

            servers.push(McpServerConfig {
                name,
                description,
                transport,
                url,
                command,
                args,
                auth: None,
            });
        }
        Ok(servers)
    }

    async fn unregister_mcp(&self, name: &str) -> Result<(), AttaError> {
        debug!(mcp = %name, "unregistering MCP server");

        let result = sqlx::query("DELETE FROM mcp_servers WHERE name = $1")
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("unregister mcp: {e}")))?;

        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound {
                entity_type: "mcp_server".to_string(),
                id: name.to_string(),
            }
            .into());
        }

        Ok(())
    }
}

// ── CronStore ──

#[async_trait::async_trait]
impl CronStore for PostgresStore {
    async fn save_cron_job(&self, job: &CronJob) -> Result<(), AttaError> {
        let config = serde_json::to_string(&job.config)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;

        sqlx::query(
            "INSERT INTO cron_jobs (id, name, schedule, command, config, enabled, created_by, created_at, updated_at, last_run_at, next_run_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             ON CONFLICT (id) DO UPDATE SET name=$2, schedule=$3, command=$4, config=$5, enabled=$6, updated_at=$9, last_run_at=$10, next_run_at=$11"
        )
        .bind(&job.id)
        .bind(&job.name)
        .bind(&job.schedule)
        .bind(&job.command)
        .bind(&config)
        .bind(job.enabled)
        .bind(&job.created_by)
        .bind(job.created_at)
        .bind(job.updated_at)
        .bind(job.last_run_at)
        .bind(job.next_run_at)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("save cron job: {e}")))?;

        Ok(())
    }

    async fn get_cron_job(&self, id: &str) -> Result<Option<CronJob>, AttaError> {
        let row = sqlx::query("SELECT * FROM cron_jobs WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("get cron job: {e}")))?;

        match row {
            Some(row) => {
                let id: String = row
                    .try_get("id")
                    .map_err(|e| StoreError::Database(e.to_string()))?;
                let name: String = row
                    .try_get("name")
                    .map_err(|e| StoreError::Database(e.to_string()))?;
                let schedule: String = row
                    .try_get("schedule")
                    .map_err(|e| StoreError::Database(e.to_string()))?;
                let command: String = row
                    .try_get("command")
                    .map_err(|e| StoreError::Database(e.to_string()))?;
                let config_str: String = row.try_get("config").unwrap_or_else(|_| "{}".to_string());
                let enabled: bool = row.try_get("enabled").unwrap_or(true);
                let created_by: String = row
                    .try_get("created_by")
                    .unwrap_or_else(|_| "system".to_string());
                let created_at: DateTime<Utc> =
                    row.try_get("created_at").unwrap_or_else(|_| Utc::now());
                let updated_at: DateTime<Utc> =
                    row.try_get("updated_at").unwrap_or_else(|_| Utc::now());
                let last_run_at: Option<DateTime<Utc>> = row.try_get("last_run_at").ok();
                let next_run_at: Option<DateTime<Utc>> = row.try_get("next_run_at").ok();

                Ok(Some(CronJob {
                    id,
                    name,
                    schedule,
                    command,
                    config: serde_json::from_str(&config_str).unwrap_or_default(),
                    enabled,
                    created_by,
                    created_at,
                    updated_at,
                    last_run_at,
                    next_run_at,
                }))
            }
            None => Ok(None),
        }
    }

    async fn list_cron_jobs(&self, status: Option<&str>) -> Result<Vec<CronJob>, AttaError> {
        let rows = match status {
            Some("active") => {
                sqlx::query("SELECT * FROM cron_jobs WHERE enabled = true ORDER BY created_at DESC")
                    .fetch_all(&self.pool)
                    .await
            }
            Some("paused") => {
                sqlx::query(
                    "SELECT * FROM cron_jobs WHERE enabled = false ORDER BY created_at DESC",
                )
                .fetch_all(&self.pool)
                .await
            }
            _ => {
                sqlx::query("SELECT * FROM cron_jobs ORDER BY created_at DESC")
                    .fetch_all(&self.pool)
                    .await
            }
        }
        .map_err(|e| StoreError::Database(format!("list cron jobs: {e}")))?;

        let mut jobs = Vec::with_capacity(rows.len());
        for row in &rows {
            let id: String = row
                .try_get("id")
                .map_err(|e| StoreError::Database(e.to_string()))?;
            let name: String = row
                .try_get("name")
                .map_err(|e| StoreError::Database(e.to_string()))?;
            let schedule: String = row
                .try_get("schedule")
                .map_err(|e| StoreError::Database(e.to_string()))?;
            let command: String = row
                .try_get("command")
                .map_err(|e| StoreError::Database(e.to_string()))?;
            let config_str: String = row.try_get("config").unwrap_or_else(|_| "{}".to_string());
            let enabled: bool = row.try_get("enabled").unwrap_or(true);
            let created_by: String = row
                .try_get("created_by")
                .unwrap_or_else(|_| "system".to_string());
            let created_at: DateTime<Utc> =
                row.try_get("created_at").unwrap_or_else(|_| Utc::now());
            let updated_at: DateTime<Utc> =
                row.try_get("updated_at").unwrap_or_else(|_| Utc::now());
            let last_run_at: Option<DateTime<Utc>> = row.try_get("last_run_at").ok();
            let next_run_at: Option<DateTime<Utc>> = row.try_get("next_run_at").ok();

            jobs.push(CronJob {
                id,
                name,
                schedule,
                command,
                config: serde_json::from_str(&config_str).unwrap_or_default(),
                enabled,
                created_by,
                created_at,
                updated_at,
                last_run_at,
                next_run_at,
            });
        }
        Ok(jobs)
    }

    async fn delete_cron_job(&self, id: &str) -> Result<(), AttaError> {
        sqlx::query("DELETE FROM cron_jobs WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("delete cron job: {e}")))?;
        Ok(())
    }

    async fn save_cron_run(&self, run: &CronRun) -> Result<(), AttaError> {
        let status_str = match run.status {
            CronRunStatus::Running => "running",
            CronRunStatus::Completed => "completed",
            CronRunStatus::Failed => "failed",
        };

        sqlx::query(
            "INSERT INTO cron_runs (id, job_id, status, started_at, completed_at, output, error, triggered_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (id) DO UPDATE SET status=$3, completed_at=$5, output=$6, error=$7"
        )
        .bind(&run.id)
        .bind(&run.job_id)
        .bind(status_str)
        .bind(run.started_at)
        .bind(run.completed_at)
        .bind(&run.output)
        .bind(&run.error)
        .bind(&run.triggered_by)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("save cron run: {e}")))?;

        Ok(())
    }

    async fn list_cron_runs(&self, job_id: &str, limit: usize) -> Result<Vec<CronRun>, AttaError> {
        let rows = sqlx::query(
            "SELECT * FROM cron_runs WHERE job_id = $1 ORDER BY started_at DESC LIMIT $2",
        )
        .bind(job_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("list cron runs: {e}")))?;

        let mut runs = Vec::with_capacity(rows.len());
        for row in &rows {
            let id: String = row
                .try_get("id")
                .map_err(|e| StoreError::Database(e.to_string()))?;
            let job_id: String = row
                .try_get("job_id")
                .map_err(|e| StoreError::Database(e.to_string()))?;
            let status_str: String = row
                .try_get("status")
                .map_err(|e| StoreError::Database(e.to_string()))?;
            let started_at: DateTime<Utc> =
                row.try_get("started_at").unwrap_or_else(|_| Utc::now());
            let completed_at: Option<DateTime<Utc>> = row.try_get("completed_at").ok();
            let output: Option<String> = row.try_get("output").ok();
            let error: Option<String> = row.try_get("error").ok();
            let triggered_by: String = row
                .try_get("triggered_by")
                .unwrap_or_else(|_| "scheduler".to_string());

            runs.push(CronRun {
                id,
                job_id,
                status: match status_str.as_str() {
                    "completed" => CronRunStatus::Completed,
                    "failed" => CronRunStatus::Failed,
                    _ => CronRunStatus::Running,
                },
                started_at,
                completed_at,
                output,
                error,
                triggered_by,
            });
        }
        Ok(runs)
    }
}

// ── RbacStore ──

#[async_trait::async_trait]
impl RbacStore for PostgresStore {
    async fn get_roles_for_actor(
        &self,
        actor_id: &str,
    ) -> Result<Vec<atta_types::Role>, AttaError> {
        let rows = sqlx::query("SELECT role FROM role_bindings WHERE actor_id = $1")
            .bind(actor_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("get roles: {e}")))?;

        let mut roles = Vec::with_capacity(rows.len());
        for row in &rows {
            let role_str: String = row
                .try_get("role")
                .map_err(|e| StoreError::Database(e.to_string()))?;
            if let Ok(role) = serde_json::from_value(serde_json::Value::String(role_str)) {
                roles.push(role);
            }
        }
        Ok(roles)
    }

    async fn bind_role(&self, actor_id: &str, role: &atta_types::Role) -> Result<(), AttaError> {
        let role_str = serde_json::to_value(role)
            .map_err(|e| StoreError::Serialization(e.to_string()))?
            .as_str()
            .unwrap_or("viewer")
            .to_string();

        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO role_bindings (id, actor_id, actor_type, role, granted_by, granted_at) VALUES ($1, $2, 'user', $3, 'system', NOW()) ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(actor_id)
        .bind(&role_str)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("bind role: {e}")))?;

        Ok(())
    }

    async fn unbind_role(&self, actor_id: &str, role: &atta_types::Role) -> Result<(), AttaError> {
        let role_str = serde_json::to_value(role)
            .map_err(|e| StoreError::Serialization(e.to_string()))?
            .as_str()
            .unwrap_or("viewer")
            .to_string();

        sqlx::query("DELETE FROM role_bindings WHERE actor_id = $1 AND role = $2")
            .bind(actor_id)
            .bind(&role_str)
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("unbind role: {e}")))?;

        Ok(())
    }
}

// ── Usage 追踪 ──

#[async_trait::async_trait]
impl UsageStore for PostgresStore {
    async fn record_usage(&self, record: &UsageRecord) -> Result<(), AttaError> {
        sqlx::query(
            "INSERT INTO usage_records (id, task_id, model, input_tokens, output_tokens, total_tokens, cost_usd, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(&record.id)
        .bind(&record.task_id)
        .bind(&record.model)
        .bind(record.input_tokens as i64)
        .bind(record.output_tokens as i64)
        .bind(record.total_tokens as i64)
        .bind(record.cost_usd)
        .bind(record.created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("record usage: {e}")))?;

        debug!(id = %record.id, model = %record.model, tokens = record.total_tokens, "usage recorded");
        Ok(())
    }

    async fn get_usage_summary(&self, since: DateTime<Utc>) -> Result<UsageSummary, AttaError> {
        let row = sqlx::query(
            "SELECT COALESCE(SUM(input_tokens), 0) as input_tokens,
                    COALESCE(SUM(output_tokens), 0) as output_tokens,
                    COALESCE(SUM(total_tokens), 0) as total_tokens,
                    COALESCE(SUM(cost_usd), 0.0) as total_cost,
                    COUNT(*) as request_count
             FROM usage_records WHERE created_at >= $1",
        )
        .bind(since)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("usage summary: {e}")))?;

        let input_tokens: i64 = row.try_get("input_tokens").unwrap_or(0);
        let output_tokens: i64 = row.try_get("output_tokens").unwrap_or(0);
        let total_tokens: i64 = row.try_get("total_tokens").unwrap_or(0);
        let total_cost: f64 = row.try_get("total_cost").unwrap_or(0.0);
        let request_count: i64 = row.try_get("request_count").unwrap_or(0);

        let model_rows = sqlx::query(
            "SELECT model,
                    COALESCE(SUM(total_tokens), 0) as tokens,
                    COALESCE(SUM(cost_usd), 0.0) as cost_usd,
                    COUNT(*) as request_count
             FROM usage_records WHERE created_at >= $1
             GROUP BY model ORDER BY tokens DESC",
        )
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("usage by model: {e}")))?;

        let by_model: Vec<ModelUsage> = model_rows
            .iter()
            .map(|r| {
                let model: String = r.try_get("model").unwrap_or_default();
                let tokens: i64 = r.try_get("tokens").unwrap_or(0);
                let cost: f64 = r.try_get("cost_usd").unwrap_or(0.0);
                let count: i64 = r.try_get("request_count").unwrap_or(0);
                ModelUsage {
                    model,
                    tokens: tokens as u64,
                    cost_usd: cost,
                    request_count: count as u64,
                }
            })
            .collect();

        Ok(UsageSummary {
            total_tokens: total_tokens as u64,
            total_cost_usd: total_cost,
            input_tokens: input_tokens as u64,
            output_tokens: output_tokens as u64,
            request_count: request_count as u64,
            by_model,
            period: String::new(),
        })
    }

    async fn get_usage_daily(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<UsageDaily>, AttaError> {
        let rows = sqlx::query(
            "SELECT DATE(created_at) as date,
                    COALESCE(SUM(total_tokens), 0) as tokens,
                    COALESCE(SUM(cost_usd), 0.0) as cost_usd,
                    COALESCE(SUM(input_tokens), 0) as input_tokens,
                    COALESCE(SUM(output_tokens), 0) as output_tokens
             FROM usage_records
             WHERE created_at >= $1 AND created_at < $2
             GROUP BY DATE(created_at)
             ORDER BY date",
        )
        .bind(start)
        .bind(end)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("usage daily: {e}")))?;

        let daily: Vec<UsageDaily> = rows
            .iter()
            .map(|r| {
                let date: String = r.try_get::<chrono::NaiveDate, _>("date")
                    .map(|d| d.to_string())
                    .unwrap_or_default();
                let tokens: i64 = r.try_get("tokens").unwrap_or(0);
                let cost: f64 = r.try_get("cost_usd").unwrap_or(0.0);
                let input: i64 = r.try_get("input_tokens").unwrap_or(0);
                let output: i64 = r.try_get("output_tokens").unwrap_or(0);
                UsageDaily {
                    date,
                    tokens: tokens as u64,
                    cost_usd: cost,
                    input_tokens: input as u64,
                    output_tokens: output as u64,
                }
            })
            .collect();

        Ok(daily)
    }
}

// ── Remote Agent 存储 ──

#[async_trait::async_trait]
impl RemoteAgentStore for PostgresStore {
    async fn register_remote_agent(
        &self,
        agent: &atta_types::RemoteAgent,
        token_hash: &str,
    ) -> Result<(), AttaError> {
        let capabilities =
            serde_json::to_string(&agent.capabilities).unwrap_or_else(|_| "[]".to_string());
        let status = match agent.status {
            atta_types::RemoteAgentStatus::Online => "online",
            atta_types::RemoteAgentStatus::Offline => "offline",
        };

        sqlx::query(
            "INSERT INTO remote_agents (id, name, token_hash, description, version, capabilities, status, registered_at, registered_by, token_expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(&agent.id)
        .bind(&agent.name)
        .bind(token_hash)
        .bind(&agent.description)
        .bind(&agent.version)
        .bind(&capabilities)
        .bind(status)
        .bind(agent.registered_at)
        .bind(&agent.registered_by)
        .bind(agent.token_expires_at)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("register remote agent: {e}")))?;

        Ok(())
    }

    async fn get_remote_agent(
        &self,
        id: &str,
    ) -> Result<Option<atta_types::RemoteAgent>, AttaError> {
        let row = sqlx::query("SELECT * FROM remote_agents WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("get remote agent: {e}")))?;

        match row {
            Some(r) => Ok(Some(pg_remote_agent_from_row(&r)?)),
            None => Ok(None),
        }
    }

    async fn get_remote_agent_by_token(
        &self,
        token_hash: &str,
    ) -> Result<Option<atta_types::RemoteAgent>, AttaError> {
        let row = sqlx::query("SELECT * FROM remote_agents WHERE token_hash = $1")
            .bind(token_hash)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("get remote agent by token: {e}")))?;

        match row {
            Some(r) => Ok(Some(pg_remote_agent_from_row(&r)?)),
            None => Ok(None),
        }
    }

    async fn list_remote_agents(&self) -> Result<Vec<atta_types::RemoteAgent>, AttaError> {
        let rows =
            sqlx::query("SELECT * FROM remote_agents ORDER BY registered_at DESC")
                .fetch_all(&self.pool)
                .await
                .map_err(|e| StoreError::Database(format!("list remote agents: {e}")))?;

        rows.iter().map(pg_remote_agent_from_row).collect()
    }

    async fn update_remote_agent_status(
        &self,
        id: &str,
        status: &atta_types::RemoteAgentStatus,
    ) -> Result<(), AttaError> {
        let status_str = match status {
            atta_types::RemoteAgentStatus::Online => "online",
            atta_types::RemoteAgentStatus::Offline => "offline",
        };

        sqlx::query("UPDATE remote_agents SET status = $1 WHERE id = $2")
            .bind(status_str)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("update remote agent status: {e}")))?;

        Ok(())
    }

    async fn update_remote_agent_heartbeat(&self, id: &str) -> Result<(), AttaError> {
        sqlx::query(
            "UPDATE remote_agents SET last_heartbeat = NOW(), status = 'online' WHERE id = $1",
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("update heartbeat: {e}")))?;

        Ok(())
    }

    async fn delete_remote_agent(&self, id: &str) -> Result<(), AttaError> {
        sqlx::query("DELETE FROM remote_agents WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("delete remote agent: {e}")))?;

        Ok(())
    }

    async fn rotate_remote_agent_token(
        &self,
        id: &str,
        new_token_hash: &str,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<(), AttaError> {
        sqlx::query("UPDATE remote_agents SET token_hash = $1, token_expires_at = $2 WHERE id = $3")
            .bind(new_token_hash)
            .bind(expires_at)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("rotate remote agent token: {e}")))?;
        Ok(())
    }
}

fn pg_remote_agent_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<atta_types::RemoteAgent, AttaError> {
    let id: String = row.try_get("id").map_err(|e| StoreError::Database(e.to_string()))?;
    let name: String = row.try_get("name").map_err(|e| StoreError::Database(e.to_string()))?;
    let description: String = row.try_get("description").unwrap_or_default();
    let version: String = row
        .try_get("version")
        .unwrap_or_else(|_| "0.1.0".to_string());
    let caps_str: String = row
        .try_get("capabilities")
        .unwrap_or_else(|_| "[]".to_string());
    let status_str: String = row
        .try_get("status")
        .unwrap_or_else(|_| "offline".to_string());
    let last_heartbeat: Option<DateTime<Utc>> = row.try_get("last_heartbeat").ok().flatten();
    let registered_at: DateTime<Utc> = row
        .try_get("registered_at")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let registered_by: String = row
        .try_get("registered_by")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let token_expires_at: Option<DateTime<Utc>> = row.try_get("token_expires_at").ok().flatten();

    let capabilities: Vec<String> = serde_json::from_str(&caps_str).unwrap_or_default();
    let status = match status_str.as_str() {
        "online" => atta_types::RemoteAgentStatus::Online,
        _ => atta_types::RemoteAgentStatus::Offline,
    };

    Ok(atta_types::RemoteAgent {
        id,
        name,
        description,
        version,
        capabilities,
        status,
        last_heartbeat,
        registered_at,
        registered_by,
        token_expires_at,
    })
}

// ── StateStore (super-trait blanket impl) ──

impl StateStore for PostgresStore {}
