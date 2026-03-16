//! Model routing / switching tool

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Switch or configure LLM model for the current session
pub struct ModelRoutingTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for ModelRoutingTool {
    fn name(&self) -> &str {
        "atta-model-routing"
    }

    fn description(&self) -> &str {
        "Switch or query the active LLM model for the current conversation"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["query", "switch"],
                    "description": "Action to perform"
                },
                "model_id": {
                    "type": "string",
                    "description": "Model ID to switch to (for switch action)"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let action = args["action"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'action' is required".into()))?;

        match action {
            "query" => Ok(json!({
                "status": "not_implemented",
                "message": "model routing requires LLM provider integration"
            })),
            "switch" => Ok(json!({
                "status": "not_implemented",
                "message": "model switching requires LLM provider integration"
            })),
            other => Err(AttaError::Validation(format!("unknown action: {}", other))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_model_routing_name() {
        assert_eq!(ModelRoutingTool.name(), "atta-model-routing");
    }
}
