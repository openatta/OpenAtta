//! MCP 注册表
//!
//! 统一管理多个 MCP 客户端连接，通过名称进行路由。

use std::collections::HashMap;
use std::sync::Arc;

use atta_types::AttaError;
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::traits::{McpClient, McpToolInfo, McpToolResult};

/// 带服务器来源信息的 Tool 描述
#[derive(Debug, Clone)]
pub struct McpToolEntry {
    /// Tool 信息
    pub tool: McpToolInfo,
    /// 来源 MCP 服务器名称
    pub server_name: String,
}

/// MCP 注册表
///
/// 管理多个 MCP 客户端实例，提供按名称的增删查改和统一的 tool 调用入口。
pub struct McpRegistry {
    /// 服务器名称 → 客户端实例
    clients: RwLock<HashMap<String, Arc<dyn McpClient>>>,
}

impl McpRegistry {
    /// 创建空的 MCP 注册表
    pub fn new() -> Self {
        Self {
            clients: RwLock::new(HashMap::new()),
        }
    }

    /// 注册一个 MCP 客户端
    ///
    /// 如果已存在同名客户端，将被替换。
    pub async fn add(&self, name: impl Into<String>, client: Arc<dyn McpClient>) {
        let name = name.into();
        info!(server = %name, "registering MCP client");
        self.clients.write().await.insert(name, client);
    }

    /// 移除指定名称的 MCP 客户端
    ///
    /// 返回被移除的客户端（如果存在）。
    pub async fn remove(&self, name: &str) -> Option<Arc<dyn McpClient>> {
        info!(server = %name, "removing MCP client");
        self.clients.write().await.remove(name)
    }

    /// 获取指定名称的 MCP 客户端
    pub async fn get(&self, name: &str) -> Option<Arc<dyn McpClient>> {
        self.clients.read().await.get(name).cloned()
    }

    /// 列出所有已注册的服务器名称
    pub async fn list_servers(&self) -> Vec<String> {
        self.clients.read().await.keys().cloned().collect()
    }

    /// 列出所有服务器提供的全部 Tool（附带来源信息）
    ///
    /// 遍历所有已注册的 MCP 客户端，逐一调用 `list_tools`。
    /// 某个服务器查询失败不影响其他服务器。
    pub async fn get_tools(&self) -> Result<Vec<McpToolEntry>, AttaError> {
        let clients = self.clients.read().await;
        let mut all_tools = Vec::new();

        for (name, client) in clients.iter() {
            match client.list_tools().await {
                Ok(tools) => {
                    debug!(server = %name, count = tools.len(), "listed tools from MCP server");
                    for tool in tools {
                        all_tools.push(McpToolEntry {
                            tool,
                            server_name: name.clone(),
                        });
                    }
                }
                Err(e) => {
                    tracing::warn!(server = %name, error = %e, "failed to list tools from MCP server, skipping");
                }
            }
        }

        Ok(all_tools)
    }

    /// 调用指定服务器上的 Tool
    ///
    /// # Errors
    /// - `AttaError::McpServerNotFound` 如果指定名称的服务器未注册
    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolResult, AttaError> {
        let client = {
            let clients = self.clients.read().await;
            clients
                .get(server_name)
                .cloned()
                .ok_or_else(|| AttaError::McpServerNotFound(server_name.to_string()))?
        };

        debug!(server = %server_name, tool = %tool_name, "calling MCP tool");
        client.call_tool(tool_name, arguments).await
    }
}

