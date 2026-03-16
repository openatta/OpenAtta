//! Manually trigger a cron task

use std::sync::Arc;

use atta_types::{AttaError, CronScheduler, RiskLevel};
use serde_json::{json, Value};

/// Manually triggers a scheduled cron task for immediate execution
pub struct CronRunTool {
    /// Optional cron scheduler backend
    pub scheduler: Option<Arc<dyn CronScheduler>>,
}

impl CronRunTool {
    /// Create a new CronRunTool without a scheduler backend
    pub fn new() -> Self {
        Self { scheduler: None }
    }

    /// Attach a cron scheduler backend
    pub fn with_scheduler(mut self, scheduler: Arc<dyn CronScheduler>) -> Self {
        self.scheduler = Some(scheduler);
        self
    }
}

impl Default for CronRunTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl atta_types::NativeTool for CronRunTool {
    fn name(&self) -> &str {
        "atta-cron-run"
    }

    fn description(&self) -> &str {
        "Manually trigger a scheduled cron task for immediate execution"
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
                    "description": "ID of the cron task to run"
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
                "message": "CronEngine is not available. Triggering cron tasks requires the cron engine to be configured."
            }));
        };

        let run = scheduler.trigger_job(task_id).await?;
        Ok(json!({
            "status": "triggered",
            "run": run,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_cron_run_name() {
        assert_eq!(CronRunTool::new().name(), "atta-cron-run");
    }

    #[tokio::test]
    async fn test_cron_run_no_scheduler() {
        let tool = CronRunTool::new();
        let result = tool.execute(json!({"task_id": "test-123"})).await.unwrap();
        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("not available"));
    }
}
