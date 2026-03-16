//! Chromium-based browser backend using headless_chrome
//!
//! Provides a real browser automation backend powered by headless Chrome.
//! Feature-gated under "browser-chromium" to avoid pulling in heavy dependencies.
//!
//! Requires Chrome/Chromium to be installed on the system.

use std::sync::Arc;

use atta_types::AttaError;
use headless_chrome::{Browser, LaunchOptions, Tab};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::browser::BrowserBackend;

/// Browser backend using headless_chrome
pub struct ChromiumBackend {
    /// Shared browser instance
    browser: Browser,
    /// Current active tab
    tab: Arc<Mutex<Arc<Tab>>>,
}

impl ChromiumBackend {
    /// Create a new ChromiumBackend.
    ///
    /// Launches a headless Chrome instance. Requires Chrome/Chromium on PATH.
    pub fn new() -> Result<Self, AttaError> {
        let browser = Browser::new(
            LaunchOptions::default_builder()
                .headless(true)
                .build()
                .map_err(|e| AttaError::Validation(format!("browser launch options: {}", e)))?,
        )
        .map_err(|e| {
            AttaError::SecurityViolation(format!(
                "failed to launch headless Chrome: {}. Is Chrome/Chromium installed?",
                e
            ))
        })?;

        let tab = browser.new_tab().map_err(|e| {
            AttaError::SecurityViolation(format!("failed to create browser tab: {}", e))
        })?;

        Ok(Self {
            browser,
            tab: Arc::new(Mutex::new(tab)),
        })
    }
}

#[async_trait::async_trait]
impl BrowserBackend for ChromiumBackend {
    async fn navigate(&self, url: &str) -> Result<Value, AttaError> {
        let tab = self.tab.lock().await;
        let url = url.to_string();

        // headless_chrome is synchronous, run in blocking thread
        let tab_clone = Arc::clone(&tab);
        let result = tokio::task::spawn_blocking(move || {
            tab_clone
                .navigate_to(&url)
                .map_err(|e| AttaError::Validation(format!("navigation failed: {}", e)))?;
            tab_clone
                .wait_until_navigated()
                .map_err(|e| AttaError::Validation(format!("wait failed: {}", e)))?;
            let title = tab_clone
                .get_title()
                .unwrap_or_else(|_| "unknown".to_string());
            let current_url = tab_clone.get_url();
            Ok::<Value, AttaError>(json!({
                "status": "navigated",
                "url": current_url,
                "title": title,
            }))
        })
        .await
        .map_err(|e| AttaError::Validation(format!("spawn_blocking failed: {}", e)))??;

        Ok(result)
    }

    async fn click(&self, selector: &str) -> Result<Value, AttaError> {
        let tab = self.tab.lock().await;
        let selector = selector.to_string();

        let tab_clone = Arc::clone(&tab);
        tokio::task::spawn_blocking(move || {
            tab_clone
                .find_element(&selector)
                .map_err(|e| {
                    AttaError::Validation(format!("element not found '{}': {}", selector, e))
                })?
                .click()
                .map_err(|e| AttaError::Validation(format!("click failed: {}", e)))?;
            Ok::<Value, AttaError>(json!({
                "status": "clicked",
                "selector": selector,
            }))
        })
        .await
        .map_err(|e| AttaError::Validation(format!("spawn_blocking failed: {}", e)))?
    }

    async fn type_text(&self, selector: &str, text: &str) -> Result<Value, AttaError> {
        let tab = self.tab.lock().await;
        let selector = selector.to_string();
        let text = text.to_string();

        let tab_clone = Arc::clone(&tab);
        tokio::task::spawn_blocking(move || {
            tab_clone
                .find_element(&selector)
                .map_err(|e| {
                    AttaError::Validation(format!("element not found '{}': {}", selector, e))
                })?
                .type_into(&text)
                .map_err(|e| AttaError::Validation(format!("type failed: {}", e)))?;
            Ok::<Value, AttaError>(json!({
                "status": "typed",
                "selector": selector,
                "text_length": text.len(),
            }))
        })
        .await
        .map_err(|e| AttaError::Validation(format!("spawn_blocking failed: {}", e)))?
    }

    async fn extract(&self, selector: Option<&str>) -> Result<Value, AttaError> {
        let tab = self.tab.lock().await;
        let selector = selector.map(|s| s.to_string());

        let tab_clone = Arc::clone(&tab);
        tokio::task::spawn_blocking(move || {
            let content = match &selector {
                Some(sel) => {
                    let element = tab_clone.find_element(sel).map_err(|e| {
                        AttaError::Validation(format!("element not found '{}': {}", sel, e))
                    })?;
                    element
                        .get_inner_text()
                        .map_err(|e| AttaError::Validation(format!("get text failed: {}", e)))?
                }
                None => {
                    // Get full page content
                    tab_clone
                        .find_element("body")
                        .map_err(|e| AttaError::Validation(format!("body not found: {}", e)))?
                        .get_inner_text()
                        .map_err(|e| {
                            AttaError::Validation(format!("get body text failed: {}", e))
                        })?
                }
            };

            // Truncate to avoid sending huge content
            let truncated = if content.len() > 50_000 {
                format!("{}...[truncated at 50000 chars]", &content[..50_000])
            } else {
                content.clone()
            };

            Ok::<Value, AttaError>(json!({
                "status": "extracted",
                "selector": selector,
                "content": truncated,
                "content_length": content.len(),
            }))
        })
        .await
        .map_err(|e| AttaError::Validation(format!("spawn_blocking failed: {}", e)))?
    }

    async fn screenshot(&self) -> Result<Value, AttaError> {
        let tab = self.tab.lock().await;

        let tab_clone = Arc::clone(&tab);
        tokio::task::spawn_blocking(move || {
            let png_data = tab_clone
                .capture_screenshot(headless_chrome::protocol::cdp::Page::CaptureScreenshot {
                    format: Some(
                        headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption::Png,
                    ),
                    quality: None,
                    clip: None,
                    from_surface: Some(true),
                    capture_beyond_viewport: Some(false),
                })
                .map_err(|e| AttaError::Validation(format!("screenshot failed: {}", e)))?;

            use base64::Engine;
            let base64 = base64::engine::general_purpose::STANDARD.encode(&png_data);

            Ok::<Value, AttaError>(json!({
                "status": "screenshot",
                "format": "png",
                "data_base64": base64,
                "size_bytes": png_data.len(),
            }))
        })
        .await
        .map_err(|e| AttaError::Validation(format!("spawn_blocking failed: {}", e)))?
    }

    async fn close(&self) -> Result<(), AttaError> {
        // Browser is dropped when ChromiumBackend is dropped
        // Close the current tab explicitly
        let tab = self.tab.lock().await;
        let tab_clone = Arc::clone(&tab);
        let _ = tokio::task::spawn_blocking(move || {
            tab_clone.close(true).ok();
        })
        .await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // Tests require Chrome installed — run manually with:
    // cargo test -p atta-tools --features browser-chromium -- --ignored

    #[test]
    #[ignore]
    fn test_chromium_backend_creates() {
        use super::*;
        let backend = ChromiumBackend::new();
        assert!(
            backend.is_ok(),
            "Chrome/Chromium must be installed for this test"
        );
    }
}
