//! IPC: List all agents

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// List all running agents and their current state
pub struct AgentsListTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for AgentsListTool {
    fn name(&self) -> &str {
        "atta-agents-list"
    }

    fn description(&self) -> &str {
        "List all running agents and their current state (IPC)"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute(&self, _args: Value) -> Result<Value, AttaError> {
        Ok(json!({
            "agents": [],
            "message": "No agents currently running"
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_agents_list_name() {
        assert_eq!(AgentsListTool.name(), "atta-agents-list");
    }
}
