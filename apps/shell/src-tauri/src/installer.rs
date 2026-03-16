//! Installer IPC commands — installation detection, download, extraction, deployment.

use std::io::Read as _;
use std::path::PathBuf;

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::Emitter;

use zeroize::Zeroize;

use crate::autostart;
use crate::utils::{copy_dir_recursive, validate_url, UrlPolicy};

/// Global download cancellation token, managed as Tauri state.
pub struct DownloadCancelState {
    pub token: tokio::sync::Mutex<tokio_util::sync::CancellationToken>,
}

impl Default for DownloadCancelState {
    fn default() -> Self {
        Self {
            token: tokio::sync::Mutex::new(tokio_util::sync::CancellationToken::new()),
        }
    }
}

// ── Types ──

/// Installation status returned to the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct InstallStatus {
    /// Whether `.manifest.json` exists in ATTA_HOME
    pub installed: bool,
    /// ATTA_HOME path
    pub home: String,
    /// Whether `etc/connections.yaml` exists
    pub has_connections: bool,
}

/// Disk space information.
#[derive(Debug, Clone, Serialize)]
pub struct DiskSpace {
    /// Available bytes on the volume
    pub available: u64,
    /// Total bytes on the volume
    pub total: u64,
    /// Whether there is enough space (> 500 MB)
    pub sufficient: bool,
}

/// Download progress event payload.
#[derive(Debug, Clone, Serialize)]
struct DownloadProgress {
    downloaded: u64,
    total: Option<u64>,
    percent: Option<f64>,
}

/// Extract progress event payload.
#[derive(Debug, Clone, Serialize)]
struct ExtractProgress {
    extracted: u64,
    current_file: String,
}

// ── Detection & info commands ──

/// Check the current installation status.
#[tauri::command]
pub fn check_installation() -> InstallStatus {
    let home = autostart::resolve_home();
    let manifest = home.join(".manifest.json");
    let connections = home.join("etc/connections.yaml");
    InstallStatus {
        installed: manifest.exists(),
        home: home.to_string_lossy().to_string(),
        has_connections: connections.exists(),
    }
}

/// Get the default ATTA_HOME path.
#[tauri::command]
pub fn get_default_home() -> String {
    autostart::resolve_home().to_string_lossy().to_string()
}

/// Get current platform identifier (e.g. "darwin-aarch64").
#[tauri::command]
pub fn get_platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// Check available disk space at the given path.
#[tauri::command]
pub fn check_disk_space(path: String) -> DiskSpace {
    // Use `fs2` style statvfs on unix; for now a simple fallback
    let (available, total) = get_disk_space(&path);
    let min_required = 500 * 1024 * 1024; // 500 MB
    DiskSpace {
        available,
        total,
        sufficient: available >= min_required,
    }
}

/// Check whether a directory is writable by creating and removing a temporary file.
#[tauri::command]
pub fn check_writable(path: String) -> Result<bool, String> {
    let dir = PathBuf::from(&path);
    // Ensure directory exists (create if needed)
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return Err(format!("[PERMISSION] Cannot create directory: {e}"));
    }
    let probe = dir.join(".atta_write_probe");
    match std::fs::write(&probe, b"probe") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            Ok(true)
        }
        Err(e) => Err(format!("[PERMISSION] Directory is not writable: {e}")),
    }
}

// ── Install flow commands ──

/// Fetch the component manifest from a URL.
#[tauri::command]
pub async fn fetch_manifest(url: String) -> Result<serde_json::Value, String> {
    validate_url(&url, UrlPolicy::AllowRemote)?;
    let resp = reqwest::get(&url).await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let manifest: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(manifest)
}

/// Cancel an in-progress download.
#[tauri::command]
pub async fn cancel_download(state: tauri::State<'_, DownloadCancelState>) -> Result<(), String> {
    let token = state.token.lock().await;
    token.cancel();
    Ok(())
}

/// Download a component archive, emitting `download-progress` events.
///
/// IPC wrapper that manages cancellation token from Tauri state.
#[tauri::command]
pub async fn download_components(
    app: tauri::AppHandle,
    state: tauri::State<'_, DownloadCancelState>,
    url: String,
    dest: String,
) -> Result<(), String> {
    validate_url(&url, UrlPolicy::AllowRemote)?;
    // Reset cancellation token for this download
    {
        let mut token = state.token.lock().await;
        *token = tokio_util::sync::CancellationToken::new();
    }
    let cancel_token = state.token.lock().await.clone();
    download_components_inner(app, url, dest, Some(cancel_token)).await
}

