//! CDP (Chrome DevTools Protocol) browser backend
//!
//! Communicates directly with any CDP-compatible browser via WebSocket.
//! No dependency on Node.js or the `headless_chrome` crate.
//!
//! Two entry points:
//! - [`CdpBackend::connect`] — attach to an already-running CDP endpoint
//! - [`CdpBackend::launch`]  — auto-launch headless Chrome and connect

use std::sync::atomic::{AtomicU64, Ordering};

use atta_types::AttaError;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::browser::BrowserBackend;

type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

/// Browser backend using the Chrome DevTools Protocol over WebSocket.
///
/// Works with any browser that exposes a CDP endpoint (Chrome, Chromium,
/// Edge, or a future fingerprint browser).
pub struct CdpBackend {
    ws: Mutex<WsStream>,
    next_id: AtomicU64,
    /// Child process handle — `Some` when we launched Chrome ourselves.
    child: Mutex<Option<tokio::process::Child>>,
}

impl CdpBackend {
    /// Connect to an existing CDP WebSocket endpoint.
    ///
    /// The URL should be a full `ws://` DevTools URL, e.g.:
    /// `ws://127.0.0.1:9222/devtools/page/XXXX`
    pub async fn connect(ws_url: &str) -> Result<Self, AttaError> {
        let (ws, _) = tokio_tungstenite::connect_async(ws_url)
            .await
            .map_err(|e| AttaError::Validation(format!("CDP WebSocket connect failed: {e}")))?;

        let backend = Self {
            ws: Mutex::new(ws),
            next_id: AtomicU64::new(1),
            child: Mutex::new(None),
        };

        backend.send_command("Page.enable", json!({})).await?;
        Ok(backend)
    }

    /// Launch headless Chrome and connect to it via CDP.
    ///
    /// Finds Chrome/Chromium on the system, launches with
    /// `--remote-debugging-port`, discovers the WebSocket target, and connects.
    pub async fn launch() -> Result<Self, AttaError> {
        let chrome = find_chrome().ok_or_else(|| {
            AttaError::Validation(
                "Chrome/Chromium not found. Install Chrome or use \
                 CdpBackend::connect() with an existing CDP endpoint."
                    .into(),
            )
        })?;

        let port = pick_free_port();
        let user_data_dir = std::env::temp_dir().join(format!("cdp-atta-{}", std::process::id()));

        let child = tokio::process::Command::new(&chrome)
            .arg("--headless=new")
            .arg(format!("--remote-debugging-port={port}"))
            .arg(format!("--user-data-dir={}", user_data_dir.display()))
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-gpu")
            .arg("about:blank")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| AttaError::Validation(format!("failed to launch Chrome: {e}")))?;

        // Discover the WebSocket URL (retries until Chrome is ready)
        let ws_url = discover_target(port).await?;

        let (ws, _) = tokio_tungstenite::connect_async(&ws_url)
            .await
            .map_err(|e| AttaError::Validation(format!("CDP connect after launch failed: {e}")))?;

        let backend = Self {
            ws: Mutex::new(ws),
            next_id: AtomicU64::new(1),
            child: Mutex::new(Some(child)),
        };

        backend.send_command("Page.enable", json!({})).await?;
        Ok(backend)
    }

    /// Send a CDP command and wait for its response.
    async fn send_command(&self, method: &str, params: Value) -> Result<Value, AttaError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let msg = json!({ "id": id, "method": method, "params": params });

        let mut ws = self.ws.lock().await;

        ws.send(tokio_tungstenite::tungstenite::Message::Text(
            msg.to_string(),
        ))
        .await
        .map_err(|e| AttaError::Validation(format!("CDP send failed: {e}")))?;

        // Read frames until we find the response matching our id
        loop {
            let frame = ws
                .next()
                .await
                .ok_or_else(|| AttaError::Validation("CDP WebSocket closed".into()))?
                .map_err(|e| AttaError::Validation(format!("CDP read failed: {e}")))?;

            if let tokio_tungstenite::tungstenite::Message::Text(text) = frame {
                let resp: Value = serde_json::from_str(&text).unwrap_or_default();
                if resp.get("id").and_then(|v| v.as_u64()) == Some(id) {
                    if let Some(error) = resp.get("error") {
                        let msg = error
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown CDP error");
                        return Err(AttaError::Validation(format!(
                            "CDP {method} error: {msg}"
                        )));
                    }
                    return Ok(resp.get("result").cloned().unwrap_or(json!({})));
                }
                // Event or unrelated response — skip
            }
        }
    }
}

#[async_trait::async_trait]
impl BrowserBackend for CdpBackend {
    async fn navigate(&self, url: &str) -> Result<Value, AttaError> {
        self.send_command("Page.navigate", json!({ "url": url }))
            .await?;

        // Brief wait for page load
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let eval = self
            .send_command(
                "Runtime.evaluate",
                json!({ "expression": "document.title" }),
            )
            .await?;

        let title = eval
            .pointer("/result/value")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        Ok(json!({
            "status": "navigated",
            "url": url,
            "title": title,
        }))
    }

    async fn click(&self, selector: &str) -> Result<Value, AttaError> {
        let sel_json = serde_json::to_string(selector).unwrap_or_default();
        let js = format!(
            r#"(() => {{ const el = document.querySelector({sel}); if (!el) throw new Error("element not found"); el.click(); return "clicked"; }})()"#,
            sel = sel_json,
        );

        let result = self
            .send_command("Runtime.evaluate", json!({ "expression": js }))
            .await?;

        if result.get("exceptionDetails").is_some() {
            return Err(AttaError::Validation(format!(
                "element not found: '{selector}'"
            )));
        }

        Ok(json!({
            "status": "clicked",
            "selector": selector,
        }))
    }

