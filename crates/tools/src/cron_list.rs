//! List scheduled cron tasks

use std::sync::Arc;

use atta_types::{AttaError, CronScheduler, RiskLevel};
use serde_json::{json, Value};

/// Lists all registered cron/scheduled tasks
pub struct CronListTool {
    /// Optional cron scheduler backend
    pub scheduler: Option<Arc<dyn CronScheduler>>,
}

impl CronListTool {
    /// Create a new CronListTool without a scheduler backend
    pub fn new() -> Self {
        Self { scheduler: None }
    }

    /// Attach a cron scheduler backend
    pub fn with_scheduler(mut self, scheduler: Arc<dyn CronScheduler>) -> Self {
        self.scheduler = Some(scheduler);
        self
    }
}

impl Default for CronListTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl atta_types::NativeTool for CronListTool {
    fn name(&self) -> &str {
        "atta-cron-list"
    }

    fn description(&self) -> &str {
        "List all registered scheduled/cron tasks with their schedules and status"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "description": "Filter by status: active, paused, all",
                    "enum": ["active", "paused", "all"]
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let Some(ref scheduler) = self.scheduler else {
            return Ok(json!({
                "status": "error",
                "message": "CronEngine is not available. Cron listing requires the cron engine to be configured."
            }));
        };

        let status = args.get("status").and_then(|v| v.as_str()).unwrap_or("all");
        let status_filter = if status == "all" { None } else { Some(status) };

        let jobs = scheduler.list_jobs(status_filter).await?;
        Ok(json!({
            "tasks": jobs,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_cron_list_name() {
        assert_eq!(CronListTool::new().name(), "atta-cron-list");
    }

    #[tokio::test]
    async fn test_cron_list_no_scheduler() {
        let tool = CronListTool::new();
        let result = tool.execute(json!({})).await.unwrap();
        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("not available"));
    }
}
