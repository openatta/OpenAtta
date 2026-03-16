//! PDF text extraction tool

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Extract text from PDF files
pub struct PdfReadTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for PdfReadTool {
    fn name(&self) -> &str {
        "atta-pdf-read"
    }

    fn description(&self) -> &str {
        "Extract text content from a PDF file"
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
                    "description": "Path to the PDF file"
                },
                "pages": {
                    "type": "string",
                    "description": "Page range (e.g., '1-5', '3', '10-20')"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'path' is required".into()))?;

        // Use pdftotext (poppler-utils) for extraction
        let mut cmd = tokio::process::Command::new("pdftotext");
        cmd.args([path, "-"]);

        if let Some(pages) = args["pages"].as_str() {
            if let Some((start, end)) = pages.split_once('-') {
                cmd.args(["-f", start, "-l", end]);
            } else {
                cmd.args(["-f", pages, "-l", pages]);
            }
        }

        let output = cmd.output().await.map_err(|e| AttaError::Other(e.into()))?;

        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout).to_string();
            Ok(json!({
                "text": text,
                "length": text.len(),
            }))
        } else {
            Err(AttaError::Other(anyhow::anyhow!(
                "pdftotext failed: {}",
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
    fn test_pdf_read_name() {
        assert_eq!(PdfReadTool.name(), "atta-pdf-read");
    }
}
