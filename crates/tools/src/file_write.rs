//! File write tool

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Write content to a file
pub struct FileWriteTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for FileWriteTool {
    fn name(&self) -> &str {
        "atta-file-write"
    }

    fn description(&self) -> &str {
        "Write content to a file (creates or overwrites)"
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
                    "description": "Absolute path to the file"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write"
                },
                "append": {
                    "type": "boolean",
                    "description": "Append instead of overwrite",
                    "default": false
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'path' is required".into()))?;
        let content = args["content"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'content' is required".into()))?;
        let append = args["append"].as_bool().unwrap_or(false);

        // Ensure parent directory exists
        if let Some(parent) = std::path::Path::new(path).parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| AttaError::Other(e.into()))?;
        }

        if append {
            use tokio::io::AsyncWriteExt;
            let mut file = tokio::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(path)
                .await
                .map_err(|e| AttaError::Other(e.into()))?;
            file.write_all(content.as_bytes())
                .await
                .map_err(|e| AttaError::Other(e.into()))?;
            file.flush().await.map_err(|e| AttaError::Other(e.into()))?;
        } else {
            tokio::fs::write(path, content)
                .await
                .map_err(|e| AttaError::Other(e.into()))?;
        }

        Ok(json!({
            "path": path,
            "bytes_written": content.len(),
            "append": append,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_file_write_name() {
        assert_eq!(FileWriteTool.name(), "atta-file-write");
    }

    #[tokio::test]
    async fn test_write_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("new_file.txt");

        let result = FileWriteTool
            .execute(json!({
                "path": file_path.to_str().unwrap(),
                "content": "hello world"
            }))
            .await
            .unwrap();

        assert_eq!(result["bytes_written"], 11);
        assert_eq!(result["append"], false);
        assert_eq!(result["path"], file_path.to_str().unwrap());

        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn test_write_overwrites_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("overwrite.txt");
        std::fs::write(&file_path, "old content").unwrap();

        FileWriteTool
            .execute(json!({
                "path": file_path.to_str().unwrap(),
                "content": "new content"
            }))
            .await
            .unwrap();

        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "new content");
    }

    #[tokio::test]
    async fn test_write_append_mode() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("append.txt");
        std::fs::write(&file_path, "first ").unwrap();

        FileWriteTool
            .execute(json!({
                "path": file_path.to_str().unwrap(),
                "content": "second",
                "append": true
            }))
            .await
            .unwrap();

        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "first second");
    }

    #[tokio::test]
    async fn test_write_creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("sub").join("dir").join("deep.txt");

        FileWriteTool
            .execute(json!({
                "path": file_path.to_str().unwrap(),
                "content": "deep content"
            }))
            .await
            .unwrap();

        assert!(file_path.exists());
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "deep content");
    }

    #[tokio::test]
    async fn test_write_missing_path_returns_validation_error() {
        let result = FileWriteTool
            .execute(json!({ "content": "some text" }))
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AttaError::Validation(_)));
    }

    #[tokio::test]
    async fn test_write_missing_content_returns_validation_error() {
        let result = FileWriteTool
            .execute(json!({ "path": "/tmp/atta_test_whatever.txt" }))
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AttaError::Validation(_)));
    }

    #[tokio::test]
    async fn test_write_append_creates_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("new_append.txt");

        FileWriteTool
            .execute(json!({
                "path": file_path.to_str().unwrap(),
                "content": "appended",
                "append": true
            }))
            .await
            .unwrap();

        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "appended");
    }
}
