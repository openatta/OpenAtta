//! 自动启动 attaos 服务
//!
//! 检测 attaos 服务是否运行，若未运行则自动启动并等待就绪。

use std::path::Path;
use std::time::Duration;

use crate::client::AttaClient;

/// 确保 attaos 服务正在运行
///
/// 1. 尝试健康检查
/// 2. 若失败，spawn attaos 进程（传递 --home 和 --port）
/// 3. 轮询等待服务就绪（最多 15 秒）
pub async fn ensure_server(base_url: &str, port: u16, home: &Path) -> Result<(), anyhow::Error> {
    let client = AttaClient::new(base_url);

    // Already running?
    if client.health().await.unwrap_or(false) {
        return Ok(());
    }

    eprintln!("attaos not running, starting server on port {port}...");

    // Try to find attaos binary
    let bin = find_attaos_binary();
    std::process::Command::new(&bin)
        .arg("--home")
        .arg(home.as_os_str())
        .arg("--port")
        .arg(port.to_string())
        .arg("--skip-update-check")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("failed to spawn attaos: {e} (tried: {bin})"))?;

    // Poll until ready
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if client.health().await.unwrap_or(false) {
            eprintln!("attaos is ready.");
            return Ok(());
        }
    }

    anyhow::bail!("attaos failed to start within 15 seconds")
}

/// Find the attaos binary path
fn find_attaos_binary() -> String {
    // 1. Same directory as current exe
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("attaos");
            if candidate.exists() {
                return candidate.to_string_lossy().to_string();
            }
        }
    }

    // 2. Fall back to PATH
    "attaos".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Mutex to serialize tests that touch the sibling "attaos" binary
    static BINARY_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn find_attaos_binary_returns_sibling_when_exists() {
        let _guard = BINARY_LOCK.lock().unwrap();

        // Place a fake "attaos" binary next to the current test exe
        let exe = std::env::current_exe().expect("should get current exe");
        let dir = exe.parent().expect("exe should have parent dir");
        let fake_bin = dir.join("attaos");

        // Create the fake binary file
        std::fs::write(&fake_bin, b"fake").expect("should create fake binary");

        let result = find_attaos_binary();
        assert_eq!(
            result,
            fake_bin.to_string_lossy().to_string(),
            "should return sibling attaos path"
        );

        // Clean up
        let _ = std::fs::remove_file(&fake_bin);
    }

    #[test]
    fn find_attaos_binary_falls_back_to_path_name() {
        let _guard = BINARY_LOCK.lock().unwrap();

        // If no sibling binary exists, should return bare "attaos"
        let exe = std::env::current_exe().expect("should get current exe");
        let dir = exe.parent().expect("exe should have parent dir");
        let fake_bin = dir.join("attaos");

        // Make sure no sibling binary exists
        let _ = std::fs::remove_file(&fake_bin);

        let result = find_attaos_binary();
        assert_eq!(result, "attaos");
    }
}
