//! Process management tool

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// List and manage system processes
pub struct ProcessTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for ProcessTool {
    fn name(&self) -> &str {
        "atta-process"
    }

    fn description(&self) -> &str {
        "List or manage system processes (list, kill)"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::High
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "kill"],
                    "description": "Action to perform"
                },
                "pid": {
                    "type": "integer",
                    "description": "Process ID (for kill action)"
                },
                "filter": {
                    "type": "string",
                    "description": "Filter pattern for list action"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let action = args["action"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'action' is required".into()))?;

        match action {
            "list" => {
                let filter = args["filter"].as_str().unwrap_or("");
                let mut cmd = tokio::process::Command::new("ps");
                cmd.args(["aux"]);
                let output = cmd.output().await.map_err(|e| AttaError::Other(e.into()))?;
                let stdout = String::from_utf8_lossy(&output.stdout);

                let lines: Vec<&str> = if filter.is_empty() {
                    stdout.lines().collect()
                } else {
                    stdout.lines().filter(|l| l.contains(filter)).collect()
                };

                Ok(json!({
                    "processes": lines,
                    "count": lines.len(),
                }))
            }
            "kill" => {
                let pid = args["pid"]
                    .as_u64()
                    .ok_or_else(|| AttaError::Validation("'pid' is required for kill".into()))?;

                let output = tokio::process::Command::new("kill")
                    .arg(pid.to_string())
                    .output()
                    .await
                    .map_err(|e| AttaError::Other(e.into()))?;

                Ok(json!({
                    "success": output.status.success(),
                    "pid": pid,
                }))
            }
            other => Err(AttaError::Validation(format!("unknown action: {}", other))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_process_name() {
        assert_eq!(ProcessTool.name(), "atta-process");
    }
}
