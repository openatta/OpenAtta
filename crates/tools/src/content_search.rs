//! Content search tool (full-text search)

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Search file contents with regex or literal patterns
pub struct ContentSearchTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for ContentSearchTool {
    fn name(&self) -> &str {
        "atta-content-search"
    }

    fn description(&self) -> &str {
        "Search file contents for a pattern using ripgrep (rg) or grep"
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
                    "description": "Search pattern (regex)"
                },
                "path": {
                    "type": "string",
                    "description": "Directory or file to search in"
                },
                "glob": {
                    "type": "string",
                    "description": "File glob filter (e.g., '*.rs')"
                },
                "case_insensitive": {
                    "type": "boolean",
                    "default": false
                },
                "limit": {
                    "type": "integer",
                    "description": "Max number of matches",
                    "default": 50
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let pattern = args["pattern"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'pattern' is required".into()))?;
        let path = args["path"].as_str().unwrap_or(".");
        let case_insensitive = args["case_insensitive"].as_bool().unwrap_or(false);
        let limit = args["limit"].as_u64().unwrap_or(50);

        // Try rg first, fall back to grep
        let mut cmd = if which_exists("rg") {
            let mut c = tokio::process::Command::new("rg");
            c.args(["--json", "-m", &limit.to_string()]);
            if case_insensitive {
                c.arg("-i");
            }
            if let Some(g) = args["glob"].as_str() {
                c.args(["--glob", g]);
            }
            c.arg(pattern);
            c.arg(path);
            c
        } else {
            let mut c = tokio::process::Command::new("grep");
            c.args(["-rn", "--include"]);
            let glob_pattern = args["glob"].as_str().unwrap_or("*");
            c.arg(glob_pattern);
            if case_insensitive {
                c.arg("-i");
            }
            c.arg(pattern);
            c.arg(path);
            c
        };

        let output = cmd.output().await.map_err(|e| AttaError::Other(e.into()))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().take(limit as usize).collect();

        Ok(json!({
            "matches": lines,
            "count": lines.len(),
            "tool": if which_exists("rg") { "ripgrep" } else { "grep" },
        }))
    }
}

fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_content_search_name() {
        assert_eq!(ContentSearchTool.name(), "atta-content-search");
    }

    #[tokio::test]
    async fn test_search_finds_matching_content() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("haystack.txt"), "needle in a haystack\n").unwrap();
        std::fs::write(dir.path().join("other.txt"), "nothing here\n").unwrap();

        let result = ContentSearchTool
            .execute(json!({
                "pattern": "needle",
                "path": dir.path().to_str().unwrap()
            }))
            .await
            .unwrap();

        let count = result["count"].as_u64().unwrap();
        assert!(count >= 1, "expected at least one match, got {count}");

        let matches = result["matches"].as_array().unwrap();
        let joined = matches
            .iter()
            .map(|m| m.as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("needle"),
            "expected 'needle' in matches output: {joined}"
        );
    }

    #[tokio::test]
    async fn test_search_no_matches_returns_no_content_hits() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("file.txt"), "just some text\n").unwrap();

        let result = ContentSearchTool
            .execute(json!({
                "pattern": "zzz_nonexistent_zzz",
                "path": dir.path().to_str().unwrap()
            }))
            .await
            .unwrap();

        // When using rg --json, the output may include summary lines even
        // with no content matches, so verify none of the output lines contain
        // the actual search pattern.
        let matches = result["matches"].as_array().unwrap();
        let joined = matches
            .iter()
            .map(|m| m.as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !joined.contains("zzz_nonexistent_zzz"),
            "did not expect the search pattern in output: {joined}"
        );
    }

    #[tokio::test]
    async fn test_search_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("case.txt"), "Hello WORLD\n").unwrap();

        let result = ContentSearchTool
            .execute(json!({
                "pattern": "hello",
                "path": dir.path().to_str().unwrap(),
                "case_insensitive": true
            }))
            .await
            .unwrap();

        let count = result["count"].as_u64().unwrap();
        assert!(count >= 1, "expected case-insensitive match, got {count}");
    }

    #[tokio::test]
    async fn test_search_missing_pattern_returns_validation_error() {
        let result = ContentSearchTool.execute(json!({})).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AttaError::Validation(_)));
    }

    #[tokio::test]
    async fn test_search_reports_tool_used() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("t.txt"), "data\n").unwrap();

        let result = ContentSearchTool
            .execute(json!({
                "pattern": "data",
                "path": dir.path().to_str().unwrap()
            }))
            .await
            .unwrap();

        let tool = result["tool"].as_str().unwrap();
        assert!(
            tool == "ripgrep" || tool == "grep",
            "unexpected tool: {tool}"
        );
    }
}
