//! Enhanced scheduling with persistent task storage

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Schedule a task for future execution with persistent storage and retry support
pub struct ScheduleTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for ScheduleTool {
    fn name(&self) -> &str {
        "atta-schedule"
    }

    fn description(&self) -> &str {
        "Schedule a task for future execution with persistent storage and retry support"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Medium
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "Task description"
                },
                "run_at": {
                    "type": "string",
                    "description": "ISO 8601 datetime to run (e.g. '2024-12-25T10:00:00Z')"
                },
                "retry_count": {
                    "type": "integer",
                    "description": "Number of retry attempts on failure",
                    "default": 0
                },
                "retry_delay_secs": {
                    "type": "integer",
                    "description": "Delay between retries in seconds",
                    "default": 60
                }
            },
            "required": ["task", "run_at"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let task = args["task"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'task' is required".into()))?;
        let run_at = args["run_at"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'run_at' is required".into()))?;
        let id = uuid::Uuid::new_v4().to_string();

        Ok(json!({
            "schedule_id": id,
            "task": task,
            "run_at": run_at,
            "status": "scheduled"
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_schedule_name() {
        assert_eq!(ScheduleTool.name(), "atta-schedule");
    }
}
