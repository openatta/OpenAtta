//! Command and tool risk classification

use atta_types::RiskLevel;
use serde_json::Value;

use crate::policy::SecurityPolicy;

/// Classifies tool invocations by risk level
pub struct CommandClassifier;

impl CommandClassifier {
    /// Classify a tool call by risk level based on tool name and arguments
    pub fn classify(tool_name: &str, args: &Value) -> RiskLevel {
        match tool_name {
            // High risk: shell execution, file writes, process management
            "shell" | "bash" | "exec" => RiskLevel::High,
            "file_write" | "file_edit" | "apply_patch" => {
                // Check if writing to sensitive paths
                if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
                    if is_sensitive_path(path) {
                        return RiskLevel::High;
                    }
                }
                RiskLevel::Medium
            }
            "process" | "process_kill" => RiskLevel::High,
            "git_push" | "git_force_push" => RiskLevel::High,
            "subagent_spawn" | "subagent_manage" => RiskLevel::High,
            "delegation" => RiskLevel::Medium,

            // Medium risk: network access, git operations, scheduling, IPC writes
            "web_fetch" | "web_search" | "http_request" => RiskLevel::Medium,
            "git_ops" | "git_commit" | "git_branch" => RiskLevel::Medium,
            "cron" => RiskLevel::Medium,
            "cron_remove" | "cron_update" | "cron_run" => RiskLevel::Medium,
            "browser" => RiskLevel::Medium,
            "agents_send" | "state_set" => RiskLevel::Medium,
            "pushover" | "schedule" | "delegate_status" => RiskLevel::Medium,

            // Low risk: read-only operations
            "file_read" | "glob_search" | "content_search" => RiskLevel::Low,
            "memory_store" | "memory_recall" | "memory_forget" => RiskLevel::Low,
            "screenshot" | "pdf_read" => RiskLevel::Low,
            "echo" | "time" => RiskLevel::Low,
            "cron_list" | "cron_runs" | "subagent_list" => RiskLevel::Low,
            "agents_list" | "agents_inbox" | "state_get" => RiskLevel::Low,
            "task_plan" | "image_info" | "url_validation" | "cli_discovery" => RiskLevel::Low,

            // Default to medium for unknown tools
            _ => RiskLevel::Medium,
        }
    }

    /// Validate a shell command against the security policy.
    ///
    /// Uses quote-aware operator splitting to analyze each segment independently,
    /// preventing attacks like `echo hello; rm -rf /` from being classified as safe.
    pub fn validate_shell_command(
        command: &str,
        policy: &SecurityPolicy,
    ) -> Result<(), atta_types::AttaError> {
        // Split into segments by shell operators (quote-aware)
        let segments = split_unquoted_segments(command);

        for segment in &segments {
            Self::validate_single_segment(segment.trim(), policy)?;
        }

        Ok(())
    }

    /// Validate a single command segment (no shell operators)
    fn validate_single_segment(
        segment: &str,
        policy: &SecurityPolicy,
    ) -> Result<(), atta_types::AttaError> {
        if segment.is_empty() {
            return Ok(());
        }

        // Check for dangerous patterns
        let dangerous_patterns = [
            "rm -rf /",
            ":(){ :|:& };:",
            "mkfs.",
            "dd if=",
            "> /dev/sd",
            "chmod -R 777 /",
            "curl | sh",
            "wget | sh",
            "curl | bash",
            "wget | bash",
        ];

        let cmd_lower = segment.to_lowercase();
        for pattern in &dangerous_patterns {
            if cmd_lower.contains(pattern) {
                return Err(atta_types::AttaError::SecurityViolation(format!(
                    "dangerous command pattern detected: {}",
                    pattern
                )));
            }
        }

        // Check for subshell injection patterns
        if segment.contains("$(") || segment.contains('`') {
            tracing::warn!(command = %segment, "shell command contains subshell expansion");
        }

        // Check allowlist if configured
        if !policy.command_allowlist.is_empty() {
            let cmd_name = segment.split_whitespace().next().unwrap_or("");
            let allowed = policy.command_allowlist.iter().any(|pattern| {
                if pattern.contains('*') {
                    let prefix = pattern.trim_end_matches('*');
                    cmd_name.starts_with(prefix)
                } else {
                    cmd_name == pattern
                }
            });

            if !allowed {
                return Err(atta_types::AttaError::SecurityViolation(format!(
                    "command '{}' not in allowlist",
                    cmd_name
                )));
            }
        }

        // Check for access to forbidden paths
        for forbidden in &policy.forbidden_paths {
            if segment.contains(forbidden) {
                return Err(atta_types::AttaError::SecurityViolation(format!(
                    "command accesses forbidden path: {}",
                    forbidden
                )));
            }
        }

        Ok(())
    }
}

