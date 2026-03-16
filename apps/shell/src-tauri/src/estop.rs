//! Emergency stop (E-Stop) integration.
//!
//! Reads/writes `$ATTA_HOME/data/estop.json` shared with the attaos server.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// E-Stop state, matching `crates/security/src/estop/` format.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EstopState {
    #[serde(default)]
    pub kill_all: bool,
    #[serde(default)]
    pub network_kill: bool,
    #[serde(default)]
    pub blocked_domains: Vec<String>,
    #[serde(default)]
    pub frozen_tools: Vec<String>,
    #[serde(default)]
    pub updated_at: String,
}

/// Query the current E-Stop state.
#[tauri::command]
pub fn estop_status(home: String) -> Result<EstopState, String> {
    let path = estop_path(&home);
    if !path.exists() {
        return Ok(EstopState::default());
    }
    let data =
        std::fs::read_to_string(&path).map_err(|e| format!("[SECURITY] Cannot read estop: {e}"))?;
    serde_json::from_str(&data).map_err(|e| format!("[SECURITY] Invalid estop.json: {e}"))
}

/// Engage an E-Stop level.
///
/// `level` must be one of: `kill_all`, `network_kill`.
#[tauri::command]
pub fn estop_engage(home: String, level: String) -> Result<(), String> {
    set_estop_level(&home, &level, true)
}

/// Resume (disengage) an E-Stop level.
#[tauri::command]
pub fn estop_resume(home: String, level: String) -> Result<(), String> {
    set_estop_level(&home, &level, false)
}

fn set_estop_level(home: &str, level: &str, engaged: bool) -> Result<(), String> {
    let path = estop_path(home);
    let mut state = if path.exists() {
        let data = std::fs::read_to_string(&path)
            .map_err(|e| format!("[SECURITY] Cannot read estop: {e}"))?;
        serde_json::from_str::<EstopState>(&data).unwrap_or_default()
    } else {
        EstopState::default()
    };

    match level {
        "kill_all" => state.kill_all = engaged,
        "network_kill" => state.network_kill = engaged,
        _ => return Err(format!("[SECURITY] Unknown E-Stop level: {level}")),
    }

    state.updated_at = chrono::Utc::now().to_rfc3339();

    // Ensure data directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("[SECURITY] Cannot create data directory: {e}"))?;
    }

    // Atomic write
    let tmp_path = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(&state)
        .map_err(|e| format!("[SECURITY] Cannot serialize estop: {e}"))?;
    std::fs::write(&tmp_path, &json)
        .map_err(|e| format!("[SECURITY] Cannot write estop: {e}"))?;
    std::fs::rename(&tmp_path, &path)
        .map_err(|e| format!("[SECURITY] Cannot finalize estop: {e}"))?;

    let action = if engaged { "engaged" } else { "resumed" };
    tracing::info!(level = %level, action, "E-Stop state changed");
    crate::utils::audit_log(
        std::path::Path::new(home),
        &format!("estop_{action}"),
        "estop.json",
        level,
    );

    Ok(())
}

fn estop_path(home: &str) -> PathBuf {
    let home_path = if home.is_empty() {
        crate::autostart::resolve_home()
    } else {
        PathBuf::from(home)
    };
    home_path.join("data/estop.json")
}
