//! Mock tool implementations for integration testing

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use atta_types::{AttaError, RiskLevel, ToolBinding, ToolDef, ToolRegistry, ToolSchema};

/// Echo tool — returns input message as output
pub fn echo_tool_def() -> ToolDef {
    ToolDef {
        name: "echo".to_string(),
        description: "Echoes the input message".to_string(),
        binding: ToolBinding::Builtin {
            handler_name: "echo".to_string(),
        },
        risk_level: RiskLevel::Low,
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "message": { "type": "string" }
            },
            "required": ["message"]
        }),
    }
}

/// Failing tool — always returns an error
pub fn failing_tool_def() -> ToolDef {
    ToolDef {
        name: "failing_tool".to_string(),
        description: "Always fails".to_string(),
        binding: ToolBinding::Builtin {
            handler_name: "failing_tool".to_string(),
        },
        risk_level: RiskLevel::Low,
        parameters: serde_json::json!({"type": "object"}),
    }
}

/// High-risk tool
pub fn high_risk_tool_def() -> ToolDef {
    ToolDef {
        name: "dangerous_tool".to_string(),
        description: "A high-risk tool".to_string(),
        binding: ToolBinding::Builtin {
            handler_name: "dangerous_tool".to_string(),
        },
        risk_level: RiskLevel::High,
        parameters: serde_json::json!({"type": "object"}),
    }
}

/// Counting tool — tracks invocation count via shared Arc
pub struct CountingRegistry {
    tools: RwLock<HashMap<String, ToolDef>>,
    pub invocation_count: Arc<Mutex<usize>>,
    /// If set, the tool with this name will return an error
    pub fail_tool: Option<String>,
}

impl CountingRegistry {
    pub fn new(tools: Vec<ToolDef>) -> Self {
        let map: HashMap<String, ToolDef> =
            tools.into_iter().map(|t| (t.name.clone(), t)).collect();
        Self {
            tools: RwLock::new(map),
            invocation_count: Arc::new(Mutex::new(0)),
            fail_tool: None,
        }
    }

    pub fn with_failing(mut self, tool_name: &str) -> Self {
        self.fail_tool = Some(tool_name.to_string());
        self
    }

    pub fn count(&self) -> usize {
        *self.invocation_count.lock().unwrap()
    }
}

#[async_trait::async_trait]
impl ToolRegistry for CountingRegistry {
    fn register(&self, tool: ToolDef) {
        self.tools.write().unwrap().insert(tool.name.clone(), tool);
    }
    fn unregister(&self, name: &str) {
        self.tools.write().unwrap().remove(name);
    }
    fn get(&self, name: &str) -> Option<ToolDef> {
        self.tools.read().unwrap().get(name).cloned()
    }
    fn get_schema(&self, name: &str) -> Option<ToolSchema> {
        self.get(name).map(|t| ToolSchema::from(&t))
    }
    fn list_schemas(&self) -> Vec<ToolSchema> {
        self.tools
            .read()
            .unwrap()
            .values()
            .map(ToolSchema::from)
            .collect()
    }
    fn list_all(&self) -> Vec<ToolDef> {
        self.tools.read().unwrap().values().cloned().collect()
    }
    async fn invoke(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> Result<serde_json::Value, AttaError> {
        *self.invocation_count.lock().unwrap() += 1;

        if self.fail_tool.as_deref() == Some(tool_name) {
            return Err(AttaError::ToolNotFound(tool_name.to_string()));
        }

        // Echo tool returns the message argument
        if tool_name == "echo" {
            let msg = arguments
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("(no message)");
            return Ok(serde_json::json!({ "output": msg }));
        }

        Ok(serde_json::json!({ "result": format!("ok from {}", tool_name) }))
    }
}

/// Simple mock registry that always succeeds
pub struct SimpleRegistry {
    tools: RwLock<HashMap<String, ToolDef>>,
}

impl SimpleRegistry {
    pub fn new(tools: Vec<ToolDef>) -> Self {
        let map: HashMap<String, ToolDef> =
            tools.into_iter().map(|t| (t.name.clone(), t)).collect();
        Self {
            tools: RwLock::new(map),
        }
    }

    pub fn empty() -> Self {
        Self::new(vec![])
    }
}

#[async_trait::async_trait]
impl ToolRegistry for SimpleRegistry {
    fn register(&self, tool: ToolDef) {
        self.tools.write().unwrap().insert(tool.name.clone(), tool);
    }
    fn unregister(&self, name: &str) {
        self.tools.write().unwrap().remove(name);
    }
    fn get(&self, name: &str) -> Option<ToolDef> {
        self.tools.read().unwrap().get(name).cloned()
    }
    fn get_schema(&self, name: &str) -> Option<ToolSchema> {
        self.get(name).map(|t| ToolSchema::from(&t))
    }
    fn list_schemas(&self) -> Vec<ToolSchema> {
        self.tools
            .read()
            .unwrap()
            .values()
            .map(ToolSchema::from)
            .collect()
    }
    fn list_all(&self) -> Vec<ToolDef> {
        self.tools.read().unwrap().values().cloned().collect()
    }
    async fn invoke(
        &self,
        tool_name: &str,
        _arguments: &serde_json::Value,
    ) -> Result<serde_json::Value, AttaError> {
        Ok(serde_json::json!({ "result": format!("ok from {}", tool_name) }))
    }
}
