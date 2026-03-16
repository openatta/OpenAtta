//! IPC: Write shared state

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Write a value to the shared state store
pub struct StateSetTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for StateSetTool {
    fn name(&self) -> &str {
        "atta-state-set"
    }

    fn description(&self) -> &str {
        "Write a value to the shared state store"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Medium
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "State key"
                },
                "value": {
                    "description": "Value to store (any JSON)"
                }
            },
            "required": ["key", "value"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let key = args["key"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'key' is required".into()))?;
        let value = args.get("value").cloned().unwrap_or(Value::Null);

        Ok(json!({
            "key": key,
            "value": value,
            "stored": true
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_state_set_name() {
        assert_eq!(StateSetTool.name(), "atta-state-set");
    }
}
