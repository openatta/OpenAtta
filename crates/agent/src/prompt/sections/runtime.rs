//! Runtime section — host info, OS, model name

use super::super::section::{PromptContext, PromptSection};

/// Runtime section — hostname, OS, model identifier
pub struct RuntimeSection;

impl PromptSection for RuntimeSection {
    fn name(&self) -> &str {
        "Runtime"
    }

    fn priority(&self) -> u32 {
        70
    }

    fn build(&self, ctx: &PromptContext) -> Option<String> {
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        let os = std::env::consts::OS;
        let arch = std::env::consts::ARCH;

        let mut info = format!("- Host: {hostname}\n- OS: {os} ({arch})");

        if !ctx.model_id.is_empty() {
            info.push_str(&format!("\n- Model: {}", ctx.model_id));
        }

        Some(info)
    }
}
