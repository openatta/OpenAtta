//! SSE (Server-Sent Events) MCP 客户端
//!
//! 基于 HTTP SSE 握手 + HTTP POST 发送请求的 MCP 传输方式。
//! 遵循 MCP SSE 传输协议：
//! 1. GET /sse 建立 SSE 连接，从 `endpoint` 事件获取 POST 地址
//! 2. POST JSON-RPC 请求到该 endpoint
//! 3. 从 SSE 流读取匹配 id 的 JSON-RPC 响应

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use async_trait::async_trait;
use atta_types::AttaError;
use reqwest::Client;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::jsonrpc::{JsonRpcRequest, JsonRpcResponse};
use crate::traits::{McpClient, McpContent, McpToolInfo, McpToolResult};

/// SSE MCP 客户端
///
/// 通过 SSE 流接收服务器事件，HTTP POST 发送 JSON-RPC 请求。
/// 适用于远程 MCP 服务器场景。
pub struct SseMcpClient {
    /// 服务器名称
    name: String,
    /// SSE 端点 URL（初始连接地址）
    url: String,
    /// HTTP 客户端
    http_client: Client,
    /// POST 请求目标 URL（从 SSE 握手获得）
    message_endpoint: Mutex<Option<String>>,
    /// 请求 ID 递增计数器
    next_id: AtomicU64,
    /// Whether the MCP initialize handshake has been completed
    initialized: AtomicBool,
}

impl SseMcpClient {
    /// 创建 SSE MCP 客户端
    ///
    /// # Arguments
    /// * `name` - 服务器名称
    /// * `url` - SSE 端点 URL（如 `http://localhost:3000/sse`）
    pub fn new(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            url: url.into(),
            http_client: Client::new(),
            message_endpoint: Mutex::new(None),
            next_id: AtomicU64::new(1),
            initialized: AtomicBool::new(false),
        }
    }

    /// 建立 SSE 连接并获取 message endpoint
    ///
    /// MCP SSE 协议要求先 GET /sse，服务器返回的第一个 `endpoint` 事件
    /// 包含后续 POST 请求的目标 URL。
    async fn ensure_connected(&self) -> Result<String, AttaError> {
        // Check cached endpoint first
        {
            let guard = self.message_endpoint.lock().await;
            if let Some(ref ep) = *guard {
                return Ok(ep.clone());
            }
        }

        debug!(server = %self.name, url = %self.url, "connecting to SSE endpoint");

        let response = self
            .http_client
            .get(&self.url)
            .header("Accept", "text/event-stream")
            .send()
            .await
            .map_err(|e| {
                AttaError::McpServerNotConnected(format!(
                    "failed to connect to SSE endpoint '{}': {}",
                    self.name, e
                ))
            })?;

        if !response.status().is_success() {
            return Err(AttaError::McpServerNotConnected(format!(
                "SSE endpoint '{}' returned status {}",
                self.name,
                response.status()
            )));
        }

        // Read SSE stream to find the endpoint event
        let body = response.text().await.map_err(|e| {
            AttaError::McpServerNotConnected(format!(
                "failed to read SSE stream from '{}': {}",
                self.name, e
            ))
        })?;

        let endpoint = parse_endpoint_from_sse(&body, &self.url).ok_or_else(|| {
            AttaError::McpServerNotConnected(format!(
                "no 'endpoint' event found in SSE stream from '{}'",
                self.name
            ))
        })?;

        debug!(server = %self.name, endpoint = %endpoint, "obtained message endpoint");

        let mut guard = self.message_endpoint.lock().await;
        *guard = Some(endpoint.clone());
        drop(guard);

        // Perform MCP initialize handshake once
        if !self.initialized.load(Ordering::Acquire) {
            if let Err(e) = self.do_initialize(&endpoint).await {
                warn!(server = %self.name, error = %e, "MCP initialize handshake failed, continuing anyway");
            } else {
                self.initialized.store(true, Ordering::Release);
            }
        }

        Ok(endpoint)
    }

    /// Perform the MCP protocol initialize handshake over HTTP POST.
    async fn do_initialize(&self, endpoint: &str) -> Result<(), AttaError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let request = JsonRpcRequest::new(
            id,
            "initialize",
            Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "attaos",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
        );

        let response = self
            .http_client
            .post(endpoint)
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "failed to POST initialize to MCP server '{}': {}",
                    self.name,
                    e
                )
            })?;

        let body = response.text().await.map_err(|e| {
            anyhow::anyhow!(
                "failed to read initialize response from MCP server '{}': {}",
                self.name,
                e
            )
        })?;

        let resp = parse_jsonrpc_response(&body, &self.name)?;
        if let Some(err) = resp.error {
            return Err(anyhow::anyhow!(
                "MCP server '{}' initialize error: {} (code {})",
                self.name,
                err.message,
                err.code
            )
            .into());
        }

        debug!(server = %self.name, "MCP initialize succeeded, sending initialized notification");

        // Send initialized notification
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        if let Err(e) = self
            .http_client
            .post(endpoint)
            .json(&notification)
            .send()
            .await
        {
            warn!(server = %self.name, error = %e, "failed to send initialized notification");
        }

        debug!(server = %self.name, "MCP SSE handshake complete");
        Ok(())
    }

    /// 发送 JSON-RPC 请求并读取响应
    async fn send_request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<JsonRpcResponse, AttaError> {
        let endpoint = self.ensure_connected().await?;

        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let request = JsonRpcRequest::new(id, method, params);

        debug!(server = %self.name, id, method, "sending SSE JSON-RPC request");

        let response = self
            .http_client
            .post(&endpoint)
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "failed to POST to MCP server '{}' endpoint: {}",
                    self.name,
                    e
                )
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = match response.text().await {
                Ok(text) => text,
                Err(e) => {
                    tracing::warn!(error = %e, "failed to read MCP SSE response body");
                    String::new()
                }
            };
            return Err(anyhow::anyhow!(
                "MCP server '{}' POST returned status {}: {}",
                self.name,
                status,
                body
            )
            .into());
        }

        // The response body contains the JSON-RPC response
        // Some SSE MCP servers return it directly as JSON
        let body = response.text().await.map_err(|e| {
            anyhow::anyhow!(
                "failed to read response body from MCP server '{}': {}",
                self.name,
                e
            )
        })?;

        // Try parsing the response body directly as JSON-RPC
        // The body may contain SSE formatted data or direct JSON
        let resp = parse_jsonrpc_response(&body, &self.name)?;

        Ok(resp)
    }
}

