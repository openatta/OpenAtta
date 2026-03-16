//! Browser automation tool with pluggable backend
//!
//! Provides a trait-based architecture for browser automation.
//! Default uses `StubBrowserBackend` which returns not-implemented.
//! Real backends (e.g., chromiumoxide, playwright) can be plugged in.

use std::sync::Arc;

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Backend trait for browser automation
#[async_trait::async_trait]
pub trait BrowserBackend: Send + Sync + 'static {
    /// Navigate to a URL
    async fn navigate(&self, url: &str) -> Result<Value, AttaError>;
    /// Click an element by CSS selector
    async fn click(&self, selector: &str) -> Result<Value, AttaError>;
    /// Type text into an element by CSS selector
    async fn type_text(&self, selector: &str, text: &str) -> Result<Value, AttaError>;
    /// Extract content from page, optionally filtered by selector
    async fn extract(&self, selector: Option<&str>) -> Result<Value, AttaError>;
    /// Take a screenshot
    async fn screenshot(&self) -> Result<Value, AttaError>;
    /// Close the browser session
    async fn close(&self) -> Result<(), AttaError>;
}

/// Stub backend that returns not-implemented for all actions
pub struct StubBrowserBackend;

#[async_trait::async_trait]
impl BrowserBackend for StubBrowserBackend {
    async fn navigate(&self, url: &str) -> Result<Value, AttaError> {
        Ok(json!({
            "status": "not_implemented",
            "action": "navigate",
            "url": url,
            "message": "browser backend not configured"
        }))
    }

    async fn click(&self, selector: &str) -> Result<Value, AttaError> {
        Ok(json!({
            "status": "not_implemented",
            "action": "click",
            "selector": selector,
            "message": "browser backend not configured"
        }))
    }

    async fn type_text(&self, selector: &str, text: &str) -> Result<Value, AttaError> {
        Ok(json!({
            "status": "not_implemented",
            "action": "type",
            "selector": selector,
            "text": text,
            "message": "browser backend not configured"
        }))
    }

    async fn extract(&self, selector: Option<&str>) -> Result<Value, AttaError> {
        Ok(json!({
            "status": "not_implemented",
            "action": "extract",
            "selector": selector,
            "message": "browser backend not configured"
        }))
    }

    async fn screenshot(&self) -> Result<Value, AttaError> {
        Ok(json!({
            "status": "not_implemented",
            "action": "screenshot",
            "message": "browser backend not configured"
        }))
    }

    async fn close(&self) -> Result<(), AttaError> {
        Ok(())
    }
}

/// Browser automation tool with pluggable backend
pub struct BrowserTool {
    backend: Arc<dyn BrowserBackend>,
}

impl BrowserTool {
    /// Create a BrowserTool with a custom backend
    pub fn with_backend(backend: Arc<dyn BrowserBackend>) -> Self {
        Self { backend }
    }
}

impl Default for BrowserTool {
    fn default() -> Self {
        Self {
            backend: Arc::new(StubBrowserBackend),
        }
    }
}

#[async_trait::async_trait]
impl atta_types::NativeTool for BrowserTool {
    fn name(&self) -> &str {
        "atta-browser"
    }

    fn description(&self) -> &str {
        "Automate browser actions: navigate, click, type, extract content, screenshot"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Medium
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["navigate", "click", "type", "extract", "screenshot", "close"],
                    "description": "Browser action to perform"
                },
                "url": {
                    "type": "string",
                    "description": "URL to navigate to (required for 'navigate')"
                },
                "selector": {
                    "type": "string",
                    "description": "CSS selector for element interaction (required for 'click' and 'type')"
                },
                "text": {
                    "type": "string",
                    "description": "Text to type (required for 'type')"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let action = args["action"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'action' is required".into()))?;

        match action {
            "navigate" => {
                let url = args["url"].as_str().ok_or_else(|| {
                    AttaError::Validation("'url' is required for navigate".into())
                })?;
                self.backend.navigate(url).await
            }
            "click" => {
                let selector = args["selector"].as_str().ok_or_else(|| {
                    AttaError::Validation("'selector' is required for click".into())
                })?;
                self.backend.click(selector).await
            }
            "type" => {
                let selector = args["selector"].as_str().ok_or_else(|| {
                    AttaError::Validation("'selector' is required for type".into())
                })?;
                let text = args["text"]
                    .as_str()
                    .ok_or_else(|| AttaError::Validation("'text' is required for type".into()))?;
                self.backend.type_text(selector, text).await
            }
            "extract" => {
                let selector = args["selector"].as_str();
                self.backend.extract(selector).await
            }
            "screenshot" => self.backend.screenshot().await,
            "close" => {
                self.backend.close().await?;
                Ok(json!({"status": "closed"}))
            }
            _ => Err(AttaError::Validation(format!(
                "unknown browser action: '{}'",
                action
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_browser_name() {
        let tool = BrowserTool::default();
        assert_eq!(tool.name(), "atta-browser");
    }

    #[tokio::test]
    async fn test_stub_navigate() {
        let tool = BrowserTool::default();
        let result = tool
            .execute(json!({"action": "navigate", "url": "https://example.com"}))
            .await
            .unwrap();
        assert_eq!(result["status"], "not_implemented");
        assert_eq!(result["action"], "navigate");
    }

    #[tokio::test]
    async fn test_stub_click() {
        let tool = BrowserTool::default();
        let result = tool
            .execute(json!({"action": "click", "selector": "#btn"}))
            .await
            .unwrap();
        assert_eq!(result["status"], "not_implemented");
    }

    #[tokio::test]
    async fn test_stub_type() {
        let tool = BrowserTool::default();
        let result = tool
            .execute(json!({"action": "type", "selector": "#input", "text": "hello"}))
            .await
            .unwrap();
        assert_eq!(result["status"], "not_implemented");
    }

    #[tokio::test]
    async fn test_stub_extract() {
        let tool = BrowserTool::default();
        let result = tool.execute(json!({"action": "extract"})).await.unwrap();
        assert_eq!(result["status"], "not_implemented");
    }

    #[tokio::test]
    async fn test_stub_screenshot() {
        let tool = BrowserTool::default();
        let result = tool.execute(json!({"action": "screenshot"})).await.unwrap();
        assert_eq!(result["status"], "not_implemented");
    }

    #[tokio::test]
    async fn test_stub_close() {
        let tool = BrowserTool::default();
        let result = tool.execute(json!({"action": "close"})).await.unwrap();
        assert_eq!(result["status"], "closed");
    }

    #[tokio::test]
    async fn test_unknown_action() {
        let tool = BrowserTool::default();
        let result = tool.execute(json!({"action": "fly"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_missing_required_args() {
        let tool = BrowserTool::default();
        // navigate without url
        assert!(tool.execute(json!({"action": "navigate"})).await.is_err());
        // click without selector
        assert!(tool.execute(json!({"action": "click"})).await.is_err());
        // type without selector
        assert!(tool
            .execute(json!({"action": "type", "text": "hi"}))
            .await
            .is_err());
    }
}