/// Internal download implementation with retry, timeout, and optional cancellation.
///
/// Retries up to 3 times on failure with exponential backoff (1s, 2s, 4s).
/// Uses a connect timeout of 30 seconds.
pub async fn download_components_inner(
    app: tauri::AppHandle,
    url: String,
    dest: String,
    cancel: Option<tokio_util::sync::CancellationToken>,
) -> Result<(), String> {
    let cancel = cancel.unwrap_or_default();

    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .read_timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| format!("[NETWORK] Failed to create HTTP client: {e}"))?;

    let max_retries = 3u32;
    let mut last_err = String::new();

    for attempt in 0..=max_retries {
        if cancel.is_cancelled() {
            return Err("Download cancelled".to_string());
        }

        if attempt > 0 {
            let delay = std::time::Duration::from_secs(1 << (attempt - 1)); // 1s, 2s, 4s
            let _ = app.emit(
                "download-progress",
                DownloadProgress {
                    downloaded: 0,
                    total: None,
                    percent: None,
                },
            );
            tracing::warn!(attempt, "download retry after {delay:?}");
            tokio::time::sleep(delay).await;
        }

        match download_once(&client, &app, &url, &dest, &cancel).await {
            Ok(()) => return Ok(()),
            Err(e) if e == "Download cancelled" => return Err(e),
            Err(e) => {
                last_err = e;
                if attempt < max_retries {
                    tracing::warn!(attempt, error = %last_err, "download failed, will retry");
                }
            }
        }
    }

    Err(format!("[NETWORK] Download failed after {max_retries} retries: {last_err}"))
}

/// Single download attempt with cancellation support.
async fn download_once(
    client: &reqwest::Client,
    app: &tauri::AppHandle,
    url: &str,
    dest: &str,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<(), String> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("[NETWORK] {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("[NETWORK] HTTP {}", resp.status()));
    }
    let total = resp.content_length();
    let mut downloaded: u64 = 0;

    let dest_path = PathBuf::from(dest);
    let tmp_path = dest_path.with_extension("tmp");
    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("[DISK] {e}"))?;
    }

    let mut file =
        std::fs::File::create(&tmp_path).map_err(|e| format!("[DISK] {e}"))?;
    let mut stream = resp.bytes_stream();
    use futures::StreamExt;
    use std::io::Write;

    // Rate-limit progress events to avoid flooding the frontend
    let mut last_emit = std::time::Instant::now();
    let emit_interval = std::time::Duration::from_millis(200);

    while let Some(chunk) = stream.next().await {
        if cancel.is_cancelled() {
            drop(file);
            let _ = std::fs::remove_file(&tmp_path);
            return Err("Download cancelled".to_string());
        }
        let chunk = chunk.map_err(|e| format!("[NETWORK] {e}"))?;
        file.write_all(&chunk).map_err(|e| format!("[DISK] {e}"))?;
        downloaded += chunk.len() as u64;

        if last_emit.elapsed() >= emit_interval {
            let percent = total.map(|t| {
                if t > 0 {
                    (downloaded as f64 / t as f64 * 100.0).min(100.0)
                } else {
                    0.0
                }
            });

            let _ = app.emit(
                "download-progress",
                DownloadProgress {
                    downloaded,
                    total,
                    percent,
                },
            );
            last_emit = std::time::Instant::now();
        }
    }

    // Emit a final 100% progress event
    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            downloaded,
            total,
            percent: total.map(|_| 100.0),
        },
    );

    // Ensure file is flushed, then atomically move to final destination
    drop(file);
    std::fs::rename(&tmp_path, &dest_path)
        .map_err(|e| format!("[DISK] Failed to finalize download: {e}"))?;

    Ok(())
}

/// Verify a file's SHA-256 hash.
#[tauri::command]
pub fn verify_package(path: String, sha256: String) -> bool {
    match compute_sha256(&path) {
        Ok(hash) => hash.eq_ignore_ascii_case(&sha256),
        Err(_) => false,
    }
}

