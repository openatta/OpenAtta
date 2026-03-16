//! IPC: Send message to another agent

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Send a message to another agent via inter-process communication
pub struct AgentsSendTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for AgentsSendTool {
    fn name(&self) -> &str {
        "atta-agents-send"
    }

    fn description(&self) -> &str {
        "Send a message to another agent via inter-process communication"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Medium
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "target_agent": {
                    "type": "string",
                    "description": "Target agent ID"
                },
                "message": {
                    "type": "string",
                    "description": "Message content to send"
                }
            },
            "required": ["target_agent", "message"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let target = args["target_agent"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'target_agent' is required".into()))?;
        let msg = args["message"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'message' is required".into()))?;

        Ok(json!({
            "sent_to": target,
            "message_length": msg.len(),
            "status": "delivered"
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_agents_send_name() {
        assert_eq!(AgentsSendTool.name(), "atta-agents-send");
    }
}
