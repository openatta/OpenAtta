//! Manager IPC commands — manifest reading, installation verification, backup/rollback, server control.

use std::path::PathBuf;

use serde::Serialize;
use tauri::Emitter;

use crate::autostart;
use crate::utils::{copy_dir_recursive, validate_url, UrlPolicy};

// ── Types ──

/// Status of a single installed component.
#[derive(Debug, Clone, Serialize)]
pub struct ComponentStatus {
    /// Component name (e.g. "attaos", "webui", "skills")
    pub name: String,
    /// Whether the component files exist
    pub present: bool,
    /// Expected version from manifest
    pub expected_version: Option<String>,
}

// ── IPC Commands ──

/// Read the installation manifest from `$ATTA_HOME/.manifest.json`.
#[tauri::command]
pub fn read_manifest(home: String) -> Result<serde_json::Value, String> {
    let home_path = resolve_home_path(&home);
    let path = home_path.join(".manifest.json");
    let data = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&data).map_err(|e| e.to_string())
}

/// Verify the integrity of the current installation.
///
/// Checks that expected directories and binaries exist.
#[tauri::command]
pub fn verify_installation(home: String) -> Result<Vec<ComponentStatus>, String> {
    let home_path = resolve_home_path(&home);
    let mut statuses = Vec::new();

    // Read manifest for version info
    let manifest = read_manifest_internal(&home_path);

    // Check attaos binary
    let bin_name = if cfg!(windows) { "attaos.exe" } else { "attaos" };
    let attaos_bin = home_path.join("bin").join(bin_name);
    statuses.push(ComponentStatus {
        name: "attaos".to_string(),
        present: attaos_bin.exists(),
        expected_version: manifest
            .as_ref()
            .and_then(|m| m["version"].as_str().map(String::from)),
    });

    // Check WebUI
    let webui_index = home_path.join("lib/webui/index.html");
    statuses.push(ComponentStatus {
        name: "webui".to_string(),
        present: webui_index.exists(),
        expected_version: None,
    });

    // Check skills directory
    let skills_dir = home_path.join("lib/skills");
    let has_skills = skills_dir.exists()
        && std::fs::read_dir(&skills_dir)
            .map(|mut rd| rd.next().is_some())
            .unwrap_or(false);
    statuses.push(ComponentStatus {
        name: "skills".to_string(),
        present: has_skills,
        expected_version: None,
    });

    // Check flows directory
    let flows_dir = home_path.join("lib/flows");
    let has_flows = flows_dir.exists()
        && std::fs::read_dir(&flows_dir)
            .map(|mut rd| rd.next().is_some())
            .unwrap_or(false);
    statuses.push(ComponentStatus {
        name: "flows".to_string(),
        present: has_flows,
        expected_version: None,
    });

    // Check data directory
    statuses.push(ComponentStatus {
        name: "data".to_string(),
        present: home_path.join("data").exists(),
        expected_version: None,
    });

    Ok(statuses)
}

/// Create a backup of the current installation.
///
/// Copies `bin/`, `lib/`, and `.manifest.json` to a timestamped backup directory.
/// Returns the backup directory path.
#[tauri::command]
pub fn backup_current(home: String) -> Result<String, String> {
    let home_path = resolve_home_path(&home);
    let timestamp = chrono_timestamp();
    let backup_dir = home_path.join(format!("backups/{timestamp}"));

    std::fs::create_dir_all(&backup_dir).map_err(|e| e.to_string())?;

    // Backup manifest
    let manifest_src = home_path.join(".manifest.json");
    if manifest_src.exists() {
        std::fs::copy(&manifest_src, backup_dir.join(".manifest.json"))
            .map_err(|e| e.to_string())?;
    }

    // Backup bin/
    let bin_src = home_path.join("bin");
    if bin_src.exists() {
        copy_dir_recursive(&bin_src, &backup_dir.join("bin")).map_err(|e| e.to_string())?;
    }

    // Backup lib/
    let lib_src = home_path.join("lib");
    if lib_src.exists() {
        copy_dir_recursive(&lib_src, &backup_dir.join("lib")).map_err(|e| e.to_string())?;
    }

    // Note: data/ is NOT backed up because it can be very large and is not
    // modified during upgrades. The data/ backup in backups/ is wasteful
    // since rollback does not restore it either.

    // Backup etc/
    let etc_src = home_path.join("etc");
    if etc_src.exists() {
        copy_dir_recursive(&etc_src, &backup_dir.join("etc")).map_err(|e| e.to_string())?;
    }

    // Rotate backups: keep only the latest 5
    rotate_backups(&home_path, 5);

    Ok(backup_dir.to_string_lossy().to_string())
}

