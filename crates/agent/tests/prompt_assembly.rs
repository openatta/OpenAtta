//! TG1 — Full prompt assembly integration tests
//!
//! Verifies that `SystemPromptBuilder` correctly assembles all 10 default sections,
//! respects priority ordering, and handles edge cases (empty tools, empty skills,
//! missing channel, custom sections, etc.).

mod common;

use atta_agent::prompt::{PromptContext, PromptSection, SkillsPromptMode, SystemPromptBuilder};
use common::fixtures::{make_skill, make_tool_schema};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// 1. Default builder includes Identity and Safety (minimum viable prompt)
// ---------------------------------------------------------------------------
#[test]
fn test_default_builder_includes_identity_and_safety() {
    let builder = SystemPromptBuilder::with_defaults();
    let ctx = PromptContext::default();
    let output = builder.build(&ctx);

    // Identity section always renders (intro text is unconditional)
    assert!(
        output.contains("## Identity"),
        "Default prompt must contain Identity section"
    );
    // Safety section always renders
    assert!(
        output.contains("## Safety Rules"),
        "Default prompt must contain Safety Rules section"
    );
}

// ---------------------------------------------------------------------------
// 2. Default builder includes the Prompt Guard section
// ---------------------------------------------------------------------------
#[test]
fn test_default_builder_includes_prompt_guard() {
    let builder = SystemPromptBuilder::with_defaults();
    let ctx = PromptContext::default();
    let output = builder.build(&ctx);

    assert!(
        output.contains("## Prompt Injection Guard"),
        "Default prompt must contain Prompt Injection Guard section"
    );
    // Guard has priority 5, Identity has 10 — Guard must appear first
    let guard_pos = output.find("## Prompt Injection Guard").unwrap();
    let identity_pos = output.find("## Identity").unwrap();
    assert!(
        guard_pos < identity_pos,
        "Prompt Injection Guard (priority 5) must appear before Identity (priority 10)"
    );
}

// ---------------------------------------------------------------------------
// 3. Full context with tools, skills, workspace, channel — all 10 sections present
// ---------------------------------------------------------------------------
#[test]
fn test_full_context_all_sections() {
    let dir = TempDir::new().unwrap();
    // Create at least one identity file so the workspace identity loading works
    std::fs::write(dir.path().join("AGENTS.md"), "Agent role info").unwrap();

    let tool = make_tool_schema(
        "file_read",
        "Read a file",
        serde_json::json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"]
        }),
    );

    let skill = make_skill("summarize");

    let ctx = PromptContext {
        workspace_root: Some(dir.path().to_path_buf()),
        tools: vec![tool],
        skills: vec![skill],
        channel: Some("telegram".to_string()),
        current_state: None,
        model_id: "gpt-4".to_string(),
        skills_prompt_mode: SkillsPromptMode::Full,
    };

    let builder = SystemPromptBuilder::with_defaults();
    let output = builder.build(&ctx);

    // All 10 section headers expected
    let expected_headers = [
        "## Prompt Injection Guard",
        "## Identity",
        "## Safety Rules",
        "## Available Tools",
        "## Available Skills",
        "## Workspace",
        "## Runtime",
        "## Date & Time",
        "## Conversation Control",
        "## Channel",
    ];

    for header in &expected_headers {
        assert!(
            output.contains(header),
            "Full-context prompt is missing section: {header}"
        );
    }
}

// ---------------------------------------------------------------------------
// 4. Section priority ordering: Guard(5) < Identity(10) < Safety(15) < Tools(20) < Skills(25)
// ---------------------------------------------------------------------------
#[test]
fn test_section_priority_ordering() {
    let dir = TempDir::new().unwrap();
    let tool = make_tool_schema("ping", "Ping", serde_json::json!({}));
    let skill = make_skill("s1");

    let ctx = PromptContext {
        workspace_root: Some(dir.path().to_path_buf()),
        tools: vec![tool],
        skills: vec![skill],
        channel: Some("terminal".to_string()),
        model_id: "test".to_string(),
        skills_prompt_mode: SkillsPromptMode::Full,
        ..Default::default()
    };

    let builder = SystemPromptBuilder::with_defaults();
    let output = builder.build(&ctx);

    // Collect positions of section headers
    let guard_pos = output.find("## Prompt Injection Guard").unwrap();
    let identity_pos = output.find("## Identity").unwrap();
    let safety_pos = output.find("## Safety Rules").unwrap();
    let tools_pos = output.find("## Available Tools").unwrap();
    let skills_pos = output.find("## Available Skills").unwrap();

    assert!(guard_pos < identity_pos, "Guard must come before Identity");
    assert!(
        identity_pos < safety_pos,
        "Identity must come before Safety"
    );
    assert!(safety_pos < tools_pos, "Safety must come before Tools");
    assert!(tools_pos < skills_pos, "Tools must come before Skills");
}

