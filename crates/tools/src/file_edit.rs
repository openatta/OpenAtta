//! File edit tool (search and replace)

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Search and replace in files
pub struct FileEditTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for FileEditTool {
    fn name(&self) -> &str {
        "atta-file-edit"
    }

    fn description(&self) -> &str {
        "Edit a file by replacing a specific string with a new string"
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
                "old_string": {
                    "type": "string",
                    "description": "Exact string to search for"
                },
                "new_string": {
                    "type": "string",
                    "description": "Replacement string"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace all occurrences (default: false)",
                    "default": false
                }
            },
            "required": ["path", "old_string", "new_string"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'path' is required".into()))?;
        let old_string = args["old_string"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'old_string' is required".into()))?;
        let new_string = args["new_string"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'new_string' is required".into()))?;
        let replace_all = args["replace_all"].as_bool().unwrap_or(false);

        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        let occurrences = content.matches(old_string).count();
        if occurrences == 0 {
            return Err(AttaError::Validation(format!(
                "old_string not found in {}",
                path
            )));
        }

        if !replace_all && occurrences > 1 {
            return Err(AttaError::Validation(format!(
                "old_string found {} times in {} — use replace_all or provide more context",
                occurrences, path
            )));
        }

        let new_content = if replace_all {
            content.replace(old_string, new_string)
        } else {
            content.replacen(old_string, new_string, 1)
        };

        tokio::fs::write(path, &new_content)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        Ok(json!({
            "path": path,
            "replacements": if replace_all { occurrences } else { 1 },
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_file_edit_name() {
        assert_eq!(FileEditTool.name(), "atta-file-edit");
    }

    #[tokio::test]
    async fn test_edit_single_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("edit.txt");
        std::fs::write(&file_path, "Hello World").unwrap();

        let result = FileEditTool
            .execute(json!({
                "path": file_path.to_str().unwrap(),
                "old_string": "World",
                "new_string": "Rust"
            }))
            .await
            .unwrap();

        assert_eq!(result["replacements"], 1);

        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "Hello Rust");
    }

    #[tokio::test]
    async fn test_edit_replace_all() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("replace_all.txt");
        std::fs::write(&file_path, "foo bar foo baz foo").unwrap();

        let result = FileEditTool
            .execute(json!({
                "path": file_path.to_str().unwrap(),
                "old_string": "foo",
                "new_string": "qux",
                "replace_all": true
            }))
            .await
            .unwrap();

        assert_eq!(result["replacements"], 3);

        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "qux bar qux baz qux");
    }

    #[tokio::test]
    async fn test_edit_old_string_not_found_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("notfound.txt");
        std::fs::write(&file_path, "some content").unwrap();

        let result = FileEditTool
            .execute(json!({
                "path": file_path.to_str().unwrap(),
                "old_string": "nonexistent",
                "new_string": "replacement"
            }))
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AttaError::Validation(_)));
    }

    #[tokio::test]
    async fn test_edit_ambiguous_match_without_replace_all_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("ambiguous.txt");
        std::fs::write(&file_path, "aaa bbb aaa").unwrap();

        let result = FileEditTool
            .execute(json!({
                "path": file_path.to_str().unwrap(),
                "old_string": "aaa",
                "new_string": "ccc"
            }))
            .await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("2 times"));
    }

    #[tokio::test]
    async fn test_edit_multiline_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("multiline.txt");
        std::fs::write(&file_path, "line1\nold line\nline3\n").unwrap();

        FileEditTool
            .execute(json!({
                "path": file_path.to_str().unwrap(),
                "old_string": "old line",
                "new_string": "new line"
            }))
            .await
            .unwrap();

        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "line1\nnew line\nline3\n");
    }

    #[tokio::test]
    async fn test_edit_nonexistent_file_returns_error() {
        let result = FileEditTool
            .execute(json!({
                "path": "/tmp/atta_test_nonexistent_edit_xyz.txt",
                "old_string": "a",
                "new_string": "b"
            }))
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_edit_missing_params_returns_validation_error() {
        let result = FileEditTool.execute(json!({ "path": "/tmp/x.txt" })).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AttaError::Validation(_)));
    }
}