/// Rollback to a previous backup.
///
/// Restores `bin/`, `lib/`, and `.manifest.json` from the backup directory.
#[tauri::command]
pub fn rollback(home: String, backup: String) -> Result<(), String> {
    let home_path = resolve_home_path(&home);
    let backup_path = PathBuf::from(&backup);

    if !backup_path.exists() {
        return Err(format!("backup directory does not exist: {backup}"));
    }

    // Security: verify backup path is within ATTA_HOME/backups/
    let backups_dir = home_path.join("backups");
    if let (Ok(bc), Ok(bd)) = (backup_path.canonicalize(), backups_dir.canonicalize()) {
        if !bc.starts_with(&bd) {
            return Err(format!(
                "[PERMISSION] Backup path must be within {}/backups/",
                home_path.display()
            ));
        }
    }

    // Restore manifest
    let manifest_backup = backup_path.join(".manifest.json");
    if manifest_backup.exists() {
        std::fs::copy(&manifest_backup, home_path.join(".manifest.json"))
            .map_err(|e| e.to_string())?;
    }

    // Restore bin/
    let bin_backup = backup_path.join("bin");
    if bin_backup.exists() {
        let bin_dst = home_path.join("bin");
        if bin_dst.exists() {
            std::fs::remove_dir_all(&bin_dst).map_err(|e| e.to_string())?;
        }
        copy_dir_recursive(&bin_backup, &bin_dst).map_err(|e| e.to_string())?;
    }

    // Restore lib/
    let lib_backup = backup_path.join("lib");
    if lib_backup.exists() {
        let lib_dst = home_path.join("lib");
        if lib_dst.exists() {
            std::fs::remove_dir_all(&lib_dst).map_err(|e| e.to_string())?;
        }
        copy_dir_recursive(&lib_backup, &lib_dst).map_err(|e| e.to_string())?;
    }

    // Note: data/ is backed up but NOT restored during rollback, because
    // user data may have changed since the backup. Only bin/, lib/, and
    // .manifest.json (which are replaced during upgrades) are restored.

    // Note: etc/ is backed up but NOT restored during rollback, because
    // user may have changed config (API keys, connections) since the backup.
    // The etc/ backup remains available for manual recovery if needed.

    Ok(())
}

