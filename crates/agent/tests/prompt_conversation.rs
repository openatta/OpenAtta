//! TG6 — Conversation control section integration tests
//!
//! Validates that the ConversationControlSection documents the [SILENT] and
//! [REPLY:channel:user] tags, specifies a default reply policy, and reports
//! the correct priority.

mod common;

use atta_agent::prompt::sections::ConversationControlSection;
use atta_agent::prompt::{PromptContext, PromptSection};

/// Helper: build the conversation control section with a default context.
fn conversation_output() -> String {
    let section = ConversationControlSection;
    let ctx = PromptContext::default();
    section
        .build(&ctx)
        .expect("ConversationControlSection should always produce output")
}

// ---------------------------------------------------------------------------
// 1. [SILENT] tag
// ---------------------------------------------------------------------------

#[test]
fn test_silent_tag_documented() {
    let output = conversation_output();
    assert!(
        output.contains("[SILENT]"),
        "ConversationControlSection must document the [SILENT] tag for suppressing user-facing replies"
    );
}

// ---------------------------------------------------------------------------
// 2. [REPLY:channel:user] tag
// ---------------------------------------------------------------------------

#[test]
fn test_reply_tag_documented() {
    let output = conversation_output();
    assert!(
        output.contains("[REPLY:channel:user]"),
        "ConversationControlSection must document the [REPLY:channel:user] directed-reply tag"
    );
}

// ---------------------------------------------------------------------------
// 3. Default reply policy
// ---------------------------------------------------------------------------

#[test]
fn test_default_reply_documented() {
    let output = conversation_output();
    assert!(
        output.contains("originating channel") && output.contains("sender"),
        "ConversationControlSection must state that the default is to reply to the originating channel and sender"
    );
}

// ---------------------------------------------------------------------------
// 4. Priority value
// ---------------------------------------------------------------------------

#[test]
fn test_priority_is_45() {
    let section = ConversationControlSection;
    assert_eq!(
        section.priority(),
        45,
        "ConversationControlSection priority must be 45"
    );
}
