//! SQLite 实现的 StateStore
//!
//! Desktop 版默认存储后端。使用 sqlx::SqlitePool 管理连接池，
//! 启动时自动运行 migrations。

use chrono::{DateTime, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use tracing::{debug, info};
use uuid::Uuid;

use atta_types::error::StoreError;
use atta_types::package::ServiceAccount;
use atta_types::usage::{ModelUsage, UsageDaily, UsageRecord, UsageSummary};
use atta_types::{
    Actor, ApprovalFilter, ApprovalRequest, ApprovalStatus, AttaError, CronJob, CronRun,
    CronRunStatus, FlowDef, FlowState, McpServerConfig, NodeCapacity, NodeInfo, NodeStatus,
    PackageRecord, PluginManifest, SkillDef, StateTransition, Task, TaskFilter, TaskStatus,
    ToolDef,
};

use crate::common::{task_status_from_db, task_status_to_db};
use crate::{
    ApprovalStore, CronStore, FlowStore, McpStore, NodeStore, PackageStore, RbacStore,
    RegistryStore, RemoteAgentStore, ServiceAccountStore, StateStore, TaskStore, UsageStore,
};

/// SQLite 状态存储
///
/// Desktop 版的默认持久化实现。所有数据存储在本地 SQLite 数据库中。
pub struct SqliteStore {
    pool: SqlitePool,
}

impl SqliteStore {
    /// 连接数据库并运行 migrations
    ///
    /// # Arguments
    /// * `path` - SQLite 数据库文件路径，如 `~/.atta/data.db`
    ///
    /// # Errors
    /// 如果连接失败或 migration 执行失败，返回 `AttaError::Store`
    pub async fn open(path: &str) -> Result<Self, AttaError> {
        info!(path = %path, "opening SQLite database");

        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .map_err(|e| StoreError::Database(format!("failed to connect: {e}")))?;

        // 启用 WAL 模式以提升并发性能
        sqlx::query("PRAGMA journal_mode=WAL;")
            .execute(&pool)
            .await
            .map_err(|e| StoreError::Database(format!("failed to set WAL mode: {e}")))?;

        // 运行 migrations
        sqlx::migrate!("../../migrations/sqlite")
            .run(&pool)
            .await
            .map_err(|e| StoreError::Database(format!("migration failed: {e}")))?;

        info!("SQLite database ready");
        Ok(Self { pool })
    }

    /// 获取连接池引用（供测试和内部使用）
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

// ── 辅助函数 ──

/// 从数据库行构建 Task
fn task_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Task, AttaError> {
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
    let error_msg: Option<String> = row.try_get("error_message").ok();
    let state_data_str: String = row
        .try_get("state_data")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let input_str: String = row
        .try_get("input")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let output_str: Option<String> = row
        .try_get::<Option<String>, _>("output")
        .ok()
        .flatten()
        .filter(|s| !s.is_empty());
    let created_by: String = row
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

    let created_by_actor: Actor =
        serde_json::from_str(&created_by).unwrap_or_else(|_| Actor::user(created_by.clone()));

    Ok(Task {
        id,
        flow_id,
        current_state,
        state_data,
        input,
        output,
        status: task_status_from_db(&status_str, error_msg),
        created_by: created_by_actor,
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
        version: row.try_get::<i64, _>("version").unwrap_or(0) as u64,
    })
}

// ── TaskStore ──

#[async_trait::async_trait]
impl TaskStore for SqliteStore {
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
        let completed_at = task.completed_at.map(|dt| dt.to_rfc3339());

        debug!(task_id = %id, "creating task");

        sqlx::query(
            "INSERT INTO tasks (id, flow_id, current_state, status, error_message, state_data, input, output, created_by, created_at, updated_at, completed_at, version)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
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
        .bind(&completed_at)
        .bind(task.version as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("insert task: {e}")))?;

        Ok(())
    }

    async fn get_task(&self, id: &Uuid) -> Result<Option<Task>, AttaError> {
        let id_str = id.to_string();
        debug!(task_id = %id_str, "getting task");

        let row = sqlx::query("SELECT * FROM tasks WHERE id = ?")
            .bind(&id_str)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("get task: {e}")))?;

        match row {
            Some(row) => Ok(Some(task_from_row(&row)?)),
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

        debug!(task_id = %id_str, status = %status_str, "updating task status");

        let result = sqlx::query(
            "UPDATE tasks SET status = ?, error_message = ?, updated_at = ?, completed_at = ? WHERE id = ?"
        )
        .bind(&status_str)
        .bind(&error_msg)
        .bind(&now)
        .bind(&completed_at)
        .bind(&id_str)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("update task status: {e}")))?;

        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound {
                entity_type: "task".to_string(),
                id: id_str,
            }
            .into());
        }

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
            sql.push_str(" AND status = ?");
            args.push(s);
        }
        if let Some(ref flow_id) = filter.flow_id {
            sql.push_str(" AND flow_id = ?");
            args.push(flow_id.clone());
        }
        if let Some(ref created_by) = filter.created_by {
            sql.push_str(" AND created_by LIKE ? ESCAPE '\\'");
            // Escape LIKE wildcards in user input
            let escaped = created_by
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            args.push(format!("%{escaped}%"));
        }

        sql.push_str(" ORDER BY created_at DESC LIMIT ? OFFSET ?");

        // 由于 sqlx 的动态绑定限制，这里使用 sqlx::query 配合手动绑定
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
            tasks.push(task_from_row(row)?);
        }
        Ok(tasks)
    }

    async fn merge_task_state_data(
        &self,
        task_id: &Uuid,
        data: serde_json::Value,
    ) -> Result<(), AttaError> {
        let task_id_str = task_id.to_string();
        let now = Utc::now().to_rfc3339();

        debug!(task_id = %task_id_str, "merging task state data");

        // 使用事务包裹读-改-写，避免并发竞态
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StoreError::Database(format!("begin tx: {e}")))?;

        // 读取当前 state_data
        let row = sqlx::query("SELECT state_data FROM tasks WHERE id = ?")
            .bind(&task_id_str)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| StoreError::Database(format!("get state_data: {e}")))?
            .ok_or_else(|| StoreError::NotFound {
                entity_type: "task".to_string(),
                id: task_id_str.clone(),
            })?;

        let current_str: String = row
            .try_get("state_data")
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let mut current: serde_json::Value = serde_json::from_str(&current_str)
            .map_err(|e| StoreError::Serialization(format!("state_data: {e}")))?;

        // JSON merge: 将 data 中的键值对合并到 current
        if let (Some(current_obj), Some(data_obj)) = (current.as_object_mut(), data.as_object()) {
            for (k, v) in data_obj {
                current_obj.insert(k.clone(), v.clone());
            }
        }

        let merged = serde_json::to_string(&current)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;

        sqlx::query("UPDATE tasks SET state_data = ?, updated_at = ? WHERE id = ?")
            .bind(&merged)
            .bind(&now)
            .bind(&task_id_str)
            .execute(&mut *tx)
            .await
            .map_err(|e| StoreError::Database(format!("update state_data: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| StoreError::Database(format!("commit tx: {e}")))?;

        Ok(())
    }
}

// ── FlowStore ──

#[async_trait::async_trait]
impl FlowStore for SqliteStore {
    async fn save_flow_def(&self, flow: &FlowDef) -> Result<(), AttaError> {
        let definition =
            serde_json::to_string(flow).map_err(|e| StoreError::Serialization(e.to_string()))?;
        let now = Utc::now().to_rfc3339();

        debug!(flow_id = %flow.id, "saving flow definition");

        sqlx::query(
            "INSERT INTO flow_defs (id, version, name, description, definition, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                version = excluded.version,
                name = excluded.name,
                description = excluded.description,
                definition = excluded.definition,
                updated_at = excluded.updated_at"
        )
        .bind(&flow.id)
        .bind(&flow.version)
        .bind(&flow.name)
        .bind(&flow.description)
        .bind(&definition)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("save flow def: {e}")))?;

        Ok(())
    }

    async fn get_flow_def(&self, id: &str) -> Result<Option<FlowDef>, AttaError> {
        debug!(flow_id = %id, "getting flow definition");

        let row = sqlx::query("SELECT definition FROM flow_defs WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("get flow def: {e}")))?;

        match row {
            Some(row) => {
                let def_str: String = row
                    .try_get("definition")
                    .map_err(|e| StoreError::Database(e.to_string()))?;
                let flow: FlowDef = serde_json::from_str(&def_str)
                    .map_err(|e| StoreError::Serialization(format!("flow def: {e}")))?;
                Ok(Some(flow))
            }
            None => Ok(None),
        }
    }

    async fn save_flow_state(&self, task_id: &Uuid, state: &FlowState) -> Result<(), AttaError> {
        let task_id_str = task_id.to_string();
        let history = serde_json::to_string(&state.history)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let now = Utc::now().to_rfc3339();

        debug!(task_id = %task_id_str, state = %state.current_state, "saving flow state");

        sqlx::query(
            "INSERT INTO flow_states (task_id, current_state, history, retry_count, updated_at)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(task_id) DO UPDATE SET
                current_state = excluded.current_state,
                history = excluded.history,
                retry_count = excluded.retry_count,
                updated_at = excluded.updated_at",
        )
        .bind(&task_id_str)
        .bind(&state.current_state)
        .bind(&history)
        .bind(state.retry_count as i64)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("save flow state: {e}")))?;

        Ok(())
    }

    async fn get_flow_state(&self, task_id: &Uuid) -> Result<Option<FlowState>, AttaError> {
        let task_id_str = task_id.to_string();
        debug!(task_id = %task_id_str, "getting flow state");

        let row = sqlx::query("SELECT * FROM flow_states WHERE task_id = ?")
            .bind(&task_id_str)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("get flow state: {e}")))?;

        match row {
            Some(row) => {
                let current_state: String = row
                    .try_get("current_state")
                    .map_err(|e| StoreError::Database(e.to_string()))?;
                let history_str: String = row
                    .try_get("history")
                    .map_err(|e| StoreError::Database(e.to_string()))?;
                let retry_count: i64 = row
                    .try_get("retry_count")
                    .map_err(|e| StoreError::Database(e.to_string()))?;

                let history: Vec<StateTransition> = serde_json::from_str(&history_str)
                    .map_err(|e| StoreError::Serialization(format!("flow history: {e}")))?;

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
            .map_err(|e| StoreError::Database(format!("list flow defs: {e}")))?;

        let mut flows = Vec::with_capacity(rows.len());
        for row in &rows {
            let def_str: String = row
                .try_get("definition")
                .map_err(|e| StoreError::Database(e.to_string()))?;
            let flow: FlowDef = serde_json::from_str(&def_str)
                .map_err(|e| StoreError::Serialization(format!("flow def: {e}")))?;
            flows.push(flow);
        }
        Ok(flows)
    }

    async fn delete_flow_def(&self, id: &str) -> Result<(), AttaError> {
        debug!(flow_id = %id, "deleting flow definition");

        let result = sqlx::query("DELETE FROM flow_defs WHERE id = ?")
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

    async fn list_skill_defs(&self) -> Result<Vec<SkillDef>, AttaError> {
        RegistryStore::list_skills(self).await
    }

    async fn create_task_with_flow(
        &self,
        task: &Task,
        flow_state: &FlowState,
    ) -> Result<(), AttaError> {
        // Verify flow definition exists
        let flow_exists = sqlx::query("SELECT 1 FROM flow_defs WHERE id = ?")
            .bind(&task.flow_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("check flow exists: {e}")))?;
        if flow_exists.is_none() {
            return Err(AttaError::FlowNotFound(task.flow_id.clone()));
        }

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
        let completed_at = task.completed_at.map(|dt| dt.to_rfc3339());

        debug!(task_id = %id, "creating task with flow (transaction)");

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StoreError::Database(format!("begin tx: {e}")))?;

        // 插入 task
        sqlx::query(
            "INSERT INTO tasks (id, flow_id, current_state, status, error_message, state_data, input, output, created_by, created_at, updated_at, completed_at, version)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
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
        .bind(&completed_at)
        .bind(0_i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| StoreError::Database(format!("insert task in tx: {e}")))?;

        // 插入 flow state
        let history = serde_json::to_string(&flow_state.history)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO flow_states (task_id, current_state, history, retry_count, updated_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&flow_state.current_state)
        .bind(&history)
        .bind(flow_state.retry_count as i64)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| StoreError::Database(format!("insert flow state in tx: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| StoreError::Database(format!("commit tx: {e}")))?;

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
        let task_id_str = task_id.to_string();
        let (status_str, error_msg) = task_status_to_db(&new_status);
        let now = Utc::now().to_rfc3339();
        let new_version = expected_version + 1;

        debug!(
            task_id = %task_id_str,
            new_state = %new_state,
            status = %status_str,
            expected_version,
            "advancing task (transaction)"
        );

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StoreError::Database(format!("begin tx: {e}")))?;

        // 更新 task 状态 with optimistic version check
        let result = sqlx::query(
            "UPDATE tasks SET status = ?, error_message = ?, current_state = ?, updated_at = ?, version = ? WHERE id = ? AND version = ?"
        )
        .bind(&status_str)
        .bind(&error_msg)
        .bind(new_state)
        .bind(&now)
        .bind(new_version as i64)
        .bind(&task_id_str)
        .bind(expected_version as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| StoreError::Database(format!("update task in tx: {e}")))?;

        if result.rows_affected() == 0 {
            // Query actual version before rollback
            let actual_row = sqlx::query("SELECT version FROM tasks WHERE id = ?")
                .bind(&task_id_str)
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
                id: task_id_str,
                expected: expected_version,
                actual: actual_version,
            });
        }

        // 读取当前 flow state 的 history
        let row = sqlx::query("SELECT history, retry_count FROM flow_states WHERE task_id = ?")
            .bind(&task_id_str)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| StoreError::Database(format!("get flow state in tx: {e}")))?;

        let (mut history, retry_count): (Vec<StateTransition>, i64) = match row {
            Some(row) => {
                let history_str: String = row
                    .try_get("history")
                    .map_err(|e| StoreError::Database(e.to_string()))?;
                let rc: i64 = row
                    .try_get("retry_count")
                    .map_err(|e| StoreError::Database(e.to_string()))?;
                let h: Vec<StateTransition> = serde_json::from_str(&history_str)
                    .map_err(|e| StoreError::Serialization(format!("flow history: {e}")))?;
                (h, rc)
            }
            None => {
                if let Err(e) = tx.rollback().await {
                    tracing::warn!(error = %e, "transaction rollback failed");
                }
                return Err(StoreError::NotFound {
                    entity_type: "flow_state".to_string(),
                    id: task_id_str.clone(),
                }
                .into());
            }
        };

        // 追加新 transition
        history.push(transition.clone());
        let history_json = serde_json::to_string(&history)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;

        // 更新 flow state
        sqlx::query(
            "INSERT INTO flow_states (task_id, current_state, history, retry_count, updated_at)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(task_id) DO UPDATE SET
                current_state = excluded.current_state,
                history = excluded.history,
                updated_at = excluded.updated_at",
        )
        .bind(&task_id_str)
        .bind(new_state)
        .bind(&history_json)
        .bind(retry_count)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| StoreError::Database(format!("update flow state in tx: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| StoreError::Database(format!("commit tx: {e}")))?;

        Ok(())
    }
}

