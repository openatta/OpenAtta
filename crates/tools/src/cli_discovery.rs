//! CLI tool discovery

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Discover available CLI tools on the system
pub struct CliDiscoveryTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for CliDiscoveryTool {
    fn name(&self) -> &str {
        "atta-cli-discovery"
    }

    fn description(&self) -> &str {
        "Discover available CLI tools on the system (checks PATH)"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tools": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "List of tool names to check for (e.g. ['git', 'docker', 'node'])"
                }
            },
            "required": ["tools"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let tools = args["tools"]
            .as_array()
            .ok_or_else(|| AttaError::Validation("'tools' array is required".into()))?;

        let mut results = Vec::new();
        for tool in tools {
            let name = tool.as_str().unwrap_or("");
            if name.is_empty() {
                continue;
            }

            let available = tokio::process::Command::new("which")
                .arg(name)
                .output()
                .await
                .map(|o| o.status.success())
                .unwrap_or(false);

            results.push(json!({
                "name": name,
                "available": available,
            }));
        }

        Ok(json!({ "tools": results }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_cli_discovery_name() {
        assert_eq!(CliDiscoveryTool.name(), "atta-cli-discovery");
    }
}
