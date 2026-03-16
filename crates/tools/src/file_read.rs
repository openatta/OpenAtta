//! File read tool

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Read file contents with optional offset/limit
pub struct FileReadTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for FileReadTool {
    fn name(&self) -> &str {
        "atta-file-read"
    }

    fn description(&self) -> &str {
        "Read the contents of a file, optionally with line offset and limit"
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
                    "description": "Absolute path to the file"
                },
                "offset": {
                    "type": "integer",
                    "description": "Line number to start reading from (0-indexed)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to read"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'path' is required".into()))?;

        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let offset = args["offset"].as_u64().unwrap_or(0) as usize;
        let limit = args["limit"].as_u64().map(|l| l as usize);

        let selected: Vec<&str> = lines
            .into_iter()
            .skip(offset)
            .take(limit.unwrap_or(usize::MAX))
            .collect();

        Ok(json!({
            "content": selected.join("\n"),
            "total_lines": total_lines,
            "offset": offset,
            "lines_returned": selected.len(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_file_read_name() {
        assert_eq!(FileReadTool.name(), "atta-file-read");
    }

    #[tokio::test]
    async fn test_read_entire_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("hello.txt");
        std::fs::write(&file_path, "line1\nline2\nline3\n").unwrap();

        let result = FileReadTool
            .execute(json!({ "path": file_path.to_str().unwrap() }))
            .await
            .unwrap();

        assert_eq!(result["total_lines"], 3);
        assert_eq!(result["offset"], 0);
        assert_eq!(result["lines_returned"], 3);
        let content = result["content"].as_str().unwrap();
        assert!(content.contains("line1"));
        assert!(content.contains("line2"));
        assert!(content.contains("line3"));
    }

    #[tokio::test]
    async fn test_read_with_offset() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("offset.txt");
        std::fs::write(&file_path, "aaa\nbbb\nccc\nddd\n").unwrap();

        let result = FileReadTool
            .execute(json!({ "path": file_path.to_str().unwrap(), "offset": 2 }))
            .await
            .unwrap();

        assert_eq!(result["total_lines"], 4);
        assert_eq!(result["offset"], 2);
        assert_eq!(result["lines_returned"], 2);
        let content = result["content"].as_str().unwrap();
        assert!(!content.contains("aaa"));
        assert!(!content.contains("bbb"));
        assert!(content.contains("ccc"));
        assert!(content.contains("ddd"));
    }

    #[tokio::test]
    async fn test_read_with_limit() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("limit.txt");
        std::fs::write(&file_path, "one\ntwo\nthree\nfour\nfive\n").unwrap();

        let result = FileReadTool
            .execute(json!({ "path": file_path.to_str().unwrap(), "limit": 2 }))
            .await
            .unwrap();

        assert_eq!(result["total_lines"], 5);
        assert_eq!(result["lines_returned"], 2);
        let content = result["content"].as_str().unwrap();
        assert!(content.contains("one"));
        assert!(content.contains("two"));
        assert!(!content.contains("three"));
    }

    #[tokio::test]
    async fn test_read_with_offset_and_limit() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("both.txt");
        std::fs::write(&file_path, "a\nb\nc\nd\ne\n").unwrap();

        let result = FileReadTool
            .execute(json!({ "path": file_path.to_str().unwrap(), "offset": 1, "limit": 2 }))
            .await
            .unwrap();

        assert_eq!(result["lines_returned"], 2);
        let content = result["content"].as_str().unwrap();
        assert!(content.contains("b"));
        assert!(content.contains("c"));
        assert!(!content.contains("a"));
        assert!(!content.contains("d"));
    }

    #[tokio::test]
    async fn test_read_nonexistent_file_returns_error() {
        let result = FileReadTool
            .execute(json!({ "path": "/tmp/atta_test_nonexistent_file_xyz.txt" }))
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_read_missing_path_param_returns_validation_error() {
        let result = FileReadTool.execute(json!({})).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AttaError::Validation(_)));
    }

    #[tokio::test]
    async fn test_read_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("empty.txt");
        std::fs::write(&file_path, "").unwrap();

        let result = FileReadTool
            .execute(json!({ "path": file_path.to_str().unwrap() }))
            .await
            .unwrap();

        assert_eq!(result["total_lines"], 0);
        assert_eq!(result["lines_returned"], 0);
    }
}