// ── RegistryStore ──

#[async_trait::async_trait]
impl RegistryStore for SqliteStore {
    async fn register_plugin(&self, manifest: &PluginManifest) -> Result<(), AttaError> {
        let permissions = serde_json::to_string(&manifest.permissions)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let manifest_json = serde_json::to_string(manifest)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let now = Utc::now().to_rfc3339();

        debug!(plugin = %manifest.name, "registering plugin");

        sqlx::query(
            "INSERT INTO plugins (name, version, description, author, organization, permissions, manifest, installed_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(name) DO UPDATE SET
                version = excluded.version,
                description = excluded.description,
                author = excluded.author,
                organization = excluded.organization,
                permissions = excluded.permissions,
                manifest = excluded.manifest,
                updated_at = excluded.updated_at"
        )
        .bind(&manifest.name)
        .bind(&manifest.version)
        .bind(&manifest.description)
        .bind(&manifest.author)
        .bind(&manifest.organization)
        .bind(&permissions)
        .bind(&manifest_json)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("register plugin: {e}")))?;

        Ok(())
    }

    async fn unregister_plugin(&self, name: &str) -> Result<(), AttaError> {
        debug!(plugin = %name, "unregistering plugin");

        let result = sqlx::query("DELETE FROM plugins WHERE name = ?")
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("unregister plugin: {e}")))?;

        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound {
                entity_type: "plugin".to_string(),
                id: name.to_string(),
            }
            .into());
        }

        Ok(())
    }

    async fn list_plugins(&self) -> Result<Vec<PluginManifest>, AttaError> {
        let rows = sqlx::query("SELECT manifest FROM plugins ORDER BY name")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("list plugins: {e}")))?;

        let mut plugins = Vec::with_capacity(rows.len());
        for row in &rows {
            let manifest_str: String = row
                .try_get("manifest")
                .map_err(|e| StoreError::Database(e.to_string()))?;
            let manifest: PluginManifest = serde_json::from_str(&manifest_str)
                .map_err(|e| StoreError::Serialization(format!("plugin manifest: {e}")))?;
            plugins.push(manifest);
        }
        Ok(plugins)
    }

    async fn register_tool(&self, tool: &ToolDef) -> Result<(), AttaError> {
        let (plugin_name, mcp_server) = match &tool.binding {
            atta_types::ToolBinding::Mcp { server_name } => (None, Some(server_name.clone())),
            atta_types::ToolBinding::Builtin { handler_name } => {
                (Some(format!("builtin:{}", handler_name)), None)
            }
            atta_types::ToolBinding::Native { handler_name } => {
                (Some(format!("native:{}", handler_name)), None)
            }
        };
        let risk_level = format!("{:?}", tool.risk_level).to_lowercase();
        let parameters = serde_json::to_string(&tool.parameters)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let now = Utc::now().to_rfc3339();

        debug!(tool = %tool.name, "registering tool");

        sqlx::query(
            "INSERT INTO tool_defs (name, description, plugin_name, mcp_server, risk_level, parameters, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(name) DO UPDATE SET
                description = excluded.description,
                plugin_name = excluded.plugin_name,
                mcp_server = excluded.mcp_server,
                risk_level = excluded.risk_level,
                parameters = excluded.parameters"
        )
        .bind(&tool.name)
        .bind(&tool.description)
        .bind(&plugin_name)
        .bind(&mcp_server)
        .bind(&risk_level)
        .bind(&parameters)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("register tool: {e}")))?;

        Ok(())
    }

    async fn list_tools(&self) -> Result<Vec<ToolDef>, AttaError> {
        let rows = sqlx::query("SELECT * FROM tool_defs WHERE enabled = 1 ORDER BY name")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("list tools: {e}")))?;

        let mut tools = Vec::with_capacity(rows.len());
        for row in &rows {
            tools.push(tool_from_row(row)?);
        }
        Ok(tools)
    }

    async fn register_skill(&self, skill: &SkillDef) -> Result<(), AttaError> {
        let definition =
            serde_json::to_string(skill).map_err(|e| StoreError::Serialization(e.to_string()))?;
        let risk_level = format!("{:?}", skill.risk_level).to_lowercase();
        let tags = serde_json::to_string(&skill.tags)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let now = Utc::now().to_rfc3339();

        debug!(skill_id = %skill.id, "registering skill");

        sqlx::query(
            "INSERT INTO skill_defs (id, version, name, definition, risk_level, tags, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                version = excluded.version,
                name = excluded.name,
                definition = excluded.definition,
                risk_level = excluded.risk_level,
                tags = excluded.tags,
                updated_at = excluded.updated_at"
        )
        .bind(&skill.id)
        .bind(&skill.version)
        .bind(&skill.name)
        .bind(&definition)
        .bind(&risk_level)
        .bind(&tags)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("register skill: {e}")))?;

        Ok(())
    }

    async fn list_skills(&self) -> Result<Vec<SkillDef>, AttaError> {
        let rows = sqlx::query("SELECT definition FROM skill_defs ORDER BY id")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("list skills: {e}")))?;

        let mut skills = Vec::with_capacity(rows.len());
        for row in &rows {
            let def_str: String = row
                .try_get("definition")
                .map_err(|e| StoreError::Database(e.to_string()))?;
            let skill: SkillDef = serde_json::from_str(&def_str)
                .map_err(|e| StoreError::Serialization(format!("skill def: {e}")))?;
            skills.push(skill);
        }
        Ok(skills)
    }

    async fn get_tool(&self, name: &str) -> Result<Option<ToolDef>, AttaError> {
        debug!(tool = %name, "getting tool");

        let row = sqlx::query("SELECT * FROM tool_defs WHERE name = ?")
            .bind(name)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("get tool: {e}")))?;

        match row {
            Some(row) => Ok(Some(tool_from_row(&row)?)),
            None => Ok(None),
        }
    }

    async fn get_skill(&self, id: &str) -> Result<Option<SkillDef>, AttaError> {
        debug!(skill_id = %id, "getting skill");

        let row = sqlx::query("SELECT definition FROM skill_defs WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("get skill: {e}")))?;

        match row {
            Some(row) => {
                let def_str: String = row
                    .try_get("definition")
                    .map_err(|e| StoreError::Database(e.to_string()))?;
                let skill: SkillDef = serde_json::from_str(&def_str)
                    .map_err(|e| StoreError::Serialization(format!("skill def: {e}")))?;
                Ok(Some(skill))
            }
            None => Ok(None),
        }
    }

    async fn get_plugin(&self, name: &str) -> Result<Option<PluginManifest>, AttaError> {
        debug!(plugin = %name, "getting plugin");

        let row = sqlx::query("SELECT manifest FROM plugins WHERE name = ?")
            .bind(name)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("get plugin: {e}")))?;

        match row {
            Some(row) => {
                let manifest_str: String = row
                    .try_get("manifest")
                    .map_err(|e| StoreError::Database(e.to_string()))?;
                let manifest: PluginManifest = serde_json::from_str(&manifest_str)
                    .map_err(|e| StoreError::Serialization(format!("plugin manifest: {e}")))?;
                Ok(Some(manifest))
            }
            None => Ok(None),
        }
    }

    async fn delete_skill(&self, id: &str) -> Result<(), AttaError> {
        debug!(skill_id = %id, "deleting skill");

        let result = sqlx::query("DELETE FROM skill_defs WHERE id = ?")
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
impl PackageStore for SqliteStore {
    async fn register_package(&self, pkg: &PackageRecord) -> Result<(), AttaError> {
        let package_type = serde_json::to_value(&pkg.package_type)
            .map_err(|e| StoreError::Serialization(e.to_string()))?
            .as_str()
            .unwrap_or("plugin")
            .to_string();
        let installed_at = pkg.installed_at.to_rfc3339();

        debug!(package = %pkg.name, version = %pkg.version, "registering package");

        sqlx::query(
            "INSERT INTO packages (name, version, package_type, installed_at, installed_by)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(name, version) DO UPDATE SET
                package_type = excluded.package_type,
                installed_at = excluded.installed_at,
                installed_by = excluded.installed_by",
        )
        .bind(&pkg.name)
        .bind(&pkg.version)
        .bind(&package_type)
        .bind(&installed_at)
        .bind(&pkg.installed_by)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("register package: {e}")))?;

        Ok(())
    }

    async fn get_package(&self, name: &str) -> Result<Option<PackageRecord>, AttaError> {
        debug!(package = %name, "getting package");

        let row =
            sqlx::query("SELECT * FROM packages WHERE name = ? ORDER BY installed_at DESC LIMIT 1")
                .bind(name)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| StoreError::Database(format!("get package: {e}")))?;

        match row {
            Some(row) => {
                let name: String = row
                    .try_get("name")
                    .map_err(|e| StoreError::Database(e.to_string()))?;
                let version: String = row
                    .try_get("version")
                    .map_err(|e| StoreError::Database(e.to_string()))?;
                let package_type_str: String = row
                    .try_get("package_type")
                    .map_err(|e| StoreError::Database(e.to_string()))?;
                let installed_at_str: String = row
                    .try_get("installed_at")
                    .map_err(|e| StoreError::Database(e.to_string()))?;
                let installed_by: String = row
                    .try_get("installed_by")
                    .map_err(|e| StoreError::Database(e.to_string()))?;

                let package_type =
                    serde_json::from_value(serde_json::Value::String(package_type_str))
                        .map_err(|e| StoreError::Serialization(format!("package_type: {e}")))?;
                let installed_at = DateTime::parse_from_rfc3339(&installed_at_str)
                    .map_err(|e| StoreError::Database(format!("invalid installed_at: {e}")))?
                    .with_timezone(&Utc);

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
impl ServiceAccountStore for SqliteStore {
    async fn get_service_account_by_key(
        &self,
        api_key_hash: &str,
    ) -> Result<Option<ServiceAccount>, AttaError> {
        debug!("looking up service account by API key hash");

        let row =
            sqlx::query("SELECT * FROM service_accounts WHERE api_key_hash = ? AND enabled = 1")
                .bind(api_key_hash)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| StoreError::Database(format!("get service account: {e}")))?;

        match row {
            Some(row) => {
                let id_str: String = row
                    .try_get("id")
                    .map_err(|e| StoreError::Database(e.to_string()))?;
                let name: String = row
                    .try_get("name")
                    .map_err(|e| StoreError::Database(e.to_string()))?;
                let key_hash: String = row
                    .try_get("api_key_hash")
                    .map_err(|e| StoreError::Database(e.to_string()))?;
                let roles_str: String = row
                    .try_get("roles")
                    .map_err(|e| StoreError::Database(e.to_string()))?;
                let created_at_str: String = row
                    .try_get("created_at")
                    .map_err(|e| StoreError::Database(e.to_string()))?;
                let enabled: bool = row
                    .try_get::<i32, _>("enabled")
                    .map_err(|e| StoreError::Database(e.to_string()))
                    .map(|v| v != 0)?;

                let id = Uuid::parse_str(&id_str)
                    .map_err(|e| StoreError::Database(format!("invalid UUID: {e}")))?;
                let roles = serde_json::from_str(&roles_str)
                    .map_err(|e| StoreError::Serialization(format!("roles: {e}")))?;
                let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                    .map_err(|e| StoreError::Database(format!("invalid created_at: {e}")))?
                    .with_timezone(&Utc);

                Ok(Some(ServiceAccount {
                    id,
                    name,
                    api_key_hash: key_hash,
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
impl NodeStore for SqliteStore {
    async fn upsert_node(&self, node_info: &NodeInfo) -> Result<(), AttaError> {
        let heartbeat_str = node_info.last_heartbeat.to_rfc3339();
        let now = node_info.last_heartbeat.to_rfc3339();
        let labels = serde_json::to_string(&node_info.labels)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;

        debug!(node_id = %node_info.id, "upserting node");

        sqlx::query(
            "INSERT INTO nodes (id, hostname, status, total_memory, available_memory, running_agents, max_concurrent, labels, last_heartbeat, registered_at)
             VALUES (?, ?, 'online', ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                hostname = excluded.hostname,
                total_memory = excluded.total_memory,
                available_memory = excluded.available_memory,
                running_agents = excluded.running_agents,
                max_concurrent = excluded.max_concurrent,
                labels = excluded.labels,
                last_heartbeat = excluded.last_heartbeat"
        )
        .bind(&node_info.id)
        .bind(&node_info.hostname)
        .bind(node_info.capacity.total_memory as i64)
        .bind(node_info.capacity.available_memory as i64)
        .bind(node_info.capacity.running_agents as i64)
        .bind(node_info.capacity.max_concurrent as i64)
        .bind(&labels)
        .bind(&heartbeat_str)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("upsert node: {e}")))?;

        Ok(())
    }

    async fn get_node(&self, node_id: &str) -> Result<Option<NodeInfo>, AttaError> {
        debug!(node_id = %node_id, "getting node");

        let row = sqlx::query("SELECT * FROM nodes WHERE id = ?")
            .bind(node_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("get node: {e}")))?;

        match row {
            Some(row) => Ok(Some(node_from_row(&row)?)),
            None => Ok(None),
        }
    }

    async fn list_nodes(&self) -> Result<Vec<NodeInfo>, AttaError> {
        let rows = sqlx::query("SELECT * FROM nodes ORDER BY id")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("list nodes: {e}")))?;

        let mut nodes = Vec::with_capacity(rows.len());
        for row in &rows {
            nodes.push(node_from_row(row)?);
        }
        Ok(nodes)
    }

    async fn list_nodes_after(&self, cutoff: DateTime<Utc>) -> Result<Vec<NodeInfo>, AttaError> {
        let cutoff_str = cutoff.to_rfc3339();

        let rows = sqlx::query("SELECT * FROM nodes WHERE last_heartbeat > ? ORDER BY id")
            .bind(&cutoff_str)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("list nodes after: {e}")))?;

        let mut nodes = Vec::with_capacity(rows.len());
        for row in &rows {
            nodes.push(node_from_row(row)?);
        }
        Ok(nodes)
    }

    async fn update_node_status(&self, node_id: &str, status: NodeStatus) -> Result<(), AttaError> {
        let status_str = match status {
            NodeStatus::Online => "online",
            NodeStatus::Draining => "draining",
            NodeStatus::Offline => "offline",
        };

        debug!(node_id = %node_id, status = %status_str, "updating node status");

        let result = sqlx::query("UPDATE nodes SET status = ? WHERE id = ?")
            .bind(status_str)
            .bind(node_id)
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("update node status: {e}")))?;

        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound {
                entity_type: "node".to_string(),
                id: node_id.to_string(),
            }
            .into());
        }

        Ok(())
    }
}

