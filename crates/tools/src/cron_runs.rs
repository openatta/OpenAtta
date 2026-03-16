//! View cron task execution history

use std::sync::Arc;

use atta_types::{AttaError, CronScheduler, RiskLevel};
use serde_json::{json, Value};

/// Views execution history and logs for a scheduled cron task
pub struct CronRunsTool {
    /// Optional cron scheduler backend
    pub scheduler: Option<Arc<dyn CronScheduler>>,
}

impl CronRunsTool {
    /// Create a new CronRunsTool without a scheduler backend
    pub fn new() -> Self {
        Self { scheduler: None }
    }

    /// Attach a cron scheduler backend
    pub fn with_scheduler(mut self, scheduler: Arc<dyn CronScheduler>) -> Self {
        self.scheduler = Some(scheduler);
        self
    }
}

impl Default for CronRunsTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl atta_types::NativeTool for CronRunsTool {
    fn name(&self) -> &str {
        "atta-cron-runs"
    }

    fn description(&self) -> &str {
        "View execution history and logs for a scheduled cron task"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "ID of the cron task"
                },
                "limit": {
                    "type": "integer",
                    "description": "Max number of runs to return",
                    "default": 10
                }
            },
            "required": ["task_id"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let task_id = args["task_id"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'task_id' is required".into()))?;
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

        let Some(ref scheduler) = self.scheduler else {
            return Ok(json!({
                "status": "error",
                "message": "CronEngine is not available. Viewing cron history requires the cron engine to be configured."
            }));
        };

        let runs = scheduler.job_history(task_id, limit).await?;
        Ok(json!({
            "task_id": task_id,
            "runs": runs,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_cron_runs_name() {
        assert_eq!(CronRunsTool::new().name(), "atta-cron-runs");
    }

    #[tokio::test]
    async fn test_cron_runs_no_scheduler() {
        let tool = CronRunsTool::new();
        let result = tool.execute(json!({"task_id": "test-123"})).await.unwrap();
        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("not available"));
    }
}
