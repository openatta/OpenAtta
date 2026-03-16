//! Remove a scheduled cron task

use std::sync::Arc;

use atta_types::{AttaError, CronScheduler, RiskLevel};
use serde_json::{json, Value};

/// Removes a scheduled cron task by ID
pub struct CronRemoveTool {
    /// Optional cron scheduler backend
    pub scheduler: Option<Arc<dyn CronScheduler>>,
}

impl CronRemoveTool {
    /// Create a new CronRemoveTool without a scheduler backend
    pub fn new() -> Self {
        Self { scheduler: None }
    }

    /// Attach a cron scheduler backend
    pub fn with_scheduler(mut self, scheduler: Arc<dyn CronScheduler>) -> Self {
        self.scheduler = Some(scheduler);
        self
    }
}

impl Default for CronRemoveTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl atta_types::NativeTool for CronRemoveTool {
    fn name(&self) -> &str {
        "atta-cron-remove"
    }

    fn description(&self) -> &str {
        "Remove a scheduled/cron task by its ID"
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
                    "description": "ID of the cron task to remove"
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
                "message": "CronEngine is not available. Removing cron tasks requires the cron engine to be configured."
            }));
        };

        scheduler.cancel_job(task_id).await?;
        Ok(json!({
            "status": "removed",
            "task_id": task_id,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_cron_remove_name() {
        assert_eq!(CronRemoveTool::new().name(), "atta-cron-remove");
    }

    #[tokio::test]
    async fn test_cron_remove_no_scheduler() {
        let tool = CronRemoveTool::new();
        let result = tool.execute(json!({"task_id": "test-123"})).await.unwrap();
        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("not available"));
    }
}