// ── ApprovalStore ──

#[async_trait::async_trait]
impl ApprovalStore for SqliteStore {
    async fn save_approval(&self, approval: &ApprovalRequest) -> Result<(), AttaError> {
        let id = approval.id.to_string();
        let task_id = approval.task_id.to_string();
        let requested_by = serde_json::to_string(&approval.requested_by)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let approver_role = serde_json::to_value(&approval.approver_role)
            .map_err(|e| StoreError::Serialization(e.to_string()))?
            .as_str()
            .unwrap_or("approver")
            .to_string();
        let status = serde_json::to_value(&approval.status)
            .map_err(|e| StoreError::Serialization(e.to_string()))?
            .as_str()
            .unwrap_or("pending")
            .to_string();
        let context = serde_json::to_string(&approval.context)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let resolved_by = approval
            .resolved_by
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let resolved_at = approval.resolved_at.map(|t| t.to_rfc3339());
        let timeout_at = approval.timeout_at.to_rfc3339();
        let created_at = approval.created_at.to_rfc3339();

        debug!(approval_id = %id, task_id = %task_id, "saving approval");

        sqlx::query(
            "INSERT INTO approvals (id, task_id, requested_by, approver_role, status, context, resolved_by, resolved_at, timeout_at, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                status = excluded.status,
                resolved_by = excluded.resolved_by,
                resolved_at = excluded.resolved_at"
        )
        .bind(&id)
        .bind(&task_id)
        .bind(&requested_by)
        .bind(&approver_role)
        .bind(&status)
        .bind(&context)
        .bind(&resolved_by)
        .bind(&resolved_at)
        .bind(&timeout_at)
        .bind(&created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("save approval: {e}")))?;

