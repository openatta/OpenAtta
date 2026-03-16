//! TG4 — Skills Full/Compact/Auto mode switching integration tests
//!
//! Verifies that `SkillsSection` renders skills in the correct format
//! depending on the `SkillsPromptMode` (Full XML, Compact markdown, Auto
//! with threshold at 5), and handles edge cases like forced modes and
//! empty skill lists.

mod common;

use atta_agent::prompt::sections::SkillsSection;
use atta_agent::prompt::{PromptContext, PromptSection, SkillsPromptMode};
use common::fixtures::make_skill;

/// Helper: build N skills with sequential IDs (skill_0, skill_1, ...).
fn make_n_skills(n: usize) -> Vec<atta_types::SkillDef> {
    (0..n).map(|i| make_skill(&format!("skill_{i}"))).collect()
}

// ---------------------------------------------------------------------------
// 1. Compact mode — markdown bullet format: **id** (vX.Y): desc
// ---------------------------------------------------------------------------
#[test]
fn test_compact_mode_markdown_format() {
    let ctx = PromptContext {
        skills: vec![make_skill("summarize")],
        skills_prompt_mode: SkillsPromptMode::Compact,
        ..Default::default()
    };

    let section = SkillsSection;
    let output = section.build(&ctx).unwrap();

    // Compact format: "- **summarize** (v1.0): summarize description"
    assert!(
        output.contains("**summarize** (v1.0): summarize description"),
        "Compact mode must use markdown bold + version + description. Output:\n{output}"
    );
    // Must NOT contain XML tags
    assert!(
        !output.contains("<available_skills>"),
        "Compact mode must not contain XML tags"
    );
    assert!(
        !output.contains("<skill"),
        "Compact mode must not contain <skill> elements"
    );
}

// ---------------------------------------------------------------------------
// 2. Full mode — XML format: <available_skills><skill id="..." version="...">
// ---------------------------------------------------------------------------
#[test]
fn test_full_mode_xml_format() {
    let ctx = PromptContext {
        skills: vec![make_skill("research")],
        skills_prompt_mode: SkillsPromptMode::Full,
        ..Default::default()
    };

    let section = SkillsSection;
    let output = section.build(&ctx).unwrap();

    assert!(
        output.contains("<available_skills>"),
        "Full mode must start with <available_skills>"
    );
    assert!(
        output.contains("</available_skills>"),
        "Full mode must end with </available_skills>"
    );
    assert!(
        output.contains("<skill id=\"research\" version=\"1.0\">"),
        "Full mode must include skill id and version in XML attributes"
    );
    assert!(
        output.contains("</skill>"),
        "Full mode must close each <skill> tag"
    );
}

// ---------------------------------------------------------------------------
// 3. Full mode includes system_prompt
// ---------------------------------------------------------------------------
#[test]
fn test_full_mode_includes_system_prompt() {
    let ctx = PromptContext {
        skills: vec![make_skill("analyst")],
        skills_prompt_mode: SkillsPromptMode::Full,
        ..Default::default()
    };

    let section = SkillsSection;
    let output = section.build(&ctx).unwrap();

    // The make_skill fixture creates: system_prompt = "You are a {id} expert"
    assert!(
        output.contains("<system_prompt>You are a analyst expert</system_prompt>"),
        "Full mode must include <system_prompt> with the skill's system prompt. Output:\n{output}"
    );
}

// ---------------------------------------------------------------------------
// 4. Full mode includes tools list
// ---------------------------------------------------------------------------
#[test]
fn test_full_mode_includes_tools() {
    let ctx = PromptContext {
        skills: vec![make_skill("coder")],
        skills_prompt_mode: SkillsPromptMode::Full,
        ..Default::default()
    };

    let section = SkillsSection;
    let output = section.build(&ctx).unwrap();

    // make_skill fixtures have tools = ["web_fetch", "file_read"]
    assert!(
        output.contains("<tools>web_fetch, file_read</tools>"),
        "Full mode must include <tools> with comma-separated tool names. Output:\n{output}"
    );
}

// ---------------------------------------------------------------------------
// 5. Auto mode with 3 skills (<=5) uses Full format
// ---------------------------------------------------------------------------
#[test]
fn test_auto_3_skills_uses_full() {
    let ctx = PromptContext {
        skills: make_n_skills(3),
        skills_prompt_mode: SkillsPromptMode::Auto,
        ..Default::default()
    };

    let section = SkillsSection;
    let output = section.build(&ctx).unwrap();

    assert!(
        output.contains("<available_skills>"),
        "Auto mode with 3 skills (<=5) must use Full XML format"
    );
    assert!(
        output.contains("<skill id=\"skill_0\""),
        "All skills must appear in full mode"
    );
    assert!(
        output.contains("<skill id=\"skill_2\""),
        "All skills must appear in full mode"
    );
}

