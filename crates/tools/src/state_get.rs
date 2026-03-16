//! IPC: Read shared state

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Read a value from the shared state store by key
pub struct StateGetTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for StateGetTool {
    fn name(&self) -> &str {
        "atta-state-get"
    }

    fn description(&self) -> &str {
        "Read a value from the shared state store by key"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "State key to read"
                }
            },
            "required": ["key"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let key = args["key"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'key' is required".into()))?;

        Ok(json!({
            "key": key,
            "value": null,
            "found": false
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_state_get_name() {
        assert_eq!(StateGetTool.name(), "atta-state-get");
    }
}