        Ok(())
    }

    async fn get_approval(&self, id: &Uuid) -> Result<Option<ApprovalRequest>, AttaError> {
        let id_str = id.to_string();
        debug!(approval_id = %id_str, "getting approval");

        let row = sqlx::query("SELECT * FROM approvals WHERE id = ?")
            .bind(&id_str)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("get approval: {e}")))?;

        match row {
            Some(row) => Ok(Some(approval_from_row(&row)?)),
            None => Ok(None),
        }
    }

    async fn list_approvals(
        &self,
        filter: &ApprovalFilter,
    ) -> Result<Vec<ApprovalRequest>, AttaError> {
        let mut sql = String::from("SELECT * FROM approvals WHERE 1=1");
        let mut args: Vec<String> = Vec::new();

        if let Some(ref status) = filter.status {
            let status_str = serde_json::to_value(status)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| "pending".to_string());
            sql.push_str(" AND status = ?");
            args.push(status_str);
        }
        if let Some(ref role) = filter.approver_role {
            sql.push_str(" AND approver_role = ?");
            args.push(role.clone());
        }
        if let Some(ref task_id) = filter.task_id {
            sql.push_str(" AND task_id = ?");
            args.push(task_id.to_string());
        }

        sql.push_str(" ORDER BY created_at DESC LIMIT ? OFFSET ?");

        let mut query = sqlx::query(&sql);
        for arg in &args {
            query = query.bind(arg);
        }
        query = query.bind(filter.limit as i64);
        query = query.bind(filter.offset as i64);

        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("list approvals: {e}")))?;

        let mut approvals = Vec::with_capacity(rows.len());
        for row in &rows {
            approvals.push(approval_from_row(row)?);
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
        let id_str = id.to_string();
        let status_str = serde_json::to_value(&status)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| "pending".to_string());
        let resolved_by_str = serde_json::to_string(resolved_by)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let now = Utc::now().to_rfc3339();

        debug!(approval_id = %id_str, status = %status_str, "updating approval status");

        let result = sqlx::query(
            "UPDATE approvals SET status = ?, resolved_by = ?, resolved_at = ?, comment = ? WHERE id = ?",
        )
        .bind(&status_str)
        .bind(&resolved_by_str)
        .bind(&now)
        .bind(comment)
        .bind(&id_str)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("update approval status: {e}")))?;

        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound {
                entity_type: "approval".to_string(),
                id: id_str,
            }
            .into());
        }

        Ok(())
    }
}

