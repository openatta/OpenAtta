//! Tool 类型定义与 ToolRegistry trait

use serde::{Deserialize, Serialize};

use crate::AttaError;

/// Tool 绑定来源
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ToolBinding {
    Mcp { server_name: String },
    Builtin { handler_name: String },
    Native { handler_name: String },
}

/// Tool 风险等级
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

impl Default for RiskLevel {
    fn default() -> Self {
        Self::Low
    }
}

/// 已注册的 Tool 完整定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub binding: ToolBinding,
    pub risk_level: RiskLevel,
    pub parameters: serde_json::Value,
}

/// Tool Schema（供 LLM 感知）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

impl From<&ToolDef> for ToolSchema {
    fn from(t: &ToolDef) -> Self {
        Self {
            name: t.name.clone(),
            description: t.description.clone(),
            parameters: t.parameters.clone(),
        }
    }
}

/// Tool 注册表 trait
///
/// 所有 Tool 的注册、查询、调用统一接口。Core 层通过 `Arc<dyn ToolRegistry>`
/// 持有实例，ConditionEvaluator 用 `&dyn ToolRegistry` 进行条件求值。
#[async_trait::async_trait]
pub trait ToolRegistry: Send + Sync + 'static {
    /// 注册一个 Tool
    fn register(&self, tool: ToolDef);

    /// 注销指定名称的 Tool
    fn unregister(&self, name: &str);

    /// 根据名称获取 Tool 定义（返回 owned clone）
    fn get(&self, name: &str) -> Option<ToolDef>;

    /// 获取 Tool 的 Schema（供 LLM 感知）
    fn get_schema(&self, name: &str) -> Option<ToolSchema>;

    /// 列出所有已注册 Tool 的 Schema
    fn list_schemas(&self) -> Vec<ToolSchema>;

    /// 列出所有已注册 Tool 的完整定义
    fn list_all(&self) -> Vec<ToolDef>;

    /// 调用 Tool
    async fn invoke(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> Result<serde_json::Value, AttaError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tool_def() -> ToolDef {
        ToolDef {
            name: "web_search".to_string(),
            description: "Search the web".to_string(),
            binding: ToolBinding::Native {
                handler_name: "search-plugin".to_string(),
            },
            risk_level: RiskLevel::Medium,
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                },
                "required": ["query"]
            }),
        }
    }

    // ── ToolSchema From<&ToolDef> ──

    #[test]
    fn tool_schema_from_tool_def_copies_name_desc_params() {
        let td = sample_tool_def();
        let schema = ToolSchema::from(&td);

        assert_eq!(schema.name, "web_search");
        assert_eq!(schema.description, "Search the web");
        assert_eq!(schema.parameters, td.parameters);
    }

    #[test]
    fn tool_schema_from_tool_def_does_not_include_binding_or_risk() {
        let td = sample_tool_def();
        let schema = ToolSchema::from(&td);
        let json = serde_json::to_value(&schema).unwrap();

        // ToolSchema should not have binding or risk_level fields
        assert!(json.get("binding").is_none());
        assert!(json.get("risk_level").is_none());
    }

    // ── RiskLevel default ──

    #[test]
    fn risk_level_default_is_low() {
        let rl = RiskLevel::default();
        assert_eq!(rl, RiskLevel::Low);
    }

    // ── RiskLevel serde ──

    #[test]
    fn risk_level_serde_round_trip() {
        let cases = vec![
            (RiskLevel::Low, r#""low""#),
            (RiskLevel::Medium, r#""medium""#),
            (RiskLevel::High, r#""high""#),
        ];
        for (level, expected) in cases {
            let json = serde_json::to_string(&level).unwrap();
            assert_eq!(json, expected);
            let back: RiskLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(back, level);
        }
    }

    // ── ToolBinding serde ──

    #[test]
    fn tool_binding_mcp_serde_round_trip() {
        let binding = ToolBinding::Mcp {
            server_name: "brave-search".to_string(),
        };
        let json = serde_json::to_string(&binding).unwrap();
        let back: ToolBinding = serde_json::from_str(&json).unwrap();
        match back {
            ToolBinding::Mcp { server_name } => assert_eq!(server_name, "brave-search"),
            _ => panic!("expected Mcp binding"),
        }
    }

    #[test]
    fn tool_binding_builtin_serde_round_trip() {
        let binding = ToolBinding::Builtin {
            handler_name: "list_tools".to_string(),
        };
        let json = serde_json::to_string(&binding).unwrap();
        let back: ToolBinding = serde_json::from_str(&json).unwrap();
        match back {
            ToolBinding::Builtin { handler_name } => assert_eq!(handler_name, "list_tools"),
            _ => panic!("expected Builtin binding"),
        }
    }

    #[test]
    fn tool_binding_native_serde_round_trip() {
        let binding = ToolBinding::Native {
            handler_name: "file_read".to_string(),
        };
        let json = serde_json::to_string(&binding).unwrap();
        let back: ToolBinding = serde_json::from_str(&json).unwrap();
        match back {
            ToolBinding::Native { handler_name } => assert_eq!(handler_name, "file_read"),
            _ => panic!("expected Native binding"),
        }
    }

    // ── ToolDef serde round-trip ──

    #[test]
    fn tool_def_serde_round_trip() {
        let td = sample_tool_def();
        let json = serde_json::to_string(&td).unwrap();
        let back: ToolDef = serde_json::from_str(&json).unwrap();

        assert_eq!(back.name, "web_search");
        assert_eq!(back.description, "Search the web");
        assert_eq!(back.risk_level, RiskLevel::Medium);
        assert_eq!(back.parameters, td.parameters);
    }

    // ── ToolSchema serde round-trip ──

    #[test]
    fn tool_schema_serde_round_trip() {
        let schema = ToolSchema {
            name: "calc".to_string(),
            description: "Calculator".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        };
        let json = serde_json::to_string(&schema).unwrap();
        let back: ToolSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "calc");
        assert_eq!(back.description, "Calculator");
    }
}
