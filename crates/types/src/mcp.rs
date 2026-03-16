//! MCP 服务器配置类型

use serde::{Deserialize, Serialize};

/// MCP 传输方式
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpTransport {
    Stdio,
    Sse,
}

/// MCP Server 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub description: Option<String>,
    pub transport: McpTransport,
    pub url: Option<String>,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub auth: Option<McpAuthConfig>,
}

/// MCP 认证配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpAuthConfig {
    #[serde(rename = "type")]
    pub auth_type: String,
    pub token_ref: Option<String>,
}
