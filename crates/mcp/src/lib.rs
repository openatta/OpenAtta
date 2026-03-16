//! MCP (Model Context Protocol) 客户端
//!
//! 提供与 MCP 服务器通信的客户端实现，支持 Stdio 和 SSE 两种传输方式。
//! 通过 `McpRegistry` 统一管理多个 MCP 服务器连接。

pub mod jsonrpc;
pub mod registry;
pub mod sse;
pub mod stdio;
pub mod traits;

pub use jsonrpc::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};
pub use registry::McpRegistry;
pub use sse::SseMcpClient;
pub use stdio::StdioMcpClient;
pub use traits::McpClient;
