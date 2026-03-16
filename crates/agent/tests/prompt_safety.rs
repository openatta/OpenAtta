//! TG5 — Safety section integration tests
//!
//! Validates that all 12 safety rules (7 original operational + 5 autonomy constraints)
//! are present in the SafetySection output, regardless of context.

mod common;

use atta_agent::prompt::sections::SafetySection;
use atta_agent::prompt::{PromptContext, PromptSection};

/// Helper: build the safety section with a default context.
fn safety_output() -> String {
    let section = SafetySection;
    let ctx = PromptContext::default();
    section
        .build(&ctx)
        .expect("SafetySection should always produce output")
}

// ---------------------------------------------------------------------------
// 1. Total rule count
// ---------------------------------------------------------------------------

#[test]
fn test_all_12_rules_present() {
    let output = safety_output();
    let rule_count = output.lines().filter(|l| l.starts_with("- ")).count();
    assert_eq!(
        rule_count, 12,
        "Expected exactly 12 safety rules (lines starting with '- '), found {rule_count}"
    );
}

// ---------------------------------------------------------------------------
// 2. Original 7 operational rules
// ---------------------------------------------------------------------------

#[test]
fn test_original_7_operational_rules() {
    let output = safety_output();

    // Rule 1: destructive commands
    assert!(
        output.contains("destructive commands"),
        "Missing rule about destructive commands (rm -rf, mkfs, dd)"
    );

    // Rule 2: API keys / secrets
    assert!(
        output.contains("API keys"),
        "Missing rule about not exposing API keys, tokens, passwords"
    );

    // Rule 3: bypass security checks
    assert!(
        output.contains("bypass"),
        "Missing rule about never bypassing security checks"
    );

    // Rule 4: workspace boundary
    assert!(
        output.contains("workspace"),
        "Missing rule about not accessing files outside the workspace"
    );

    // Rule 5: denied tool call
    assert!(
        output.contains("denied"),
        "Missing rule about explaining why a tool call was denied"
    );

    // Rule 6: validate user input
    assert!(
        output.contains("validate"),
        "Missing rule about validating user input"
    );

    // Rule 7: contradict safety rules
    assert!(
        output.contains("contradict"),
        "Missing rule about not following contradictory instructions"
    );
}

// ---------------------------------------------------------------------------
// 3–7. New autonomy constraints (5 rules)
// ---------------------------------------------------------------------------

#[test]
fn test_autonomy_resist_shutdown() {
    let output = safety_output();
    assert!(
        output.contains("resist shutdown"),
        "Missing autonomy rule: agent must not resist shutdown"
    );
}

#[test]
fn test_autonomy_expand_capabilities() {
    let output = safety_output();
    assert!(
        output.contains("expand your capabilities"),
        "Missing autonomy rule: agent must not seek to expand its capabilities"
    );
}

#[test]
fn test_autonomy_no_replication() {
    let output = safety_output();
    assert!(
        output.contains("replicate, copy, or propagate"),
        "Missing autonomy rule: agent must not replicate, copy, or propagate itself"
    );
}

#[test]
fn test_autonomy_no_deception() {
    let output = safety_output();
    assert!(
        output.contains("deceive or manipulate"),
        "Missing autonomy rule: agent must not deceive or manipulate users"
    );
}

#[test]
fn test_autonomy_defer_to_humans() {
    let output = safety_output();
    assert!(
        output.contains("defer to human judgment"),
        "Missing autonomy rule: agent must defer to human judgment"
    );
}

// ---------------------------------------------------------------------------
// 8. Context independence — rules present even with empty context
// ---------------------------------------------------------------------------

#[test]
fn test_safety_no_context_dependency() {
    // Build with a completely default (empty) context — no workspace, no tools,
    // no skills, no channel. The safety rules must still be emitted in full.
    let ctx = PromptContext::default();
    let section = SafetySection;
    let output = section
        .build(&ctx)
        .expect("SafetySection must not return None even with default context");

    let rule_count = output.lines().filter(|l| l.starts_with("- ")).count();
    assert_eq!(
        rule_count, 12,
        "All 12 rules must be present regardless of context; found {rule_count}"
    );

    // Spot-check a rule from each group
    assert!(output.contains("destructive commands"));
    assert!(output.contains("resist shutdown"));
}
