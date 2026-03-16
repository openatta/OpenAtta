//! List active sub-agents

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

use crate::AgentRegistryRef;

/// Lists all active sub-agents and their current status
pub struct SubagentListTool {
    pub registry: AgentRegistryRef,
}

impl SubagentListTool {
    pub fn new(registry: AgentRegistryRef) -> Self {
        Self { registry }
    }
}

#[async_trait::async_trait]
impl atta_types::NativeTool for SubagentListTool {
    fn name(&self) -> &str {
        "atta-subagent-list"
    }

    fn description(&self) -> &str {
        "List all active sub-agents and their current status"
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
        let Some(registry) = &self.registry else {
            return Ok(json!({
                "subagents": [],
                "message": "AgentRegistry not available"
            }));
        };

        let agents = registry.list_json().await;

        Ok(json!({
            "subagents": agents,
            "message": "sub-agents listed"
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_subagent_list_name() {
        let tool = SubagentListTool { registry: None };
        assert_eq!(tool.name(), "atta-subagent-list");
    }
}
