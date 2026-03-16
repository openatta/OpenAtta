//! System service registration for attaos auto-start at login.
//!
//! Supports:
//! - macOS: LaunchAgent plist (`~/Library/LaunchAgents/com.attaos.daemon.plist`)
//! - Linux: systemd user service (`~/.config/systemd/user/attaos.service`)
//! - Windows: scheduled task via `schtasks`

use serde::Serialize;

/// Status of the system service.
#[derive(Debug, Clone, Serialize)]
pub struct ServiceStatus {
    /// Whether a service entry is installed
    pub installed: bool,
    /// Whether the service is currently running (best-effort check)
    pub running: bool,
    /// Init system detected
    pub init_system: String,
}

/// Install attaos as a system service that auto-starts at login.
#[tauri::command]
pub fn service_install(home: String) -> Result<(), String> {
    let init = detect_init_system();
    match init.as_str() {
        "launchd" => install_launchd(&home),
        "systemd" => install_systemd(&home),
        "task_scheduler" => install_schtasks(&home),
        _ => Err(format!(
            "[CONFIG] Unsupported init system: {init}. Cannot install auto-start service."
        )),
    }
}

/// Uninstall the auto-start service.
#[tauri::command]
pub fn service_uninstall() -> Result<(), String> {
    let init = detect_init_system();
    match init.as_str() {
        "launchd" => uninstall_launchd(),
        "systemd" => uninstall_systemd(),
        "task_scheduler" => uninstall_schtasks(),
        _ => Err(format!(
            "[CONFIG] Unsupported init system: {init}. Cannot uninstall service."
        )),
    }
}

/// Query the current service status.
#[tauri::command]
pub fn service_status() -> Result<ServiceStatus, String> {
    let init = detect_init_system();
    let (installed, running) = match init.as_str() {
        "launchd" => status_launchd(),
        "systemd" => status_systemd(),
        "task_scheduler" => status_schtasks(),
        _ => (false, false),
    };
    Ok(ServiceStatus {
        installed,
        running,
        init_system: init,
    })
}

// ── Init system detection ──

fn detect_init_system() -> String {
    if cfg!(target_os = "macos") {
        "launchd".to_string()
    } else if cfg!(target_os = "linux") {
        // Check if systemd is available
        if std::path::Path::new("/run/systemd/system").exists() {
            "systemd".to_string()
        } else {
            "unknown".to_string()
        }
    } else if cfg!(target_os = "windows") {
        "task_scheduler".to_string()
    } else {
        "unknown".to_string()
    }
}

// ── macOS: launchd ──

fn launchd_plist_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(home)
        .join("Library/LaunchAgents/com.attaos.daemon.plist")
}

fn install_launchd(atta_home: &str) -> Result<(), String> {
    let bin = find_attaos_bin(atta_home)?;
    let plist_path = launchd_plist_path();
    if let Some(parent) = plist_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("[CONFIG] {e}"))?;
    }

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.attaos.daemon</string>
    <key>ProgramArguments</key>
    <array>
        <string>{bin}</string>
        <string>--home</string>
        <string>{atta_home}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
    <key>StandardOutPath</key>
    <string>{atta_home}/log/attaos.log</string>
    <key>StandardErrorPath</key>
    <string>{atta_home}/log/attaos.log</string>
</dict>
</plist>"#
    );

    std::fs::write(&plist_path, plist).map_err(|e| format!("[CONFIG] Failed to write plist: {e}"))?;

    // Load the agent
    let status = std::process::Command::new("launchctl")
        .args(["load", &plist_path.to_string_lossy()])
        .status()
        .map_err(|e| format!("[CONFIG] Failed to run launchctl: {e}"))?;
    if !status.success() {
        tracing::warn!("launchctl load returned non-zero (may already be loaded)");
    }

    Ok(())
}

fn uninstall_launchd() -> Result<(), String> {
    let plist_path = launchd_plist_path();
    if plist_path.exists() {
        let _ = std::process::Command::new("launchctl")
            .args(["unload", &plist_path.to_string_lossy()])
            .status();
        std::fs::remove_file(&plist_path).map_err(|e| format!("[CONFIG] {e}"))?;
    }
    Ok(())
}

