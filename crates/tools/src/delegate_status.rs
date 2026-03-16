//! Delegation coordination status query

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Query the status of a delegated task
pub struct DelegateStatusTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for DelegateStatusTool {
    fn name(&self) -> &str {
        "atta-delegate-status"
    }

    fn description(&self) -> &str {
        "Query the status of a delegated task or coordination group"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Medium
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "delegation_id": {
                    "type": "string",
                    "description": "ID of the delegation to query"
                }
            },
            "required": ["delegation_id"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let id = args["delegation_id"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'delegation_id' is required".into()))?;

        Ok(json!({
            "delegation_id": id,
            "status": "unknown",
            "message": "Delegation not found"
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_delegate_status_name() {
        assert_eq!(DelegateStatusTool.name(), "atta-delegate-status");
    }
}
