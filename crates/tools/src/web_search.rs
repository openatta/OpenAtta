//! Web search tool

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Search the web via a search API
pub struct WebSearchTool;

#[async_trait::async_trait]
impl atta_types::NativeTool for WebSearchTool {
    fn name(&self) -> &str {
        "atta-web-search"
    }

    fn description(&self) -> &str {
        "Search the web and return results"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Medium
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "num_results": {
                    "type": "integer",
                    "description": "Number of results to return",
                    "default": 10
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let _query = args["query"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'query' is required".into()))?;

        // Placeholder — requires search API key configuration
        Ok(json!({
            "status": "not_implemented",
            "message": "web search requires a search API key (e.g., SerpAPI, Brave Search)"
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_web_search_name() {
        assert_eq!(WebSearchTool.name(), "atta-web-search");
    }
}