fn status_launchd() -> (bool, bool) {
    let plist_path = launchd_plist_path();
    let installed = plist_path.exists();
    let running = if installed {
        std::process::Command::new("launchctl")
            .args(["list", "com.attaos.daemon"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    } else {
        false
    };
    (installed, running)
}

// ── Linux: systemd ──

fn systemd_service_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(home)
        .join(".config/systemd/user/attaos.service")
}

fn install_systemd(atta_home: &str) -> Result<(), String> {
    let bin = find_attaos_bin(atta_home)?;
    let service_path = systemd_service_path();
    if let Some(parent) = service_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("[CONFIG] {e}"))?;
    }

    let unit = format!(
        "[Unit]\n\
         Description=AttaOS Server\n\
         After=network.target\n\n\
         [Service]\n\
         Type=simple\n\
         ExecStart={bin} --home {atta_home}\n\
         Restart=on-failure\n\
         RestartSec=5\n\n\
         [Install]\n\
         WantedBy=default.target\n"
    );

    std::fs::write(&service_path, unit)
        .map_err(|e| format!("[CONFIG] Failed to write service file: {e}"))?;

    // Enable the service
    let status = std::process::Command::new("systemctl")
        .args(["--user", "enable", "attaos.service"])
        .status()
        .map_err(|e| format!("[CONFIG] Failed to run systemctl: {e}"))?;
    if !status.success() {
        return Err("[CONFIG] systemctl --user enable failed".into());
    }

    // Start the service
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "start", "attaos.service"])
        .status();

    Ok(())
}

fn uninstall_systemd() -> Result<(), String> {
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "stop", "attaos.service"])
        .status();
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "disable", "attaos.service"])
        .status();
    let service_path = systemd_service_path();
    if service_path.exists() {
        std::fs::remove_file(&service_path).map_err(|e| format!("[CONFIG] {e}"))?;
    }
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();
    Ok(())
}

fn status_systemd() -> (bool, bool) {
    let installed = systemd_service_path().exists();
    let running = if installed {
        std::process::Command::new("systemctl")
            .args(["--user", "is-active", "--quiet", "attaos.service"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        false
    };
    (installed, running)
}

// ── Windows: Task Scheduler ──

fn install_schtasks(atta_home: &str) -> Result<(), String> {
    let bin = find_attaos_bin(atta_home)?;
    let status = std::process::Command::new("schtasks")
        .args([
            "/CREATE",
            "/TN",
            "AttaOS",
            "/TR",
            &format!("\"{bin}\" --home \"{atta_home}\""),
            "/SC",
            "ONLOGON",
            "/RL",
            "LIMITED",
            "/F",
        ])
        .status()
        .map_err(|e| format!("[CONFIG] Failed to run schtasks: {e}"))?;
    if !status.success() {
        return Err("[CONFIG] schtasks /CREATE failed".into());
    }
    Ok(())
}

fn uninstall_schtasks() -> Result<(), String> {
    let status = std::process::Command::new("schtasks")
        .args(["/DELETE", "/TN", "AttaOS", "/F"])
        .status()
        .map_err(|e| format!("[CONFIG] Failed to run schtasks: {e}"))?;
    if !status.success() {
        tracing::warn!("schtasks /DELETE returned non-zero (may not exist)");
    }
    Ok(())
}

fn status_schtasks() -> (bool, bool) {
    let output = std::process::Command::new("schtasks")
        .args(["/QUERY", "/TN", "AttaOS", "/FO", "CSV", "/NH"])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout).to_lowercase();
            let installed = text.contains("attaos");
            let running = text.contains("running");
            (installed, running)
        }
        _ => (false, false),
    }
}

// ── Helpers ──

fn find_attaos_bin(atta_home: &str) -> Result<String, String> {
    let bin_name = if cfg!(windows) {
        "attaos.exe"
    } else {
        "attaos"
    };
    let in_home = std::path::Path::new(atta_home).join("bin").join(bin_name);
    if in_home.exists() {
        return Ok(in_home.to_string_lossy().to_string());
    }
    Err(format!(
        "[CONFIG] attaos binary not found at {}",
        in_home.display()
    ))
}
