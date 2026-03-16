//! WebSocket-based approval backend
//!
//! Sends approval prompts through a WebSocket channel and waits for
//! a response. Integrates with the WebUI's approval dialog.

use std::sync::Arc;
use std::time::Duration;

use atta_types::AttaError;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot, RwLock};
use tracing::{info, warn};

use super::manager::ApprovalBackend;
use super::types::{ToolApprovalRequest, ToolApprovalResponse};

/// An approval request sent over WebSocket
#[derive(Debug, Clone, Serialize)]
pub struct WsApprovalPrompt {
    /// Unique request ID for matching responses
    pub request_id: String,
    /// Tool being invoked
    pub tool_name: String,
    /// Tool arguments (serialized)
    pub arguments: serde_json::Value,
    /// Risk level description
    pub risk_level: String,
    /// Human-readable description
    pub description: String,
}

/// An approval response received from WebSocket
#[derive(Debug, Clone, Deserialize)]
pub struct WsApprovalReply {
    /// Request ID this reply is for
    pub request_id: String,
    /// Decision: "yes", "no", or "always"
    pub decision: String,
}

/// WebSocket approval backend
///
/// Sends prompts via `prompt_tx` and waits for responses on pending oneshot channels.
pub struct WsApprovalBackend {
    /// Channel to send prompts to WebSocket handler
    prompt_tx: mpsc::Sender<WsApprovalPrompt>,
    /// Pending requests awaiting responses
    pending: Arc<RwLock<std::collections::HashMap<String, oneshot::Sender<ToolApprovalResponse>>>>,
    /// Timeout for each approval request
    timeout: Duration,
}

impl WsApprovalBackend {
    /// Create a new WebSocket approval backend.
    ///
    /// Returns the backend and a receiver for prompts that should be
    /// forwarded to connected WebSocket clients.
    pub fn new(timeout: Duration) -> (Self, mpsc::Receiver<WsApprovalPrompt>) {
        let (prompt_tx, prompt_rx) = mpsc::channel(16);
        let backend = Self {
            prompt_tx,
            pending: Arc::new(RwLock::new(std::collections::HashMap::new())),
            timeout,
        };
        (backend, prompt_rx)
    }

    /// Handle an approval reply from a WebSocket client.
    ///
    /// Call this when the WebSocket handler receives a reply message.
    pub async fn handle_reply(&self, reply: WsApprovalReply) {
        let sender = {
            let mut pending = self.pending.write().await;
            pending.remove(&reply.request_id)
        };

        if let Some(sender) = sender {
            let response = match reply.decision.as_str() {
                "yes" => ToolApprovalResponse::Yes,
                "always" => ToolApprovalResponse::Always,
                _ => ToolApprovalResponse::No,
            };
            let _ = sender.send(response);
            info!(request_id = %reply.request_id, "WebSocket approval reply received");
        } else {
            warn!(
                request_id = %reply.request_id,
                "received reply for unknown/expired approval request"
            );
        }
    }
}

#[async_trait::async_trait]
impl ApprovalBackend for WsApprovalBackend {
    async fn request_approval(
        &self,
        req: &ToolApprovalRequest,
    ) -> Result<ToolApprovalResponse, AttaError> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();

        // Register pending request
        {
            let mut pending = self.pending.write().await;
            pending.insert(request_id.clone(), tx);
        }

        // Send prompt to WebSocket
        let prompt = WsApprovalPrompt {
            request_id: request_id.clone(),
            tool_name: req.tool_name.clone(),
            arguments: req.arguments.clone(),
            risk_level: format!("{:?}", req.risk_level),
            description: req.description.clone(),
        };

        self.prompt_tx.send(prompt).await.map_err(|_| {
            AttaError::SecurityViolation("WebSocket approval channel closed".to_string())
        })?;

        info!(
            tool = %req.tool_name,
            request_id = %request_id,
            "approval prompt sent via WebSocket"
        );

        // Wait for response with timeout
        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => {
                // Sender dropped — clean up
                let mut pending = self.pending.write().await;
                pending.remove(&request_id);
                Ok(ToolApprovalResponse::No)
            }
            Err(_) => {
                // Timeout — clean up and deny
                let mut pending = self.pending.write().await;
                pending.remove(&request_id);
                Err(AttaError::ApprovalTimeout {
                    tool: req.tool_name.clone(),
                    timeout_secs: self.timeout.as_secs(),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::RiskLevel;

    fn make_request(tool: &str) -> ToolApprovalRequest {
        ToolApprovalRequest {
            tool_name: tool.to_string(),
            arguments: serde_json::json!({}),
            risk_level: RiskLevel::High,
            description: format!("Execute {tool}"),
        }
    }

    #[tokio::test]
    async fn test_ws_approval_yes() {
        let (backend, mut prompt_rx) = WsApprovalBackend::new(Duration::from_secs(5));
        let backend = Arc::new(backend);
        let backend_clone = Arc::clone(&backend);

        // Spawn a task to handle the prompt
        tokio::spawn(async move {
            if let Some(prompt) = prompt_rx.recv().await {
                backend_clone
                    .handle_reply(WsApprovalReply {
                        request_id: prompt.request_id,
                        decision: "yes".to_string(),
                    })
                    .await;
            }
        });

        let result = backend.request_approval(&make_request("shell")).await;
        assert_eq!(result.unwrap(), ToolApprovalResponse::Yes);
    }

    #[tokio::test]
    async fn test_ws_approval_timeout() {
        let (backend, _prompt_rx) = WsApprovalBackend::new(Duration::from_millis(50));

        let result = backend.request_approval(&make_request("shell")).await;
        assert!(result.is_err());
    }
}
