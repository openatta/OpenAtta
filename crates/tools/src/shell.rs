//! Shell command execution tool

use std::time::Duration;

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Shell command execution tool
pub struct ShellTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for ShellTool {
    fn name(&self) -> &str {
        "atta-shell"
    }

    fn description(&self) -> &str {
        "Execute a shell command and return its output"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::High
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to execute"
                },
                "working_dir": {
                    "type": "string",
                    "description": "Working directory for the command"
                },
                "timeout_seconds": {
                    "type": "integer",
                    "description": "Timeout in seconds (default: 30)",
                    "default": 30
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let cmd = args["command"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'command' is required".into()))?;

        let timeout = args["timeout_seconds"].as_u64().unwrap_or(30);

        let mut command = tokio::process::Command::new("sh");
        command.arg("-c").arg(cmd);

        if let Some(dir) = args["working_dir"].as_str() {
            command.current_dir(dir);
        }

        let output = tokio::time::timeout(Duration::from_secs(timeout), command.output())
            .await
            .map_err(|_| {
                AttaError::Runtime(atta_types::RuntimeError::Timeout(Duration::from_secs(
                    timeout,
                )))
            })?
            .map_err(|e| AttaError::Other(e.into()))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        // Truncate very long outputs
        let max_len = 100_000;
        let stdout_str = if stdout.len() > max_len {
            format!("{}...[truncated]", &stdout[..max_len])
        } else {
            stdout.to_string()
        };
        let stderr_str = if stderr.len() > max_len {
            format!("{}...[truncated]", &stderr[..max_len])
        } else {
            stderr.to_string()
        };

        Ok(json!({
            "exit_code": output.status.code(),
            "stdout": stdout_str,
            "stderr": stderr_str,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_shell_tool_name() {
        assert_eq!(ShellTool.name(), "atta-shell");
    }

    #[test]
    fn test_shell_tool_risk() {
        assert_eq!(ShellTool.risk_level(), RiskLevel::High);
    }

    #[tokio::test]
    async fn test_shell_execute() {
        let result = ShellTool
            .execute(json!({"command": "echo hello"}))
            .await
            .unwrap();
        assert_eq!(result["stdout"].as_str().unwrap().trim(), "hello");
        assert_eq!(result["exit_code"], 0);
    }

    #[tokio::test]
    async fn test_shell_exit_code_nonzero() {
        let result = ShellTool
            .execute(json!({"command": "exit 42"}))
            .await
            .unwrap();

        assert_eq!(result["exit_code"], 42);
    }

    #[tokio::test]
    async fn test_shell_captures_stderr() {
        let result = ShellTool
            .execute(json!({"command": "echo err_msg >&2"}))
            .await
            .unwrap();

        let stderr = result["stderr"].as_str().unwrap();
        assert!(stderr.contains("err_msg"));
    }

    #[tokio::test]
    async fn test_shell_working_dir() {
        let dir = tempfile::tempdir().unwrap();

        let result = ShellTool
            .execute(json!({
                "command": "pwd",
                "working_dir": dir.path().to_str().unwrap()
            }))
            .await
            .unwrap();

        let stdout = result["stdout"].as_str().unwrap().trim();
        // On macOS, /tmp -> /private/tmp, so canonicalize both for comparison
        let expected = std::fs::canonicalize(dir.path()).unwrap();
        let actual = std::fs::canonicalize(stdout).unwrap();
        assert_eq!(actual, expected);
    }

    #[tokio::test]
    async fn test_shell_timeout() {
        let result = ShellTool
            .execute(json!({
                "command": "sleep 60",
                "timeout_seconds": 1
            }))
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(
                err,
                AttaError::Runtime(atta_types::RuntimeError::Timeout(_))
            ),
            "expected timeout error, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn test_shell_missing_command_returns_validation_error() {
        let result = ShellTool.execute(json!({})).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AttaError::Validation(_)));
    }

    #[tokio::test]
    async fn test_shell_multiline_output() {
        let result = ShellTool
            .execute(json!({"command": "printf 'line1\nline2\nline3'"}))
            .await
            .unwrap();

        let stdout = result["stdout"].as_str().unwrap();
        assert!(stdout.contains("line1"));
        assert!(stdout.contains("line2"));
        assert!(stdout.contains("line3"));
    }
}
