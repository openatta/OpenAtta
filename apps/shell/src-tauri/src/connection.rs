//! Connection management IPC commands — test, save, load, remove server connections.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::autostart;
use crate::http_client;
use crate::utils::audit_log;

// ── Types ──

/// Result of testing a remote server connection.
#[derive(Debug, Clone, Serialize)]
pub struct ConnectionTestResult {
    /// Whether the server responded successfully
    pub reachable: bool,
    /// Server version, if returned
    pub version: Option<String>,
    /// Error message on failure
    pub error: Option<String>,
}

/// A saved server connection entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub name: String,
    pub url: String,
    pub auth_type: String,
    #[serde(default)]
    pub default: bool,
    /// Encrypted API key (stored with `enc:` prefix)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// SSH tunnel enabled
    #[serde(default)]
    pub ssh_enabled: bool,
    /// SSH target (user@host)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_target: Option<String>,
    /// Path to SSH identity key
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_identity: Option<String>,
    /// Remote attaos port for SSH tunnel
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_remote_port: Option<u16>,
}

/// Connections file structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConnectionsFile {
    connections: Vec<Connection>,
}

// ── IPC Commands ──

/// Test connectivity to a remote AttaOS server.
#[tauri::command]
pub async fn test_connection(url: String, api_key: Option<String>) -> Result<ConnectionTestResult, String> {
    let health_url = format!("{}/api/v1/health", url.trim_end_matches('/'));

    let client = http_client::build_client(None, None)?;
    let req = http_client::authed_get(&client, &health_url, api_key.as_deref())
        .timeout(std::time::Duration::from_secs(10));

    match req.send().await {
        Ok(resp) => {
            if resp.status().is_success() {
                let body: serde_json::Value = resp.json().await.unwrap_or_default();
                let version = body["version"].as_str().map(String::from);
                Ok(ConnectionTestResult {
                    reachable: true,
                    version,
                    error: None,
                })
            } else {
                let status = resp.status();
                Ok(ConnectionTestResult {
                    reachable: false,
                    version: None,
                    error: Some(format!("HTTP {status}")),
                })
            }
        }
        Err(e) => Ok(ConnectionTestResult {
            reachable: false,
            version: None,
            error: Some(e.to_string()),
        }),
    }
}

/// Save a new connection entry to `$ATTA_HOME/etc/connections.yaml`.
#[tauri::command]
pub fn save_connection(
    home: String,
    name: String,
    url: String,
    auth_type: String,
    api_key: Option<String>,
) -> Result<(), String> {
    // Validate inputs
    if name.trim().is_empty() {
        return Err("[CONFIG] Connection name cannot be empty".to_string());
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("[CONFIG] URL must start with http:// or https://".to_string());
    }
    // Warn about insecure connections to remote servers
    if url.starts_with("http://")
        && !url.starts_with("http://localhost")
        && !url.starts_with("http://127.0.0.1")
    {
        tracing::warn!(
            url = %url,
            "saving insecure HTTP connection — credentials may be transmitted in plaintext"
        );
    }

    let home_path = if home.is_empty() {
        autostart::resolve_home()
    } else {
        PathBuf::from(&home)
    };
    let path = home_path.join("etc/connections.yaml");

    let mut file = load_connections_file(&path);

    // Encrypt api_key if provided — refuse to store plaintext on failure
    let encrypted_key = match api_key {
        Some(key) if !key.is_empty() => {
            let master = crate::secrets::MasterKey::load_or_create(&home_path)
                .map_err(|e| format!("[SECURITY] failed to load encryption key: {e}"))?;
            let encrypted = crate::secrets::encrypt(&master, key.as_bytes())
                .map_err(|e| format!("[SECURITY] failed to encrypt api_key: {e}"))?;
            Some(encrypted)
        }
        _ => None,
    };

    // Remove existing connection with same name
    file.connections.retain(|c| c.name != name);

    // If this is the first connection, make it default
    let is_default = file.connections.is_empty();
    file.connections.push(Connection {
        name: name.clone(),
        url,
        auth_type,
        default: is_default,
        api_key: encrypted_key,
        ssh_enabled: false,
        ssh_target: None,
        ssh_identity: None,
        ssh_remote_port: None,
    });

    save_connections_file(&path, &file)?;
    audit_log(&home_path, "save_connection", "connections.yaml", &name);
    Ok(())
}

/// Load all saved connections.
#[tauri::command]
pub fn load_connections(home: String) -> Result<Vec<Connection>, String> {
    let home_path = if home.is_empty() {
        autostart::resolve_home()
    } else {
        PathBuf::from(&home)
    };
    let path = home_path.join("etc/connections.yaml");
    let file = load_connections_file(&path);
    Ok(file.connections)
}

/// Remove a connection by name.
#[tauri::command]
pub fn remove_connection(home: String, name: String) -> Result<(), String> {
    let home_path = if home.is_empty() {
        autostart::resolve_home()
    } else {
        PathBuf::from(&home)
    };
    let path = home_path.join("etc/connections.yaml");

    let mut file = load_connections_file(&path);
    file.connections.retain(|c| c.name != name);

    // Ensure there's still a default if connections remain
    if !file.connections.is_empty() && !file.connections.iter().any(|c| c.default) {
        file.connections[0].default = true;
    }

    save_connections_file(&path, &file)?;
    audit_log(&home_path, "remove_connection", "connections.yaml", &name);
    Ok(())
}

/// Decrypt the API key for a connection.
///
/// Returns `None` if the connection has no api_key or decryption fails.
pub fn decrypt_connection_secret(home: &std::path::Path, name: &str) -> Option<String> {
    let path = home.join("etc/connections.yaml");
    let file = load_connections_file(&path);
    let conn = file.connections.iter().find(|c| c.name == name)?;
    let encrypted = conn.api_key.as_ref()?;

    if !crate::secrets::is_encrypted(encrypted) {
        // Plaintext (legacy), return as-is
        return Some(encrypted.clone());
    }

    let master = crate::secrets::MasterKey::load_or_create(home).ok()?;
    let plaintext = crate::secrets::decrypt(&master, encrypted).ok()?;
    String::from_utf8(plaintext).ok()
}

// ── Helpers ──

fn load_connections_file(path: &std::path::Path) -> ConnectionsFile {
    if let Ok(data) = std::fs::read_to_string(path) {
        if let Ok(file) = serde_yaml::from_str(&data) {
            return file;
        }
    }
    ConnectionsFile {
        connections: Vec::new(),
    }
}

fn save_connections_file(path: &std::path::Path, file: &ConnectionsFile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let data = serde_yaml::to_string(file).map_err(|e| e.to_string())?;
    // Atomic write: tmp + rename
    let tmp_path = path.with_extension("yaml.tmp");
    std::fs::write(&tmp_path, &data).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp_path, path).map_err(|e| e.to_string())
}