/// Extract a `.tar.gz` archive, emitting `extract-progress` events.
#[tauri::command]
pub async fn extract_package(
    app: tauri::AppHandle,
    tar_gz: String,
    dest: String,
) -> Result<(), String> {
    let tar_gz = tar_gz.clone();
    let dest = dest.clone();
    let app = app.clone();

    // Run in blocking thread to avoid blocking the async runtime
    tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(&tar_gz).map_err(|e| e.to_string())?;
        let decoder = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(decoder);

        let dest_path = PathBuf::from(&dest);
        std::fs::create_dir_all(&dest_path).map_err(|e| e.to_string())?;
        let dest_canonical = dest_path
            .canonicalize()
            .map_err(|e| format!("cannot canonicalize dest: {e}"))?;

        // Archive extraction limits (zip-bomb protection)
        let max_entries: u64 = 50_000;
        let max_total_bytes: u64 = 1_073_741_824; // 1 GB
        let max_single_file: u64 = 256 * 1024 * 1024; // 256 MB
        let mut total_bytes: u64 = 0;

        for (extracted, entry) in (0_u64..).zip(archive.entries().map_err(|e| e.to_string())?) {
            let mut entry = entry.map_err(|e| e.to_string())?;
            let entry_path = entry
                .path()
                .map_err(|e| e.to_string())?
                .to_path_buf();
            let current_file = entry_path.to_string_lossy().to_string();

            // Security: reject absolute paths
            if entry_path.is_absolute() {
                return Err(format!(
                    "[INTEGRITY] tar entry has absolute path: {current_file}"
                ));
            }

            // Security: reject path traversal (../)
            for component in entry_path.components() {
                if matches!(component, std::path::Component::ParentDir) {
                    return Err(format!(
                        "[INTEGRITY] tar entry contains path traversal: {current_file}"
                    ));
                }
            }

            // Security: reject symlinks pointing outside dest
            if entry.header().entry_type().is_symlink() {
                if let Ok(Some(target)) = entry.link_name() {
                    if target.is_absolute() {
                        return Err(format!(
                            "[INTEGRITY] tar entry has absolute symlink: {current_file}"
                        ));
                    }
                    // Resolve the symlink relative to its parent
                    let resolved = dest_canonical
                        .join(entry_path.parent().unwrap_or(std::path::Path::new("")))
                        .join(&*target);
                    // Normalize without requiring existence
                    let mut normalized = PathBuf::new();
                    for c in resolved.components() {
                        match c {
                            std::path::Component::ParentDir => { normalized.pop(); }
                            std::path::Component::CurDir => {}
                            _ => normalized.push(c),
                        }
                    }
                    if !normalized.starts_with(&dest_canonical) {
                        return Err(format!(
                            "[INTEGRITY] tar symlink escapes destination: {current_file} -> {}",
                            target.display()
                        ));
                    }
                }
            }

            // Extraction limits: entry count
            if extracted >= max_entries {
                return Err(format!(
                    "[INTEGRITY] Archive exceeds maximum entry count ({max_entries})"
                ));
            }

            // [SECURITY] Zip-bomb protection: check entry size from tar headers
            // BEFORE unpacking to prevent decompression bombs from consuming disk.
            let entry_size = entry.header().size().unwrap_or(0);
            if entry_size > max_single_file {
                return Err(format!(
                    "[SECURITY] single entry exceeds maximum size ({max_single_file} bytes): {current_file}"
                ));
            }

            total_bytes = total_bytes.saturating_add(entry_size);
            if total_bytes > max_total_bytes {
                return Err(format!(
                    "[SECURITY] archive exceeds maximum total size ({max_total_bytes} bytes)"
                ));
            }

            // Size checks passed — now safe to unpack
            entry.unpack_in(&dest_canonical).map_err(|e| e.to_string())?;

            let _ = app.emit(
                "extract-progress",
                ExtractProgress {
                    extracted: extracted + 1,
                    current_file,
                },
            );
        }

        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Install extracted components into ATTA_HOME.
///
/// Copies `bin/`, `models/`, `lib/webui/`, `lib/skills/`, `lib/flows/`, `templates/`,
/// from `package_dir` into `home`. Sets executable permission on `bin/*`.
#[tauri::command]
pub fn install_components(package_dir: String, home: String) -> Result<(), String> {
    let src = PathBuf::from(&package_dir)
        .canonicalize()
        .map_err(|e| format!("[PERMISSION] Invalid package directory: {e}"))?;
    let dst = PathBuf::from(&home);
    std::fs::create_dir_all(&dst).map_err(|e| e.to_string())?;
    let dst = dst
        .canonicalize()
        .map_err(|e| format!("[PERMISSION] Invalid home directory: {e}"))?;

    let dirs_to_copy = [
        ("bin", "bin"),
        ("models", "models"),
        ("lib/webui", "lib/webui"),
        ("lib/skills", "lib/skills"),
        ("lib/flows", "lib/flows"),
        ("templates", "templates"),
    ];

    for (src_rel, dst_rel) in &dirs_to_copy {
        let s = src.join(src_rel);
        let d = dst.join(dst_rel);
        if s.exists() {
            copy_dir_recursive(&s, &d).map_err(|e| e.to_string())?;
        }
    }

    // chmod +x on bin/*
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let bin_dir = dst.join("bin");
        if bin_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&bin_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        if let Err(e) = (|| -> std::io::Result<()> {
                            let mut perms = std::fs::metadata(&path)?.permissions();
                            perms.set_mode(0o755);
                            std::fs::set_permissions(&path, perms)
                        })() {
                            tracing::warn!(
                                path = %path.display(),
                                error = %e,
                                "failed to set executable permission"
                            );
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Configuration input from the installer wizard.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ConfigInput {
    /// LLM provider: "auto" | "anthropic" | "openai" | "deepseek"
    pub provider: String,
    /// API key for the provider
    pub api_key: String,
    /// Server listen port
    pub port: u16,
}

/// Generate configuration files in `$ATTA_HOME/etc/`.
///
/// Writes `attaos.yaml` and `keys.env` only when non-default values are provided.
#[tauri::command]
pub fn generate_config(home: String, mut config: ConfigInput) -> Result<(), String> {
    let etc_dir = PathBuf::from(&home).join("etc");
    std::fs::create_dir_all(&etc_dir).map_err(|e| e.to_string())?;

    // Write attaos.yaml if provider or port differ from defaults
    let has_custom_provider = !config.provider.is_empty() && config.provider != "auto";
    let has_custom_port = config.port != 3000;

    if has_custom_provider || has_custom_port {
        let port = if has_custom_port {
            config.port
        } else {
            3000
        };
        let provider = if has_custom_provider {
            &config.provider
        } else {
            "auto"
        };
        let yaml = format!(
            "# AttaOS configuration (generated by installer)\n\
             server:\n  port: {port}\n\n\
             llm:\n  provider: {provider}\n"
        );
        let conf_path = etc_dir.join("attaos.yaml");
        let conf_tmp = etc_dir.join("attaos.yaml.tmp");
        std::fs::write(&conf_tmp, yaml).map_err(|e| e.to_string())?;
        std::fs::rename(&conf_tmp, &conf_path).map_err(|e| e.to_string())?;
    }

    // Write keys.yaml if an API key was provided (encrypted with ChaCha20-Poly1305)
    if !config.api_key.is_empty() {
        let env_var = match config.provider.as_str() {
            "openai" => "OPENAI_API_KEY",
            "deepseek" => "DEEPSEEK_API_KEY",
            "anthropic" => "ANTHROPIC_API_KEY",
            _ => "LLM_API_KEY",
        };
        // Encrypt the API key before writing to disk
        let home_path = std::path::Path::new(&home);
        let encrypted_value =
            match crate::secrets::MasterKey::load_or_create(home_path) {
                Ok(master) => {
                    crate::secrets::encrypt(&master, config.api_key.as_bytes())
                        .unwrap_or_else(|_| config.api_key.clone())
                }
                Err(_) => config.api_key.clone(), // fallback to plaintext
            };
        // Use serde_yaml for safe serialization (prevents YAML injection)
        let mut keys_map = std::collections::BTreeMap::new();
        keys_map.insert(env_var.to_string(), encrypted_value);
        let keys_yaml = serde_yaml::to_string(&keys_map).map_err(|e| e.to_string())?;
        let keys_content = format!("# API keys (generated by installer)\n{keys_yaml}");

        let keys_path = etc_dir.join("keys.yaml");
        let keys_tmp = etc_dir.join("keys.yaml.tmp");
        std::fs::write(&keys_tmp, &keys_content).map_err(|e| e.to_string())?;
        std::fs::rename(&keys_tmp, &keys_path).map_err(|e| e.to_string())?;

        // Restrict file permissions to owner-only on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&keys_path, perms).map_err(|e| e.to_string())?;
        }
    }

    // Zeroize sensitive data before dropping
    config.api_key.zeroize();

    crate::utils::audit_log(
        std::path::Path::new(&home),
        "generate_config",
        "attaos.yaml/keys.yaml",
        "installer configuration generated",
    );

    Ok(())
}

/// Start the attaos server, reusing the autostart logic.
#[tauri::command]
pub async fn start_server(home: String, port: u16) -> Result<u16, String> {
    autostart::ensure_server(std::path::Path::new(&home), port).await
}

/// Write the installation manifest to `$ATTA_HOME/.manifest.json`.
///
/// Validates required fields, then writes atomically (tmp + rename).
#[tauri::command]
pub fn write_manifest(home: String, manifest: serde_json::Value) -> Result<(), String> {
    // Schema validation: must be an object with a version field
    if !manifest.is_object() {
        return Err("[INTEGRITY] Manifest must be a JSON object".to_string());
    }
    if manifest.get("version").and_then(|v| v.as_str()).is_none() {
        return Err("[INTEGRITY] Manifest must contain a 'version' field".to_string());
    }

    let path = PathBuf::from(&home).join(".manifest.json");
    let tmp_path = PathBuf::from(&home).join(".manifest.json.tmp");
    let data = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    std::fs::write(&tmp_path, &data)
        .map_err(|e| format!("[DISK] Failed to write manifest: {e}"))?;
    std::fs::rename(&tmp_path, &path)
        .map_err(|e| format!("[DISK] Failed to finalize manifest: {e}"))
}

/// Clean up the tmp directory under ATTA_HOME.
#[tauri::command]
pub fn cleanup_tmp(home: String) -> Result<(), String> {
    let tmp_dir = PathBuf::from(&home).join("tmp");
    if tmp_dir.exists() {
        std::fs::remove_dir_all(&tmp_dir).map_err(|e| format!("[DISK] Failed to clean tmp: {e}"))?;
    }
    Ok(())
}

/// Open a file selection dialog and return the chosen path.
#[tauri::command]
pub async fn select_file(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let file = app.dialog().file().blocking_pick_file();
    Ok(file.map(|f| f.to_string()))
}

// ── Internal helpers ──

/// Compute SHA-256 hex digest of a file.
fn compute_sha256(path: &str) -> Result<String, std::io::Error> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Extract the first numeric value from a line (for fsutil output parsing).
#[cfg(not(unix))]
fn extract_number(line: &str) -> Option<u64> {
    line.split(':')
        .nth(1)?
        .trim()
        .replace(',', "")
        .replace('.', "")
        .trim()
        .parse()
        .ok()
}

/// Get available and total disk space for the given path.
///
/// Uses platform-specific APIs; returns (0, 0) if unavailable.
fn get_disk_space(path: &str) -> (u64, u64) {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        let c_path = match CString::new(path) {
            Ok(p) => p,
            Err(_) => return (0, 0),
        };
        unsafe {
            let mut stat: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(c_path.as_ptr(), &mut stat) == 0 {
                let available = (stat.f_bavail as u64).saturating_mul(stat.f_frsize);
                let total = (stat.f_blocks as u64).saturating_mul(stat.f_frsize);
                return (available, total);
            }
        }
        (0, 0)
    }
    #[cfg(not(unix))]
    {
        // On Windows, use fsutil to query disk space
        let output = std::process::Command::new("fsutil")
            .args(["volume", "diskfree", path])
            .output();
        match output {
            Ok(out) if out.status.success() => {
                let text = String::from_utf8_lossy(&out.stdout);
                let mut total: u64 = 0;
                let mut avail: u64 = 0;
                for line in text.lines() {
                    let line_lower = line.to_lowercase();
                    // Parse "Total free bytes : 123456789"
                    if line_lower.contains("total free bytes") {
                        if let Some(val) = extract_number(line) {
                            avail = val;
                        }
                    }
                    // Parse "Total bytes : 123456789"
                    if line_lower.contains("total bytes") && !line_lower.contains("free") {
                        if let Some(val) = extract_number(line) {
                            total = val;
                        }
                    }
                }
                (avail, total)
            }
            _ => (0, 0),
        }
    }
}
