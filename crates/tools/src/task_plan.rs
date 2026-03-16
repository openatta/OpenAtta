//! Task decomposition planning

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Decompose a complex task into ordered sub-tasks with dependencies
pub struct TaskPlanTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for TaskPlanTool {
    fn name(&self) -> &str {
        "atta-task-plan"
    }

    fn description(&self) -> &str {
        "Decompose a complex task into ordered sub-tasks with dependencies"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "The task to decompose"
                },
                "max_subtasks": {
                    "type": "integer",
                    "description": "Maximum number of sub-tasks",
                    "default": 10
                }
            },
            "required": ["task"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let task = args["task"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'task' is required".into()))?;

        Ok(json!({
            "task": task,
            "subtasks": [],
            "message": "Task planning requires LLM integration — returning empty plan"
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_task_plan_name() {
        assert_eq!(TaskPlanTool.name(), "atta-task-plan");
    }
}