// ── McpStore ──

#[async_trait::async_trait]
impl McpStore for SqliteStore {
    async fn register_mcp(&self, mcp_def: &McpServerConfig) -> Result<(), AttaError> {
        let transport = match mcp_def.transport {
            atta_types::McpTransport::Stdio => "stdio",
            atta_types::McpTransport::Sse => "sse",
        };
        let args = serde_json::to_string(&mcp_def.args)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let auth_config = mcp_def
            .auth
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let now = Utc::now().to_rfc3339();

        debug!(mcp = %mcp_def.name, "registering MCP server");

        sqlx::query(
            "INSERT INTO mcp_servers (name, description, transport, url, command, args, auth_config, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(name) DO UPDATE SET
                description = excluded.description,
                transport = excluded.transport,
                url = excluded.url,
                command = excluded.command,
                args = excluded.args,
                auth_config = excluded.auth_config"
        )
        .bind(&mcp_def.name)
        .bind(&mcp_def.description)
        .bind(transport)
        .bind(&mcp_def.url)
        .bind(&mcp_def.command)
        .bind(&args)
        .bind(&auth_config)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("register mcp: {e}")))?;

        Ok(())
    }

    async fn list_mcp_servers(&self) -> Result<Vec<McpServerConfig>, AttaError> {
        let rows = sqlx::query("SELECT * FROM mcp_servers ORDER BY name")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("list mcp servers: {e}")))?;

        let mut servers = Vec::with_capacity(rows.len());
        for row in &rows {
            servers.push(mcp_from_row(row)?);
        }
        Ok(servers)
    }

    async fn unregister_mcp(&self, name: &str) -> Result<(), AttaError> {
        debug!(mcp = %name, "unregistering MCP server");

        let result = sqlx::query("DELETE FROM mcp_servers WHERE name = ?")
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
impl CronStore for SqliteStore {
    async fn save_cron_job(&self, job: &CronJob) -> Result<(), AttaError> {
        let config = serde_json::to_string(&job.config)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let created_at = job.created_at.to_rfc3339();
        let updated_at = job.updated_at.to_rfc3339();
        let last_run = job.last_run_at.map(|d| d.to_rfc3339());
        let next_run = job.next_run_at.map(|d| d.to_rfc3339());

        sqlx::query(
            "INSERT OR REPLACE INTO cron_jobs (id, name, schedule, command, config, enabled, created_by, created_at, updated_at, last_run_at, next_run_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&job.id)
        .bind(&job.name)
        .bind(&job.schedule)
        .bind(&job.command)
        .bind(&config)
        .bind(job.enabled)
        .bind(&job.created_by)
        .bind(&created_at)
        .bind(&updated_at)
        .bind(&last_run)
        .bind(&next_run)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("save cron job: {e}")))?;

        Ok(())
    }

    async fn get_cron_job(&self, id: &str) -> Result<Option<CronJob>, AttaError> {
        let row = sqlx::query("SELECT * FROM cron_jobs WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("get cron job: {e}")))?;

        match row {
            Some(row) => Ok(Some(cron_job_from_row(&row)?)),
            None => Ok(None),
        }
    }

    async fn list_cron_jobs(&self, status: Option<&str>) -> Result<Vec<CronJob>, AttaError> {
        let rows = match status {
            Some("active") => {
                sqlx::query("SELECT * FROM cron_jobs WHERE enabled = 1 ORDER BY created_at DESC")
                    .fetch_all(&self.pool)
                    .await
            }
            Some("paused") => {
                sqlx::query("SELECT * FROM cron_jobs WHERE enabled = 0 ORDER BY created_at DESC")
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
            jobs.push(cron_job_from_row(row)?);
        }
        Ok(jobs)
    }

    async fn delete_cron_job(&self, id: &str) -> Result<(), AttaError> {
        sqlx::query("DELETE FROM cron_jobs WHERE id = ?")
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
        let started_at = run.started_at.to_rfc3339();
        let completed_at = run.completed_at.map(|d| d.to_rfc3339());

        sqlx::query(
            "INSERT OR REPLACE INTO cron_runs (id, job_id, status, started_at, completed_at, output, error, triggered_by)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&run.id)
        .bind(&run.job_id)
        .bind(status_str)
        .bind(&started_at)
        .bind(&completed_at)
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
            "SELECT * FROM cron_runs WHERE job_id = ? ORDER BY started_at DESC LIMIT ?",
        )
        .bind(job_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("list cron runs: {e}")))?;

        let mut runs = Vec::with_capacity(rows.len());
        for row in &rows {
            runs.push(cron_run_from_row(row)?);
        }
        Ok(runs)
    }
}

// ── RbacStore ──

#[async_trait::async_trait]
impl RbacStore for SqliteStore {
    async fn get_roles_for_actor(
        &self,
        actor_id: &str,
    ) -> Result<Vec<atta_types::Role>, AttaError> {
        let rows = sqlx::query("SELECT role FROM role_bindings WHERE actor_id = ?")
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

        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT OR IGNORE INTO role_bindings (id, actor_id, actor_type, role, granted_by, granted_at) VALUES (?, ?, 'user', ?, 'system', datetime('now'))",
        )
        .bind(&id)
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

        sqlx::query("DELETE FROM role_bindings WHERE actor_id = ? AND role = ?")
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
impl UsageStore for SqliteStore {
    async fn record_usage(&self, record: &UsageRecord) -> Result<(), AttaError> {
        let created_at = record.created_at.to_rfc3339();
        sqlx::query(
            "INSERT INTO usage_records (id, task_id, model, input_tokens, output_tokens, total_tokens, cost_usd, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&record.id)
        .bind(&record.task_id)
        .bind(&record.model)
        .bind(record.input_tokens as i64)
        .bind(record.output_tokens as i64)
        .bind(record.total_tokens as i64)
        .bind(record.cost_usd)
        .bind(&created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("record usage: {e}")))?;

        debug!(id = %record.id, model = %record.model, tokens = record.total_tokens, "usage recorded");
        Ok(())
    }

    async fn get_usage_summary(&self, since: DateTime<Utc>) -> Result<UsageSummary, AttaError> {
        let since_str = since.to_rfc3339();

        // Aggregate totals
        let row = sqlx::query(
            "SELECT COALESCE(SUM(input_tokens), 0) as input_tokens,
                    COALESCE(SUM(output_tokens), 0) as output_tokens,
                    COALESCE(SUM(total_tokens), 0) as total_tokens,
                    COALESCE(SUM(cost_usd), 0.0) as total_cost,
                    COUNT(*) as request_count
             FROM usage_records WHERE created_at >= ?",
        )
        .bind(&since_str)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("usage summary: {e}")))?;

        let input_tokens: i64 = row.try_get("input_tokens").unwrap_or(0);
        let output_tokens: i64 = row.try_get("output_tokens").unwrap_or(0);
        let total_tokens: i64 = row.try_get("total_tokens").unwrap_or(0);
        let total_cost: f64 = row.try_get("total_cost").unwrap_or(0.0);
        let request_count: i64 = row.try_get("request_count").unwrap_or(0);

        // Aggregate by model
        let model_rows = sqlx::query(
            "SELECT model,
                    COALESCE(SUM(total_tokens), 0) as tokens,
                    COALESCE(SUM(cost_usd), 0.0) as cost_usd,
                    COUNT(*) as request_count
             FROM usage_records WHERE created_at >= ?
             GROUP BY model ORDER BY tokens DESC",
        )
        .bind(&since_str)
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
            period: String::new(), // caller fills this in
        })
    }

    async fn get_usage_daily(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<UsageDaily>, AttaError> {
        let start_str = start.to_rfc3339();
        let end_str = end.to_rfc3339();

        let rows = sqlx::query(
            "SELECT DATE(created_at) as date,
                    COALESCE(SUM(total_tokens), 0) as tokens,
                    COALESCE(SUM(cost_usd), 0.0) as cost_usd,
                    COALESCE(SUM(input_tokens), 0) as input_tokens,
                    COALESCE(SUM(output_tokens), 0) as output_tokens
             FROM usage_records
             WHERE created_at >= ? AND created_at < ?
             GROUP BY DATE(created_at)
             ORDER BY date",
        )
        .bind(&start_str)
        .bind(&end_str)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("usage daily: {e}")))?;

        let daily: Vec<UsageDaily> = rows
            .iter()
            .map(|r| {
                let date: String = r.try_get("date").unwrap_or_default();
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
impl RemoteAgentStore for SqliteStore {
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
        let registered_at = agent.registered_at.to_rfc3339();

        let token_expires_at = agent.token_expires_at.map(|t| t.to_rfc3339());

        sqlx::query(
            "INSERT INTO remote_agents (id, name, token_hash, description, version, capabilities, status, registered_at, registered_by, token_expires_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&agent.id)
        .bind(&agent.name)
        .bind(token_hash)
        .bind(&agent.description)
        .bind(&agent.version)
        .bind(&capabilities)
        .bind(status)
        .bind(&registered_at)
        .bind(&agent.registered_by)
        .bind(&token_expires_at)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Database(format!("register remote agent: {e}")))?;

        Ok(())
    }

    async fn get_remote_agent(
        &self,
        id: &str,
    ) -> Result<Option<atta_types::RemoteAgent>, AttaError> {
        let row = sqlx::query("SELECT * FROM remote_agents WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("get remote agent: {e}")))?;

        match row {
            Some(r) => Ok(Some(remote_agent_from_row(&r)?)),
            None => Ok(None),
        }
    }

    async fn get_remote_agent_by_token(
        &self,
        token_hash: &str,
    ) -> Result<Option<atta_types::RemoteAgent>, AttaError> {
        let row = sqlx::query("SELECT * FROM remote_agents WHERE token_hash = ?")
            .bind(token_hash)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("get remote agent by token: {e}")))?;

        match row {
            Some(r) => Ok(Some(remote_agent_from_row(&r)?)),
            None => Ok(None),
        }
    }

    async fn list_remote_agents(&self) -> Result<Vec<atta_types::RemoteAgent>, AttaError> {
        let rows = sqlx::query("SELECT * FROM remote_agents ORDER BY registered_at DESC")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("list remote agents: {e}")))?;

        rows.iter().map(remote_agent_from_row).collect()
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

        sqlx::query("UPDATE remote_agents SET status = ? WHERE id = ?")
            .bind(status_str)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("update remote agent status: {e}")))?;

        Ok(())
    }

    async fn update_remote_agent_heartbeat(&self, id: &str) -> Result<(), AttaError> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query("UPDATE remote_agents SET last_heartbeat = ?, status = 'online' WHERE id = ?")
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("update heartbeat: {e}")))?;

        Ok(())
    }

    async fn delete_remote_agent(&self, id: &str) -> Result<(), AttaError> {
        sqlx::query("DELETE FROM remote_agents WHERE id = ?")
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
        let expires_str = expires_at.map(|t| t.to_rfc3339());
        sqlx::query("UPDATE remote_agents SET token_hash = ?, token_expires_at = ? WHERE id = ?")
            .bind(new_token_hash)
            .bind(&expires_str)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Database(format!("rotate remote agent token: {e}")))?;
        Ok(())
    }
}

