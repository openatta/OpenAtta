//! EstopManager — load, activate, resume, check

use std::path::PathBuf;
use std::sync::RwLock;

use atta_types::AttaError;
use tracing::{error, info, warn};

use super::types::{EstopLevel, EstopState};

/// Emergency stop manager with JSON state persistence
///
/// Fail-closed: if the state file is corrupted, defaults to KillAll.
pub struct EstopManager {
    state: RwLock<EstopState>,
    state_file: PathBuf,
}

impl EstopManager {
    /// Load E-Stop state from file. Fail-closed on corruption.
    pub fn load(state_file: PathBuf) -> Self {
        let state = if state_file.exists() {
            match std::fs::read_to_string(&state_file) {
                Ok(content) => match serde_json::from_str::<EstopState>(&content) {
                    Ok(s) => {
                        info!(path = %state_file.display(), "loaded E-Stop state");
                        s
                    }
                    Err(e) => {
                        error!(
                            error = %e,
                            path = %state_file.display(),
                            "corrupted E-Stop state file — fail-closed (KillAll)"
                        );
                        EstopState {
                            kill_all: true,
                            activated_at: Some(chrono::Utc::now().to_rfc3339()),
                            ..Default::default()
                        }
                    }
                },
                Err(e) => {
                    error!(
                        error = %e,
                        path = %state_file.display(),
                        "failed to read E-Stop state — fail-closed (KillAll)"
                    );
                    EstopState {
                        kill_all: true,
                        activated_at: Some(chrono::Utc::now().to_rfc3339()),
                        ..Default::default()
                    }
                }
            }
        } else {
            EstopState::default()
        };

        Self {
            state: RwLock::new(state),
            state_file,
        }
    }

    /// Activate an E-Stop level
    pub fn activate(&self, level: EstopLevel) -> Result<(), AttaError> {
        let mut state = self
            .state
            .write()
            .map_err(|_| AttaError::Other(anyhow::anyhow!("E-Stop lock poisoned")))?;

        let now = chrono::Utc::now().to_rfc3339();
        state.activated_at = Some(now);

        match level {
            EstopLevel::KillAll => {
                state.kill_all = true;
                // Generate OTP for resume
                let otp = format!("{:06}", rand_otp());
                info!(otp = %otp, "E-Stop KillAll activated — OTP generated for resume");
                state.resume_otp = Some(otp);
            }
            EstopLevel::NetworkKill => {
                state.network_kill = true;
                info!("E-Stop NetworkKill activated");
            }
            EstopLevel::DomainBlock(domains) => {
                for d in &domains {
                    if !state.blocked_domains.contains(d) {
                        state.blocked_domains.push(d.clone());
                    }
                }
                info!(domains = ?domains, "E-Stop DomainBlock activated");
            }
            EstopLevel::ToolFreeze(tools) => {
                for t in &tools {
                    if !state.frozen_tools.contains(t) {
                        state.frozen_tools.push(t.clone());
                    }
                }
                info!(tools = ?tools, "E-Stop ToolFreeze activated");
            }
        }

        drop(state);
        self.persist()
    }

    /// Resume from E-Stop (optionally requires OTP for KillAll)
    pub fn resume(&self, otp: Option<&str>) -> Result<(), AttaError> {
        let mut state = self
            .state
            .write()
            .map_err(|_| AttaError::Other(anyhow::anyhow!("E-Stop lock poisoned")))?;

        // If KillAll is active, require OTP
        if state.kill_all {
            match (&state.resume_otp, otp) {
                (Some(expected), Some(provided)) if expected == provided => {
                    info!("E-Stop KillAll resumed with valid OTP");
                }
                (Some(_), Some(_)) => {
                    warn!("E-Stop resume attempted with invalid OTP");
                    return Err(AttaError::SecurityViolation(
                        "invalid E-Stop resume OTP".to_string(),
                    ));
                }
                (Some(_), None) => {
                    return Err(AttaError::SecurityViolation(
                        "E-Stop KillAll resume requires OTP".to_string(),
                    ));
                }
                (None, _) => {
                    // No OTP set, allow resume
                }
            }
        }

        *state = EstopState::default();
        info!("E-Stop fully resumed — all restrictions cleared");

        drop(state);
        self.persist()
    }

