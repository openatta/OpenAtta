//! Channel-based approval backend — sends approval prompts through messaging channels
//!
//! Routes approval requests through a `Channel` implementation's
//! `send_approval_prompt()` method and waits for a callback response
//! via the oneshot pattern (same as WebSocket backend).

use std::sync::Arc;
use std::time::Duration;

use atta_types::AttaError;
use tokio::sync::{oneshot, RwLock};
use tracing::{info, warn};

use super::manager::ApprovalBackend;
use super::types::{ToolApprovalRequest, ToolApprovalResponse};

/// Trait for sending approval prompts through a channel.
///
/// This avoids a circular dependency between atta-security and atta-channel
/// by defining a minimal interface that atta-channel can implement.
#[async_trait::async_trait]
pub trait ApprovalChannel: Send + Sync {
    /// Send an approval prompt to a recipient
    async fn send_approval_prompt(
        &self,
        recipient: &str,
        request_id: &str,
        tool_name: &str,
        arguments: &serde_json::Value,
        thread_ts: Option<String>,
    ) -> Result<(), AttaError>;
}

/// Approval backend that routes approval requests through a messaging channel.
///
/// Uses the oneshot pattern: sends a prompt via channel, registers a pending
/// oneshot receiver, and waits for `handle_reply()` to be called when the
/// user responds through the channel.
pub struct ChannelApprovalBackend {
    /// Channel name for logging
    channel_name: String,
    /// Recipient to send approval prompts to (user ID, etc.)
    recipient: String,
    /// The channel to send prompts through
    channel: Arc<dyn ApprovalChannel>,
    /// Pending requests awaiting responses
    pending: Arc<RwLock<std::collections::HashMap<String, oneshot::Sender<ToolApprovalResponse>>>>,
    /// Timeout for each approval request
    timeout: Duration,
}

impl ChannelApprovalBackend {
    /// Create a new channel approval backend
    pub fn new(
        channel_name: String,
        recipient: String,
        channel: Arc<dyn ApprovalChannel>,
        timeout: Duration,
    ) -> Self {
        Self {
            channel_name,
            recipient,
            channel,
            pending: Arc::new(RwLock::new(std::collections::HashMap::new())),
            timeout,
        }
    }

    /// Handle an approval reply from the channel.
    ///
    /// Call this when the channel receives a message matching an approval
    /// response pattern (e.g., "approve <request_id>" or "deny <request_id>").
    pub async fn handle_reply(&self, request_id: &str, decision: &str) {
        let sender = {
            let mut pending = self.pending.write().await;
            pending.remove(request_id)
        };

        if let Some(sender) = sender {
            let response = match decision {
                "yes" | "approve" | "approved" | "y" => ToolApprovalResponse::Yes,
                "always" | "allow" => ToolApprovalResponse::Always,
                _ => ToolApprovalResponse::No,
            };
            let _ = sender.send(response);
            info!(
                request_id = %request_id,
                channel = %self.channel_name,
                "channel approval reply received"
            );
        } else {
            warn!(
                request_id = %request_id,
                "received reply for unknown/expired approval request"
            );
        }
    }

    /// Get a clone of the pending map for external reply handling
    pub fn pending_requests(
        &self,
    ) -> Arc<RwLock<std::collections::HashMap<String, oneshot::Sender<ToolApprovalResponse>>>> {
        Arc::clone(&self.pending)
    }
}

#[async_trait::async_trait]
impl ApprovalBackend for ChannelApprovalBackend {
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

        // Send prompt through channel
        if let Err(e) = self
            .channel
            .send_approval_prompt(
                &self.recipient,
                &request_id,
                &req.tool_name,
                &req.arguments,
                None,
            )
            .await
        {
            // Clean up on send failure
            let mut pending = self.pending.write().await;
            pending.remove(&request_id);
            return Err(e);
        }

        info!(
            tool = %req.tool_name,
            request_id = %request_id,
            channel = %self.channel_name,
            "approval prompt sent via channel"
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

    /// Mock channel that always succeeds
    struct MockApprovalChannel;

    #[async_trait::async_trait]
    impl ApprovalChannel for MockApprovalChannel {
        async fn send_approval_prompt(
            &self,
            _recipient: &str,
            _request_id: &str,
            _tool_name: &str,
            _arguments: &serde_json::Value,
            _thread_ts: Option<String>,
        ) -> Result<(), AttaError> {
            Ok(())
        }
    }

    fn make_request(tool: &str) -> ToolApprovalRequest {
        ToolApprovalRequest {
            tool_name: tool.to_string(),
            arguments: serde_json::json!({}),
            risk_level: RiskLevel::High,
            description: format!("Execute {tool}"),
        }
    }

    #[tokio::test]
    async fn test_channel_approval_yes() {
        let channel = Arc::new(MockApprovalChannel);
        let backend = ChannelApprovalBackend::new(
            "test".to_string(),
            "user1".to_string(),
            channel,
            Duration::from_secs(5),
        );

        let pending = backend.pending_requests();

        // Spawn a task to handle the approval
        let pending_clone = Arc::clone(&pending);
        tokio::spawn(async move {
            // Wait for the request to be registered
            loop {
                let map = pending_clone.read().await;
                if let Some(key) = map.keys().next().cloned() {
                    drop(map);
                    // Approve it
                    let mut map = pending_clone.write().await;
                    if let Some(sender) = map.remove(&key) {
                        let _ = sender.send(ToolApprovalResponse::Yes);
                    }
                    break;
                }
                drop(map);
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });

        let result = backend.request_approval(&make_request("shell")).await;
        assert_eq!(result.unwrap(), ToolApprovalResponse::Yes);
    }

    #[tokio::test]
    async fn test_channel_approval_handle_reply() {
        let channel = Arc::new(MockApprovalChannel);
        let backend = Arc::new(ChannelApprovalBackend::new(
            "test".to_string(),
            "user1".to_string(),
            channel,
            Duration::from_secs(5),
        ));

        let backend_clone = Arc::clone(&backend);
        let pending = backend.pending_requests();

        // Spawn a task to handle via handle_reply()
        tokio::spawn(async move {
            loop {
                let map = pending.read().await;
                if let Some(key) = map.keys().next().cloned() {
                    drop(map);
                    backend_clone.handle_reply(&key, "approve").await;
                    break;
                }
                drop(map);
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });

        let result = backend.request_approval(&make_request("shell")).await;
        assert_eq!(result.unwrap(), ToolApprovalResponse::Yes);
    }

    #[tokio::test]
    async fn test_channel_approval_timeout() {
        let channel = Arc::new(MockApprovalChannel);
        let backend = ChannelApprovalBackend::new(
            "test".to_string(),
            "user1".to_string(),
            channel,
            Duration::from_millis(50),
        );

        let result = backend.request_approval(&make_request("shell")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_channel_approval_deny() {
        let channel = Arc::new(MockApprovalChannel);
        let backend = Arc::new(ChannelApprovalBackend::new(
            "test".to_string(),
            "user1".to_string(),
            channel,
            Duration::from_secs(5),
        ));

        let backend_clone = Arc::clone(&backend);
        let pending = backend.pending_requests();

        tokio::spawn(async move {
            loop {
                let map = pending.read().await;
                if let Some(key) = map.keys().next().cloned() {
                    drop(map);
                    backend_clone.handle_reply(&key, "deny").await;
                    break;
                }
                drop(map);
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });

        let result = backend.request_approval(&make_request("shell")).await;
        assert_eq!(result.unwrap(), ToolApprovalResponse::No);
    }
}
