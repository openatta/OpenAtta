//! Stdio MCP 客户端
//!
//! 通过子进程的 stdin/stdout 以 NDJSON（Newline-Delimited JSON）方式
//! 与 MCP 服务器通信。每行一个完整的 JSON-RPC 消息。

use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Context;
use async_trait::async_trait;
use atta_types::AttaError;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::jsonrpc::{JsonRpcRequest, JsonRpcResponse};
use crate::traits::{McpClient, McpContent, McpToolInfo, McpToolResult};

/// Stdio MCP 客户端
///
/// 通过启动子进程，借助 stdin/stdout 进行 JSON-RPC 通信。
/// 符合 MCP 规范的 Stdio 传输方式。
pub struct StdioMcpClient {
    /// 服务器名称（用于日志和错误报告）
    name: String,
    /// 子进程句柄（必须保持存活，drop 时 kill_on_drop 生效）
    #[allow(dead_code)]
    child: Mutex<Child>,
    /// stdin 写入器
    stdin: Mutex<tokio::process::ChildStdin>,
    /// stdout 读取器（按行读取 NDJSON）
    stdout: Mutex<BufReader<tokio::process::ChildStdout>>,
    /// 请求 ID 递增计数器
    next_id: AtomicU64,
    /// Serializes request-response pairs to prevent interleaving
    request_lock: Mutex<()>,
}

impl StdioMcpClient {
    /// 启动子进程并创建 Stdio MCP 客户端
    ///
    /// # Arguments
    /// * `name` - 服务器名称
    /// * `command` - 可执行文件路径
    /// * `args` - 命令行参数
    pub async fn spawn(
        name: impl Into<String>,
        command: &str,
        args: &[String],
    ) -> Result<Self, AttaError> {
        let name = name.into();
        debug!(server = %name, command = %command, ?args, "spawning MCP stdio server");

        let mut child = Command::new(command)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("failed to spawn MCP server '{name}': {command}"))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("failed to capture stdin for MCP server '{name}'"))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("failed to capture stdout for MCP server '{name}'"))?;

        let client = Self {
            name,
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            stdout: Mutex::new(BufReader::new(stdout)),
            next_id: AtomicU64::new(1),
            request_lock: Mutex::new(()),
        };

        // Perform MCP initialize handshake (best-effort — warn on failure)
        if let Err(e) = client.initialize().await {
            warn!(server = %client.name, error = %e, "MCP initialize handshake failed, continuing anyway");
        }

        Ok(client)
    }

    /// Perform the MCP protocol initialize handshake.
    ///
    /// Sends `initialize` request and, upon success, sends `notifications/initialized`.
    async fn initialize(&self) -> Result<(), AttaError> {
        let resp = self
            .send_request(
                "initialize",
                Some(serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "attaos",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                })),
            )
            .await?;

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

        // Send initialized notification (no id — it's a notification)
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        let mut line = serde_json::to_string(&notification)?;
        line.push('\n');

        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| anyhow::anyhow!("failed to send initialized notification: {e}"))?;
        stdin
            .flush()
            .await
            .map_err(|e| anyhow::anyhow!("failed to flush initialized notification: {e}"))?;

        debug!(server = %self.name, "MCP handshake complete");
        Ok(())
    }

    /// 发送 JSON-RPC 请求并等待响应
    async fn send_request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<JsonRpcResponse, AttaError> {
        let _request_guard = self.request_lock.lock().await;
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let request = JsonRpcRequest::new(id, method, params);

        let mut line = serde_json::to_string(&request)?;
        line.push('\n');

        debug!(server = %self.name, id, method, "sending JSON-RPC request");

        // Write request to stdin
        {
            let mut stdin = self.stdin.lock().await;
            stdin
                .write_all(line.as_bytes())
                .await
                .with_context(|| format!("failed to write to MCP server '{}' stdin", self.name))?;
            stdin
                .flush()
                .await
                .with_context(|| format!("failed to flush MCP server '{}' stdin", self.name))?;
        }

        // Read response from stdout (line by line, skip notifications)
        // Wrap with a 30-second timeout to avoid blocking forever if the server hangs.
        let response: JsonRpcResponse = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            async {
                let mut stdout = self.stdout.lock().await;
                loop {
                    let mut buf = String::new();
                    let bytes_read = stdout.read_line(&mut buf).await.with_context(|| {
                        format!("failed to read from MCP server '{}' stdout", self.name)
                    })?;

                    if bytes_read == 0 {
                        return Err(AttaError::McpServerNotConnected(format!(
                            "MCP server '{}' closed stdout unexpectedly",
                            self.name
                        )));
                    }

                    let trimmed = buf.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    // Try to parse as a response (has id field)
                    if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(trimmed) {
                        // Check id matches our request
                        if resp.id == request.id {
                            break Ok(resp);
                        }
                        // Mismatched id — could be a response to an older request; skip
                        warn!(
                            server = %self.name,
                            expected_id = id,
                            "received response with mismatched id, skipping"
                        );
                        continue;
                    }

                    // Not a valid response — might be a notification or log line; skip
                    debug!(server = %self.name, line = trimmed, "skipping non-response line");
                }
            },
        )
        .await
        .map_err(|_| {
            AttaError::McpServerNotConnected(format!(
                "MCP server '{}' request timed out after 30s (method: {})",
                self.name, method
            ))
        })??;

        Ok(response)
    }
}

#[async_trait]
impl McpClient for StdioMcpClient {
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

        debug!(server = %self.name, count = tools.len(), "listed tools");
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

        // Parse the MCP tool result
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

        debug!(server = %self.name, tool = tool_name, is_error, "tool call completed");
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

        debug!(server = %self.name, "ping succeeded");
        Ok(())
    }
}

impl Drop for StdioMcpClient {
    fn drop(&mut self) {
        // kill_on_drop is set, so the child process will be killed
        // when the Child handle is dropped. We just log the event.
        debug!(server = %self.name, "dropping StdioMcpClient, child process will be killed");
    }
}
