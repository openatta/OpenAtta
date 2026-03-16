//! TG2 — Identity section integration tests
//!
//! Tests identity file loading (all 8 files), partial loading, UTF-8 safe
//! truncation at 20KB, multibyte/emoji boundary safety, and edge cases
//! (empty files, no workspace, missing files).

mod common;

use atta_agent::prompt::sections::IdentitySection;
use atta_agent::prompt::{PromptContext, PromptSection};
use tempfile::TempDir;

/// The 8 identity files loaded by IdentitySection, in order.
const IDENTITY_FILES: &[&str] = &[
    "AGENTS.md",
    "SOUL.md",
    "USER.md",
    "TOOLS.md",
    "IDENTITY.md",
    "HEARTBEAT.md",
    "BOOTSTRAP.md",
    "MEMORY.md",
];

/// 20KB limit per file
const MAX_FILE_SIZE: usize = 20 * 1024;

/// Helper: create a workspace with all 8 identity files populated.
fn workspace_with_all_files() -> TempDir {
    let dir = TempDir::new().unwrap();
    for filename in IDENTITY_FILES {
        let content = format!("Content of {filename}");
        std::fs::write(dir.path().join(filename), &content).unwrap();
    }
    dir
}

/// Helper: build IdentitySection output for a given workspace directory.
fn build_identity(dir: &TempDir) -> String {
    let ctx = PromptContext {
        workspace_root: Some(dir.path().to_path_buf()),
        ..Default::default()
    };
    let section = IdentitySection;
    section
        .build(&ctx)
        .expect("IdentitySection always returns Some")
}

// ---------------------------------------------------------------------------
// 1. All 8 identity files loaded — each appears under its ### header
// ---------------------------------------------------------------------------
#[test]
fn test_all_8_files_loaded() {
    let dir = workspace_with_all_files();
    let output = build_identity(&dir);

    for filename in IDENTITY_FILES {
        assert!(
            output.contains(&format!("### {filename}")),
            "Missing header for {filename}"
        );
        assert!(
            output.contains(&format!("Content of {filename}")),
            "Missing content for {filename}"
        );
    }

    // Verify exactly 8 "###" headers from identity files
    let header_count = output.matches("### ").count();
    assert_eq!(
        header_count, 8,
        "Expected 8 identity file headers, got {header_count}"
    );
}

// ---------------------------------------------------------------------------
// 2. Partial files loaded — only existing files appear
// ---------------------------------------------------------------------------
#[test]
fn test_partial_files_loaded() {
    let dir = TempDir::new().unwrap();
    // Create only 3 of the 8 files
    std::fs::write(dir.path().join("AGENTS.md"), "Agent role definition").unwrap();
    std::fs::write(dir.path().join("SOUL.md"), "Soul values").unwrap();
    std::fs::write(dir.path().join("MEMORY.md"), "Past memories").unwrap();

    let output = build_identity(&dir);

    // Present files
    assert!(output.contains("### AGENTS.md"));
    assert!(output.contains("Agent role definition"));
    assert!(output.contains("### SOUL.md"));
    assert!(output.contains("Soul values"));
    assert!(output.contains("### MEMORY.md"));
    assert!(output.contains("Past memories"));

    // Absent files
    assert!(!output.contains("### USER.md"));
    assert!(!output.contains("### TOOLS.md"));
    assert!(!output.contains("### IDENTITY.md"));
    assert!(!output.contains("### HEARTBEAT.md"));
    assert!(!output.contains("### BOOTSTRAP.md"));

    // Exactly 3 headers
    let header_count = output.matches("### ").count();
    assert_eq!(header_count, 3);
}

// ---------------------------------------------------------------------------
// 3. Truncation at 20KB — large files get truncated with marker
// ---------------------------------------------------------------------------
#[test]
fn test_truncation_at_20kb() {
    let dir = TempDir::new().unwrap();

    // Create a file well over 20KB (30KB of 'a')
    let large_content = "a".repeat(30 * 1024);
    std::fs::write(dir.path().join("AGENTS.md"), &large_content).unwrap();

    let output = build_identity(&dir);

    assert!(
        output.contains("[truncated at 20KB]"),
        "Large file must show truncation marker"
    );
    // The content part before the marker should not exceed 20KB
    let agents_section_start = output.find("### AGENTS.md").unwrap();
    let after_header = &output[agents_section_start..];
    // The entire agents section (including header) should be well under 30KB
    // because the content was truncated at 20KB
    assert!(
        after_header.len() < 25 * 1024,
        "Truncated section should be much smaller than the original 30KB"
    );
}

// ---------------------------------------------------------------------------
// 4. UTF-8 multibyte truncation safety — Chinese characters at boundary
// ---------------------------------------------------------------------------
#[test]
fn test_utf8_multibyte_truncation_safe() {
    let dir = TempDir::new().unwrap();

    // Each Chinese character is 3 bytes. Create a string > 20KB of Chinese text.
    // 20 * 1024 / 3 ~ 6827 chars. We'll use 7500 chars to exceed the limit.
    let chinese_content: String = "\u{4F60}".repeat(7500); // '你' = 3 bytes
    assert!(chinese_content.len() > MAX_FILE_SIZE);

    std::fs::write(dir.path().join("SOUL.md"), &chinese_content).unwrap();

    let output = build_identity(&dir);

    // Must not panic, and must produce valid UTF-8 (if it didn't, this wouldn't compile/run)
    assert!(output.contains("[truncated at 20KB]"));

    // Extract the truncated content between "### SOUL.md\n" and "\n... [truncated"
    let soul_start = output.find("### SOUL.md\n").unwrap() + "### SOUL.md\n".len();
    let truncation_marker = output.find("\n... [truncated at 20KB]").unwrap();
    let truncated_text = &output[soul_start..truncation_marker];

    // Verify the truncated text is valid UTF-8 and every char is our Chinese character
    for ch in truncated_text.chars() {
        assert_eq!(
            ch, '\u{4F60}',
            "Truncation should not produce partial chars"
        );
    }
}

