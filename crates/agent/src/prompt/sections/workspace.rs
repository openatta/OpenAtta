//! Workspace section — current working directory info

use super::super::section::{PromptContext, PromptSection};

/// Workspace section — reports the current workspace path
pub struct WorkspaceSection;

impl PromptSection for WorkspaceSection {
    fn name(&self) -> &str {
        "Workspace"
    }

    fn priority(&self) -> u32 {
        60
    }

    fn build(&self, ctx: &PromptContext) -> Option<String> {
        ctx.workspace_root
            .as_ref()
            .map(|root| format!("Working directory: `{}`", root.display()))
    }
}
