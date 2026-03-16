//! 记忆系统类型

use serde::{Deserialize, Serialize};

/// 记忆类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    TaskResult,
    SkillExperience,
    UserPreference,
    Knowledge,
}
