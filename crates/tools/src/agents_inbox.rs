//! IPC: Check inbox for messages from other agents

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Check the inbox for messages received from other agents
pub struct AgentsInboxTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for AgentsInboxTool {
    fn name(&self) -> &str {
        "atta-agents-inbox"
    }

    fn description(&self) -> &str {
        "Check the inbox for messages received from other agents"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "description": "Max messages to return",
                    "default": 10
                },
                "unread_only": {
                    "type": "boolean",
                    "description": "Only show unread messages",
                    "default": true
                }
            },
            "required": []
        })
    }

    async fn execute(&self, _args: Value) -> Result<Value, AttaError> {
        Ok(json!({
            "messages": [],
            "unread_count": 0
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_agents_inbox_name() {
        assert_eq!(AgentsInboxTool.name(), "atta-agents-inbox");
    }
}
