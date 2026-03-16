//! Spawn a sub-agent

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

use crate::AgentRegistryRef;

/// Spawns a new sub-agent to handle a delegated task in parallel
pub struct SubagentSpawnTool {
    pub registry: AgentRegistryRef,
}

impl SubagentSpawnTool {
    pub fn new(registry: AgentRegistryRef) -> Self {
        Self { registry }
    }
}

#[async_trait::async_trait]
impl atta_types::NativeTool for SubagentSpawnTool {
    fn name(&self) -> &str {
        "atta-subagent-spawn"
    }

    fn description(&self) -> &str {
        "Spawn a new sub-agent to handle a delegated task in parallel"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::High
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "Task description for the sub-agent"
                },
                "tools": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Tools available to the sub-agent (empty = all)"
                },
                "max_iterations": {
                    "type": "integer",
                    "description": "Max ReAct iterations",
                    "default": 5
                }
            },
            "required": ["task"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let task = args["task"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'task' is required".into()))?
            .to_string();

        let Some(registry) = &self.registry else {
            return Err(AttaError::Validation(
                "AgentRegistry not available".to_string(),
            ));
        };

        let id = registry.spawn_task(task.clone()).await;

        Ok(json!({
            "subagent_id": id,
            "task": task,
            "status": "spawned",
            "message": format!("Sub-agent {} spawned for task", id)
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_subagent_spawn_name() {
        let tool = SubagentSpawnTool { registry: None };
        assert_eq!(tool.name(), "atta-subagent-spawn");
    }
}
