//! Skill 定义类型

use serde::{Deserialize, Serialize};

use crate::tool::RiskLevel;

fn default_source() -> String {
    "builtin".to_string()
}

/// Skill 定义（行为模板）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDef {
    pub id: String,
    pub version: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub system_prompt: String,
    pub tools: Vec<String>,
    pub steps: Option<Vec<SkillStep>>,
    pub output_format: Option<String>,
    pub requires_approval: bool,
    pub risk_level: RiskLevel,
    pub tags: Vec<String>,
    pub variables: Option<Vec<VariableDef>>,
    /// Skill author
    pub author: Option<String>,
    /// "builtin" | "imported"
    #[serde(default = "default_source")]
    pub source: String,
}

/// Skill 执行步骤
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillStep {
    pub action: String,
    pub description: Option<String>,
}

/// Skill 变量定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableDef {
    pub name: String,
    pub description: Option<String>,
    pub required: bool,
    pub default: Option<serde_json::Value>,
}
