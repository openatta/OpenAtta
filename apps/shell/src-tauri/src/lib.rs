//! AttaOS Shell — 桌面 Shell（Tauri WebView + 系统托盘 + 自动更新）
//!
//! 启动时自动拉起 attaos 服务，将 WebView 指向服务地址。
//! 关闭窗口时隐藏到系统托盘而非退出。
//! 内置更新器，通过第二窗口检查并安装更新。

mod autostart;
mod connection;
mod estop;
mod http_client;
mod installer;
mod manager;
mod secrets;
mod service;
mod tunnel;
mod utils;
mod watcher;

use serde::Serialize;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Emitter, Manager, WindowEvent,
};
use tauri_plugin_updater::UpdaterExt;
use tokio::sync::Mutex;
use tracing::{info, warn};

// ── Updater types ──

/// 更新信息返回给前端
#[derive(Debug, Clone, Serialize)]
struct UpdateInfo {
    current_version: String,
    version: String,
    body: String,
}

/// 下载进度事件（发送给前端）
#[derive(Debug, Clone, Serialize)]
struct UpdateProgress {
    /// 已下载字节数
    downloaded: u64,
    /// 总字节数（可能为 0 表示未知）
    total: Option<u64>,
    /// 下载百分比（0-100），total 未知时为 None
    percent: Option<f64>,
}

/// 全局缓存的更新信息
struct CachedUpdate {
    info: Option<UpdateInfo>,
}

/// 应用状态
struct AppUpdateState {
    cached: Mutex<CachedUpdate>,
}

/// 检测错误消息是否为签名验证相关错误
fn is_signature_error(err_msg: &str) -> bool {
    let lower = err_msg.to_lowercase();
    lower.contains("signature")
        || lower.contains("pubkey")
        || lower.contains("public key")
        || lower.contains("signing")
        || lower.contains("verify")
}

// ── IPC commands ──

/// 显示主窗口
#[tauri::command]
fn show_window(app: tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        window.show().ok();
        window.set_focus().ok();
    }
}

/// 检查是否有新版本可用
#[tauri::command]
async fn check_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppUpdateState>,
) -> Result<Option<UpdateInfo>, String> {
    let updater = app.updater().map_err(|e| e.to_string())?;

    let update = match updater.check().await {
        Ok(update) => update,
        Err(e) => {
            let err_msg = e.to_string();
            if is_signature_error(&err_msg) {
                warn!(
                    error = %err_msg,
                    "update signature verification failed — \
                     skipping in development mode. Generate a real signing key with: \
                     tauri signer generate -w ~/.tauri/attaos.key"
                );
                if cfg!(debug_assertions) {
                    return Ok(None);
                }
            }
            return Err(err_msg);
        }
    };

    match update {
        Some(update) => {
            let current = app.package_info().version.to_string();
            let update_info = UpdateInfo {
                current_version: current,
                version: update.version.clone(),
                body: update.body.clone().unwrap_or_default(),
            };

            let mut cached = state.cached.lock().await;
            cached.info = Some(update_info.clone());

            Ok(Some(update_info))
        }
        None => {
            let mut cached = state.cached.lock().await;
            cached.info = None;
            Ok(None)
        }
    }
}

/// 下载并安装更新，通过事件报告进度
#[tauri::command]
async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    let updater = app.updater().map_err(|e| e.to_string())?;

    let update = match updater.check().await {
        Ok(Some(update)) => update,
        Ok(None) => return Err("no update available".to_string()),
        Err(e) => {
            let err_msg = e.to_string();
            if is_signature_error(&err_msg) && cfg!(debug_assertions) {
                warn!(
                    error = %err_msg,
                    "update signature verification failed during install — \
                     skipping in development mode"
                );
                return Err("update signature verification failed (development build — \
                     generate a signing key with `tauri signer generate`)"
                    .to_string());
            }
            return Err(err_msg);
        }
    };

    info!(version = %update.version, "downloading update");

    let app_handle = app.clone();
    let cumulative = std::cell::Cell::new(0u64);

    match update
        .download_and_install(
            move |chunk_len, content_len| {
                let downloaded = cumulative.get() + chunk_len as u64;
                cumulative.set(downloaded);
                let total = content_len;
                let percent = total.map(|t| {
                    if t > 0 {
                        (downloaded as f64 / t as f64 * 100.0).min(100.0)
                    } else {
                        0.0
                    }
                });

                let progress = UpdateProgress {
                    downloaded,
                    total,
                    percent,
                };

                let _ = app_handle.emit("update-progress", &progress);
            },
            || {
                info!("update download complete, ready to install");
            },
        )
        .await
    {
        Ok(()) => {
            info!("update installed successfully");
            Ok(())
        }
        Err(e) => {
            let err_msg = e.to_string();
            if is_signature_error(&err_msg) && cfg!(debug_assertions) {
                warn!(
                    error = %err_msg,
                    "update signature verification failed during download/install — \
                     skipping in development mode"
                );
                return Err("update signature verification failed (development build — \
                     generate a signing key with `tauri signer generate`)"
                    .to_string());
            }
            Err(err_msg)
        }
    }
}

