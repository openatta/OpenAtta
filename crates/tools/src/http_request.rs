//! Generic HTTP request tool

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Make arbitrary HTTP requests
pub struct HttpRequestTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for HttpRequestTool {
    fn name(&self) -> &str {
        "atta-http-request"
    }

    fn description(&self) -> &str {
        "Make an HTTP request with custom method, headers, and body"
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
                    "description": "Request URL"
                },
                "method": {
                    "type": "string",
                    "enum": ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"],
                    "default": "GET"
                },
                "headers": {
                    "type": "object",
                    "description": "Custom HTTP headers"
                },
                "body": {
                    "type": "string",
                    "description": "Request body"
                },
                "json": {
                    "description": "JSON request body (overrides body)"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let url = args["url"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'url' is required".into()))?;
        let method = args["method"].as_str().unwrap_or("GET");

        let client = reqwest::Client::new();
        let mut request = match method {
            "GET" => client.get(url),
            "POST" => client.post(url),
            "PUT" => client.put(url),
            "PATCH" => client.patch(url),
            "DELETE" => client.delete(url),
            "HEAD" => client.head(url),
            other => {
                return Err(AttaError::Validation(format!(
                    "unsupported method: {}",
                    other
                )))
            }
        };

        if let Some(headers) = args["headers"].as_object() {
            for (key, value) in headers {
                if let Some(v) = value.as_str() {
                    request = request.header(key.as_str(), v);
                }
            }
        }

        if !args["json"].is_null() {
            request = request.json(&args["json"]);
        } else if let Some(body) = args["body"].as_str() {
            request = request.body(body.to_string());
        }

        let response = request
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        let status = response.status().as_u16();
        let headers: serde_json::Map<String, Value> = response
            .headers()
            .iter()
            .filter_map(|(k, v)| {
                v.to_str()
                    .ok()
                    .map(|val| (k.to_string(), Value::String(val.to_string())))
            })
            .collect();

        let body = response
            .text()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        Ok(json!({
            "status": status,
            "headers": headers,
            "body": body,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_http_request_name() {
        assert_eq!(HttpRequestTool.name(), "atta-http-request");
    }
}
