//! SystemPromptBuilder — assembles sections into a complete system prompt

use super::section::{PromptContext, PromptSection};
use super::sections;

/// Builds a system prompt from composable sections
pub struct SystemPromptBuilder {
    sections: Vec<Box<dyn PromptSection>>,
}

impl SystemPromptBuilder {
    /// Create a builder with all 10 default sections
    pub fn with_defaults() -> Self {
        Self {
            sections: vec![
                Box::new(sections::PromptGuardSection),
                Box::new(sections::IdentitySection),
                Box::new(sections::SafetySection),
                Box::new(sections::ToolsSection),
                Box::new(sections::SkillsSection),
                Box::new(sections::WorkspaceSection),
                Box::new(sections::RuntimeSection),
                Box::new(sections::DateTimeSection),
                Box::new(sections::ConversationControlSection),
                Box::new(sections::ChannelMediaSection),
            ],
        }
    }

    /// Create an empty builder
    pub fn new() -> Self {
        Self {
            sections: Vec::new(),
        }
    }

    /// Add a custom section
    pub fn add_section(mut self, section: impl PromptSection + 'static) -> Self {
        self.sections.push(Box::new(section));
        self
    }

    /// Build the complete system prompt
    ///
    /// Sections are sorted by priority (lower first). Empty sections are skipped.
    /// Each section is rendered as `## <NAME>\n<content>`.
    pub fn build(&self, ctx: &PromptContext) -> String {
        let mut sorted: Vec<&Box<dyn PromptSection>> = self.sections.iter().collect();
        sorted.sort_by_key(|s| s.priority());

        let mut parts = Vec::new();
        for section in sorted {
            if let Some(content) = section.build(ctx) {
                if !content.is_empty() {
                    parts.push(format!("## {}\n{}", section.name(), content));
                }
            }
        }

        parts.join("\n\n")
    }
}

impl Default for SystemPromptBuilder {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestSection {
        name: &'static str,
        priority: u32,
        content: Option<&'static str>,
    }

    impl PromptSection for TestSection {
        fn name(&self) -> &str {
            self.name
        }
        fn priority(&self) -> u32 {
            self.priority
        }
        fn build(&self, _ctx: &PromptContext) -> Option<String> {
            self.content.map(|s| s.to_string())
        }
    }

    #[test]
    fn test_builder_sorts_by_priority() {
        let builder = SystemPromptBuilder::new()
            .add_section(TestSection {
                name: "B",
                priority: 20,
                content: Some("second"),
            })
            .add_section(TestSection {
                name: "A",
                priority: 10,
                content: Some("first"),
            });

        let result = builder.build(&PromptContext::default());
        let a_pos = result.find("## A").unwrap();
        let b_pos = result.find("## B").unwrap();
        assert!(a_pos < b_pos);
    }

    #[test]
    fn test_builder_skips_empty_sections() {
        let builder = SystemPromptBuilder::new()
            .add_section(TestSection {
                name: "Present",
                priority: 10,
                content: Some("here"),
            })
            .add_section(TestSection {
                name: "Absent",
                priority: 20,
                content: None,
            });

        let result = builder.build(&PromptContext::default());
        assert!(result.contains("Present"));
        assert!(!result.contains("Absent"));
    }

    #[test]
    fn test_with_defaults_produces_output() {
        let builder = SystemPromptBuilder::with_defaults();
        let result = builder.build(&PromptContext::default());
        // Should at least have Identity and Safety
        assert!(result.contains("Identity"));
        assert!(result.contains("Safety"));
    }
}