// ── Helpers ──

/// Load the default connection URL and name from `connections.yaml`.
fn load_default_connection(
    path: &std::path::Path,
) -> Result<(String, Option<String>), String> {
    let data = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let yaml: serde_json::Value = serde_yaml::from_str(&data).map_err(|e| e.to_string())?;
    let conn = yaml["connections"]
        .as_array()
        .and_then(|arr| arr.iter().find(|c| c["default"].as_bool() == Some(true)))
        .or_else(|| yaml["connections"].as_array().and_then(|a| a.first()))
        .ok_or_else(|| "no connection found".to_string())?;
    let url = conn["url"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| "connection has no URL".to_string())?;
    let name = conn["name"].as_str().map(String::from);
    Ok((url, name))
}

// ── App entry ──

/// 运行 Tauri 应用
pub fn run() {
    let port: u16 = std::env::var("ATTA_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);

    tracing::info!(port, "AttaOS Shell starting");

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppUpdateState {
            cached: Mutex::new(CachedUpdate { info: None }),
        })
        .manage(installer::DownloadCancelState::default())
        .manage(std::sync::Arc::new(tunnel::TunnelRegistry::default()))
        .invoke_handler(tauri::generate_handler![
            show_window,
            check_update,
            install_update,
            // installer
            installer::check_installation,
            installer::get_default_home,
            installer::get_platform,
            installer::check_disk_space,
            installer::check_writable,
            installer::fetch_manifest,
            installer::download_components,
            installer::verify_package,
            installer::extract_package,
            installer::install_components,
            installer::start_server,
            installer::write_manifest,
            installer::generate_config,
            installer::select_file,
            installer::cancel_download,
            installer::cleanup_tmp,
            // connection
            connection::test_connection,
            connection::save_connection,
            connection::load_connections,
            connection::remove_connection,
            // manager
            manager::read_manifest,
            manager::verify_installation,
            manager::backup_current,
            manager::rollback,
            manager::stop_server,
            manager::check_component_update,
            manager::upgrade_components,
            manager::restart_server,
            manager::server_status,
            manager::read_server_log,
            // service
            service::service_install,
            service::service_uninstall,
            service::service_status,
            // estop
            estop::estop_status,
            estop::estop_engage,
            estop::estop_resume,
            // tunnel
            tunnel::start_tunnel,
            tunnel::stop_tunnel,
        ])
        .setup(move |app| {
            // Build tray menu
            let status_item =
                MenuItem::with_id(app, "server_status", "Server: Starting...", false, None::<&str>)?;
            let console_item =
                MenuItem::with_id(app, "console", "Open Console", true, None::<&str>)?;
            let restart_item =
                MenuItem::with_id(app, "restart_server", "Restart Server", true, None::<&str>)?;
            let estop_item =
                MenuItem::with_id(app, "estop", "E-Stop: Normal", true, None::<&str>)?;
            let autostart_item =
                MenuItem::with_id(app, "auto_start", "Auto-Start: Checking...", true, None::<&str>)?;
            let update_item =
                MenuItem::with_id(app, "check_updates", "Check Updates", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[&status_item, &estop_item, &console_item, &restart_item, &autostart_item, &update_item, &quit_item],
            )?;

            // Build tray icon
            let tray_icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-icon@2x.png"))
                .expect("failed to load tray icon");
            let _tray = TrayIconBuilder::new()
                .icon(tray_icon)
                .menu(&menu)
                .tooltip("AttaOS")
                .on_menu_event(move |app, event| match event.id.as_ref() {
                    "console" => {
                        if let Some(window) = app.get_webview_window("main") {
                            window.show().ok();
                            window.set_focus().ok();
                        }
                    }
                    "restart_server" => {
                        tracing::info!("restart server requested from tray");
                        let app_clone = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let home = autostart::resolve_home()
                                .to_string_lossy()
                                .to_string();
                            match manager::restart_server(home).await {
                                Ok(p) => {
                                    tracing::info!(port = p, "server restarted from tray");
                                    if let Err(e) = app_clone.emit("server-restarted", p) {
                                        tracing::warn!(event = "server-restarted", error = %e, "failed to emit event to frontend");
                                    }
                                }
                                Err(e) => {
                                    tracing::error!(error = %e, "tray restart failed");
                                    if let Err(e) = app_clone.emit("server-crashed", e.to_string()) {
                                        tracing::warn!(event = "server-crashed", error = %e, "failed to emit event to frontend");
                                    }
                                }
                            }
                        });
                    }
                    "estop" => {
                        // Toggle kill_all E-Stop
                        let home = autostart::resolve_home()
                            .to_string_lossy()
                            .to_string();
                        match estop::estop_status(home.clone()) {
                            Ok(state) if state.kill_all => {
                                let _ = estop::estop_resume(home, "kill_all".into());
                            }
                            _ => {
                                let _ = estop::estop_engage(home, "kill_all".into());
                            }
                        }
                    }
                    "auto_start" => {
                        tracing::info!("auto-start toggle requested from tray");
                        match service::service_status() {
                            Ok(status) if status.installed => {
                                if let Err(e) = service::service_uninstall() {
                                    tracing::error!(error = %e, "failed to uninstall service");
                                }
                            }
                            _ => {
                                let home = autostart::resolve_home()
                                    .to_string_lossy()
                                    .to_string();
                                if let Err(e) = service::service_install(home) {
                                    tracing::error!(error = %e, "failed to install service");
                                }
                            }
                        }
                    }
                    "check_updates" => {
                        tracing::info!("check updates requested — opening updater window");
                        if let Some(window) = app.get_webview_window("updater") {
                            window.show().ok();
                            window.set_focus().ok();
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            // Periodic tray status updater
            // NOTE: Race condition exists between PID file check and health endpoint query.
            // If server crashes between checks, status may briefly show stale data.
            // This is acceptable for a 15-second polling UI indicator.
            let tray_status_item = status_item.clone();
            let tray_estop_item = estop_item.clone();
            let tray_autostart_item = autostart_item.clone();
            let tray_port = port;
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                    let home = autostart::resolve_home().to_string_lossy().to_string();
                    let text = match manager::server_status(home).await {
                        Ok(s) if s.running => {
                            format!("Server: Running (port {})", s.port.unwrap_or(tray_port))
                        }
                        _ => "Server: Stopped".to_string(),
                    };
                    let _ = tray_status_item.set_text(&text);

                    // Update E-Stop label
                    let home2 = autostart::resolve_home().to_string_lossy().to_string();
                    let estop_text = match estop::estop_status(home2) {
                        Ok(s) if s.kill_all => "E-Stop: ENGAGED (Kill All)",
                        Ok(s) if s.network_kill => "E-Stop: ENGAGED (Network)",
                        _ => "E-Stop: Normal",
                    };
                    let _ = tray_estop_item.set_text(estop_text);

                    // Update auto-start label
                    let autostart_text = match service::service_status() {
                        Ok(s) if s.installed => "Auto-Start: Enabled",
                        _ => "Auto-Start: Disabled",
                    };
                    let _ = tray_autostart_item.set_text(autostart_text);
                }
            });

            // Three-state startup detection
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let home = autostart::resolve_home();
                let manifest_path = home.join(".manifest.json");
                let connections_path = home.join("etc/connections.yaml");

                // Start config watcher for all modes
                if let Err(e) = watcher::start_config_watcher(app_handle.clone(), home.clone()) {
                    tracing::warn!(error = %e, "failed to start config watcher");
                }

                if manifest_path.exists() {
                    // ── Installed mode: start attaos → navigate WebUI ──
                    match autostart::ensure_server(&home, port).await {
                        Ok(p) => {
                            let url = format!("http://localhost:{p}");
                            tracing::info!(%url, "navigating WebView to AttaOS");
                            if let Some(w) = app_handle.get_webview_window("main") {
                                if let Ok(parsed) = url.parse() {
                                    w.navigate(parsed).ok();
                                } else {
                                    tracing::error!(%url, "invalid URL for WebView navigation");
                                }
                                w.show().ok();
                            }
                            // Start watchdog to auto-restart if server crashes
                            let wd_handle = app_handle.clone();
                            let wd_home = home.clone();
                            tokio::spawn(autostart::start_watchdog(wd_handle, wd_home, p));
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "failed to start attaos");
                            // Show installer for recovery
                            if let Some(w) = app_handle.get_webview_window("main") {
                                if let Ok(parsed) = "/installer.html".parse() {
                                    w.navigate(parsed).ok();
                                }
                                w.show().ok();
                            }
                        }
                    }
                } else if connections_path.exists() {
                    // ── Remote connection mode: navigate to remote URL ──
                    match load_default_connection(&connections_path) {
                        Ok((mut url, conn_name)) => {
                            // If SSH tunnel is enabled, establish tunnel first
                            if let Some(ref name) = conn_name {
                                let conns = connection::load_connections(
                                    home.to_string_lossy().to_string(),
                                )
                                .unwrap_or_default();
                                if let Some(conn) = conns.iter().find(|c| &c.name == name) {
                                    if conn.ssh_enabled {
                                        if let (Some(target), remote_port) = (
                                            conn.ssh_target.as_deref(),
                                            conn.ssh_remote_port.unwrap_or(3000),
                                        ) {
                                            match tunnel::SshTunnel::start(
                                                target,
                                                remote_port,
                                                conn.ssh_identity.as_deref(),
                                            )
                                            .await
                                            {
                                                Ok(tun) => {
                                                    url = format!(
                                                        "http://localhost:{}",
                                                        tun.local_port()
                                                    );
                                                    tracing::info!(
                                                        local_port = tun.local_port(),
                                                        "SSH tunnel established for remote connection"
                                                    );
                                                    // Store tunnel in registry to keep it alive
                                                    let registry: tauri::State<'_, std::sync::Arc<tunnel::TunnelRegistry>> = app_handle.state();
                                                    registry.insert(name.clone(), tun).await;
                                                }
                                                Err(e) => {
                                                    tracing::error!(
                                                        error = %e,
                                                        "failed to establish SSH tunnel"
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // If connection has an api_key, check health with auth
                            let api_key = conn_name.as_deref().and_then(|n| {
                                connection::decrypt_connection_secret(&home, n)
                            });
                            if let Some(ref key) = api_key {
                                let health_url = format!(
                                    "{}/api/v1/health",
                                    url.trim_end_matches('/')
                                );
                                if !autostart::health_check_with_auth(&health_url, Some(key))
                                    .await
                                {
                                    tracing::warn!(
                                        "remote server health check failed (with auth)"
                                    );
                                }
                            }
                            tracing::info!(%url, "navigating WebView to remote server");
                            if let Some(w) = app_handle.get_webview_window("main") {
                                if let Ok(parsed) = url.parse() {
                                    w.navigate(parsed).ok();
                                } else {
                                    tracing::error!(%url, "invalid connection URL");
                                }
                                w.show().ok();
                            }
                        }
                        Err(e) => tracing::error!(error = %e, "failed to load connection"),
                    }
                } else {
                    // ── Not installed: show installer wizard ──
                    tracing::info!("no installation detected, showing installer");
                    if let Some(w) = app_handle.get_webview_window("main") {
                        if let Ok(parsed) = "/installer.html".parse() {
                            w.navigate(parsed).ok();
                        }
                        w.show().ok();
                    }
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Only hide-to-tray for the main window; let other windows close normally
                if window.label() == "main" {
                    api.prevent_close();
                    window.hide().ok();
                    tracing::info!("main window hidden to tray");
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error running AttaOS Shell");
}