// ── StateStore (super-trait blanket impl) ──

impl StateStore for SqliteStore {}

// ── 行解析辅助函数 ──

/// 从数据库行构建 RemoteAgent
fn remote_agent_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<atta_types::RemoteAgent, AttaError> {
    let id: String = row.try_get("id").map_err(|e| StoreError::Database(e.to_string()))?;
    let name: String = row.try_get("name").map_err(|e| StoreError::Database(e.to_string()))?;
    let description: String = row.try_get("description").unwrap_or_default();
    let version: String = row.try_get("version").unwrap_or_else(|_| "0.1.0".to_string());
    let caps_str: String = row.try_get("capabilities").unwrap_or_else(|_| "[]".to_string());
    let status_str: String = row.try_get("status").unwrap_or_else(|_| "offline".to_string());
    let heartbeat_str: Option<String> = row.try_get("last_heartbeat").ok().flatten();
    let registered_at_str: String = row
        .try_get("registered_at")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let registered_by: String = row
        .try_get("registered_by")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let token_expires_at_str: Option<String> = row.try_get("token_expires_at").ok().flatten();

    let capabilities: Vec<String> = serde_json::from_str(&caps_str).unwrap_or_default();
    let status = match status_str.as_str() {
        "online" => atta_types::RemoteAgentStatus::Online,
        _ => atta_types::RemoteAgentStatus::Offline,
    };
    let last_heartbeat = heartbeat_str
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));
    let registered_at = DateTime::parse_from_rfc3339(&registered_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let token_expires_at = token_expires_at_str
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&chrono::Utc)));

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

/// 从数据库行构建 ToolDef
fn tool_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<ToolDef, AttaError> {
    let name: String = row
        .try_get("name")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let description: String = row
        .try_get("description")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let plugin_name: Option<String> = row.try_get("plugin_name").ok();
    let mcp_server: Option<String> = row.try_get("mcp_server").ok();
    let risk_level_str: String = row
        .try_get("risk_level")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let parameters_str: String = row
        .try_get("parameters")
        .map_err(|e| StoreError::Database(e.to_string()))?;

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
    } else if let Some(ref ms) = mcp_server {
        atta_types::ToolBinding::Mcp {
            server_name: ms.clone(),
        }
    } else {
        return Err(StoreError::Database(format!(
            "tool '{}' has neither plugin_name nor mcp_server",
            name
        ))
        .into());
    };

    let risk_level = match risk_level_str.as_str() {
        "low" => atta_types::RiskLevel::Low,
        "medium" => atta_types::RiskLevel::Medium,
        "high" => atta_types::RiskLevel::High,
        _ => atta_types::RiskLevel::Low,
    };

    let parameters: serde_json::Value = serde_json::from_str(&parameters_str)
        .map_err(|e| StoreError::Serialization(format!("parameters: {e}")))?;

    Ok(ToolDef {
        name,
        description,
        binding,
        risk_level,
        parameters,
    })
}

/// 从数据库行构建 NodeInfo
fn node_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<NodeInfo, AttaError> {
    let id: String = row
        .try_get("id")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let hostname: String = row
        .try_get("hostname")
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
    let running_agents: i64 = row
        .try_get("running_agents")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let max_concurrent: i64 = row
        .try_get("max_concurrent")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let last_heartbeat_str: String = row
        .try_get("last_heartbeat")
        .map_err(|e| StoreError::Database(e.to_string()))?;

    let labels_str: String = row.try_get("labels").unwrap_or_else(|_| "[]".to_string());
    let labels: Vec<String> = serde_json::from_str(&labels_str).unwrap_or_default();

    let status = match status_str.as_str() {
        "online" => NodeStatus::Online,
        "draining" => NodeStatus::Draining,
        "offline" => NodeStatus::Offline,
        _ => NodeStatus::Offline,
    };

    let last_heartbeat = DateTime::parse_from_rfc3339(&last_heartbeat_str)
        .map_err(|e| StoreError::Database(format!("invalid last_heartbeat: {e}")))?
        .with_timezone(&Utc);

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

/// 从数据库行构建 ApprovalRequest
fn approval_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<ApprovalRequest, AttaError> {
    let id_str: String = row
        .try_get("id")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let task_id_str: String = row
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
    let resolved_by_str: Option<String> = row.try_get("resolved_by").ok();
    let resolved_at_str: Option<String> = row.try_get("resolved_at").ok();
    let timeout_at_str: String = row
        .try_get("timeout_at")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let created_at_str: String = row
        .try_get("created_at")
        .map_err(|e| StoreError::Database(e.to_string()))?;

    let id =
        Uuid::parse_str(&id_str).map_err(|e| StoreError::Database(format!("invalid UUID: {e}")))?;
    let task_id = Uuid::parse_str(&task_id_str)
        .map_err(|e| StoreError::Database(format!("invalid task_id UUID: {e}")))?;
    let requested_by: Actor =
        serde_json::from_str(&requested_by_str).unwrap_or_else(|_| Actor::system());
    let approver_role = serde_json::from_value(serde_json::Value::String(approver_role_str))
        .unwrap_or(atta_types::Role::Approver);
    let status: ApprovalStatus = serde_json::from_value(serde_json::Value::String(status_str))
        .unwrap_or(ApprovalStatus::Pending);
    let context: atta_types::ApprovalContext = serde_json::from_str(&context_str)
        .map_err(|e| StoreError::Serialization(format!("approval context: {e}")))?;
    let resolved_by: Option<Actor> = resolved_by_str.and_then(|s| serde_json::from_str(&s).ok());
    let resolved_at = resolved_at_str
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|dt| dt.with_timezone(&Utc));
    let timeout_at = DateTime::parse_from_rfc3339(&timeout_at_str)
        .map_err(|e| StoreError::Database(format!("invalid timeout_at: {e}")))?
        .with_timezone(&Utc);
    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map_err(|e| StoreError::Database(format!("invalid created_at: {e}")))?
        .with_timezone(&Utc);

    // 计算 timeout duration（从 created_at 到 timeout_at）
    let timeout_duration = (timeout_at - created_at).to_std().unwrap_or_default();

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

/// 从数据库行构建 CronJob
fn cron_job_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<CronJob, AttaError> {
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
    let created_at: String = row
        .try_get("created_at")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let updated_at: String = row
        .try_get("updated_at")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let last_run_at: Option<String> = row.try_get("last_run_at").ok();
    let next_run_at: Option<String> = row.try_get("next_run_at").ok();

    let config: serde_json::Value = serde_json::from_str(&config_str).unwrap_or_default();

    Ok(CronJob {
        id,
        name,
        schedule,
        command,
        config,
        enabled,
        created_by,
        created_at: DateTime::parse_from_rfc3339(&created_at)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        updated_at: DateTime::parse_from_rfc3339(&updated_at)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        last_run_at: last_run_at.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|d| d.with_timezone(&Utc))
        }),
        next_run_at: next_run_at.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|d| d.with_timezone(&Utc))
        }),
    })
}