    async fn type_text(&self, selector: &str, text: &str) -> Result<Value, AttaError> {
        // Focus the target element
        let sel_json = serde_json::to_string(selector).unwrap_or_default();
        let focus_js = format!(
            r#"(() => {{ const el = document.querySelector({sel}); if (!el) throw new Error("element not found"); el.focus(); return "focused"; }})()"#,
            sel = sel_json,
        );

        let result = self
            .send_command("Runtime.evaluate", json!({ "expression": focus_js }))
            .await?;

        if result.get("exceptionDetails").is_some() {
            return Err(AttaError::Validation(format!(
                "element not found: '{selector}'"
            )));
        }

        // Insert text via the Input domain
        self.send_command("Input.insertText", json!({ "text": text }))
            .await?;

        Ok(json!({
            "status": "typed",
            "selector": selector,
            "text_length": text.len(),
        }))
    }

    async fn extract(&self, selector: Option<&str>) -> Result<Value, AttaError> {
        let js = match selector {
            Some(sel) => {
                let sel_json = serde_json::to_string(sel).unwrap_or_default();
                format!(
                    r#"(() => {{ const el = document.querySelector({sel}); return el ? el.innerText : ""; }})()"#,
                    sel = sel_json,
                )
            }
            None => "document.body.innerText".to_string(),
        };

        let result = self
            .send_command("Runtime.evaluate", json!({ "expression": js }))
            .await?;

        let content = result
            .pointer("/result/value")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let truncated = if content.len() > 50_000 {
            format!("{}...[truncated at 50000 chars]", &content[..50_000])
        } else {
            content.clone()
        };

        Ok(json!({
            "status": "extracted",
            "selector": selector,
            "content": truncated,
            "content_length": content.len(),
        }))
    }

    async fn screenshot(&self) -> Result<Value, AttaError> {
        let result = self
            .send_command("Page.captureScreenshot", json!({ "format": "png" }))
            .await?;

        let data = result
            .get("data")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        use base64::Engine;
        let size = base64::engine::general_purpose::STANDARD
            .decode(&data)
            .map(|d| d.len())
            .unwrap_or(0);

        Ok(json!({
            "status": "screenshot",
            "format": "png",
            "data_base64": data,
            "size_bytes": size,
        }))
    }

    async fn close(&self) -> Result<(), AttaError> {
        let _ = self.send_command("Browser.close", json!({})).await;

        let mut child = self.child.lock().await;
        if let Some(ref mut proc) = *child {
            let _ = proc.kill().await;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Find Chrome or Chromium binary on the system.
fn find_chrome() -> Option<String> {
    let candidates: &[&str] = if cfg!(target_os = "macos") {
        &[
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
        ]
    } else if cfg!(target_os = "windows") {
        &[
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        ]
    } else {
        &[]
    };

    for path in candidates {
        if std::path::Path::new(path).exists() {
            return Some(path.to_string());
        }
    }

    // Fall back to PATH lookup
    let names: &[&str] = if cfg!(target_os = "windows") {
        &["chrome.exe"]
    } else {
        &[
            "google-chrome",
            "google-chrome-stable",
            "chromium",
            "chromium-browser",
        ]
    };

    for name in names {
        if let Ok(output) = std::process::Command::new("which")
            .arg(name)
            .output()
        {
            if output.status.success() {
                return Some(name.to_string());
            }
        }
    }

    None
}

/// Bind to port 0 to get a free port from the OS.
fn pick_free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .unwrap_or(9222)
}

/// Discover the first page target's WebSocket URL via Chrome's `/json` endpoint.
///
/// Retries up to 10 times (3 s total) to account for Chrome startup time.
async fn discover_target(port: u16) -> Result<String, AttaError> {
    let url = format!("http://127.0.0.1:{port}/json");

    for attempt in 0..10 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }

        let resp = match reqwest::get(&url).await {
            Ok(r) => r,
            Err(_) => continue,
        };

        let targets: Vec<Value> = match resp.json().await {
            Ok(t) => t,
            Err(_) => continue,
        };

        // Prefer a "page" target
        for target in &targets {
            if target.get("type").and_then(|v| v.as_str()) == Some("page") {
                if let Some(ws) = target.get("webSocketDebuggerUrl").and_then(|v| v.as_str()) {
                    return Ok(ws.to_string());
                }
            }
        }

        // Otherwise take the first target with a WebSocket URL
        if let Some(ws) = targets
            .first()
            .and_then(|t| t.get("webSocketDebuggerUrl"))
            .and_then(|v| v.as_str())
        {
            return Ok(ws.to_string());
        }
    }

    Err(AttaError::Validation(format!(
        "no CDP targets found at http://127.0.0.1:{port}/json after 10 attempts"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_chrome_does_not_panic() {
        let _ = find_chrome();
    }

    #[test]
    fn test_pick_free_port() {
        let port = pick_free_port();
        assert!(port > 0);
    }

    #[tokio::test]
    #[ignore]
    async fn test_cdp_launch_and_navigate() {
        let backend = CdpBackend::launch()
            .await
            .expect("Chrome must be installed");
        let result = backend.navigate("https://example.com").await;
        assert!(result.is_ok(), "navigate should succeed: {result:?}");
        let _ = backend.close().await;
    }
}