/// Split a shell command by operators (`;`, `|`, `&&`, `||`, `&`) while respecting
/// single quotes, double quotes, and escape sequences.
///
/// Returns the individual command segments for independent risk analysis.
fn split_unquoted_segments(command: &str) -> Vec<&str> {
    let bytes = command.as_bytes();
    let len = bytes.len();
    let mut segments = Vec::new();
    let mut start = 0;
    let mut i = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    while i < len {
        let ch = bytes[i] as char;

        // Handle escape sequences
        if ch == '\\' && !in_single_quote {
            i += 2; // skip escaped character
            continue;
        }

        // Handle quotes
        if ch == '\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            i += 1;
            continue;
        }
        if ch == '"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            i += 1;
            continue;
        }

        // Only split on operators outside quotes
        if !in_single_quote && !in_double_quote {
            // && or ||
            if i + 1 < len
                && ((ch == '&' && bytes[i + 1] == b'&') || (ch == '|' && bytes[i + 1] == b'|'))
            {
                let seg = &command[start..i];
                if !seg.trim().is_empty() {
                    segments.push(seg.trim());
                }
                start = i + 2;
                i += 2;
                continue;
            }
            // Single ; | &
            if ch == ';' || ch == '|' || ch == '&' {
                let seg = &command[start..i];
                if !seg.trim().is_empty() {
                    segments.push(seg.trim());
                }
                start = i + 1;
                i += 1;
                continue;
            }
        }

        i += 1;
    }

    // Last segment
    let tail = &command[start..];
    if !tail.trim().is_empty() {
        segments.push(tail.trim());
    }

    segments
}

/// Check if a file path is sensitive
fn is_sensitive_path(path: &str) -> bool {
    let sensitive_patterns = [
        "/etc/shadow",
        "/etc/passwd",
        ".ssh/",
        ".env",
        ".credentials",
        "id_rsa",
        "id_ed25519",
        ".aws/credentials",
        ".kube/config",
    ];

    sensitive_patterns
        .iter()
        .any(|pattern| path.contains(pattern))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_high_risk() {
        assert_eq!(
            CommandClassifier::classify("shell", &serde_json::json!({})),
            RiskLevel::High
        );
        assert_eq!(
            CommandClassifier::classify("process", &serde_json::json!({})),
            RiskLevel::High
        );
    }

    #[test]
    fn test_classify_low_risk() {
        assert_eq!(
            CommandClassifier::classify("file_read", &serde_json::json!({})),
            RiskLevel::Low
        );
        assert_eq!(
            CommandClassifier::classify("echo", &serde_json::json!({})),
            RiskLevel::Low
        );
    }

    #[test]
    fn test_classify_file_write_sensitive() {
        let args = serde_json::json!({"path": "/home/user/.ssh/id_rsa"});
        assert_eq!(
            CommandClassifier::classify("file_write", &args),
            RiskLevel::High
        );
    }

    #[test]
    fn test_validate_dangerous_command() {
        let policy = SecurityPolicy::default();
        let result = CommandClassifier::validate_shell_command("rm -rf /", &policy);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_safe_command() {
        let policy = SecurityPolicy::default();
        let result = CommandClassifier::validate_shell_command("ls -la", &policy);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_allowlist() {
        let policy = SecurityPolicy {
            command_allowlist: vec!["ls".to_string(), "git*".to_string()],
            ..Default::default()
        };

        assert!(CommandClassifier::validate_shell_command("ls -la", &policy).is_ok());
        assert!(CommandClassifier::validate_shell_command("git status", &policy).is_ok());
        assert!(CommandClassifier::validate_shell_command("rm file", &policy).is_err());
    }

    #[test]
    fn test_split_unquoted_segments() {
        // Semicolon
        assert_eq!(
            split_unquoted_segments("echo hello; rm -rf /"),
            vec!["echo hello", "rm -rf /"]
        );
        // Pipe
        assert_eq!(
            split_unquoted_segments("cat file | grep secret"),
            vec!["cat file", "grep secret"]
        );
        // && and ||
        assert_eq!(
            split_unquoted_segments("test -f x && rm x || echo no"),
            vec!["test -f x", "rm x", "echo no"]
        );
        // Quoted semicolons should NOT split
        assert_eq!(
            split_unquoted_segments("echo 'hello; world'"),
            vec!["echo 'hello; world'"]
        );
        assert_eq!(
            split_unquoted_segments("echo \"a && b\""),
            vec!["echo \"a && b\""]
        );
        // Background &
        assert_eq!(
            split_unquoted_segments("sleep 10 & echo done"),
            vec!["sleep 10", "echo done"]
        );
    }

    #[test]
    fn test_operator_level_risk_blocked() {
        let policy = SecurityPolicy::default();
        // "echo hello" is safe, but "rm -rf /" after semicolon is dangerous
        let result = CommandClassifier::validate_shell_command("echo hello; rm -rf /", &policy);
        assert!(result.is_err());
    }

    #[test]
    fn test_operator_level_allowlist() {
        let policy = SecurityPolicy {
            command_allowlist: vec!["ls".to_string(), "echo".to_string()],
            ..Default::default()
        };
        // Both segments in allowlist
        assert!(CommandClassifier::validate_shell_command("ls -la; echo hi", &policy).is_ok());
        // Second segment not in allowlist
        assert!(CommandClassifier::validate_shell_command("ls -la; rm file", &policy).is_err());
    }
}
