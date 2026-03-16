//! SSH tunnel management for remote attaos connections.
//!
//! Establishes an SSH port-forward (`ssh -N -L`) to a remote attaos server.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use crate::autostart;
use crate::connection;

/// An active SSH tunnel.
pub struct SshTunnel {
    child: Child,
    local_port: u16,
}

impl SshTunnel {
    /// Start an SSH tunnel forwarding a local port to a remote attaos server.
    pub async fn start(
        target: &str,
        remote_port: u16,
        identity: Option<&str>,
    ) -> Result<Self, String> {
        let local_port = find_free_port()
            .map_err(|e| format!("[TUNNEL] Cannot find free local port: {e}"))?;

        let forward = format!("{local_port}:localhost:{remote_port}");
        let mut cmd = Command::new("ssh");
        cmd.arg("-N") // No remote command
            .arg("-L")
            .arg(&forward)
            .arg("-o")
            .arg("StrictHostKeyChecking=accept-new")
            .arg("-o")
            .arg("ConnectTimeout=10")
            .arg("-o")
            .arg("ServerAliveInterval=30")
            .arg("-o")
            .arg("ServerAliveCountMax=3")
            .arg("-o")
            .arg("ExitOnForwardFailure=yes");

        if let Some(key_path) = identity {
            let key = std::path::Path::new(key_path);
            if !key.exists() {
                return Err(format!("SSH identity file not found: {key_path}"));
            }
            cmd.arg("-i").arg(key_path);
        }
        cmd.arg(target);

        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::piped());

        let child = cmd
            .spawn()
            .map_err(|e| format!("[TUNNEL] Failed to spawn ssh: {e}"))?;

        // Wait for the port to become available (up to 10s)
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(10);
        loop {
            if start.elapsed() > timeout {
                return Err("[TUNNEL] SSH tunnel failed to establish within 10 seconds".into());
            }
            if is_port_open(local_port).await {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        tracing::info!(local_port, %target, remote_port, "SSH tunnel established");
        Ok(Self { child, local_port })
    }

    /// Stop the SSH tunnel.
    pub async fn stop(&mut self) {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
        tracing::info!(local_port = self.local_port, "SSH tunnel stopped");
    }

    /// Get the local port of the tunnel.
    pub fn local_port(&self) -> u16 {
        self.local_port
    }
}

/// Global tunnel registry.
pub struct TunnelRegistry {
    tunnels: Mutex<HashMap<String, SshTunnel>>,
}

impl Default for TunnelRegistry {
    fn default() -> Self {
        Self {
            tunnels: Mutex::new(HashMap::new()),
        }
    }
}

impl TunnelRegistry {
    /// Insert a tunnel into the registry, stopping any existing tunnel for the same name.
    pub async fn insert(&self, name: String, tunnel: SshTunnel) {
        let mut tunnels = self.tunnels.lock().await;
        if let Some(mut old) = tunnels.remove(&name) {
            old.stop().await;
        }
        tunnels.insert(name, tunnel);
    }
}

/// Start a tunnel for a named connection.
#[tauri::command]
pub async fn start_tunnel(
    state: tauri::State<'_, Arc<TunnelRegistry>>,
    home: String,
    connection_name: String,
) -> Result<u16, String> {
    let home_path = if home.is_empty() {
        autostart::resolve_home()
    } else {
        std::path::PathBuf::from(&home)
    };

    let connections = connection::load_connections(home)?;
    let conn = connections
        .iter()
        .find(|c| c.name == connection_name)
        .ok_or_else(|| format!("[TUNNEL] Connection not found: {connection_name}"))?;

    if !conn.ssh_enabled {
        return Err("[TUNNEL] SSH is not enabled for this connection".into());
    }

    let target = conn
        .ssh_target
        .as_deref()
        .ok_or("[TUNNEL] SSH target (user@host) not configured")?;
    let remote_port = conn.ssh_remote_port.unwrap_or(3000);
    let identity = conn.ssh_identity.as_deref();

    let tunnel = SshTunnel::start(target, remote_port, identity).await?;
    let local_port = tunnel.local_port();

    state.insert(connection_name.clone(), tunnel).await;

    crate::utils::audit_log(
        &home_path,
        "start_tunnel",
        "connections.yaml",
        &format!("{connection_name} -> {target}:{remote_port}"),
    );

    Ok(local_port)
}

/// Stop a tunnel for a named connection.
#[tauri::command]
pub async fn stop_tunnel(
    state: tauri::State<'_, Arc<TunnelRegistry>>,
    connection_name: String,
) -> Result<(), String> {
    let mut tunnels = state.tunnels.lock().await;
    if let Some(mut tunnel) = tunnels.remove(&connection_name) {
        tunnel.stop().await;
        Ok(())
    } else {
        Err(format!(
            "[TUNNEL] No active tunnel for connection: {connection_name}"
        ))
    }
}

// ── Helpers ──

/// Find a free TCP port by binding to port 0.
fn find_free_port() -> Result<u16, std::io::Error> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    Ok(port)
}

/// Check if a local port is accepting connections.
async fn is_port_open(port: u16) -> bool {
    tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .is_ok()
}
