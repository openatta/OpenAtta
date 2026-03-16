//! Skills section — lists available skills in Full or Compact mode

use super::super::section::{PromptContext, PromptSection, SkillsPromptMode};
use atta_types::SkillDef;

/// Threshold for Auto mode: ≤ this count uses Full, otherwise Compact
const AUTO_FULL_THRESHOLD: usize = 5;

/// Skills section — lists available skill names and descriptions
pub struct SkillsSection;

impl SkillsSection {
    /// Render skills in compact one-line format
    fn build_compact(skills: &[SkillDef]) -> String {
        let mut lines = vec![format!("You can invoke {} skills:", skills.len())];
        for skill in skills {
            let desc = skill.description.as_deref().unwrap_or("No description");
            lines.push(format!("- **{}** (v{}): {}", skill.id, skill.version, desc));
        }
        lines.join("\n")
    }

    /// Render skills in full XML format with system_prompt and tools
    fn build_full(skills: &[SkillDef]) -> String {
        let mut lines = vec!["<available_skills>".to_string()];
        for skill in skills {
            let desc = skill.description.as_deref().unwrap_or("No description");
            lines.push(format!(
                "<skill id=\"{}\" version=\"{}\">",
                skill.id, skill.version
            ));
            lines.push(format!("  <description>{desc}</description>"));
            lines.push(format!(
                "  <system_prompt>{}</system_prompt>",
                skill.system_prompt
            ));
            if !skill.tools.is_empty() {
                lines.push(format!("  <tools>{}</tools>", skill.tools.join(", ")));
            }
            lines.push("</skill>".to_string());
        }
        lines.push("</available_skills>".to_string());
        lines.join("\n")
    }
}

impl PromptSection for SkillsSection {
    fn name(&self) -> &str {
        "Available Skills"
    }

    fn priority(&self) -> u32 {
        25
    }

    fn build(&self, ctx: &PromptContext) -> Option<String> {
        if ctx.skills.is_empty() {
            return None;
        }

        let use_full = match ctx.skills_prompt_mode {
            SkillsPromptMode::Full => true,
            SkillsPromptMode::Compact => false,
            SkillsPromptMode::Auto => ctx.skills.len() <= AUTO_FULL_THRESHOLD,
        };

        if use_full {
            Some(Self::build_full(&ctx.skills))
        } else {
            Some(Self::build_compact(&ctx.skills))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::section::PromptContext;

    fn make_skill(id: &str) -> SkillDef {
        SkillDef {
            id: id.to_string(),
            version: "1.0".to_string(),
            name: Some(id.to_string()),
            description: Some(format!("{id} description")),
            system_prompt: format!("You are a {id} expert"),
            tools: vec!["web_fetch".to_string(), "file_read".to_string()],
            steps: None,
            output_format: None,
            requires_approval: false,
            risk_level: Default::default(),
            tags: vec![],
            variables: None,
            author: None,
            source: "builtin".to_string(),
        }
    }

    #[test]
    fn test_compact_mode() {
        let ctx = PromptContext {
            skills: vec![make_skill("summarize")],
            skills_prompt_mode: SkillsPromptMode::Compact,
            ..Default::default()
        };

        let section = SkillsSection;
        let output = section.build(&ctx).unwrap();
        assert!(output.contains("**summarize** (v1.0)"));
        assert!(!output.contains("<skill"));
    }

    #[test]
    fn test_full_mode() {
        let ctx = PromptContext {
            skills: vec![make_skill("summarize")],
            skills_prompt_mode: SkillsPromptMode::Full,
            ..Default::default()
        };

        let section = SkillsSection;
        let output = section.build(&ctx).unwrap();
        assert!(output.contains("<available_skills>"));
        assert!(output.contains("<skill id=\"summarize\" version=\"1.0\">"));
        assert!(output.contains("<system_prompt>"));
        assert!(output.contains("<tools>web_fetch, file_read</tools>"));
        assert!(output.contains("</available_skills>"));
    }

    #[test]
    fn test_auto_mode_few_skills_uses_full() {
        let skills: Vec<SkillDef> = (0..3).map(|i| make_skill(&format!("skill_{i}"))).collect();
        let ctx = PromptContext {
            skills,
            skills_prompt_mode: SkillsPromptMode::Auto,
            ..Default::default()
        };

        let section = SkillsSection;
        let output = section.build(&ctx).unwrap();
        assert!(output.contains("<available_skills>"));
    }

    #[test]
    fn test_auto_mode_many_skills_uses_compact() {
        let skills: Vec<SkillDef> = (0..8).map(|i| make_skill(&format!("skill_{i}"))).collect();
        let ctx = PromptContext {
            skills,
            skills_prompt_mode: SkillsPromptMode::Auto,
            ..Default::default()
        };

        let section = SkillsSection;
        let output = section.build(&ctx).unwrap();
        assert!(!output.contains("<available_skills>"));
        assert!(output.contains("You can invoke 8 skills:"));
    }

    #[test]
    fn test_empty_skills() {
        let ctx = PromptContext::default();
        let section = SkillsSection;
        assert!(section.build(&ctx).is_none());
    }
}
