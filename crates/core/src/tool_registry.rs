//! Tool 注册表默认实现
//!
//! [`DefaultToolRegistry`] 是 [`ToolRegistry`] 的默认实现，支持 Builtin、Plugin、MCP 三种绑定类型。

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use atta_mcp::registry::McpRegistry;
use atta_mcp::traits::McpContent;
use atta_types::{AttaError, NativeTool, ToolBinding, ToolDef, ToolRegistry, ToolSchema};
use tracing::{error, info};

/// Builtin handler 函数类型
pub type BuiltinHandler = Arc<
    dyn Fn(
            serde_json::Value,
        ) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, AttaError>> + Send>>
        + Send
        + Sync,
>;

/// 默认 Tool 注册表实现
///
/// 使用 `RwLock<HashMap>` 保护内存中的 Tool 列表。
/// 通过注入 [`McpRegistry`] 实现对 MCP 工具的真实调用。
pub struct DefaultToolRegistry {
    tools: RwLock<HashMap<String, ToolDef>>,
    builtins: RwLock<HashMap<String, BuiltinHandler>>,
    natives: RwLock<HashMap<String, Arc<dyn NativeTool>>>,
    mcp_registry: Option<Arc<McpRegistry>>,
}

impl DefaultToolRegistry {
    /// 创建空的 Tool 注册表
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
            builtins: RwLock::new(HashMap::new()),
            natives: RwLock::new(HashMap::new()),
            mcp_registry: None,
        }
    }

    /// 注入 MCP 注册表
    pub fn with_mcp_registry(mut self, registry: Arc<McpRegistry>) -> Self {
        self.mcp_registry = Some(registry);
        self
    }

    /// 注册 builtin tool handler
    pub fn register_builtin<F, Fut>(&self, handler_name: &str, handler: F)
    where
        F: Fn(serde_json::Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<serde_json::Value, AttaError>> + Send + 'static,
    {
        let handler: BuiltinHandler = Arc::new(move |args| Box::pin(handler(args)));
        let mut builtins = self.builtins.write().unwrap_or_else(|e| {
            error!("builtins lock poisoned, recovering");
            e.into_inner()
        });
        builtins.insert(handler_name.to_string(), handler);
    }

    /// 注册原生 Rust 工具
    pub fn register_native(&self, tool: Arc<dyn NativeTool>) {
        let name = tool.name().to_string();
        self.register(ToolDef {
            name: name.clone(),
            description: tool.description().to_string(),
            binding: ToolBinding::Native {
                handler_name: name.clone(),
            },
            risk_level: tool.risk_level(),
            parameters: tool.parameters_schema(),
        });
        self.natives
            .write()
            .unwrap_or_else(|e| {
                error!("natives lock poisoned, recovering");
                e.into_inner()
            })
            .insert(name, tool);
    }

    /// Replace a native tool by name (used for late-binding, e.g. DelegationTool with runner)
    pub fn replace_native(&self, tool: Arc<dyn NativeTool>) {
        let name = tool.name().to_string();
        // Update the ToolDef
        self.register(ToolDef {
            name: name.clone(),
            description: tool.description().to_string(),
            binding: ToolBinding::Native {
                handler_name: name.clone(),
            },
            risk_level: tool.risk_level(),
            parameters: tool.parameters_schema(),
        });
        // Update the NativeTool instance
        self.natives
            .write()
            .unwrap_or_else(|e| {
                error!("natives lock poisoned, recovering");
                e.into_inner()
            })
            .insert(name, tool);
    }
}

impl Default for DefaultToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 将 MCP 返回的 McpContent 列表转换为 serde_json::Value
fn mcp_content_to_json(content: &[McpContent]) -> serde_json::Value {
    if content.len() == 1 {
        // 单个文本内容 → 直接返回字符串
        if let McpContent::Text { text } = &content[0] {
            return serde_json::Value::String(text.clone());
        }
    }
    // 多块内容 → 返回数组
    serde_json::Value::Array(
        content
            .iter()
            .map(|c| match c {
                McpContent::Text { text } => serde_json::json!({ "type": "text", "text": text }),
                McpContent::Image { data, mime_type } => {
                    serde_json::json!({ "type": "image", "data": data, "mime_type": mime_type })
                }
                McpContent::Resource { uri, text } => {
                    serde_json::json!({ "type": "resource", "uri": uri, "text": text })
                }
            })
            .collect(),
    )
}

