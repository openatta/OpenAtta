//! Task 数据结构

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::Actor;
use crate::error::AttaError;

/// 任务（Flow 的运行实例）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Uuid,
    pub flow_id: String,
    pub current_state: String,
    pub state_data: serde_json::Value,
    pub input: serde_json::Value,
    pub output: Option<serde_json::Value>,
    pub status: TaskStatus,
    pub created_by: Actor,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    /// Optimistic concurrency version — incremented on each state transition.
    /// Used to prevent lost updates when concurrent transitions target the same task.
    #[serde(default)]
    pub version: u64,
}

impl Task {
    /// Validate task fields
    pub fn validate(&self) -> Result<(), AttaError> {
        if self.flow_id.trim().is_empty() {
            return Err(AttaError::Validation("task flow_id cannot be empty".to_string()));
        }
        Ok(())
    }
}

/// 任务状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Running,
    WaitingApproval,
    Completed,
    Failed { error: String },
    Cancelled,
}

impl TaskStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Running => "running",
            Self::WaitingApproval => "waiting_approval",
            Self::Completed => "completed",
            Self::Failed { .. } => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Task 列表查询过滤
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskFilter {
    pub status: Option<TaskStatus>,
    pub flow_id: Option<String>,
    pub created_by: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
}

fn default_limit() -> usize {
    20
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── TaskStatus serde round-trip ──

    #[test]
    fn task_status_running_serde_round_trip() {
        let status = TaskStatus::Running;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, r#""running""#);
        let back: TaskStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, TaskStatus::Running);
    }

    #[test]
    fn task_status_waiting_approval_serde_round_trip() {
        let status = TaskStatus::WaitingApproval;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, r#""waiting_approval""#);
        let back: TaskStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, TaskStatus::WaitingApproval);
    }

    #[test]
    fn task_status_completed_serde_round_trip() {
        let status = TaskStatus::Completed;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, r#""completed""#);
        let back: TaskStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, TaskStatus::Completed);
    }

    #[test]
    fn task_status_cancelled_serde_round_trip() {
        let status = TaskStatus::Cancelled;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, r#""cancelled""#);
        let back: TaskStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, TaskStatus::Cancelled);
    }

    #[test]
    fn task_status_failed_serde_round_trip() {
        let status = TaskStatus::Failed {
            error: "timeout".to_string(),
        };
        let json = serde_json::to_string(&status).unwrap();
        let back: TaskStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back,
            TaskStatus::Failed {
                error: "timeout".to_string()
            }
        );
    }

    #[test]
    fn task_status_failed_json_contains_error_field() {
        let status = TaskStatus::Failed {
            error: "oom".to_string(),
        };
        let val: serde_json::Value = serde_json::to_value(&status).unwrap();
        assert_eq!(val["failed"]["error"], "oom");
    }

    // ── TaskStatus::as_str ──

    #[test]
    fn task_status_as_str() {
        assert_eq!(TaskStatus::Running.as_str(), "running");
        assert_eq!(TaskStatus::WaitingApproval.as_str(), "waiting_approval");
        assert_eq!(TaskStatus::Completed.as_str(), "completed");
        assert_eq!(TaskStatus::Failed { error: "x".into() }.as_str(), "failed");
        assert_eq!(TaskStatus::Cancelled.as_str(), "cancelled");
    }

    // ── TaskFilter default ──

    #[test]
    fn task_filter_default_has_limit_20() {
        let filter = TaskFilter::default();
        assert_eq!(filter.limit, 0); // Default trait gives 0, serde default gives 20
        assert_eq!(filter.offset, 0);
        assert!(filter.status.is_none());
        assert!(filter.flow_id.is_none());
        assert!(filter.created_by.is_none());
    }

    #[test]
    fn task_filter_serde_default_limit() {
        // When deserialized from empty JSON, the serde default_limit fn provides 20
        let filter: TaskFilter = serde_json::from_str("{}").unwrap();
        assert_eq!(filter.limit, 20);
        assert_eq!(filter.offset, 0);
    }

    #[test]
    fn task_filter_serde_round_trip_with_values() {
        let filter = TaskFilter {
            status: Some(TaskStatus::Running),
            flow_id: Some("deploy".to_string()),
            created_by: Some("alice".to_string()),
            limit: 50,
            offset: 10,
        };
        let json = serde_json::to_string(&filter).unwrap();
        let back: TaskFilter = serde_json::from_str(&json).unwrap();
        assert_eq!(back.status, Some(TaskStatus::Running));
        assert_eq!(back.flow_id.as_deref(), Some("deploy"));
        assert_eq!(back.created_by.as_deref(), Some("alice"));
        assert_eq!(back.limit, 50);
        assert_eq!(back.offset, 10);
    }

    // ── Task serde round-trip ──

    #[test]
    fn task_serde_round_trip() {
        use crate::auth::Actor;
        use chrono::Utc;

        let now = Utc::now();
        let task = Task {
            id: Uuid::new_v4(),
            flow_id: "ci-pipeline".to_string(),
            current_state: "build".to_string(),
            state_data: serde_json::json!({"attempt": 1}),
            input: serde_json::json!({"repo": "github.com/test"}),
            output: Some(serde_json::json!({"status": "ok"})),
            status: TaskStatus::Completed,
            created_by: Actor::user("dev"),
            created_at: now,
            updated_at: now,
            completed_at: Some(now),
            version: 0,
        };

        let json = serde_json::to_string(&task).unwrap();
        let back: Task = serde_json::from_str(&json).unwrap();

        assert_eq!(back.id, task.id);
        assert_eq!(back.flow_id, "ci-pipeline");
        assert_eq!(back.current_state, "build");
        assert_eq!(back.status, TaskStatus::Completed);
        assert!(back.output.is_some());
        assert!(back.completed_at.is_some());
    }
}
