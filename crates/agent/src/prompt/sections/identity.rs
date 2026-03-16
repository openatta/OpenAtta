//! Identity section — loads identity/personality files from workspace

use super::super::section::{PromptContext, PromptSection};
use tracing::debug;

const MAX_FILE_SIZE: usize = 20 * 1024; // 20KB per file
const IDENTITY_FILES: &[&str] = &[
    "AGENTS.md",    // Agent role definitions
    "SOUL.md",      // Core personality/values
    "USER.md",      // User preferences
    "TOOLS.md",     // Tool usage preferences
    "IDENTITY.md",  // Name/codename/identity markers
    "HEARTBEAT.md", // Periodic reminders / cron self-start instructions
    "BOOTSTRAP.md", // Startup initialization instructions
    "MEMORY.md",    // Persistent memory snapshot
];

/// Truncate a string at a UTF-8 safe boundary within `max_bytes`,
/// appending a truncation marker.
fn utf8_safe_truncate(content: &str, max_bytes: usize) -> String {
    if content.len() <= max_bytes {
        return content.to_string();
    }
    // Find the last char boundary at or before max_bytes
    let truncated = match content
        .char_indices()
        .take_while(|(i, _)| *i <= max_bytes)
        .last()
    {
        Some((i, c)) => &content[..i + c.len_utf8()],
        None => "",
    };
    // Ensure we don't exceed max_bytes after the last char
    let truncated = if truncated.len() > max_bytes {
        match content
            .char_indices()
            .take_while(|(i, _)| *i < max_bytes)
            .last()
        {
            Some((i, c)) => &content[..i + c.len_utf8()],
            None => "",
        }
    } else {
        truncated
    };
    format!("{truncated}\n... [truncated at 20KB]")
}

/// Identity section — loads personality/role files from the workspace root
pub struct IdentitySection;

impl PromptSection for IdentitySection {
    fn name(&self) -> &str {
        "Identity"
    }

    fn priority(&self) -> u32 {
        10
    }

    fn build(&self, ctx: &PromptContext) -> Option<String> {
        let mut parts = vec![
            "You are AttaOS, an AI agent operating system. You execute tasks using available tools, \
             follow safety rules, and provide helpful responses."
                .to_string(),
        ];

        if let Some(ref root) = ctx.workspace_root {
            for filename in IDENTITY_FILES {
                let path = root.join(filename);
                match std::fs::read_to_string(&path) {
                    Ok(content) => {
                        let processed = utf8_safe_truncate(&content, MAX_FILE_SIZE);
                        parts.push(format!("### {filename}\n{processed}"));
                    }
                    Err(_) => {
                        debug!("(file {filename} not found)");
                    }
                }
            }
        }

        Some(parts.join("\n\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_workspace() -> TempDir {
        let dir = TempDir::new().unwrap();
        for filename in IDENTITY_FILES {
            std::fs::write(dir.path().join(filename), format!("Content of {filename}")).unwrap();
        }
        dir
    }

    #[test]
    fn test_loads_all_8_identity_files() {
        let dir = create_test_workspace();
        let ctx = PromptContext {
            workspace_root: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let section = IdentitySection;
        let output = section.build(&ctx).unwrap();

        for filename in IDENTITY_FILES {
            assert!(
                output.contains(&format!("### {filename}")),
                "Missing {filename}"
            );
            assert!(output.contains(&format!("Content of {filename}")));
        }
    }

    #[test]
    fn test_utf8_safe_truncation() {
        // ASCII string
        let s = "a".repeat(MAX_FILE_SIZE + 100);
        let result = utf8_safe_truncate(&s, MAX_FILE_SIZE);
        assert!(result.contains("[truncated at 20KB]"));
        // The content part (before marker) should be at most MAX_FILE_SIZE bytes
        let content_part = result.split("\n...").next().unwrap();
        assert!(content_part.len() <= MAX_FILE_SIZE);
    }

    #[test]
    fn test_utf8_safe_truncation_multibyte() {
        // Each '你' is 3 bytes. Build a string that exceeds 10 bytes.
        let s = "你好世界测试数据额外内容"; // well over 10 bytes
        let result = utf8_safe_truncate(s, 10);
        assert!(result.contains("[truncated at 20KB]"));
        // Should not panic or produce invalid UTF-8
        let _ = result.as_bytes();
    }

    #[test]
    fn test_no_truncation_for_small_content() {
        let s = "Hello, world!";
        let result = utf8_safe_truncate(s, MAX_FILE_SIZE);
        assert_eq!(result, s);
        assert!(!result.contains("[truncated"));
    }

    #[test]
    fn test_missing_files_logged_not_in_output() {
        let dir = TempDir::new().unwrap();
        // Only create one file
        std::fs::write(dir.path().join("AGENTS.md"), "Agent info").unwrap();

        let ctx = PromptContext {
            workspace_root: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let section = IdentitySection;
        let output = section.build(&ctx).unwrap();

        assert!(output.contains("### AGENTS.md"));
        // Missing files should NOT appear in the prompt output
        assert!(!output.contains("### SOUL.md"));
        assert!(!output.contains("not found"));
    }

    #[test]
    fn test_no_workspace_root() {
        let ctx = PromptContext::default();
        let section = IdentitySection;
        let output = section.build(&ctx).unwrap();
        assert!(output.contains("AttaOS"));
        // Should only have the intro, no file sections
        assert!(!output.contains("###"));
    }
}