/// 从数据库行构建 CronRun
fn cron_run_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<CronRun, AttaError> {
    let id: String = row
        .try_get("id")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let job_id: String = row
        .try_get("job_id")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let status_str: String = row
        .try_get("status")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let started_at: String = row
        .try_get("started_at")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let completed_at: Option<String> = row.try_get("completed_at").ok();
    let output: Option<String> = row.try_get("output").ok();
    let error: Option<String> = row.try_get("error").ok();
    let triggered_by: String = row
        .try_get("triggered_by")
        .unwrap_or_else(|_| "scheduler".to_string());

    let status = match status_str.as_str() {
        "completed" => CronRunStatus::Completed,
        "failed" => CronRunStatus::Failed,
        _ => CronRunStatus::Running,
    };

    Ok(CronRun {
        id,
        job_id,
        status,
        started_at: DateTime::parse_from_rfc3339(&started_at)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        completed_at: completed_at.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|d| d.with_timezone(&Utc))
        }),
        output,
        error,
        triggered_by,
    })
}

/// 从数据库行构建 McpServerConfig
fn mcp_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<McpServerConfig, AttaError> {
    let name: String = row
        .try_get("name")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let description: Option<String> = row.try_get("description").ok();
    let transport_str: String = row
        .try_get("transport")
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let url: Option<String> = row.try_get("url").ok();
    let command: Option<String> = row.try_get("command").ok();
    let args_str: String = row.try_get("args").unwrap_or_else(|_| "[]".to_string());
    let auth_config_str: Option<String> = row.try_get("auth_config").ok();

    let transport = match transport_str.as_str() {
        "stdio" => atta_types::McpTransport::Stdio,
        "sse" => atta_types::McpTransport::Sse,
        _ => atta_types::McpTransport::Stdio,
    };

    let args: Vec<String> = serde_json::from_str(&args_str).unwrap_or_default();
    let auth = auth_config_str.and_then(|s| serde_json::from_str(&s).ok());

    Ok(McpServerConfig {
        name,
        description,
        transport,
        url,
        command,
        args,
        auth,
    })
}