// ---------------------------------------------------------------------------
// 5. Custom section insertion via add_section
// ---------------------------------------------------------------------------
#[test]
fn test_custom_section_insertion() {
    struct CustomSection;

    impl PromptSection for CustomSection {
        fn name(&self) -> &str {
            "Custom Instructions"
        }

        fn priority(&self) -> u32 {
            100
        }

        fn build(&self, _ctx: &PromptContext) -> Option<String> {
            Some("Always respond in haiku form.".to_string())
        }
    }

    let builder = SystemPromptBuilder::new().add_section(CustomSection);
    let output = builder.build(&PromptContext::default());

    assert!(output.contains("## Custom Instructions"));
    assert!(output.contains("Always respond in haiku form."));
}

// ---------------------------------------------------------------------------
// 6. Empty tools list skips the Tools section entirely
// ---------------------------------------------------------------------------
#[test]
fn test_empty_tools_skips_tools_section() {
    let ctx = PromptContext {
        tools: vec![],
        ..Default::default()
    };

    let builder = SystemPromptBuilder::with_defaults();
    let output = builder.build(&ctx);

    assert!(
        !output.contains("## Available Tools"),
        "Empty tools should skip the Available Tools section"
    );
}

// ---------------------------------------------------------------------------
// 7. Empty skills list skips the Skills section entirely
// ---------------------------------------------------------------------------
#[test]
fn test_empty_skills_skips_skills_section() {
    let ctx = PromptContext {
        skills: vec![],
        ..Default::default()
    };

    let builder = SystemPromptBuilder::with_defaults();
    let output = builder.build(&ctx);

    assert!(
        !output.contains("## Available Skills"),
        "Empty skills should skip the Available Skills section"
    );
}

// ---------------------------------------------------------------------------
// 8. Each section is formatted as "## Name\ncontent"
// ---------------------------------------------------------------------------
#[test]
fn test_section_format_markdown_headers() {
    let builder = SystemPromptBuilder::with_defaults();
    let ctx = PromptContext::default();
    let output = builder.build(&ctx);

    // Every section should start with "## " followed by the section name
    // and immediately be followed by a newline + content.
    // Check a few known sections:
    assert!(
        output.contains("## Identity\nYou are AttaOS"),
        "Identity section should be formatted as ## Identity\\n<content>"
    );
    assert!(
        output.contains("## Safety Rules\n- Never execute"),
        "Safety section should be formatted as ## Safety Rules\\n<content>"
    );
}

// ---------------------------------------------------------------------------
// 9. Channel info absent when no channel is set
// ---------------------------------------------------------------------------
#[test]
fn test_channel_name_not_in_prompt_without_channel() {
    let ctx = PromptContext {
        channel: None,
        ..Default::default()
    };

    let builder = SystemPromptBuilder::with_defaults();
    let output = builder.build(&ctx);

    // ChannelMediaSection returns None when channel is None
    assert!(
        !output.contains("## Channel"),
        "Channel section must be absent when no channel is configured"
    );
}

// ---------------------------------------------------------------------------
// 10. Workspace identity files loaded when workspace_root is set
// ---------------------------------------------------------------------------
#[test]
fn test_workspace_identity_files_loaded() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("SOUL.md"), "Kind and thoughtful").unwrap();
    std::fs::write(dir.path().join("USER.md"), "Prefers formal tone").unwrap();

    let ctx = PromptContext {
        workspace_root: Some(dir.path().to_path_buf()),
        ..Default::default()
    };

    let builder = SystemPromptBuilder::with_defaults();
    let output = builder.build(&ctx);

    assert!(
        output.contains("### SOUL.md"),
        "SOUL.md content should appear in the Identity section"
    );
    assert!(output.contains("Kind and thoughtful"));
    assert!(
        output.contains("### USER.md"),
        "USER.md content should appear in the Identity section"
    );
    assert!(output.contains("Prefers formal tone"));
}

// ---------------------------------------------------------------------------
// 11. Conversation control tags appear in output
// ---------------------------------------------------------------------------
#[test]
fn test_conversation_control_in_output() {
    let builder = SystemPromptBuilder::with_defaults();
    let ctx = PromptContext::default();
    let output = builder.build(&ctx);

    assert!(
        output.contains("[SILENT]"),
        "Conversation Control section must include [SILENT] tag"
    );
    assert!(
        output.contains("[REPLY:"),
        "Conversation Control section must include [REPLY:] tag"
    );
}

// ---------------------------------------------------------------------------
// 12. SystemPromptBuilder::new() produces empty output
// ---------------------------------------------------------------------------
#[test]
fn test_builder_new_empty() {
    let builder = SystemPromptBuilder::new();
    let output = builder.build(&PromptContext::default());

    assert!(
        output.is_empty(),
        "SystemPromptBuilder::new() with no sections should produce empty string"
    );
}
