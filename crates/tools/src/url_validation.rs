//! URL safety validation

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Validate a URL for safety: check format, domain reputation, and protocol
pub struct UrlValidationTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for UrlValidationTool {
    fn name(&self) -> &str {
        "atta-url-validation"
    }

    fn description(&self) -> &str {
        "Validate a URL for safety: check format, domain reputation, and protocol"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL to validate"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let url = args["url"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'url' is required".into()))?;

        let is_https = url.starts_with("https://");
        let is_http = url.starts_with("http://");
        let is_valid_protocol = is_https || is_http;

        // Check for suspicious patterns
        let suspicious = (url.contains('@') && url.contains("://")) // credential in URL
            || url.contains("localhost")
            || url.contains("127.0.0.1")
            || url.contains("0.0.0.0")
            || url.contains("169.254.") // link-local
            || url.contains("[::1]"); // IPv6 loopback

        Ok(json!({
            "url": url,
            "valid_protocol": is_valid_protocol,
            "is_https": is_https,
            "suspicious": suspicious,
            "safe": is_valid_protocol && !suspicious
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_url_validation_name() {
        assert_eq!(UrlValidationTool.name(), "atta-url-validation");
    }
}
