//! Proxy configuration tool

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Configure HTTP proxy settings
pub struct ProxyConfigTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for ProxyConfigTool {
    fn name(&self) -> &str {
        "atta-proxy-config"
    }

    fn description(&self) -> &str {
        "Query or configure HTTP proxy settings"
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
                    "enum": ["get", "set"],
                    "description": "Action to perform"
                },
                "http_proxy": {
                    "type": "string",
                    "description": "HTTP proxy URL"
                },
                "https_proxy": {
                    "type": "string",
                    "description": "HTTPS proxy URL"
                },
                "no_proxy": {
                    "type": "string",
                    "description": "Comma-separated list of hosts to bypass"
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
            "get" => Ok(json!({
                "http_proxy": std::env::var("HTTP_PROXY").unwrap_or_default(),
                "https_proxy": std::env::var("HTTPS_PROXY").unwrap_or_default(),
                "no_proxy": std::env::var("NO_PROXY").unwrap_or_default(),
            })),
            "set" => Ok(json!({
                "status": "not_implemented",
                "message": "proxy configuration requires runtime integration"
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
    fn test_proxy_config_name() {
        assert_eq!(ProxyConfigTool.name(), "atta-proxy-config");
    }
}
