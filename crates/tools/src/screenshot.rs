//! Screenshot tool

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Take a screenshot
pub struct ScreenshotTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for ScreenshotTool {
    fn name(&self) -> &str {
        "atta-screenshot"
    }

    fn description(&self) -> &str {
        "Take a screenshot of the desktop or a specific region"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "output_path": {
                    "type": "string",
                    "description": "Path to save the screenshot (default: /tmp/screenshot.png)"
                },
                "region": {
                    "type": "object",
                    "description": "Capture region (x, y, width, height)",
                    "properties": {
                        "x": {"type": "integer"},
                        "y": {"type": "integer"},
                        "width": {"type": "integer"},
                        "height": {"type": "integer"}
                    }
                }
            }
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let output_path = args["output_path"]
            .as_str()
            .unwrap_or("/tmp/atta-screenshot.png");

        // macOS: use screencapture, Linux: use scrot or import
        let output = if cfg!(target_os = "macos") {
            tokio::process::Command::new("screencapture")
                .args(["-x", output_path])
                .output()
                .await
                .map_err(|e| AttaError::Other(e.into()))?
        } else {
            tokio::process::Command::new("scrot")
                .arg(output_path)
                .output()
                .await
                .map_err(|e| AttaError::Other(e.into()))?
        };

        if output.status.success() {
            Ok(json!({
                "path": output_path,
                "success": true,
            }))
        } else {
            Err(AttaError::Other(anyhow::anyhow!(
                "screenshot failed: {}",
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
    fn test_screenshot_name() {
        assert_eq!(ScreenshotTool.name(), "atta-screenshot");
    }
}
