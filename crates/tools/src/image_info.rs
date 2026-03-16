//! Image metadata extraction

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Extract metadata from an image file
pub struct ImageInfoTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for ImageInfoTool {
    fn name(&self) -> &str {
        "atta-image-info"
    }

    fn description(&self) -> &str {
        "Extract metadata from an image file (dimensions, format, color space)"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the image file"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'path' is required".into()))?;

        let metadata = tokio::fs::metadata(path)
            .await
            .map_err(|e| AttaError::Other(anyhow::anyhow!("failed to read file: {e}")))?;

        Ok(json!({
            "path": path,
            "size_bytes": metadata.len(),
            "message": "Basic file metadata returned. Full image parsing requires image crate."
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_image_info_name() {
        assert_eq!(ImageInfoTool.name(), "atta-image-info");
    }
}
