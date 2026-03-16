//! Conversation control section — silent replies and directed reply tags

use super::super::section::{PromptContext, PromptSection};

/// Conversation control section — injects [SILENT] and [REPLY:] tag instructions
pub struct ConversationControlSection;

impl PromptSection for ConversationControlSection {
    fn name(&self) -> &str {
        "Conversation Control"
    }

    fn priority(&self) -> u32 {
        45
    }

    fn build(&self, _ctx: &PromptContext) -> Option<String> {
        Some(
            "- Use `[SILENT]` to indicate no user-facing response is needed \
             (e.g., background task completion).\n\
             - Use `[REPLY:channel:user]` to direct a response to a specific channel and user.\n\
             - Default: reply to the originating channel and sender."
                .to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::section::PromptContext;

    #[test]
    fn test_conversation_control_section() {
        let section = ConversationControlSection;
        let ctx = PromptContext::default();
        let output = section.build(&ctx).unwrap();

        assert!(output.contains("[SILENT]"));
        assert!(output.contains("[REPLY:channel:user]"));
        assert!(output.contains("originating channel"));
    }

    #[test]
    fn test_conversation_control_priority() {
        let section = ConversationControlSection;
        assert_eq!(section.priority(), 45);
    }
}
