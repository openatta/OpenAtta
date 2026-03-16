//! Git operations tool

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Git operations (log, diff, commit, push, branch, status)
pub struct GitOpsTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for GitOpsTool {
    fn name(&self) -> &str {
        "atta-git-ops"
    }

    fn description(&self) -> &str {
        "Perform git operations: status, log, diff, commit, branch, push, pull"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Medium
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["status", "log", "diff", "commit", "branch", "push", "pull", "add", "checkout"],
                    "description": "Git operation to perform"
                },
                "args": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Additional arguments for the git command"
                },
                "working_dir": {
                    "type": "string",
                    "description": "Repository directory"
                }
            },
            "required": ["operation"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let operation = args["operation"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'operation' is required".into()))?;

        let mut cmd = tokio::process::Command::new("git");
        cmd.arg(operation);

        if let Some(extra_args) = args["args"].as_array() {
            for arg in extra_args {
                if let Some(s) = arg.as_str() {
                    cmd.arg(s);
                }
            }
        }

        if let Some(dir) = args["working_dir"].as_str() {
            cmd.current_dir(dir);
        }

        let output = cmd.output().await.map_err(|e| AttaError::Other(e.into()))?;

        Ok(json!({
            "exit_code": output.status.code(),
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_git_ops_name() {
        assert_eq!(GitOpsTool.name(), "atta-git-ops");
    }
}