/// Stop the running attaos server by sending a shutdown signal.
///
/// Sends SIGTERM first, polls for process exit up to 5 seconds,
/// then escalates to SIGKILL if the process is still alive.
#[tauri::command]
pub async fn stop_server(home: String) -> Result<(), String> {
    let home_path = resolve_home_path(&home);
    let pid_path = home_path.join("run/attaos.pid");

    if !pid_path.exists() {
        return Ok(()); // Not running or no PID file
    }

    let pid_str = std::fs::read_to_string(&pid_path).map_err(|e| e.to_string())?;
    let pid: i32 = pid_str
        .trim()
        .parse()
        .map_err(|e: std::num::ParseIntError| e.to_string())?;

    // Verify PID belongs to attaos before sending signals.
    //
    // NOTE: There is an inherent TOCTOU race between verify_attaos_pid() and
    // libc::kill() — the PID could theoretically be recycled between the two
    // calls. This cannot be fully eliminated without pidfd (Linux 5.3+). The
    // window is extremely small and acceptable for a desktop application.
    if !verify_attaos_pid(pid) {
        tracing::warn!(pid, "PID does not belong to attaos, removing stale PID file");
        let _ = std::fs::remove_file(&pid_path);
        return Err("server process no longer exists (stale PID)".to_string());
    }

    #[cfg(unix)]
    {
        // Send SIGTERM for graceful shutdown (verified PID is attaos above)
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }

        // Poll for exit: up to 10 × 500ms = 5 seconds
        for _ in 0..10 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let alive = unsafe { libc::kill(pid, 0) } == 0;
            if !alive {
                let _ = std::fs::remove_file(&pid_path);
                return Ok(());
            }
        }

        // Process still alive — escalate to SIGKILL
        tracing::warn!(pid, "attaos did not exit after SIGTERM, sending SIGKILL");
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    #[cfg(not(unix))]
    {
        // Use taskkill on Windows to terminate the process
        let status = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        match status {
            Ok(s) if s.success() => {
                tracing::info!(pid, "attaos terminated via taskkill");
            }
            Ok(s) => {
                tracing::warn!(pid, code = ?s.code(), "taskkill returned non-zero");
            }
            Err(e) => {
                tracing::warn!(pid, error = %e, "failed to run taskkill");
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    // Clean up PID file
    let _ = std::fs::remove_file(&pid_path);

    Ok(())
}

// ── Service management commands ──

/// Restart the attaos server (stop + start).
#[tauri::command]
pub async fn restart_server(home: String) -> Result<u16, String> {
    stop_server(home.clone()).await?;
    start_server_internal(&home).await
}

/// Health status of a single server component.
#[derive(Debug, Clone, Serialize)]
pub struct ComponentHealth {
    /// Component name
    pub name: String,
    /// Status: "healthy", "degraded", "unknown"
    pub status: String,
}

/// Server status information.
#[derive(Debug, Clone, Serialize)]
pub struct ServerStatus {
    /// Whether the server process is alive
    pub running: bool,
    /// PID of the server process (if known)
    pub pid: Option<u32>,
    /// Port the server is listening on (if known)
    pub port: Option<u16>,
    /// Uptime in seconds (if PID file has mtime)
    pub uptime_secs: Option<u64>,
    /// Component-level health (from /api/v1/health or fallback)
    pub components: Vec<ComponentHealth>,
}

/// Query the current server status.
#[tauri::command]
pub async fn server_status(home: String) -> Result<ServerStatus, String> {
    let home_path = resolve_home_path(&home);
    let pid_path = home_path.join("run/attaos.pid");

    if !pid_path.exists() {
        return Ok(ServerStatus {
            running: false,
            pid: None,
            port: None,
            uptime_secs: None,
            components: Vec::new(),
        });
    }

    let pid_str = std::fs::read_to_string(&pid_path).map_err(|e| e.to_string())?;
    let pid: i32 = pid_str
        .trim()
        .parse()
        .map_err(|e: std::num::ParseIntError| e.to_string())?;

    let alive = is_process_alive(pid);

    // Estimate uptime from PID file modification time
    let uptime_secs = std::fs::metadata(&pid_path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|mtime| std::time::SystemTime::now().duration_since(mtime).ok())
        .map(|d| d.as_secs());

    // Read port from config
    let port = read_configured_port(&home_path);

    // Fetch component-level health
    let components = if alive {
        fetch_component_health(&home_path, port).await
    } else {
        Vec::new()
    };

    Ok(ServerStatus {
        running: alive,
        pid: if alive { Some(pid as u32) } else { None },
        port: if alive { Some(port) } else { None },
        uptime_secs: if alive { uptime_secs } else { None },
        components,
    })
}

/// Read the last N lines of the server log.
#[tauri::command]
pub fn read_server_log(home: String, lines: usize) -> Result<String, String> {
    let home_path = resolve_home_path(&home);
    let log_path = home_path.join("log/attaos.log");

    if !log_path.exists() {
        return Ok(String::new());
    }

    // Read at most the last 1 MB to avoid OOM on large log files
    let file = std::fs::File::open(&log_path).map_err(|e| e.to_string())?;
    let file_size = file.metadata().map_err(|e| e.to_string())?.len();
    let max_read = 1024 * 1024u64;
    let read_from = file_size.saturating_sub(max_read);

    use std::io::{Read as _, Seek, SeekFrom};
    let mut reader = std::io::BufReader::new(file);
    reader
        .seek(SeekFrom::Start(read_from))
        .map_err(|e| e.to_string())?;
    let mut buf = String::new();
    reader.read_to_string(&mut buf).map_err(|e| e.to_string())?;

    let all_lines: Vec<&str> = buf.lines().collect();
    let start = all_lines.len().saturating_sub(lines);
    Ok(all_lines[start..].join("\n"))
}

// ── Upgrade commands ──

/// Component update information.
#[derive(Debug, Clone, Serialize)]
pub struct ComponentUpdateInfo {
    /// Whether an update is available
    pub available: bool,
    /// Currently installed version
    pub current_version: String,
    /// Remote version available
    pub remote_version: String,
    /// Package download URL
    pub package_url: Option<String>,
    /// SHA-256 hash of the package
    pub sha256: Option<String>,
    /// Minimum shell version required (None if no constraint)
    pub min_shell_version: Option<String>,
    /// Whether the current shell is compatible with the update
    pub shell_compatible: bool,
}

/// Check if a component update is available by comparing local and remote manifests.
#[tauri::command]
pub async fn check_component_update(
    home: String,
    manifest_url: String,
) -> Result<ComponentUpdateInfo, String> {
    // Validate URL scheme
    validate_url(&manifest_url, UrlPolicy::AllowRemote)?;

    let home_path = resolve_home_path(&home);
    let local_manifest = read_manifest_internal(&home_path);
    let current_version = local_manifest
        .as_ref()
        .and_then(|m| m["version"].as_str())
        .unwrap_or("unknown")
        .to_string();

    // Fetch remote manifest
    let resp = reqwest::get(&manifest_url)
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let remote: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let remote_version = remote["version"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();

    // Update is available only if remote version is strictly newer
    let available = remote_version != "unknown"
        && current_version != "unknown"
        && !version_satisfies(&current_version, &remote_version);

    // Check min_shell_version compatibility
    let min_shell_version = remote["min_shell_version"].as_str().map(String::from);
    let shell_compatible = match &min_shell_version {
        Some(min_ver) => version_satisfies(env!("CARGO_PKG_VERSION"), min_ver),
        None => true,
    };

    Ok(ComponentUpdateInfo {
        available,
        current_version,
        remote_version,
        package_url: remote["package_url"].as_str().map(String::from),
        sha256: remote["sha256"].as_str().map(String::from),
        min_shell_version,
        shell_compatible,
    })
}

/// Upgrade progress event payload.
#[derive(Debug, Clone, Serialize)]
struct UpgradeProgress {
    phase: String,
    detail: String,
}

/// Perform a full component upgrade: backup → stop → download → verify → extract → deploy → write manifest → start.
///
/// On failure, automatically rolls back to the backup. Emits `upgrade-progress` events.
/// Always cleans up tmp/ regardless of success or failure.
#[tauri::command]
pub async fn upgrade_components(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::installer::DownloadCancelState>,
    home: String,
    package_url: String,
    sha256: Option<String>,
    manifest: serde_json::Value,
) -> Result<(), String> {
    // Validate URL scheme (Phase 2: HTTPS enforcement)
    validate_url(&package_url, UrlPolicy::AllowRemote)?;

    // Acquire operation lock to prevent concurrent operations
    let _lock = OperationLock::acquire(&PathBuf::from(&home))?;

    // Reset and clone cancel token for this operation
    let cancel_token = {
        let mut token = state.token.lock().await;
        *token = tokio_util::sync::CancellationToken::new();
        token.clone()
    };

    let tmp_dir = PathBuf::from(&home).join("tmp");
    let result = upgrade_components_inner(
        &app, &home, &package_url, sha256, manifest, &tmp_dir, cancel_token,
    )
    .await;

    // Always clean up tmp/ (Phase 1: tmp cleanup on all paths)
    let _ = std::fs::remove_dir_all(&tmp_dir);

    result
}

/// Inner upgrade logic separated so tmp cleanup always runs in the outer function.
async fn upgrade_components_inner(
    app: &tauri::AppHandle,
    home: &str,
    package_url: &str,
    sha256: Option<String>,
    manifest: serde_json::Value,
    tmp_dir: &std::path::Path,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<(), String> {
    let emit_progress = |phase: &str, detail: &str| {
        let _ = app.emit(
            "upgrade-progress",
            UpgradeProgress {
                phase: phase.to_string(),
                detail: detail.to_string(),
            },
        );
    };

    // 1. Disk space precheck (before any changes — no rollback needed)
    emit_progress("precheck", "Checking disk space...");
    let head_resp = reqwest::Client::new()
        .head(package_url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .ok();
    let pkg_size = head_resp
        .and_then(|r| r.content_length())
        .unwrap_or(500 * 1024 * 1024);
    let required = pkg_size * 3; // download + extract + install
    let space = crate::installer::check_disk_space(home.to_string());
    if space.available < required {
        return Err(format!(
            "[DISK] Insufficient disk space: {} available, need at least {} \
             (3× estimated package size)",
            format_bytes(space.available),
            format_bytes(required)
        ));
    }

    // 2. Backup
    emit_progress("backup", "Creating backup of current installation...");
    let backup_path = backup_current(home.to_string())?;

    // Macro-like helper to reduce rollback boilerplate
    macro_rules! rollback_on_err {
        ($phase:expr, $err:expr) => {{
            let phase = $phase;
            let original_err = $err;
            emit_progress("rollback", &format!("{phase} failed, rolling back..."));
            if let Err(rb_err) = rollback(home.to_string(), backup_path.clone()) {
                return Err(format!(
                    "{phase} failed: {original_err}; rollback also failed: {rb_err}"
                ));
            }
            // Verify rollback restored critical files
            let home_p = std::path::Path::new(home);
            if !home_p.join(".manifest.json").exists() || !home_p.join("bin").exists() {
                return Err(format!(
                    "{phase} failed: {original_err}; rollback completed but verification \
                     failed — critical files missing"
                ));
            }
            if let Err(restart_err) = start_server_internal(home).await {
                return Err(format!(
                    "{phase} failed: {original_err}; rollback OK but server restart failed: {restart_err}"
                ));
            }
            return Err(format!("{phase} failed: {original_err}"));
        }};
    }

    // 3. Stop server
    emit_progress("stop", "Stopping attaos server...");
    stop_server(home.to_string()).await?;

    // 4. Download
    emit_progress("download", "Downloading update package...");
    std::fs::create_dir_all(tmp_dir).map_err(|e| e.to_string())?;
    let dest_file = tmp_dir.join("upgrade-package.tar.gz");
    let dest_str = dest_file.to_string_lossy().to_string();

    if let Err(e) = crate::installer::download_components_inner(
        app.clone(),
        package_url.to_string(),
        dest_str.clone(),
        Some(cancel),
    )
    .await
    {
        rollback_on_err!("download", e);
    }

    // 4. Verify
    if let Some(expected_sha) = sha256 {
        emit_progress("verify", "Verifying package integrity...");
        if !crate::installer::verify_package(dest_str.clone(), expected_sha) {
            rollback_on_err!("verify", "[INTEGRITY] SHA-256 verification failed".to_string());
        }
    }

    // 5. Extract
    emit_progress("extract", "Extracting update package...");
    let extract_dir = tmp_dir.join("upgrade-extracted");
    let extract_str = extract_dir.to_string_lossy().to_string();
    if let Err(e) = crate::installer::extract_package(
        app.clone(),
        dest_str,
        extract_str.clone(),
    )
    .await
    {
        rollback_on_err!("extract", e);
    }

    // Platform compatibility check
    let extracted_manifest = extract_dir.join(".manifest.json");
    if extracted_manifest.exists() {
        if let Ok(data) = std::fs::read_to_string(&extracted_manifest) {
            if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&data) {
                if let Some(pkg_platform) = pkg["platform"].as_str() {
                    let current =
                        format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
                    if pkg_platform != current {
                        rollback_on_err!(
                            "platform-check",
                            format!(
                                "Package is for {pkg_platform} but this system is {current}"
                            )
                        );
                    }
                }
            }
        }
    }

    // 6. Deploy
    emit_progress("deploy", "Installing updated components...");
    if let Err(e) = crate::installer::install_components(extract_str, home.to_string()) {
        rollback_on_err!("deploy", e);
    }

    // 7. Write manifest
    emit_progress("manifest", "Updating installation manifest...");
    if let Err(e) = crate::installer::write_manifest(home.to_string(), manifest) {
        rollback_on_err!("manifest", e);
    }

    // 8. Start server — special handling for double-failure
    emit_progress("start", "Starting attaos server...");
    if let Err(e) = start_server_internal(home).await {
        emit_progress("rollback", "Server start failed, rolling back...");
        if let Err(rb_err) = rollback(home.to_string(), backup_path) {
            return Err(format!(
                "server start failed: {e}; rollback also failed: {rb_err}"
            ));
        }
        // Verify rollback restored critical files
        let home_p = std::path::Path::new(home);
        if !home_p.join(".manifest.json").exists() || !home_p.join("bin").exists() {
            return Err(format!(
                "server start failed: {e}; rollback completed but verification \
                 failed — critical files missing"
            ));
        }
        if let Err(restart_err) = start_server_internal(home).await {
            return Err(format!(
                "server start failed: {e}; rollback OK but restart after rollback also failed: {restart_err}"
            ));
        }
        return Err(format!("server start failed (rolled back): {e}"));
    }

    emit_progress("done", "Upgrade complete!");
    Ok(())
}

/// Internal helper to start the server using configured port.
async fn start_server_internal(home: &str) -> Result<u16, String> {
    let port = read_configured_port(std::path::Path::new(home));
    autostart::ensure_server(std::path::Path::new(home), port).await
}

/// Read the configured server port from `$ATTA_HOME/etc/attaos.yaml`,
/// falling back to `ATTA_PORT` env var, then default 3000.
fn read_configured_port(home: &std::path::Path) -> u16 {
    let config_path = home.join("etc/attaos.yaml");
    if let Ok(data) = std::fs::read_to_string(&config_path) {
        if let Ok(yaml) = serde_yaml::from_str::<serde_json::Value>(&data) {
            if let Some(port) = yaml["server"]["port"].as_u64() {
                if port > 0 && port <= 65535 {
                    return port as u16;
                }
            }
        }
    }
    std::env::var("ATTA_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000)
}

// ── Helpers ──

/// Fetch component-level health from the server's /api/v1/health endpoint.
///
/// Falls back to file-system checks if the server doesn't return component info.
async fn fetch_component_health(
    home: &std::path::Path,
    port: u16,
) -> Vec<ComponentHealth> {
    let url = format!("http://localhost:{port}/api/v1/health");
    if let Ok(resp) = reqwest::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
    {
        if let Ok(body) = resp.json::<serde_json::Value>().await {
            if let Some(components) = body["components"].as_object() {
                return components
                    .iter()
                    .map(|(name, val)| ComponentHealth {
                        name: name.clone(),
                        status: val
                            .as_str()
                            .or_else(|| val["status"].as_str())
                            .unwrap_or("unknown")
                            .to_string(),
                    })
                    .collect();
            }
        }
    }

    // Fallback: check critical files/directories
    let checks: Vec<(&str, bool)> = vec![
        ("binary", home.join("bin/attaos").exists() || home.join("bin/attaos.exe").exists()),
        ("webui", home.join("lib/webui/index.html").exists()),
        ("data", home.join("data").exists()),
    ];
    checks
        .into_iter()
        .map(|(name, present)| ComponentHealth {
            name: name.to_string(),
            status: if present { "healthy" } else { "degraded" }.to_string(),
        })
        .collect()
}

fn resolve_home_path(home: &str) -> PathBuf {
    if home.is_empty() {
        autostart::resolve_home()
    } else {
        PathBuf::from(home)
    }
}

fn read_manifest_internal(home: &std::path::Path) -> Option<serde_json::Value> {
    let path = home.join(".manifest.json");
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

fn chrono_timestamp() -> String {
    // Simple timestamp without chrono dependency: YYYYMMDD-HHMMSS
    use std::time::SystemTime;
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    // Convert to a readable-ish format
    format!("backup-{secs}")
}

/// Check if a process is alive.
fn is_process_alive(pid: i32) -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        // On Windows, try to query the process via tasklist
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .map(|o| {
                let text = String::from_utf8_lossy(&o.stdout);
                text.contains(&pid.to_string())
            })
            .unwrap_or(false)
    }
}

/// Verify that a PID belongs to an attaos process (not a recycled PID).
fn verify_attaos_pid(pid: i32) -> bool {
    #[cfg(unix)]
    {
        let output = std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "comm="])
            .output();
        match output {
            Ok(out) => {
                let comm = String::from_utf8_lossy(&out.stdout)
                    .trim()
                    .to_lowercase();
                if comm.is_empty() {
                    return false;
                }
                comm.contains("attaos")
            }
            Err(_) => true, // Cannot verify — assume ours to be safe
        }
    }
    #[cfg(not(unix))]
    {
        let output = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output();
        match output {
            Ok(out) => {
                let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
                text.contains("attaos")
            }
            Err(_) => true,
        }
    }
}

// validate_url is now in utils.rs and imported at the top of this file.

/// Rotate backups, keeping only the latest `keep` entries.
fn rotate_backups(home: &std::path::Path, keep: usize) {
    let backups_dir = home.join("backups");
    let Ok(entries) = std::fs::read_dir(&backups_dir) else {
        return;
    };
    let mut dirs: Vec<_> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .collect();
    // Sort by modification time (oldest first)
    dirs.sort_by_key(|e| {
        e.metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });
    if dirs.len() > keep {
        let to_remove = dirs.len() - keep;
        for entry in dirs.into_iter().take(to_remove) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Format bytes as a human-readable string.
fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.0} MB", bytes as f64 / 1_048_576.0)
    } else {
        format!("{:.0} KB", bytes as f64 / 1_024.0)
    }
}

/// File-based operation lock to prevent concurrent upgrade/install operations.
struct OperationLock {
    path: PathBuf,
}

impl OperationLock {
    /// Acquire an exclusive operation lock using atomic file creation.
    fn acquire(home: &std::path::Path) -> Result<Self, String> {
        let path = home.join(".operation.lock");

        // Try atomic exclusive creation (O_CREAT | O_EXCL equivalent)
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                use std::io::Write;
                let _ = writeln!(file, "{}", std::process::id());
                Ok(Self { path })
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // Lock exists — check if the owning process is still alive
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(pid) = content.trim().parse::<i32>() {
                        if is_process_alive(pid) {
                            return Err(
                                "Another operation is already in progress. \
                                 Please wait for it to complete."
                                    .to_string(),
                            );
                        }
                    }
                }
                // Stale lock — remove and retry once
                let _ = std::fs::remove_file(&path);
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .map_err(|_| {
                        "Another operation is already in progress. \
                         Please wait for it to complete."
                            .to_string()
                    })?;
                use std::io::Write;
                let _ = writeln!(file, "{}", std::process::id());
                Ok(Self { path })
            }
            Err(e) => Err(format!("[PERMISSION] Cannot create lock file: {e}")),
        }
    }
}

impl Drop for OperationLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Check whether `current` semver >= `minimum` semver.
///
/// Simple numeric comparison of major.minor.patch; ignores pre-release tags.
fn version_satisfies(current: &str, minimum: &str) -> bool {
    let parse = |v: &str| -> (u32, u32, u32) {
        let mut parts = v.split('.').map(|s| {
            // Strip pre-release suffix (e.g. "1-beta" → "1")
            s.split('-')
                .next()
                .and_then(|n| n.parse::<u32>().ok())
                .unwrap_or(0)
        });
        let major = parts.next().unwrap_or(0);
        let minor = parts.next().unwrap_or(0);
        let patch = parts.next().unwrap_or(0);
        (major, minor, patch)
    };
    parse(current) >= parse(minimum)
}