impl Default for McpRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{McpContent, McpToolResult};

    /// Mock MCP 客户端，用于测试注册表逻辑
    struct MockMcpClient {
        tools: Vec<McpToolInfo>,
    }

    impl MockMcpClient {
        fn new(tools: Vec<McpToolInfo>) -> Self {
            Self { tools }
        }
    }

    #[async_trait::async_trait]
    impl McpClient for MockMcpClient {
        async fn list_tools(&self) -> Result<Vec<McpToolInfo>, AttaError> {
            Ok(self.tools.clone())
        }

        async fn call_tool(
            &self,
            tool_name: &str,
            _arguments: serde_json::Value,
        ) -> Result<McpToolResult, AttaError> {
            Ok(McpToolResult {
                is_error: false,
                content: vec![McpContent::Text {
                    text: format!("called {tool_name}"),
                }],
            })
        }

        async fn ping(&self) -> Result<(), AttaError> {
            Ok(())
        }
    }

    fn make_tool(name: &str) -> McpToolInfo {
        McpToolInfo {
            name: name.to_string(),
            description: format!("{name} tool"),
            input_schema: serde_json::json!({"type": "object"}),
        }
    }

    #[tokio::test]
    async fn test_add_and_list_servers() {
        let registry = McpRegistry::new();

        assert!(registry.list_servers().await.is_empty());

        let client = Arc::new(MockMcpClient::new(vec![]));
        registry.add("server-a", client).await;

        let servers = registry.list_servers().await;
        assert_eq!(servers.len(), 1);
        assert!(servers.contains(&"server-a".to_string()));
    }

    #[tokio::test]
    async fn test_add_replace_existing() {
        let registry = McpRegistry::new();

        let client1 = Arc::new(MockMcpClient::new(vec![make_tool("tool-v1")]));
        registry.add("server-a", client1).await;

        let client2 = Arc::new(MockMcpClient::new(vec![make_tool("tool-v2")]));
        registry.add("server-a", client2).await;

        let servers = registry.list_servers().await;
        assert_eq!(servers.len(), 1);

        // Should have the replaced client's tools
        let tools = registry.get_tools().await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool.name, "tool-v2");
    }

    #[tokio::test]
    async fn test_remove() {
        let registry = McpRegistry::new();

        let client = Arc::new(MockMcpClient::new(vec![]));
        registry.add("server-a", client).await;

        let removed = registry.remove("server-a").await;
        assert!(removed.is_some());
        assert!(registry.list_servers().await.is_empty());

        // Removing non-existent returns None
        let removed = registry.remove("server-a").await;
        assert!(removed.is_none());
    }

    #[tokio::test]
    async fn test_get() {
        let registry = McpRegistry::new();

        let client = Arc::new(MockMcpClient::new(vec![]));
        registry.add("server-a", client).await;

        assert!(registry.get("server-a").await.is_some());
        assert!(registry.get("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn test_get_tools_multiple_servers() {
        let registry = McpRegistry::new();

        let client_a = Arc::new(MockMcpClient::new(vec![
            make_tool("read_file"),
            make_tool("write_file"),
        ]));
        let client_b = Arc::new(MockMcpClient::new(vec![make_tool("search")]));

        registry.add("filesystem", client_a).await;
        registry.add("search-engine", client_b).await;

        let tools = registry.get_tools().await.unwrap();
        assert_eq!(tools.len(), 3);

        // Verify server attribution
        let fs_tools: Vec<_> = tools
            .iter()
            .filter(|t| t.server_name == "filesystem")
            .collect();
        assert_eq!(fs_tools.len(), 2);

        let search_tools: Vec<_> = tools
            .iter()
            .filter(|t| t.server_name == "search-engine")
            .collect();
        assert_eq!(search_tools.len(), 1);
        assert_eq!(search_tools[0].tool.name, "search");
    }

    #[tokio::test]
    async fn test_call_tool() {
        let registry = McpRegistry::new();

        let client = Arc::new(MockMcpClient::new(vec![make_tool("echo")]));
        registry.add("test-server", client).await;

        let result = registry
            .call_tool("test-server", "echo", serde_json::json!({"msg": "hello"}))
            .await
            .unwrap();

        assert!(!result.is_error);
        assert_eq!(result.content.len(), 1);
        match &result.content[0] {
            McpContent::Text { text } => assert_eq!(text, "called echo"),
            _ => panic!("expected text content"),
        }
    }

    #[tokio::test]
    async fn test_call_tool_server_not_found() {
        let registry = McpRegistry::new();

        let result = registry
            .call_tool("nonexistent", "tool", serde_json::json!({}))
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            AttaError::McpServerNotFound(name) => assert_eq!(name, "nonexistent"),
            other => panic!("expected McpServerNotFound, got: {other}"),
        }
    }

    #[tokio::test]
    async fn test_empty_registry_get_tools() {
        let registry = McpRegistry::new();
        let tools = registry.get_tools().await.unwrap();
        assert!(tools.is_empty());
    }

    #[tokio::test]
    async fn test_default_trait() {
        let registry = McpRegistry::default();
        assert!(registry.list_servers().await.is_empty());
    }
}
