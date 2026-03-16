//! 自动启动 attaos 服务
//!
//! 检测 attaos 服务是否运行，若未运行则自动启动并等待就绪。

use std::time::Duration;

/// Resolve ATTA_HOME from env, defaulting to ~/.atta
pub fn resolve_home() -> std::path::PathBuf {
    if let Ok(env_home) = std::env::var("ATTA_HOME") {
        return std::path::PathBuf::from(env_home);
    }
    // HOME on Unix, USERPROFILE on Windows
    let user_home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    user_home.join(".atta")
}

/// 确保 attaos 服务正在运行，返回实际使用的端口
///
/// `home` specifies ATTA_HOME; the path is passed as `--home` to the spawned process
/// rather than via environment variable (which is UB in multi-threaded Rust).
pub async fn ensure_server(home: &std::path::Path, port: u16) -> Result<u16, String> {
    let url = format!("http://localhost:{port}/api/v1/health");

    // Already running?
    if health_check(&url).await {
        return Ok(port);
    }

    tracing::info!(port, "attaos not running, starting server...");

    // Clean up leftover tmp from previous failed operations
    let tmp_dir = home.join("tmp");
    if tmp_dir.exists() {
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    let bin = find_attaos_binary(home)?;

    // Redirect stdout/stderr to log file for diagnostics
    let log_dir = home.join("log");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("attaos.log");
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .ok();
    let stdout = log_file
        .as_ref()
        .and_then(|f| f.try_clone().ok())
        .map(std::process::Stdio::from)
        .unwrap_or_else(std::process::Stdio::null);
    let stderr = log_file
        .and_then(|f| f.try_clone().ok())
        .map(std::process::Stdio::from)
        .unwrap_or_else(std::process::Stdio::null);

    let safe_env = sanitize_env();
    std::process::Command::new(&bin)
        .env_clear()
        .envs(safe_env)
        .arg("--home")
        .arg(home.as_os_str())
        .arg("--port")
        .arg(port.to_string())
        .arg("--skip-update-check")
        .stdin(std::process::Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
        .map_err(|e| format!("failed to spawn attaos: {e} (tried: {bin})"))?;

    // Poll until ready (90 × 500ms = 45s to cover fastembed model-load timeout)
    for _ in 0..90 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if health_check(&url).await {
            tracing::info!(port, "attaos is ready");
            return Ok(port);
        }
    }

    Err("attaos failed to start within 45 seconds".to_string())
}

/// Watchdog: periodically checks if the server is alive and restarts it if crashed.
///
/// Uses exponential backoff between restart attempts and gives up after 5 consecutive
/// restart failures. Emits `server-crashed` events for frontend notification.
pub async fn start_watchdog(app: tauri::AppHandle, home: std::path::PathBuf, port: u16) {
    use tauri::Emitter;
    let url = format!("http://localhost:{port}/api/v1/health");
    let mut consecutive_failures = 0u32;
    let mut restart_count = 0u32;
    let max_restarts: u32 = 5;
    let base_interval = Duration::from_secs(30);

    loop {
        // Exponential backoff after restart failures
        let interval = if restart_count > 0 {
            base_interval * 2u32.pow(restart_count.min(4))
        } else {
            base_interval
        };
        tokio::time::sleep(interval).await;

        if health_check(&url).await {
            consecutive_failures = 0;
            // Gradually reduce restart count on sustained health
            restart_count = restart_count.saturating_sub(1);
            continue;
        }

        consecutive_failures += 1;
        // Require 2 consecutive failures to avoid false positives during brief restarts
        if consecutive_failures < 2 {
            continue;
        }

        if restart_count >= max_restarts {
            tracing::error!(
                "watchdog: max restart limit ({max_restarts}) reached, stopping watchdog"
            );
            let _ = app.emit(
                "server-crashed",
                "attaos server crashed repeatedly — watchdog stopped",
            );
            break;
        }

        tracing::warn!(
            attempt = restart_count + 1,
            max = max_restarts,
            "watchdog: server unresponsive, attempting restart"
        );
        let _ = app.emit("server-crashed", "attaos server crashed, restarting...");

        match ensure_server(&home, port).await {
            Ok(p) => {
                tracing::info!(port = p, "watchdog: server restarted successfully");
                consecutive_failures = 0;
                restart_count += 1;
            }
            Err(e) => {
                tracing::error!(error = %e, "watchdog: failed to restart server");
                restart_count += 1;
            }
        }
    }
}

/// Check if the server is alive, optionally with auth.
pub async fn health_check(url: &str) -> bool {
    health_check_with_auth(url, None).await
}

/// Check if the server is alive with optional Authorization header.
pub async fn health_check_with_auth(url: &str, auth: Option<&str>) -> bool {
    let client = match crate::http_client::build_client(None, None) {
        Ok(c) => c,
        Err(_) => return false,
    };
    crate::http_client::authed_get(&client, url, auth)
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Filter environment variables to only pass safe ones to the spawned server process.
///
/// Allow-list: HOME, USER, PATH, LANG, LC_*, TERM, ATTA_HOME, ATTA_PORT, XDG_*, TMPDIR, TMP, TEMP.
/// Reject-list: AWS_*, GITHUB_*, OPENAI_*, ANTHROPIC_*, *_TOKEN, *_SECRET, *_KEY (except ATTA_*).
fn sanitize_env() -> Vec<(String, String)> {
    std::env::vars()
        .filter(|(k, _)| is_env_allowed(k))
        .collect()
}

fn is_env_allowed(key: &str) -> bool {
    let upper = key.to_uppercase();

    // Only allow specific ATTA_* variables, not all (to prevent leaking
    // ATTA_*_KEY or other sensitive ATTA-prefixed vars to child processes)
    if upper.starts_with("ATTA_") {
        return matches!(
            upper.as_str(),
            "ATTA_HOME" | "ATTA_PORT" | "ATTA_LOG" | "ATTA_LOG_LEVEL" | "ATTA_DATA_DIR"
        );
    }

    // Reject known sensitive prefixes
    if upper.starts_with("AWS_")
        || upper.starts_with("GITHUB_")
        || upper.starts_with("OPENAI_")
        || upper.starts_with("ANTHROPIC_")
        || upper.starts_with("DEEPSEEK_")
    {
        return false;
    }

    // Reject sensitive suffixes
    if upper.ends_with("_TOKEN")
        || upper.ends_with("_SECRET")
        || upper.ends_with("_KEY")
        || upper.ends_with("_PASSWORD")
    {
        return false;
    }

    // Allow-list of safe variables
    matches!(
        upper.as_str(),
        "HOME"
            | "USER"
            | "USERNAME"
            | "PATH"
            | "LANG"
            | "TERM"
            | "TMPDIR"
            | "TMP"
            | "TEMP"
            | "SHELL"
            | "LOGNAME"
            | "DISPLAY"
            | "WAYLAND_DISPLAY"
    ) || upper.starts_with("LC_")
        || upper.starts_with("XDG_")
}

fn find_attaos_binary(home: &std::path::Path) -> Result<String, String> {
    // 1. ATTA_HOME/bin/attaos
    let bin_name = if cfg!(windows) { "attaos.exe" } else { "attaos" };
    let in_home = home.join("bin").join(bin_name);
    if in_home.exists() {
        return Ok(in_home.to_string_lossy().to_string());
    }

    // 2. Same directory as shell executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(bin_name);
            if candidate.exists() {
                return Ok(candidate.to_string_lossy().to_string());
            }
        }
    }

    // 3. Check PATH
    let which_cmd = if cfg!(unix) { "which" } else { "where" };
    if let Ok(output) = std::process::Command::new(which_cmd).arg("attaos").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(path);
            }
        }
    }

    Err(format!(
        "attaos binary not found. Searched: {}, shell directory, and PATH",
        in_home.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── is_env_allowed ──

    #[test]
    fn allows_safe_system_vars() {
        assert!(is_env_allowed("HOME"));
        assert!(is_env_allowed("PATH"));
        assert!(is_env_allowed("USER"));
        assert!(is_env_allowed("LANG"));
        assert!(is_env_allowed("TERM"));
        assert!(is_env_allowed("SHELL"));
        assert!(is_env_allowed("TMPDIR"));
        assert!(is_env_allowed("DISPLAY"));
    }

    #[test]
    fn allows_specific_atta_vars() {
        assert!(is_env_allowed("ATTA_HOME"));
        assert!(is_env_allowed("ATTA_PORT"));
        assert!(is_env_allowed("ATTA_LOG"));
        assert!(is_env_allowed("ATTA_LOG_LEVEL"));
        assert!(is_env_allowed("ATTA_DATA_DIR"));
    }

    #[test]
    fn rejects_sensitive_atta_vars() {
        assert!(!is_env_allowed("ATTA_KEY"));
        assert!(!is_env_allowed("ATTA_SECRET"));
        assert!(!is_env_allowed("ATTA_API_KEY"));
    }

    #[test]
    fn allows_lc_and_xdg_prefixed() {
        assert!(is_env_allowed("LC_ALL"));
        assert!(is_env_allowed("LC_CTYPE"));
        assert!(is_env_allowed("XDG_DATA_HOME"));
        assert!(is_env_allowed("XDG_RUNTIME_DIR"));
    }

    #[test]
    fn rejects_cloud_provider_keys() {
        assert!(!is_env_allowed("AWS_ACCESS_KEY_ID"));
        assert!(!is_env_allowed("AWS_SECRET_ACCESS_KEY"));
        assert!(!is_env_allowed("GITHUB_TOKEN"));
    }

    #[test]
    fn rejects_ai_provider_keys() {
        assert!(!is_env_allowed("OPENAI_API_KEY"));
        assert!(!is_env_allowed("ANTHROPIC_API_KEY"));
        assert!(!is_env_allowed("DEEPSEEK_API_KEY"));
    }

    #[test]
    fn rejects_sensitive_suffixes() {
        assert!(!is_env_allowed("MY_TOKEN"));
        assert!(!is_env_allowed("DB_SECRET"));
        assert!(!is_env_allowed("STRIPE_KEY"));
        assert!(!is_env_allowed("DB_PASSWORD"));
    }

    #[test]
    fn rejects_unknown_vars() {
        assert!(!is_env_allowed("RANDOM_THING"));
        assert!(!is_env_allowed("MY_CUSTOM_VAR"));
    }

    #[test]
    fn case_insensitive() {
        assert!(is_env_allowed("home"));
        assert!(is_env_allowed("Home"));
        assert!(!is_env_allowed("openai_api_key"));
    }

    // ── resolve_home ──

    #[test]
    fn resolve_home_returns_path() {
        let home = resolve_home();
        // Should end with ".atta" or be from ATTA_HOME env
        assert!(
            home.to_string_lossy().contains(".atta")
                || std::env::var("ATTA_HOME").is_ok()
        );
    }
}
