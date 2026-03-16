//! Manage sub-agents (pause/resume/terminate)

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

use crate::AgentRegistryRef;

/// Manages a sub-agent: pause, resume, or terminate it
pub struct SubagentManageTool {
    pub registry: AgentRegistryRef,
}

impl SubagentManageTool {
    pub fn new(registry: AgentRegistryRef) -> Self {
        Self { registry }
    }
}

#[async_trait::async_trait]
impl atta_types::NativeTool for SubagentManageTool {
    fn name(&self) -> &str {
        "atta-subagent-manage"
    }

    fn description(&self) -> &str {
        "Manage a sub-agent: pause, resume, or terminate it"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::High
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "subagent_id": {
                    "type": "string",
                    "description": "ID of the sub-agent"
                },
                "action": {
                    "type": "string",
                    "description": "Action to perform",
                    "enum": ["pause", "resume", "terminate"]
                }
            },
            "required": ["subagent_id", "action"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let id = args["subagent_id"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'subagent_id' is required".into()))?;
        let action = args["action"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'action' is required".into()))?;

        let Some(registry) = &self.registry else {
            return Err(AttaError::Validation(
                "AgentRegistry not available".to_string(),
            ));
        };

        let result = match action {
            "pause" => registry.pause(id).await,
            "resume" => registry.resume(id).await,
            "terminate" => registry.terminate(id).await,
            other => Err(format!("unknown action: {}", other)),
        };

        match result {
            Ok(()) => Ok(json!({
                "subagent_id": id,
                "action": action,
                "status": "completed",
                "message": format!("Sub-agent {} {}", id, action)
            })),
            Err(e) => Err(AttaError::Validation(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_subagent_manage_name() {
        let tool = SubagentManageTool { registry: None };
        assert_eq!(tool.name(), "atta-subagent-manage");
    }
}
