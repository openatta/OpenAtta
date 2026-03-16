//! Glob pattern file search tool

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Search for files matching a glob pattern
pub struct GlobSearchTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for GlobSearchTool {
    fn name(&self) -> &str {
        "atta-glob-search"
    }

    fn description(&self) -> &str {
        "Search for files matching a glob pattern (e.g., '**/*.rs')"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern (e.g., '**/*.rs', 'src/**/*.ts')"
                },
                "path": {
                    "type": "string",
                    "description": "Base directory to search in (default: current dir)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results",
                    "default": 100
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let pattern = args["pattern"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'pattern' is required".into()))?;
        let base = args["path"].as_str().unwrap_or(".");
        let limit = args["limit"].as_u64().unwrap_or(100) as usize;

        let full_pattern = if pattern.starts_with('/') {
            pattern.to_string()
        } else {
            format!("{}/{}", base, pattern)
        };

        let mut matches = Vec::new();
        for entry in glob::glob(&full_pattern).map_err(|e| AttaError::Validation(e.to_string()))? {
            if matches.len() >= limit {
                break;
            }
            if let Ok(path) = entry {
                matches.push(path.to_string_lossy().to_string());
            }
        }

        Ok(json!({
            "matches": matches,
            "count": matches.len(),
            "pattern": full_pattern,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_glob_search_name() {
        assert_eq!(GlobSearchTool.name(), "atta-glob-search");
    }

    #[tokio::test]
    async fn test_glob_matches_files_by_extension() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "").unwrap();
        std::fs::write(dir.path().join("b.rs"), "").unwrap();
        std::fs::write(dir.path().join("c.txt"), "").unwrap();

        let result = GlobSearchTool
            .execute(json!({
                "pattern": "*.rs",
                "path": dir.path().to_str().unwrap()
            }))
            .await
            .unwrap();

        assert_eq!(result["count"], 2);
        let matches = result["matches"].as_array().unwrap();
        let paths: Vec<&str> = matches.iter().map(|m| m.as_str().unwrap()).collect();
        assert!(paths.iter().any(|p| p.ends_with("a.rs")));
        assert!(paths.iter().any(|p| p.ends_with("b.rs")));
    }

    #[tokio::test]
    async fn test_glob_recursive_pattern() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(dir.path().join("top.txt"), "").unwrap();
        std::fs::write(sub.join("deep.txt"), "").unwrap();

        let result = GlobSearchTool
            .execute(json!({
                "pattern": "**/*.txt",
                "path": dir.path().to_str().unwrap()
            }))
            .await
            .unwrap();

        assert_eq!(result["count"], 2);
        let matches = result["matches"].as_array().unwrap();
        let paths: Vec<&str> = matches.iter().map(|m| m.as_str().unwrap()).collect();
        assert!(paths.iter().any(|p| p.ends_with("top.txt")));
        assert!(paths.iter().any(|p| p.ends_with("deep.txt")));
    }

    #[tokio::test]
    async fn test_glob_no_matches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("file.txt"), "").unwrap();

        let result = GlobSearchTool
            .execute(json!({
                "pattern": "*.rs",
                "path": dir.path().to_str().unwrap()
            }))
            .await
            .unwrap();

        assert_eq!(result["count"], 0);
        assert!(result["matches"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_glob_with_limit() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..10 {
            std::fs::write(dir.path().join(format!("file_{}.log", i)), "").unwrap();
        }

        let result = GlobSearchTool
            .execute(json!({
                "pattern": "*.log",
                "path": dir.path().to_str().unwrap(),
                "limit": 3
            }))
            .await
            .unwrap();

        assert_eq!(result["count"], 3);
    }

    #[tokio::test]
    async fn test_glob_invalid_pattern_returns_error() {
        let result = GlobSearchTool
            .execute(json!({
                "pattern": "[invalid",
                "path": "/tmp"
            }))
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_glob_missing_pattern_returns_validation_error() {
        let result = GlobSearchTool.execute(json!({})).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AttaError::Validation(_)));
    }
}
