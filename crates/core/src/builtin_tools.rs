//! Builtin tools registration
//!
//! Registers built-in tools (echo, get_current_time) for testing and basic functionality.

use crate::tool_registry::DefaultToolRegistry;
use atta_types::{RiskLevel, ToolBinding, ToolDef, ToolRegistry};

/// Register all built-in tools on the given registry.
pub fn register_builtins(registry: &DefaultToolRegistry) {
    // builtin-echo — echoes back the input
    registry.register_builtin("echo", |args| async move {
        Ok(serde_json::json!({ "echoed": args }))
    });
    registry.register(ToolDef {
        name: "builtin-echo".to_string(),
        description: "Echo back the input arguments. Useful for testing.".to_string(),
        binding: ToolBinding::Builtin {
            handler_name: "echo".to_string(),
        },
        risk_level: RiskLevel::Low,
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "Text to echo back" }
            }
        }),
    });

    // builtin-get_current_time — returns the current UTC time
    registry.register_builtin("get_current_time", |_args| async move {
        Ok(serde_json::json!({
            "utc": chrono::Utc::now().to_rfc3339(),
        }))
    });
    registry.register(ToolDef {
        name: "builtin-get_current_time".to_string(),
        description: "Get the current UTC time.".to_string(),
        binding: ToolBinding::Builtin {
            handler_name: "get_current_time".to_string(),
        },
        risk_level: RiskLevel::Low,
        parameters: serde_json::json!({
            "type": "object",
            "properties": {}
        }),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::ToolRegistry;

    #[test]
    fn test_register_builtins() {
        let registry = DefaultToolRegistry::new();
        register_builtins(&registry);

        assert!(registry.get("builtin-echo").is_some());
        assert!(registry.get("builtin-get_current_time").is_some());
        assert_eq!(registry.list_schemas().len(), 2);
    }

    #[tokio::test]
    async fn test_echo_invoke() {
        let registry = DefaultToolRegistry::new();
        register_builtins(&registry);

        let result = registry
            .invoke("builtin-echo", &serde_json::json!({"text": "hello"}))
            .await
            .unwrap();
        assert_eq!(result, serde_json::json!({"echoed": {"text": "hello"}}));
    }

    #[tokio::test]
    async fn test_get_current_time_invoke() {
        let registry = DefaultToolRegistry::new();
        register_builtins(&registry);

        let result = registry
            .invoke("builtin-get_current_time", &serde_json::json!({}))
            .await
            .unwrap();
        assert!(result.get("utc").is_some());
    }
}
