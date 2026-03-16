//! WebSocket hub for real-time event push
//!
//! [`WsHub`] manages WebSocket connections and broadcasts events to all connected clients.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{mpsc, RwLock};
use tracing::{debug, warn};
use uuid::Uuid;

use atta_types::EventEnvelope;

/// A WebSocket client connection handle
type WsSender = mpsc::UnboundedSender<String>;

/// WebSocket hub — manages connected clients and broadcasts events
pub struct WsHub {
    clients: Arc<RwLock<HashMap<Uuid, WsSender>>>,
}

impl WsHub {
    /// Create a new empty hub
    pub fn new() -> Self {
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a new WebSocket client, returns (client_id, receiver)
    pub async fn add_client(&self) -> (Uuid, mpsc::UnboundedReceiver<String>) {
        let id = Uuid::new_v4();
        let (tx, rx) = mpsc::unbounded_channel();
        self.clients.write().await.insert(id, tx);
        debug!(%id, "WebSocket client connected");
        (id, rx)
    }

    /// Remove a client by ID
    pub async fn remove_client(&self, id: &Uuid) {
        self.clients.write().await.remove(id);
        debug!(%id, "WebSocket client disconnected");
    }

    /// Broadcast an event to all connected clients (fire-and-forget)
    pub fn broadcast(&self, event: &EventEnvelope) {
        let json = match serde_json::to_string(event) {
            Ok(j) => j,
            Err(e) => {
                warn!("failed to serialize event for broadcast: {}", e);
                return;
            }
        };

        // Use try_read to avoid blocking; skip broadcast if lock is held
        let clients = match self.clients.try_read() {
            Ok(c) => c,
            Err(_) => {
                debug!("broadcast skipped: client lock contended");
                return;
            }
        };

        for (id, tx) in clients.iter() {
            if tx.send(json.clone()).is_err() {
                debug!(%id, "broadcast to client failed (disconnected)");
            }
        }
    }

    /// Return current number of connected clients
    pub async fn client_count(&self) -> usize {
        self.clients.read().await.len()
    }
}

impl Default for WsHub {
    fn default() -> Self {
        Self::new()
    }
}