// ---------------------------------------------------------------------------
// 6. Auto mode with exactly 5 skills uses Full format
// ---------------------------------------------------------------------------
#[test]
fn test_auto_5_skills_uses_full() {
    let ctx = PromptContext {
        skills: make_n_skills(5),
        skills_prompt_mode: SkillsPromptMode::Auto,
        ..Default::default()
    };

    let section = SkillsSection;
    let output = section.build(&ctx).unwrap();

    assert!(
        output.contains("<available_skills>"),
        "Auto mode with exactly 5 skills must use Full XML format (threshold is <=5)"
    );
    // Verify all 5 skills present
    for i in 0..5 {
        assert!(
            output.contains(&format!("<skill id=\"skill_{i}\"")),
            "skill_{i} must appear in full mode"
        );
    }
}

// ---------------------------------------------------------------------------
// 7. Auto mode with 6 skills (>5) uses Compact format
// ---------------------------------------------------------------------------
#[test]
fn test_auto_6_skills_uses_compact() {
    let ctx = PromptContext {
        skills: make_n_skills(6),
        skills_prompt_mode: SkillsPromptMode::Auto,
        ..Default::default()
    };

    let section = SkillsSection;
    let output = section.build(&ctx).unwrap();

    assert!(
        !output.contains("<available_skills>"),
        "Auto mode with 6 skills (>5) must use Compact format, not XML"
    );
    assert!(
        output.contains("You can invoke 6 skills:"),
        "Compact mode must show count header"
    );
    // All 6 skills present in compact format
    for i in 0..6 {
        assert!(
            output.contains(&format!("**skill_{i}**")),
            "skill_{i} must appear in compact mode"
        );
    }
}

// ---------------------------------------------------------------------------
// 8. Auto mode with 8 skills uses Compact format
// ---------------------------------------------------------------------------
#[test]
fn test_auto_8_skills_uses_compact() {
    let ctx = PromptContext {
        skills: make_n_skills(8),
        skills_prompt_mode: SkillsPromptMode::Auto,
        ..Default::default()
    };

    let section = SkillsSection;
    let output = section.build(&ctx).unwrap();

    assert!(
        !output.contains("<available_skills>"),
        "Auto mode with 8 skills must use Compact format"
    );
    assert!(
        output.contains("You can invoke 8 skills:"),
        "Compact header must show correct count of 8"
    );
    // Spot-check a few
    assert!(output.contains("**skill_0**"));
    assert!(output.contains("**skill_7**"));
}

// ---------------------------------------------------------------------------
// 9. Forced Compact mode overrides Auto (even with few skills)
// ---------------------------------------------------------------------------
#[test]
fn test_forced_compact_overrides_auto() {
    // Only 2 skills, which Auto would render as Full, but we force Compact
    let ctx = PromptContext {
        skills: make_n_skills(2),
        skills_prompt_mode: SkillsPromptMode::Compact,
        ..Default::default()
    };

    let section = SkillsSection;
    let output = section.build(&ctx).unwrap();

    // Must be compact (no XML)
    assert!(
        !output.contains("<available_skills>"),
        "Forced Compact mode with 2 skills must not use XML"
    );
    assert!(
        output.contains("You can invoke 2 skills:"),
        "Forced Compact mode must show compact count header"
    );
    assert!(output.contains("**skill_0** (v1.0)"));
    assert!(output.contains("**skill_1** (v1.0)"));
}

// ---------------------------------------------------------------------------
// 10. Empty skills list returns None (section skipped)
// ---------------------------------------------------------------------------
#[test]
fn test_empty_skills_returns_none() {
    // Test all three modes with empty skills
    for mode in [
        SkillsPromptMode::Auto,
        SkillsPromptMode::Full,
        SkillsPromptMode::Compact,
    ] {
        let ctx = PromptContext {
            skills: vec![],
            skills_prompt_mode: mode,
            ..Default::default()
        };

        let section = SkillsSection;
        let result = section.build(&ctx);

        assert!(
            result.is_none(),
            "SkillsSection must return None when skills list is empty"
        );
    }
}
