//! Apply unified diff patch tool

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Apply a unified diff patch to a file
pub struct ApplyPatchTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for ApplyPatchTool {
    fn name(&self) -> &str {
        "atta-apply-patch"
    }

    fn description(&self) -> &str {
        "Apply a unified diff patch to a file"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Medium
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path to patch"
                },
                "patch": {
                    "type": "string",
                    "description": "Unified diff content"
                }
            },
            "required": ["path", "patch"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'path' is required".into()))?;
        let patch = args["patch"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'patch' is required".into()))?;

        // Write patch to temp file and apply with `patch` command
        let tmp_patch = format!("/tmp/atta-patch-{}.diff", uuid::Uuid::new_v4());
        tokio::fs::write(&tmp_patch, patch)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        let output = tokio::process::Command::new("patch")
            .args([path, &tmp_patch])
            .output()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        // Cleanup temp file
        let _ = tokio::fs::remove_file(&tmp_patch).await;

        if output.status.success() {
            Ok(json!({
                "success": true,
                "output": String::from_utf8_lossy(&output.stdout).to_string(),
            }))
        } else {
            Err(AttaError::Validation(format!(
                "patch failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_apply_patch_name() {
        assert_eq!(ApplyPatchTool.name(), "atta-apply-patch");
    }
}
