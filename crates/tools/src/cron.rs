//! Cron/scheduled task tool
//!
//! Main cron tool supporting schedule/list/cancel actions,
//! wired to `CronScheduler` trait for engine integration.

use std::sync::Arc;

use atta_types::{AttaError, CronScheduler, RiskLevel};
use serde_json::{json, Value};

/// Schedule and manage cron-like tasks
pub struct CronTool {
    /// Optional cron scheduler backend
    pub scheduler: Option<Arc<dyn CronScheduler>>,
}

impl CronTool {
    /// Create a new CronTool without a scheduler backend
    pub fn new() -> Self {
        Self { scheduler: None }
    }

    /// Attach a cron scheduler backend
    pub fn with_scheduler(mut self, scheduler: Arc<dyn CronScheduler>) -> Self {
        self.scheduler = Some(scheduler);
        self
    }
}

impl Default for CronTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl atta_types::NativeTool for CronTool {
    fn name(&self) -> &str {
        "atta-cron"
    }

    fn description(&self) -> &str {
        "Schedule, list, or cancel periodic tasks"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Medium
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["schedule", "list", "cancel"],
                    "description": "Cron action"
                },
                "schedule": {
                    "type": "string",
                    "description": "Cron expression (e.g., '*/5 * * * *')"
                },
                "command": {
                    "type": "string",
                    "description": "Command to schedule"
                },
                "job_id": {
                    "type": "string",
                    "description": "Job ID for cancel action"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let action = args["action"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'action' is required".into()))?;

        let Some(ref scheduler) = self.scheduler else {
            return Ok(json!({
                "status": "error",
                "message": "CronEngine is not available. Cron scheduling requires the cron engine to be configured."
            }));
        };

        match action {
            "schedule" => {
                let schedule_expr = args["schedule"].as_str().ok_or_else(|| {
                    AttaError::Validation("'schedule' is required for schedule action".into())
                })?;
                let command = args["command"].as_str().ok_or_else(|| {
                    AttaError::Validation("'command' is required for schedule action".into())
                })?;

                let job = scheduler
                    .schedule_job(command, schedule_expr, command)
                    .await?;
                Ok(json!({
                    "status": "scheduled",
                    "job": job,
                }))
            }
            "list" => {
                let jobs = scheduler.list_jobs(None).await?;
                Ok(json!({
                    "jobs": jobs,
                }))
            }
            "cancel" => {
                let job_id = args["job_id"].as_str().ok_or_else(|| {
                    AttaError::Validation("'job_id' is required for cancel action".into())
                })?;

                scheduler.cancel_job(job_id).await?;
                Ok(json!({
                    "status": "cancelled",
                    "job_id": job_id,
                }))
            }
            other => Err(AttaError::Validation(format!("unknown action: {}", other))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_cron_name() {
        assert_eq!(CronTool::new().name(), "atta-cron");
    }

    #[tokio::test]
    async fn test_cron_no_scheduler() {
        let tool = CronTool::new();
        let result = tool.execute(json!({"action": "list"})).await.unwrap();
        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("not available"));
    }
}