#[async_trait::async_trait]
impl ToolRegistry for DefaultToolRegistry {
    fn register(&self, tool: ToolDef) {
        let name = tool.name.clone();
        let mut tools = self.tools.write().unwrap_or_else(|e| {
            error!("tool registry lock poisoned, recovering");
            e.into_inner()
        });
        tools.insert(name, tool);
    }

    fn unregister(&self, name: &str) {
        let mut tools = self.tools.write().unwrap_or_else(|e| {
            error!("tool registry lock poisoned, recovering");
            e.into_inner()
        });
        tools.remove(name);
    }

    fn get(&self, name: &str) -> Option<ToolDef> {
        let tools = self.tools.read().unwrap_or_else(|e| {
            error!("tool registry lock poisoned, recovering");
            e.into_inner()
        });
        tools.get(name).cloned().or_else(|| {
            // Backward compat: try atta-{name} with underscores → hyphens
            if !name.starts_with("atta-") {
                let prefixed = format!("atta-{}", name.replace('_', "-"));
                let result = tools.get(&prefixed).cloned();
                if result.is_some() {
                    tracing::warn!(
                        old_name = name,
                        new_name = prefixed.as_str(),
                        "tool name deprecated — use the atta- prefixed name"
                    );
                }
                result
            } else {
                None
            }
        })
    }

    fn get_schema(&self, name: &str) -> Option<ToolSchema> {
        let tools = self.tools.read().unwrap_or_else(|e| {
            error!("tool registry lock poisoned, recovering");
            e.into_inner()
        });
        tools.get(name).map(ToolSchema::from).or_else(|| {
            // Backward compat: try atta-{name} with underscores → hyphens
            if !name.starts_with("atta-") {
                let prefixed = format!("atta-{}", name.replace('_', "-"));
                let result = tools.get(&prefixed).map(ToolSchema::from);
                if result.is_some() {
                    tracing::warn!(
                        old_name = name,
                        new_name = prefixed.as_str(),
                        "tool name deprecated — use the atta- prefixed name"
                    );
                }
                result
            } else {
                None
            }
        })
    }

    fn list_schemas(&self) -> Vec<ToolSchema> {
        let tools = self.tools.read().unwrap_or_else(|e| {
            error!("tool registry lock poisoned, recovering");
            e.into_inner()
        });
        tools.values().map(ToolSchema::from).collect()
    }

    fn list_all(&self) -> Vec<ToolDef> {
        let tools = self.tools.read().unwrap_or_else(|e| {
            error!("tool registry lock poisoned, recovering");
            e.into_inner()
        });
        tools.values().cloned().collect()
    }

