//! McpClient trait 定义
//!
//! 所有 MCP 客户端实现（Stdio / SSE）统一的异步接口。

use async_trait::async_trait;
use atta_types::AttaError;

/// MCP Tool 描述
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct McpToolInfo {
    /// Tool 名称
    pub name: String,
    /// Tool 描述
    pub description: String,
    /// JSON Schema 格式的参数定义
    pub input_schema: serde_json::Value,
}

/// MCP Tool 调用结果
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct McpToolResult {
    /// 调用是否成功
    pub is_error: bool,
    /// 返回内容
    pub content: Vec<McpContent>,
}

/// MCP 内容块
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpContent {
    /// 纯文本内容
    Text { text: String },
    /// 图片内容（base64）
    Image { data: String, mime_type: String },
    /// 嵌入资源
    Resource { uri: String, text: String },
}

/// MCP 客户端 trait
///
/// 定义与单个 MCP 服务器通信的统一接口。
/// Stdio 和 SSE 实现各自提供底层传输逻辑。
#[async_trait]
pub trait McpClient: Send + Sync + 'static {
    /// 列出服务器提供的所有 Tool
    async fn list_tools(&self) -> Result<Vec<McpToolInfo>, AttaError>;

    /// 调用指定 Tool
    async fn call_tool(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolResult, AttaError>;

    /// 发送 ping，检测服务器是否存活
    async fn ping(&self) -> Result<(), AttaError>;
}
