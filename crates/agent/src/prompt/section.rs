//! PromptSection trait and PromptContext

use std::path::PathBuf;

use atta_types::{SkillDef, ToolSchema};

/// Controls how skills are rendered in the system prompt
#[derive(Debug, Clone)]
pub enum SkillsPromptMode {
    /// ≤5 skills → Full, >5 → Compact
    Auto,
    /// Full XML mode with system_prompt and tools
    Full,
    /// Compact one-line listing
    Compact,
}

impl Default for SkillsPromptMode {
    fn default() -> Self {
        Self::Auto
    }
}

/// Context passed to each prompt section during build
#[derive(Default)]
pub struct PromptContext {
    /// Workspace root directory
    pub workspace_root: Option<PathBuf>,
    /// Available tools
    pub tools: Vec<ToolSchema>,
    /// Available skills
    pub skills: Vec<SkillDef>,
    /// Channel name (e.g. "terminal", "telegram")
    pub channel: Option<String>,
    /// Current flow state name (if running in a flow)
    pub current_state: Option<String>,
    /// Model identifier
    pub model_id: String,
    /// Skills prompt rendering mode
    pub skills_prompt_mode: SkillsPromptMode,
}

/// A composable section of the system prompt
pub trait PromptSection: Send + Sync {
    /// Section name (used as header)
    fn name(&self) -> &str;

    /// Priority for ordering (lower = earlier). Default: 50
    fn priority(&self) -> u32 {
        50
    }

    /// Build the section content. Return `None` to skip this section.
    fn build(&self, ctx: &PromptContext) -> Option<String>;
}