    async fn invoke(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> Result<serde_json::Value, AttaError> {
        let tool_def = {
            let tools = self.tools.read().unwrap_or_else(|e| {
            error!("tool registry lock poisoned, recovering");
            e.into_inner()
        });
            tools.get(tool_name).cloned().or_else(|| {
                // Backward compat: try atta-{name} with underscores → hyphens
                if !tool_name.starts_with("atta-") {
                    let prefixed = format!("atta-{}", tool_name.replace('_', "-"));
                    let result = tools.get(&prefixed).cloned();
                    if result.is_some() {
                        tracing::warn!(
                            old_name = tool_name,
                            new_name = prefixed.as_str(),
                            "tool name deprecated — use the atta- prefixed name"
                        );
                    }
                    result
                } else {
                    None
                }
            })
        };

        let tool_def = tool_def.ok_or_else(|| AttaError::ToolNotFound(tool_name.to_string()))?;

        match &tool_def.binding {
            ToolBinding::Builtin { handler_name } => {
                let handler = {
                    let builtins = self.builtins.read().unwrap_or_else(|e| {
                    error!("builtins lock poisoned, recovering");
                    e.into_inner()
                });
                    builtins.get(handler_name).cloned()
                };
                match handler {
                    Some(h) => {
                        info!(
                            tool = tool_name,
                            handler = handler_name.as_str(),
                            "invoking builtin tool"
                        );
                        h(arguments.clone()).await
                    }
                    None => Err(AttaError::ToolNotFound(format!(
                        "builtin handler '{}' not registered",
                        handler_name
                    ))),
                }
            }
            ToolBinding::Native { handler_name } => {
                let tool = {
                    let natives = self.natives.read().unwrap_or_else(|e| {
                    error!("natives lock poisoned, recovering");
                    e.into_inner()
                });
                    natives.get(handler_name).cloned()
                };
                match tool {
                    Some(t) => {
                        info!(
                            tool = tool_name,
                            handler = handler_name.as_str(),
                            "invoking native tool"
                        );
                        t.execute(arguments.clone()).await
                    }
                    None => Err(AttaError::ToolNotFound(format!(
                        "native handler '{}' not registered",
                        handler_name
                    ))),
                }
            }
            ToolBinding::Mcp { server_name } => {
                let registry = self.mcp_registry.as_ref().ok_or_else(|| {
                    AttaError::ToolNotFound(format!(
                        "mcp tool '{}' on server '{}': MCP registry not configured",
                        tool_name, server_name
                    ))
                })?;
                info!(
                    tool = tool_name,
                    server = server_name.as_str(),
                    "invoking MCP tool"
                );
                let result = registry
                    .call_tool(server_name, tool_name, arguments.clone())
                    .await?;

                if result.is_error {
                    let error_text = mcp_content_to_json(&result.content).to_string();
                    return Err(AttaError::Other(anyhow::anyhow!(
                        "MCP tool '{}' execution failed: {}",
                        tool_name, error_text
                    )));
                }

                Ok(mcp_content_to_json(&result.content))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::RiskLevel;

    fn make_tool(name: &str) -> ToolDef {
        ToolDef {
            name: name.to_string(),
            description: format!("Test tool: {}", name),
            binding: ToolBinding::Native {
                handler_name: name.to_string(),
            },
            risk_level: RiskLevel::Low,
            parameters: serde_json::json!({}),
        }
    }

    fn make_builtin_tool(name: &str, handler: &str) -> ToolDef {
        ToolDef {
            name: name.to_string(),
            description: format!("Builtin tool: {}", name),
            binding: ToolBinding::Builtin {
                handler_name: handler.to_string(),
            },
            risk_level: RiskLevel::Low,
            parameters: serde_json::json!({}),
        }
    }

    fn make_mcp_tool(name: &str, server: &str) -> ToolDef {
        ToolDef {
            name: name.to_string(),
            description: format!("MCP tool: {}", name),
            binding: ToolBinding::Mcp {
                server_name: server.to_string(),
            },
            risk_level: RiskLevel::Low,
            parameters: serde_json::json!({}),
        }
    }

    #[test]
    fn test_register_and_get() {
        let registry = DefaultToolRegistry::new();
        let tool = make_tool("git.clone");

        registry.register(tool.clone());
        let got = registry.get("git.clone");
        assert!(got.is_some());
        assert_eq!(got.unwrap().name, "git.clone");
    }

    #[test]
    fn test_unregister() {
        let registry = DefaultToolRegistry::new();
        registry.register(make_tool("git.clone"));
        registry.unregister("git.clone");
        assert!(registry.get("git.clone").is_none());
    }

    #[test]
    fn test_get_schema() {
        let registry = DefaultToolRegistry::new();
        registry.register(make_tool("git.clone"));

        let schema = registry.get_schema("git.clone");
        assert!(schema.is_some());
        let schema = schema.unwrap();
        assert_eq!(schema.name, "git.clone");
        assert_eq!(schema.description, "Test tool: git.clone");
    }

    #[test]
    fn test_list_schemas() {
        let registry = DefaultToolRegistry::new();
        registry.register(make_tool("git.clone"));
        registry.register(make_tool("git.push"));

        let schemas = registry.list_schemas();
        assert_eq!(schemas.len(), 2);
    }

    #[test]
    fn test_get_nonexistent() {
        let registry = DefaultToolRegistry::new();
        assert!(registry.get("nonexistent").is_none());
    }

    #[tokio::test]
    async fn test_invoke_not_found() {
        let registry = DefaultToolRegistry::new();
        let result = registry.invoke("nonexistent", &serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_invoke_builtin() {
        let registry = DefaultToolRegistry::new();
        registry.register_builtin("echo", |args| async move { Ok(args) });
        registry.register(make_builtin_tool("builtin-echo", "echo"));

        let result = registry
            .invoke("builtin-echo", &serde_json::json!({"text": "hello"}))
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), serde_json::json!({"text": "hello"}));
    }

    #[tokio::test]
    async fn test_invoke_mcp_no_registry() {
        // MCP tool without registry configured → descriptive error
        let registry = DefaultToolRegistry::new();
        registry.register(make_mcp_tool("search", "search-server"));

        let result = registry.invoke("search", &serde_json::json!({})).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("MCP registry not configured"));
    }

    #[tokio::test]
    async fn test_invoke_mcp_server_not_found() {
        // MCP tool with registry but server not registered → McpServerNotFound
        let mcp = Arc::new(McpRegistry::new());
        let registry = DefaultToolRegistry::new().with_mcp_registry(mcp);
        registry.register(make_mcp_tool("search", "nonexistent-server"));

        let result = registry.invoke("search", &serde_json::json!({})).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            AttaError::McpServerNotFound(name) => assert_eq!(name, "nonexistent-server"),
            other => panic!("expected McpServerNotFound, got: {other}"),
        }
    }

    #[tokio::test]
    async fn test_invoke_mcp_end_to_end() {
        use atta_mcp::traits::{McpClient, McpContent, McpToolInfo, McpToolResult};

        // Mock MCP client that always succeeds
        struct MockMcp;

        #[async_trait::async_trait]
        impl McpClient for MockMcp {
            async fn list_tools(&self) -> Result<Vec<McpToolInfo>, AttaError> {
                Ok(vec![])
            }
            async fn call_tool(
                &self,
                tool_name: &str,
                arguments: serde_json::Value,
            ) -> Result<McpToolResult, AttaError> {
                Ok(McpToolResult {
                    is_error: false,
                    content: vec![McpContent::Text {
                        text: format!("{}({})", tool_name, arguments),
                    }],
                })
            }
            async fn ping(&self) -> Result<(), AttaError> {
                Ok(())
            }
        }

        let mcp = Arc::new(McpRegistry::new());
        mcp.add("test-server", Arc::new(MockMcp)).await;

        let registry = DefaultToolRegistry::new().with_mcp_registry(mcp);
        registry.register(make_mcp_tool("mcp.search", "test-server"));

        let result = registry
            .invoke("mcp.search", &serde_json::json!({"query": "rust"}))
            .await;
        assert!(result.is_ok());
        let output = result.unwrap();
        // Single text content → plain string
        assert!(output.as_str().unwrap().contains("mcp.search"));
        assert!(output.as_str().unwrap().contains("rust"));
    }

    #[tokio::test]
    async fn test_invoke_mcp_error_result() {
        use atta_mcp::traits::{McpClient, McpContent, McpToolInfo, McpToolResult};

        struct ErrorMcp;

        #[async_trait::async_trait]
        impl McpClient for ErrorMcp {
            async fn list_tools(&self) -> Result<Vec<McpToolInfo>, AttaError> {
                Ok(vec![])
            }
            async fn call_tool(
                &self,
                _tool_name: &str,
                _arguments: serde_json::Value,
            ) -> Result<McpToolResult, AttaError> {
                Ok(McpToolResult {
                    is_error: true,
                    content: vec![McpContent::Text {
                        text: "permission denied".to_string(),
                    }],
                })
            }
            async fn ping(&self) -> Result<(), AttaError> {
                Ok(())
            }
        }

        let mcp = Arc::new(McpRegistry::new());
        mcp.add("err-server", Arc::new(ErrorMcp)).await;

        let registry = DefaultToolRegistry::new().with_mcp_registry(mcp);
        registry.register(make_mcp_tool("mcp.write", "err-server"));

        let result = registry.invoke("mcp.write", &serde_json::json!({})).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("permission denied"));
    }

    #[test]
    fn test_mcp_content_to_json_single_text() {
        let content = vec![McpContent::Text {
            text: "hello".to_string(),
        }];
        let json = mcp_content_to_json(&content);
        assert_eq!(json, serde_json::Value::String("hello".to_string()));
    }

    #[test]
    fn test_mcp_content_to_json_multi() {
        let content = vec![
            McpContent::Text {
                text: "line 1".to_string(),
            },
            McpContent::Text {
                text: "line 2".to_string(),
            },
        ];
        let json = mcp_content_to_json(&content);
        assert!(json.is_array());
        assert_eq!(json.as_array().unwrap().len(), 2);
    }
}