/// Parse the message endpoint URL from SSE stream data
fn parse_endpoint_from_sse(body: &str, base_url: &str) -> Option<String> {
    // SSE events are formatted as:
    // event: endpoint
    // data: /message?sessionId=xxx
    let mut current_event = String::new();

    for line in body.lines() {
        if let Some(event_name) = line.strip_prefix("event:") {
            current_event = event_name.trim().to_string();
        } else if let Some(data) = line.strip_prefix("data:") {
            if current_event == "endpoint" {
                let path = data.trim();
                // If it's a relative path, resolve against base URL
                if path.starts_with("http://") || path.starts_with("https://") {
                    return Some(path.to_string());
                }
                // Build absolute URL from base
                if let Some(base) = base_url.rfind('/') {
                    let origin = &base_url[..base];
                    return Some(format!("{}{}", origin, path));
                }
                return Some(format!("{}{}", base_url, path));
            }
        }
    }

    None
}

/// Parse JSON-RPC response from response body (may be SSE-formatted or direct JSON)
fn parse_jsonrpc_response(body: &str, server_name: &str) -> Result<JsonRpcResponse, AttaError> {
    let trimmed = body.trim();

    // Try direct JSON parse first
    if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(trimmed) {
        return Ok(resp);
    }

    // Try SSE format: look for data: lines and concatenate
    let mut json_parts = Vec::new();
    for line in trimmed.lines() {
        if let Some(data) = line.strip_prefix("data:") {
            json_parts.push(data.trim());
        }
    }

    if !json_parts.is_empty() {
        let json_str = json_parts.join("");
        if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(&json_str) {
            return Ok(resp);
        }
    }

    // Try each line as potential JSON
    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(line) {
            return Ok(resp);
        }
    }

    Err(anyhow::anyhow!(
        "failed to parse JSON-RPC response from MCP server '{}': {}",
        server_name,
        if trimmed.len() > 200 {
            &trimmed[..200]
        } else {
            trimmed
        }
    )
    .into())
}

