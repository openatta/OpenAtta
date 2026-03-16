//! Update a scheduled cron task

use std::sync::Arc;

use atta_types::{AttaError, CronScheduler, RiskLevel};
use serde_json::{json, Value};

/// Updates the schedule or configuration of an existing cron task
pub struct CronUpdateTool {
    /// Optional cron scheduler backend
    pub scheduler: Option<Arc<dyn CronScheduler>>,
}

impl CronUpdateTool {
    /// Create a new CronUpdateTool without a scheduler backend
    pub fn new() -> Self {
        Self { scheduler: None }
    }

    /// Attach a cron scheduler backend
    pub fn with_scheduler(mut self, scheduler: Arc<dyn CronScheduler>) -> Self {
        self.scheduler = Some(scheduler);
        self
    }
}

impl Default for CronUpdateTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl atta_types::NativeTool for CronUpdateTool {
    fn name(&self) -> &str {
        "atta-cron-update"
    }

    fn description(&self) -> &str {
        "Update the schedule or configuration of an existing cron task"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Medium
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "ID of the cron task"
                },
                "schedule": {
                    "type": "string",
                    "description": "New cron expression (e.g. '0 */6 * * *')"
                },
                "enabled": {
                    "type": "boolean",
                    "description": "Enable or disable the task"
                }
            },
            "required": ["task_id"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let task_id = args["task_id"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'task_id' is required".into()))?;

        let Some(ref scheduler) = self.scheduler else {
            return Ok(json!({
                "status": "error",
                "message": "CronEngine is not available. Updating cron tasks requires the cron engine to be configured."
            }));
        };

        let schedule = args.get("schedule").and_then(|v| v.as_str());
        let enabled = args.get("enabled").and_then(|v| v.as_bool());

        if schedule.is_none() && enabled.is_none() {
            return Err(AttaError::Validation(
                "at least one of 'schedule' or 'enabled' must be provided".into(),
            ));
        }

        let updated = scheduler.update_job(task_id, schedule, enabled).await?;
        Ok(json!({
            "status": "updated",
            "job": updated,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_cron_update_name() {
        assert_eq!(CronUpdateTool::new().name(), "atta-cron-update");
    }

    #[tokio::test]
    async fn test_cron_update_no_scheduler() {
        let tool = CronUpdateTool::new();
        let result = tool
            .execute(json!({"task_id": "test-123", "enabled": false}))
            .await
            .unwrap();
        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("not available"));
    }
}