// ---------------------------------------------------------------------------
// 5. Emoji (4-byte) truncation safety — no panic, no invalid UTF-8
// ---------------------------------------------------------------------------
#[test]
fn test_emoji_truncation_safe() {
    let dir = TempDir::new().unwrap();

    // Each emoji is 4 bytes. 20*1024/4 = 5120 emojis for exactly 20KB.
    // Use 6000 to exceed the limit.
    let emoji_content: String = "\u{1F600}".repeat(6000); // grinning face = 4 bytes
    assert!(emoji_content.len() > MAX_FILE_SIZE);

    std::fs::write(dir.path().join("IDENTITY.md"), &emoji_content).unwrap();

    let output = build_identity(&dir);

    // Must produce valid UTF-8 and show truncation
    assert!(output.contains("[truncated at 20KB]"));

    // Extract just the emoji portion
    let id_start = output.find("### IDENTITY.md\n").unwrap() + "### IDENTITY.md\n".len();
    let marker_start = output[id_start..].find("\n... [truncated").unwrap();
    let emoji_text = &output[id_start..id_start + marker_start];

    // Every character should be the full emoji, not a partial sequence
    for ch in emoji_text.chars() {
        assert_eq!(
            ch, '\u{1F600}',
            "Truncation at emoji boundary must not produce partial codepoints"
        );
    }
}

// ---------------------------------------------------------------------------
// 6. No truncation for small files
// ---------------------------------------------------------------------------
#[test]
fn test_no_truncation_small_file() {
    let dir = TempDir::new().unwrap();
    let small_content = "This is a small identity file.";
    std::fs::write(dir.path().join("AGENTS.md"), small_content).unwrap();

    let output = build_identity(&dir);

    assert!(output.contains(small_content));
    assert!(
        !output.contains("[truncated"),
        "Small files should not show truncation marker"
    );
}

// ---------------------------------------------------------------------------
// 7. Empty file handled gracefully
// ---------------------------------------------------------------------------
#[test]
fn test_empty_file_handled() {
    let dir = TempDir::new().unwrap();
    // Write an empty file
    std::fs::write(dir.path().join("TOOLS.md"), "").unwrap();
    // Also write a non-empty one to confirm it still works
    std::fs::write(dir.path().join("AGENTS.md"), "Agent content").unwrap();

    let output = build_identity(&dir);

    // The empty file should still appear under its header (even if content is empty)
    assert!(output.contains("### TOOLS.md"));
    // The non-empty file should have its content
    assert!(output.contains("### AGENTS.md"));
    assert!(output.contains("Agent content"));
}

// ---------------------------------------------------------------------------
// 8. No workspace root — only intro text, no file headers
// ---------------------------------------------------------------------------
#[test]
fn test_no_workspace_root_only_intro() {
    let ctx = PromptContext {
        workspace_root: None,
        ..Default::default()
    };
    let section = IdentitySection;
    let output = section.build(&ctx).unwrap();

    // Should have the intro text
    assert!(
        output.contains("AttaOS"),
        "Identity section must always include intro text"
    );
    // Should not have any file headers
    assert!(
        !output.contains("###"),
        "Without workspace root, no identity files should be loaded"
    );
}

// ---------------------------------------------------------------------------
// 9. Missing files do not appear as errors in prompt text
// ---------------------------------------------------------------------------
#[test]
fn test_missing_files_not_in_output() {
    let dir = TempDir::new().unwrap();
    // Create only AGENTS.md — the other 7 files are missing
    std::fs::write(dir.path().join("AGENTS.md"), "Agent info").unwrap();

    let output = build_identity(&dir);

    // The output should NOT contain error messages, stack traces, or "not found" text
    assert!(!output.contains("not found"));
    assert!(!output.contains("Error"));
    assert!(!output.contains("error"));
    assert!(!output.contains("No such file"));

    // Only AGENTS.md header should be present
    assert!(output.contains("### AGENTS.md"));
    assert!(!output.contains("### SOUL.md"));
    assert!(!output.contains("### USER.md"));
}

// ---------------------------------------------------------------------------
// 10. Each file appears under "### FILENAME.md" header
// ---------------------------------------------------------------------------
#[test]
fn test_file_content_under_headers() {
    let dir = TempDir::new().unwrap();

    // Create a few files with distinctive content
    std::fs::write(dir.path().join("AGENTS.md"), "UNIQUE_AGENTS_MARKER").unwrap();
    std::fs::write(dir.path().join("SOUL.md"), "UNIQUE_SOUL_MARKER").unwrap();
    std::fs::write(dir.path().join("BOOTSTRAP.md"), "UNIQUE_BOOTSTRAP_MARKER").unwrap();

    let output = build_identity(&dir);

    // Check that content follows its header (header then content on next line)
    assert!(
        output.contains("### AGENTS.md\nUNIQUE_AGENTS_MARKER"),
        "AGENTS.md content must follow immediately after its header"
    );
    assert!(
        output.contains("### SOUL.md\nUNIQUE_SOUL_MARKER"),
        "SOUL.md content must follow immediately after its header"
    );
    assert!(
        output.contains("### BOOTSTRAP.md\nUNIQUE_BOOTSTRAP_MARKER"),
        "BOOTSTRAP.md content must follow immediately after its header"
    );
}