// ── Unit Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FlowStore, TaskStore};
    use serde_json::json;

    /// Create a test store using an ephemeral temp file.
    ///
    /// In-memory SQLite (`:memory:`) does not work reliably with connection
    /// pools because each connection gets its own database. We use a unique
    /// temp file instead -- it lives only for the duration of the test.
    async fn test_store() -> SqliteStore {
        let path = format!(
            "/tmp/atta_store_unit_test_{}.db",
            Uuid::new_v4().to_string().replace('-', "")
        );
        SqliteStore::open(&path).await.unwrap()
    }

    /// Build a minimal Task for testing.
    fn make_task(flow_id: &str) -> Task {
        let now = Utc::now();
        Task {
            id: Uuid::new_v4(),
            flow_id: flow_id.to_string(),
            current_state: "start".to_string(),
            state_data: json!({"step": 0}),
            input: json!({"prompt": "hello"}),
            output: None,
            status: TaskStatus::Running,
            created_by: Actor::user("test-user"),
            created_at: now,
            updated_at: now,
            completed_at: None,
            version: 0,
        }
    }

    /// Build a minimal FlowDef for testing.
    fn make_flow_def(id: &str) -> FlowDef {
        FlowDef {
            id: id.to_string(),
            version: "0.1.0".to_string(),
            name: Some(format!("Test flow {id}")),
            description: None,
            initial_state: "start".to_string(),
            states: std::collections::HashMap::new(),
            on_error: None,
            skills: vec![],
            source: "builtin".to_string(),
        }
    }

    /// Build a FlowState for a given task id.
    fn make_flow_state(task_id: Uuid) -> FlowState {
        FlowState {
            task_id,
            current_state: "start".to_string(),
            history: vec![],
            pending_approval: None,
            retry_count: 0,
        }
    }

    // ── task_from_row round-trip ──

    #[tokio::test]
    async fn task_round_trip_all_fields() {
        let store = test_store().await;
        let now = Utc::now();
        let task = Task {
            id: Uuid::new_v4(),
            flow_id: "round-trip-flow".to_string(),
            current_state: "processing".to_string(),
            state_data: json!({"key": "value", "nested": {"a": 1}}),
            input: json!({"prompt": "test input", "params": [1, 2, 3]}),
            output: Some(json!({"result": "success"})),
            status: TaskStatus::Running,
            created_by: Actor::user("alice"),
            created_at: now,
            updated_at: now,
            completed_at: None,
            version: 0,
        };

        store.create_task(&task).await.unwrap();

        let fetched = store.get_task(&task.id).await.unwrap().unwrap();

        assert_eq!(fetched.id, task.id);
        assert_eq!(fetched.flow_id, "round-trip-flow");
        assert_eq!(fetched.current_state, "processing");
        assert_eq!(
            fetched.state_data,
            json!({"key": "value", "nested": {"a": 1}})
        );
        assert_eq!(
            fetched.input,
            json!({"prompt": "test input", "params": [1, 2, 3]})
        );
        assert_eq!(fetched.output, Some(json!({"result": "success"})));
        assert!(matches!(fetched.status, TaskStatus::Running));
        assert!(fetched.completed_at.is_none());
    }

    #[tokio::test]
    async fn task_round_trip_with_failed_status() {
        let store = test_store().await;
        let now = Utc::now();
        let task = Task {
            id: Uuid::new_v4(),
            flow_id: "fail-flow".to_string(),
            current_state: "error".to_string(),
            state_data: json!({}),
            input: json!({}),
            output: None,
            status: TaskStatus::Failed {
                error: "something went wrong".to_string(),
            },
            created_by: Actor::user("bob"),
            created_at: now,
            updated_at: now,
            completed_at: None,
            version: 0,
        };

        store.create_task(&task).await.unwrap();

        let fetched = store.get_task(&task.id).await.unwrap().unwrap();

        assert_eq!(fetched.id, task.id);
        assert_eq!(fetched.flow_id, "fail-flow");
        match &fetched.status {
            TaskStatus::Failed { error } => {
                assert_eq!(error, "something went wrong");
            }
            other => panic!("expected Failed status, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn task_round_trip_completed_with_timestamp() {
        let store = test_store().await;
        let now = Utc::now();
        let task = Task {
            id: Uuid::new_v4(),
            flow_id: "complete-flow".to_string(),
            current_state: "done".to_string(),
            state_data: json!({}),
            input: json!({"x": 42}),
            output: Some(json!({"answer": 42})),
            status: TaskStatus::Completed,
            created_by: Actor::system(),
            created_at: now,
            updated_at: now,
            completed_at: Some(now),
            version: 0,
        };

        store.create_task(&task).await.unwrap();

        let fetched = store.get_task(&task.id).await.unwrap().unwrap();

        assert_eq!(fetched.id, task.id);
        assert!(matches!(fetched.status, TaskStatus::Completed));
        assert!(fetched.completed_at.is_some());
        assert!(fetched.output.is_some());
    }

    #[tokio::test]
    async fn task_round_trip_no_output() {
        let store = test_store().await;
        let task = make_task("no-output-flow");

        store.create_task(&task).await.unwrap();

        let fetched = store.get_task(&task.id).await.unwrap().unwrap();

        assert_eq!(fetched.id, task.id);
        assert!(fetched.output.is_none());
    }

    #[tokio::test]
    async fn get_nonexistent_task_returns_none() {
        let store = test_store().await;
        let result = store.get_task(&Uuid::new_v4()).await.unwrap();
        assert!(result.is_none());
    }

    // ── LIKE escaping ──

    #[tokio::test]
    async fn list_tasks_like_escape_percent() {
        let store = test_store().await;

        // Create a task whose created_by serialized form contains a literal '%'
        let now = Utc::now();
        let task_with_percent = Task {
            id: Uuid::new_v4(),
            flow_id: "like-test".to_string(),
            current_state: "start".to_string(),
            state_data: json!({}),
            input: json!({}),
            output: None,
            status: TaskStatus::Running,
            created_by: Actor::user("user%admin"),
            created_at: now,
            updated_at: now,
            completed_at: None,
            version: 0,
        };
        let task_other = Task {
            id: Uuid::new_v4(),
            flow_id: "like-test".to_string(),
            current_state: "start".to_string(),
            state_data: json!({}),
            input: json!({}),
            output: None,
            status: TaskStatus::Running,
            created_by: Actor::user("normaluser"),
            created_at: now,
            updated_at: now,
            completed_at: None,
            version: 0,
        };

        store.create_task(&task_with_percent).await.unwrap();
        store.create_task(&task_other).await.unwrap();

        // Search for the literal '%' character -- it should NOT act as a wildcard
        let filter = TaskFilter {
            created_by: Some("user%admin".to_string()),
            limit: 100,
            ..Default::default()
        };
        let results = store.list_tasks(&filter).await.unwrap();

        // Should find exactly the task with '%' in its name, not both tasks
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, task_with_percent.id);
    }

    #[tokio::test]
    async fn list_tasks_like_escape_underscore() {
        let store = test_store().await;

        let now = Utc::now();
        let task_with_underscore = Task {
            id: Uuid::new_v4(),
            flow_id: "like-test".to_string(),
            current_state: "start".to_string(),
            state_data: json!({}),
            input: json!({}),
            output: None,
            status: TaskStatus::Running,
            created_by: Actor::user("user_name"),
            created_at: now,
            updated_at: now,
            completed_at: None,
            version: 0,
        };
        let task_other = Task {
            id: Uuid::new_v4(),
            flow_id: "like-test".to_string(),
            current_state: "start".to_string(),
            state_data: json!({}),
            input: json!({}),
            output: None,
            status: TaskStatus::Running,
            created_by: Actor::user("userXname"),
            created_at: now,
            updated_at: now,
            completed_at: None,
            version: 0,
        };

        store.create_task(&task_with_underscore).await.unwrap();
        store.create_task(&task_other).await.unwrap();

        // Search for literal '_' -- should NOT match any single char
        let filter = TaskFilter {
            created_by: Some("user_name".to_string()),
            limit: 100,
            ..Default::default()
        };
        let results = store.list_tasks(&filter).await.unwrap();

        // '_' is escaped, so "userXname" should NOT match
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, task_with_underscore.id);
    }

    #[tokio::test]
    async fn list_tasks_like_escape_combined_special_chars() {
        let store = test_store().await;

        let now = Utc::now();
        // Actor with both '%' and '_' in the name
        let task = Task {
            id: Uuid::new_v4(),
            flow_id: "like-test".to_string(),
            current_state: "start".to_string(),
            state_data: json!({}),
            input: json!({}),
            output: None,
            status: TaskStatus::Running,
            created_by: Actor::user("admin%super_user"),
            created_at: now,
            updated_at: now,
            completed_at: None,
            version: 0,
        };
        let task_decoy = Task {
            id: Uuid::new_v4(),
            flow_id: "like-test".to_string(),
            current_state: "start".to_string(),
            state_data: json!({}),
            input: json!({}),
            output: None,
            status: TaskStatus::Running,
            created_by: Actor::user("adminXsuperXuser"),
            created_at: now,
            updated_at: now,
            completed_at: None,
            version: 0,
        };

        store.create_task(&task).await.unwrap();
        store.create_task(&task_decoy).await.unwrap();

        let filter = TaskFilter {
            created_by: Some("admin%super_user".to_string()),
            limit: 100,
            ..Default::default()
        };
        let results = store.list_tasks(&filter).await.unwrap();

        // Only the exact match should be returned; the decoy should not
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, task.id);
    }

    // ── advance_task transaction ──

    #[tokio::test]
    async fn advance_task_updates_task_and_flow_atomically() {
        let store = test_store().await;
        store
            .save_flow_def(&make_flow_def("advance-flow"))
            .await
            .unwrap();
        let task = make_task("advance-flow");
        let task_id = task.id;
        let flow_state = make_flow_state(task_id);

        // Create task with initial flow state in a transaction
        store
            .create_task_with_flow(&task, &flow_state)
            .await
            .unwrap();

        // Verify initial state
        let fetched = store.get_task(&task_id).await.unwrap().unwrap();
        assert_eq!(fetched.current_state, "start");
        assert!(matches!(fetched.status, TaskStatus::Running));

        let fs = store.get_flow_state(&task_id).await.unwrap().unwrap();
        assert_eq!(fs.current_state, "start");
        assert!(fs.history.is_empty());

        // Advance: start -> processing
        let transition = StateTransition {
            from: "start".to_string(),
            to: "processing".to_string(),
            reason: "auto transition".to_string(),
            timestamp: Utc::now(),
        };

        store
            .advance_task(&task_id, TaskStatus::Running, "processing", &transition, 0)
            .await
            .unwrap();

        // Verify both task and flow state updated
        let fetched = store.get_task(&task_id).await.unwrap().unwrap();
        assert_eq!(fetched.current_state, "processing");
        assert!(matches!(fetched.status, TaskStatus::Running));

        let fs = store.get_flow_state(&task_id).await.unwrap().unwrap();
        assert_eq!(fs.current_state, "processing");
        assert_eq!(fs.history.len(), 1);
        assert_eq!(fs.history[0].from, "start");
        assert_eq!(fs.history[0].to, "processing");
        assert_eq!(fs.history[0].reason, "auto transition");
    }

    #[tokio::test]
    async fn advance_task_accumulates_history() {
        let store = test_store().await;
        store
            .save_flow_def(&make_flow_def("multi-advance-flow"))
            .await
            .unwrap();
        let task = make_task("multi-advance-flow");
        let task_id = task.id;
        let flow_state = make_flow_state(task_id);

        store
            .create_task_with_flow(&task, &flow_state)
            .await
            .unwrap();

        // Advance: start -> processing
        let t1 = StateTransition {
            from: "start".to_string(),
            to: "processing".to_string(),
            reason: "step 1".to_string(),
            timestamp: Utc::now(),
        };
        store
            .advance_task(&task_id, TaskStatus::Running, "processing", &t1, 0)
            .await
            .unwrap();

        // Advance: processing -> review
        let t2 = StateTransition {
            from: "processing".to_string(),
            to: "review".to_string(),
            reason: "step 2".to_string(),
            timestamp: Utc::now(),
        };
        store
            .advance_task(&task_id, TaskStatus::WaitingApproval, "review", &t2, 1)
            .await
            .unwrap();

        // Advance: review -> end (completed)
        let t3 = StateTransition {
            from: "review".to_string(),
            to: "end".to_string(),
            reason: "approved".to_string(),
            timestamp: Utc::now(),
        };
        store
            .advance_task(&task_id, TaskStatus::Completed, "end", &t3, 2)
            .await
            .unwrap();

        // Verify final task state
        let fetched = store.get_task(&task_id).await.unwrap().unwrap();
        assert_eq!(fetched.current_state, "end");
        assert!(matches!(fetched.status, TaskStatus::Completed));

        // Verify full transition history preserved
        let fs = store.get_flow_state(&task_id).await.unwrap().unwrap();
        assert_eq!(fs.current_state, "end");
        assert_eq!(fs.history.len(), 3);
        assert_eq!(fs.history[0].from, "start");
        assert_eq!(fs.history[0].to, "processing");
        assert_eq!(fs.history[1].from, "processing");
        assert_eq!(fs.history[1].to, "review");
        assert_eq!(fs.history[2].from, "review");
        assert_eq!(fs.history[2].to, "end");
    }
}
