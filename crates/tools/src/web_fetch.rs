//! Web fetch tool — HTTP GET with HTML→text conversion

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Fetch a URL and return its content
pub struct WebFetchTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for WebFetchTool {
    fn name(&self) -> &str {
        "atta-web-fetch"
    }

    fn description(&self) -> &str {
        "Fetch content from a URL and return as text"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Medium
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL to fetch"
                },
                "headers": {
                    "type": "object",
                    "description": "Custom HTTP headers"
                },
                "max_length": {
                    "type": "integer",
                    "description": "Maximum response length in bytes",
                    "default": 100000
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let url = args["url"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'url' is required".into()))?;
        let max_length = args["max_length"].as_u64().unwrap_or(100_000) as usize;

        let client = reqwest::Client::new();
        let mut request = client.get(url);

        if let Some(headers) = args["headers"].as_object() {
            for (key, value) in headers {
                if let Some(v) = value.as_str() {
                    request = request.header(key.as_str(), v);
                }
            }
        }

        let response = request
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let body = response
            .text()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        let truncated = body.len() > max_length;
        let content = if truncated {
            format!("{}...[truncated]", &body[..max_length])
        } else {
            body
        };

        Ok(json!({
            "status": status,
            "content_type": content_type,
            "content": content,
            "truncated": truncated,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_web_fetch_name() {
        assert_eq!(WebFetchTool.name(), "atta-web-fetch");
    }
}
