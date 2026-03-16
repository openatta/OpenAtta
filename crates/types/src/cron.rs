//! Cron 调度类型定义

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Cron 定时任务
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    /// 唯一 ID
    pub id: String,
    /// 任务名称
    pub name: String,
    /// Cron 表达式（如 "0 */5 * * * *"）
    pub schedule: String,
    /// 要执行的命令/动作
    pub command: String,
    /// 附加配置（JSON）
    #[serde(default)]
    pub config: serde_json::Value,
    /// 是否启用
    pub enabled: bool,
    /// 创建者
    pub created_by: String,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 更新时间
    pub updated_at: DateTime<Utc>,
    /// 上次运行时间
    pub last_run_at: Option<DateTime<Utc>>,
    /// 下次运行时间
    pub next_run_at: Option<DateTime<Utc>>,
}

/// Cron 运行记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronRun {
    /// 运行 ID
    pub id: String,
    /// 关联的 cron job ID
    pub job_id: String,
    /// 运行状态
    pub status: CronRunStatus,
    /// 开始时间
    pub started_at: DateTime<Utc>,
    /// 完成时间
    pub completed_at: Option<DateTime<Utc>>,
    /// 输出内容
    pub output: Option<String>,
    /// 错误信息
    pub error: Option<String>,
    /// 触发方式
    pub triggered_by: String,
}

/// Cron 运行状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CronRunStatus {
    Running,
    Completed,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn sample_cron_job() -> CronJob {
        let now = Utc::now();
        CronJob {
            id: "job-1".to_string(),
            name: "cleanup".to_string(),
            schedule: "0 0 * * *".to_string(),
            command: "delete-old-tasks".to_string(),
            config: serde_json::json!({"days": 30}),
            enabled: true,
            created_by: "admin".to_string(),
            created_at: now,
            updated_at: now,
            last_run_at: None,
            next_run_at: Some(now),
        }
    }

    fn sample_cron_run() -> CronRun {
        let now = Utc::now();
        CronRun {
            id: "run-1".to_string(),
            job_id: "job-1".to_string(),
            status: CronRunStatus::Completed,
            started_at: now,
            completed_at: Some(now),
            output: Some("deleted 42 tasks".to_string()),
            error: None,
            triggered_by: "scheduler".to_string(),
        }
    }

    // ── CronJob serde round-trip ──

    #[test]
    fn cron_job_serde_round_trip() {
        let job = sample_cron_job();
        let json = serde_json::to_string(&job).unwrap();
        let back: CronJob = serde_json::from_str(&json).unwrap();

        assert_eq!(back.id, "job-1");
        assert_eq!(back.name, "cleanup");
        assert_eq!(back.schedule, "0 0 * * *");
        assert_eq!(back.command, "delete-old-tasks");
        assert_eq!(back.config["days"], 30);
        assert!(back.enabled);
        assert_eq!(back.created_by, "admin");
        assert!(back.last_run_at.is_none());
        assert!(back.next_run_at.is_some());
    }

    #[test]
    fn cron_job_config_defaults_to_null_when_missing() {
        let now = Utc::now();
        // Build JSON without the config field; serde should use default (null)
        let json = serde_json::json!({
            "id": "job-2",
            "name": "test",
            "schedule": "* * * * *",
            "command": "echo",
            "enabled": false,
            "created_by": "system",
            "created_at": now,
            "updated_at": now,
            "last_run_at": null,
            "next_run_at": null
        });
        let job: CronJob = serde_json::from_value(json).unwrap();
        assert!(job.config.is_null());
    }

    #[test]
    fn cron_job_with_last_run_at() {
        let now = Utc::now();
        let mut job = sample_cron_job();
        job.last_run_at = Some(now);

        let json = serde_json::to_string(&job).unwrap();
        let back: CronJob = serde_json::from_str(&json).unwrap();
        assert!(back.last_run_at.is_some());
    }

    // ── CronRun serde round-trip ──

    #[test]
    fn cron_run_serde_round_trip() {
        let run = sample_cron_run();
        let json = serde_json::to_string(&run).unwrap();
        let back: CronRun = serde_json::from_str(&json).unwrap();

        assert_eq!(back.id, "run-1");
        assert_eq!(back.job_id, "job-1");
        assert_eq!(back.status, CronRunStatus::Completed);
        assert!(back.completed_at.is_some());
        assert_eq!(back.output.as_deref(), Some("deleted 42 tasks"));
        assert!(back.error.is_none());
        assert_eq!(back.triggered_by, "scheduler");
    }

    #[test]
    fn cron_run_with_error() {
        let now = Utc::now();
        let run = CronRun {
            id: "run-err".to_string(),
            job_id: "job-1".to_string(),
            status: CronRunStatus::Failed,
            started_at: now,
            completed_at: Some(now),
            output: None,
            error: Some("connection refused".to_string()),
            triggered_by: "manual".to_string(),
        };

        let json = serde_json::to_string(&run).unwrap();
        let back: CronRun = serde_json::from_str(&json).unwrap();
        assert_eq!(back.status, CronRunStatus::Failed);
        assert_eq!(back.error.as_deref(), Some("connection refused"));
        assert!(back.output.is_none());
    }

    // ── CronRunStatus serde ──

    #[test]
    fn cron_run_status_running_serialization() {
        let json = serde_json::to_string(&CronRunStatus::Running).unwrap();
        assert_eq!(json, r#""running""#);
        let back: CronRunStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, CronRunStatus::Running);
    }

    #[test]
    fn cron_run_status_completed_serialization() {
        let json = serde_json::to_string(&CronRunStatus::Completed).unwrap();
        assert_eq!(json, r#""completed""#);
        let back: CronRunStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, CronRunStatus::Completed);
    }

    #[test]
    fn cron_run_status_failed_serialization() {
        let json = serde_json::to_string(&CronRunStatus::Failed).unwrap();
        assert_eq!(json, r#""failed""#);
        let back: CronRunStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, CronRunStatus::Failed);
    }

    #[test]
    fn cron_run_status_rejects_unknown_value() {
        let result = serde_json::from_str::<CronRunStatus>(r#""paused""#);
        assert!(result.is_err());
    }
}
