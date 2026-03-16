//! Unified HTTP client factory with authentication and proxy support.

use std::path::Path;
use std::time::Duration;

/// Build a `reqwest::Client` with optional proxy configuration.
///
/// Proxy resolution order:
/// 1. Explicit `proxy` parameter
/// 2. `proxy.url` from `$ATTA_HOME/etc/attaos.yaml`
/// 3. `HTTPS_PROXY` / `HTTP_PROXY` environment variables (reqwest built-in)
pub fn build_client(home: Option<&Path>, proxy: Option<&str>) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(Duration::from_secs(300));

    // Resolve proxy URL
    let proxy_url = proxy
        .map(String::from)
        .or_else(|| read_proxy_from_config(home?));

    if let Some(url) = proxy_url {
        let proxy = reqwest::Proxy::all(&url)
            .map_err(|e| format!("[NETWORK] Invalid proxy URL: {e}"))?;
        builder = builder.proxy(proxy);
    }

    builder
        .build()
        .map_err(|e| format!("[NETWORK] Failed to create HTTP client: {e}"))
}

/// Build a GET request with optional Authorization header.
pub fn authed_get(
    client: &reqwest::Client,
    url: &str,
    auth: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut req = client.get(url);
    if let Some(token) = auth {
        req = req.header("Authorization", format!("Bearer {token}"));
    }
    req
}

/// Read `proxy.url` from `$ATTA_HOME/etc/attaos.yaml`.
fn read_proxy_from_config(home: &Path) -> Option<String> {
    let config_path = home.join("etc/attaos.yaml");
    let data = std::fs::read_to_string(config_path).ok()?;
    let yaml: serde_json::Value = serde_yaml::from_str(&data).ok()?;
    yaml["proxy"]["url"].as_str().map(String::from)
}
