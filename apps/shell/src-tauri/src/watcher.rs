//! File system watcher for configuration hot-reload.
//!
//! Monitors `$ATTA_HOME/etc/` for changes and emits `config-changed` events to the frontend.

use std::path::PathBuf;

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// Payload emitted on the `config-changed` event.
#[derive(Debug, Clone, Serialize)]
struct ConfigChangedPayload {
    file: String,
}

/// Start watching `$ATTA_HOME/etc/` for configuration file changes.
///
/// Debounces events by 500ms and emits `config-changed` to the frontend.
pub fn start_config_watcher(app: AppHandle, home: PathBuf) -> Result<(), String> {
    let etc_dir = home.join("etc");
    if !etc_dir.exists() {
        std::fs::create_dir_all(&etc_dir)
            .map_err(|e| format!("[CONFIG] Cannot create etc directory: {e}"))?;
    }

    let (tx, rx) = std::sync::mpsc::channel::<notify::Result<Event>>();

    let mut watcher = RecommendedWatcher::new(tx, notify::Config::default())
        .map_err(|e| format!("[CONFIG] Cannot create file watcher: {e}"))?;

    watcher
        .watch(&etc_dir, RecursiveMode::NonRecursive)
        .map_err(|e| format!("[CONFIG] Cannot watch etc directory: {e}"))?;

    // Spawn a thread to process file system events (notify uses std channels)
    std::thread::Builder::new()
        .name("config-watcher".into())
        .spawn(move || {
            // Keep watcher alive
            let _watcher = watcher;

            // Debounce: track last emission time per file
            let mut last_emit: std::collections::HashMap<String, std::time::Instant> =
                std::collections::HashMap::new();
            let debounce = std::time::Duration::from_millis(500);

            for event in rx {
                let Ok(event) = event else {
                    continue;
                };

                // Only care about modifications and creations
                if !matches!(
                    event.kind,
                    notify::EventKind::Modify(_) | notify::EventKind::Create(_)
                ) {
                    continue;
                }

                for path in &event.paths {
                    let file_name = path
                        .file_name()
                        .map(|f| f.to_string_lossy().to_string())
                        .unwrap_or_default();

                    // Only watch known config files
                    if !matches!(
                        file_name.as_str(),
                        "attaos.yaml" | "connections.yaml" | "keys.yaml"
                    ) {
                        continue;
                    }

                    // Debounce: skip if we emitted for this file within 500ms
                    let now = std::time::Instant::now();
                    if let Some(last) = last_emit.get(&file_name) {
                        if now.duration_since(*last) < debounce {
                            continue;
                        }
                    }
                    last_emit.insert(file_name.clone(), now);

                    tracing::debug!(file = %file_name, "config file changed");
                    let _ = app.emit(
                        "config-changed",
                        ConfigChangedPayload {
                            file: file_name,
                        },
                    );
                }
            }
            tracing::info!("config watcher thread exiting");
        })
        .map_err(|e| format!("[CONFIG] Cannot spawn watcher thread: {e}"))?;

    Ok(())
}