#[async_trait]
impl McpClient for SseMcpClient {
    async fn list_tools(&self) -> Result<Vec<McpToolInfo>, AttaError> {
        let resp = self
            .send_request("tools/list", Some(serde_json::json!({})))
            .await?;

        if let Some(err) = resp.error {
            return Err(anyhow::anyhow!(
                "MCP server '{}' tools/list error: {} (code {})",
                self.name,
                err.message,
                err.code
            )
            .into());
        }

        let result = resp.result.ok_or_else(|| {
            anyhow::anyhow!(
                "MCP server '{}' returned empty result for tools/list",
                self.name
            )
        })?;

        let tools_value = result
            .get("tools")
            .cloned()
            .unwrap_or(serde_json::Value::Array(vec![]));

        let tools: Vec<McpToolInfo> = serde_json::from_value(tools_value).map_err(|e| {
            anyhow::anyhow!(
                "failed to parse tools from MCP server '{}': {}",
                self.name,
                e
            )
        })?;

        debug!(server = %self.name, count = tools.len(), "listed tools via SSE");
        Ok(tools)
    }

    async fn call_tool(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolResult, AttaError> {
        let params = serde_json::json!({
            "name": tool_name,
            "arguments": arguments,
        });

        let resp = self.send_request("tools/call", Some(params)).await?;

        if let Some(err) = resp.error {
            return Err(anyhow::anyhow!(
                "MCP server '{}' tools/call error for '{}': {} (code {})",
                self.name,
                tool_name,
                err.message,
                err.code
            )
            .into());
        }

        let result = resp.result.ok_or_else(|| {
            anyhow::anyhow!(
                "MCP server '{}' returned empty result for tools/call '{}'",
                self.name,
                tool_name
            )
        })?;

        let is_error = result
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let content_value = result
            .get("content")
            .cloned()
            .unwrap_or(serde_json::Value::Array(vec![]));

        let content: Vec<McpContent> = serde_json::from_value(content_value).map_err(|e| {
            anyhow::anyhow!(
                "failed to parse tool result content from MCP server '{}': {}",
                self.name,
                e
            )
        })?;

        debug!(server = %self.name, tool = tool_name, is_error, "tool call completed via SSE");
        Ok(McpToolResult { is_error, content })
    }

    async fn ping(&self) -> Result<(), AttaError> {
        let resp = self.send_request("ping", None).await?;

        if let Some(err) = resp.error {
            return Err(anyhow::anyhow!(
                "MCP server '{}' ping failed: {} (code {})",
                self.name,
                err.message,
                err.code
            )
            .into());
        }

        debug!(server = %self.name, "ping succeeded via SSE");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_endpoint_from_sse() {
        let body = "event: endpoint\ndata: /message?sessionId=abc123\n\n";
        let result = parse_endpoint_from_sse(body, "http://localhost:3000/sse");
        assert_eq!(
            result,
            Some("http://localhost:3000/message?sessionId=abc123".to_string())
        );
    }

    #[test]
    fn test_parse_endpoint_absolute_url() {
        let body = "event: endpoint\ndata: http://other:4000/msg\n\n";
        let result = parse_endpoint_from_sse(body, "http://localhost:3000/sse");
        assert_eq!(result, Some("http://other:4000/msg".to_string()));
    }

    #[test]
    fn test_parse_endpoint_missing() {
        let body = "event: other\ndata: something\n\n";
        let result = parse_endpoint_from_sse(body, "http://localhost:3000/sse");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_jsonrpc_direct() {
        let body = r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#;
        let resp = parse_jsonrpc_response(body, "test").unwrap();
        assert_eq!(resp.id, serde_json::Value::Number(1.into()));
        assert!(resp.result.is_some());
    }

    #[test]
    fn test_parse_jsonrpc_sse_format() {
        let body = "data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n\n";
        let resp = parse_jsonrpc_response(body, "test").unwrap();
        assert_eq!(resp.id, serde_json::Value::Number(1.into()));
    }

    #[test]
    fn test_client_creation() {
        let client = SseMcpClient::new("test-server", "http://localhost:3000/sse");
        assert_eq!(client.name, "test-server");
        assert_eq!(client.url, "http://localhost:3000/sse");
    }
}