    /// Check if a tool call is allowed under current E-Stop state
    pub fn check(&self, tool_name: &str, args: &serde_json::Value) -> Result<(), AttaError> {
        let state = self
            .state
            .read()
            .map_err(|_| AttaError::Other(anyhow::anyhow!("E-Stop lock poisoned")))?;

        if state.kill_all {
            return Err(AttaError::EmergencyStopped(
                "E-Stop KillAll active — all tool calls blocked".to_string(),
            ));
        }

        // Check network kill
        if state.network_kill {
            let network_tools = ["web_fetch", "web_search", "http_request", "browser"];
            if network_tools.contains(&tool_name) {
                return Err(AttaError::EmergencyStopped(format!(
                    "E-Stop NetworkKill active — tool '{}' blocked",
                    tool_name
                )));
            }
        }

        // Check domain blocks (for web tools with URL args)
        if !state.blocked_domains.is_empty() {
            if let Some(url) = args.get("url").and_then(|v| v.as_str()) {
                for domain in &state.blocked_domains {
                    if url.contains(domain) {
                        return Err(AttaError::EmergencyStopped(format!(
                            "E-Stop DomainBlock active — domain '{}' blocked",
                            domain
                        )));
                    }
                }
            }
        }

        // Check frozen tools
        if state.frozen_tools.contains(&tool_name.to_string()) {
            return Err(AttaError::EmergencyStopped(format!(
                "E-Stop ToolFreeze active — tool '{}' frozen",
                tool_name
            )));
        }

        Ok(())
    }

    /// Get current E-Stop state (for API/debug)
    pub fn current_state(&self) -> EstopState {
        self.state
            .read()
            .map(|s| s.clone())
            .unwrap_or_else(|_| EstopState {
                kill_all: true,
                ..Default::default()
            })
    }

    /// Persist state to disk (atomic: write tmp -> rename)
    fn persist(&self) -> Result<(), AttaError> {
        let state = self
            .state
            .read()
            .map_err(|_| AttaError::Other(anyhow::anyhow!("E-Stop lock poisoned")))?;

        let json = serde_json::to_string_pretty(&*state)?;

        // Ensure parent directory exists
        if let Some(parent) = self.state_file.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let tmp_path = self.state_file.with_extension("json.tmp");
        std::fs::write(&tmp_path, &json)?;
        std::fs::rename(&tmp_path, &self.state_file)?;

        Ok(())
    }
}

/// Generate a cryptographically random 6-digit numeric OTP
fn rand_otp() -> u32 {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    rng.gen_range(100_000..1_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::{NamedTempFile, TempDir};

    /// Helper: create a path to a non-existent file inside a temp directory.
    /// EstopManager::load treats non-existent files as default (all clear).
    fn nonexistent_state_file() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("estop.json");
        (dir, path)
    }

    #[test]
    fn test_default_state_allows_all() {
        let (_dir, path) = nonexistent_state_file();
        let mgr = EstopManager::load(path);
        assert!(mgr.check("shell", &serde_json::json!({})).is_ok());
        assert!(mgr.check("web_fetch", &serde_json::json!({})).is_ok());
    }

    #[test]
    fn test_kill_all_blocks_everything() {
        let (_dir, path) = nonexistent_state_file();
        let mgr = EstopManager::load(path);
        mgr.activate(EstopLevel::KillAll).unwrap();
        assert!(mgr.check("shell", &serde_json::json!({})).is_err());
        assert!(mgr.check("file_read", &serde_json::json!({})).is_err());
    }

    #[test]
    fn test_network_kill_blocks_web_tools() {
        let (_dir, path) = nonexistent_state_file();
        let mgr = EstopManager::load(path);
        mgr.activate(EstopLevel::NetworkKill).unwrap();
        assert!(mgr.check("web_fetch", &serde_json::json!({})).is_err());
        assert!(mgr.check("file_read", &serde_json::json!({})).is_ok());
    }

    #[test]
    fn test_tool_freeze() {
        let (_dir, path) = nonexistent_state_file();
        let mgr = EstopManager::load(path);
        mgr.activate(EstopLevel::ToolFreeze(vec!["shell".to_string()]))
            .unwrap();
        assert!(mgr.check("shell", &serde_json::json!({})).is_err());
        assert!(mgr.check("file_read", &serde_json::json!({})).is_ok());
    }

    #[test]
    fn test_corrupted_file_fail_closed() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(b"not valid json!!!").unwrap();
        tmp.flush().unwrap();
        let mgr = EstopManager::load(tmp.path().to_path_buf());
        // Should fail-closed (KillAll)
        assert!(mgr.check("file_read", &serde_json::json!({})).is_err());
    }

    #[test]
    fn test_resume_clears_state() {
        let (_dir, path) = nonexistent_state_file();
        let mgr = EstopManager::load(path);
        mgr.activate(EstopLevel::NetworkKill).unwrap();
        assert!(mgr.check("web_fetch", &serde_json::json!({})).is_err());
        mgr.resume(None).unwrap();
        assert!(mgr.check("web_fetch", &serde_json::json!({})).is_ok());
    }
}
