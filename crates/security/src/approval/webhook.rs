//! Webhook-based approval backend
//!
//! Posts approval requests to a configured webhook URL and polls for
//! a callback response. Supports Telegram/Discord/Slack bots as endpoints.

use std::sync::Arc;
use std::time::Duration;

use atta_types::AttaError;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{info, warn};

use super::manager::ApprovalBackend;
use super::types::{ToolApprovalRequest, ToolApprovalResponse};

/// Webhook approval request payload
#[derive(Debug, Clone, Serialize)]
pub struct WebhookApprovalPayload {
    /// Unique request ID
    pub request_id: String,
    /// Tool being invoked
    pub tool_name: String,
    /// Tool arguments
    pub arguments: serde_json::Value,
    /// Risk level
    pub risk_level: String,
    /// Human-readable description
    pub description: String,
    /// Callback URL where the webhook should POST the decision
    pub callback_url: Option<String>,
}

/// Webhook approval callback payload
#[derive(Debug, Clone, Deserialize)]
pub struct WebhookApprovalCallback {
    /// Request ID this callback is for
    pub request_id: String,
    /// Decision: "yes", "no", or "always"
    pub decision: String,
    /// Requester identity for validation (optional)
    pub requester: Option<String>,
}

/// Configuration for the webhook approval backend
#[derive(Debug, Clone)]
pub struct WebhookApprovalConfig {
    /// URL to POST approval requests to
    pub webhook_url: String,
    /// Optional callback URL base (for receiving decisions)
    pub callback_url_base: Option<String>,
    /// Timeout for approval (default 30 minutes)
    pub timeout: Duration,
    /// Allowed requester identities (empty = allow all)
    pub allowed_requesters: Vec<String>,
}

impl Default for WebhookApprovalConfig {
    fn default() -> Self {
        Self {
            webhook_url: String::new(),
            callback_url_base: None,
            timeout: Duration::from_secs(30 * 60), // 30 minutes
            allowed_requesters: Vec::new(),
        }
    }
}

/// Webhook-based approval backend
///
/// Posts approval requests to external services and waits for callback responses.
/// Supports integration with Telegram, Discord, Slack bots, etc.
pub struct WebhookApprovalBackend {
    config: WebhookApprovalConfig,
    /// HTTP client for posting approval requests
    http_client: reqwest::Client,
    /// Pending requests: request_id → oneshot sender
    pending: Arc<
        RwLock<
            std::collections::HashMap<String, tokio::sync::oneshot::Sender<ToolApprovalResponse>>,
        >,
    >,
}

impl WebhookApprovalBackend {
    /// Create a new webhook approval backend
    pub fn new(config: WebhookApprovalConfig) -> Self {
        Self {
            config,
            http_client: reqwest::Client::new(),
            pending: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Handle a callback from the webhook service.
    ///
    /// Call this from an HTTP endpoint that receives approval decisions.
    pub async fn handle_callback(
        &self,
        callback: WebhookApprovalCallback,
    ) -> Result<(), AttaError> {
        // Validate requester if configured
        if !self.config.allowed_requesters.is_empty() {
            let requester = callback.requester.as_deref().unwrap_or("");
            if !self
                .config
                .allowed_requesters
                .iter()
                .any(|r| r == requester)
            {
                warn!(
                    requester = requester,
                    request_id = %callback.request_id,
                    "webhook callback from unauthorized requester"
                );
                return Err(AttaError::PermissionDenied {
                    permission: format!("requester '{}' not authorized for approvals", requester),
                });
            }
        }

        let sender = {
            let mut pending = self.pending.write().await;
            pending.remove(&callback.request_id)
        };

        if let Some(sender) = sender {
            let response = match callback.decision.as_str() {
                "yes" => ToolApprovalResponse::Yes,
                "always" => ToolApprovalResponse::Always,
                _ => ToolApprovalResponse::No,
            };
            let _ = sender.send(response);
            info!(request_id = %callback.request_id, "webhook approval callback processed");
            Ok(())
        } else {
            warn!(
                request_id = %callback.request_id,
                "webhook callback for unknown/expired request"
            );
            Err(AttaError::Validation(format!(
                "no pending request with id '{}'",
                callback.request_id
            )))
        }
    }
}

#[async_trait::async_trait]
impl ApprovalBackend for WebhookApprovalBackend {
    async fn request_approval(
        &self,
        req: &ToolApprovalRequest,
    ) -> Result<ToolApprovalResponse, AttaError> {
        if self.config.webhook_url.is_empty() {
            warn!(tool = %req.tool_name, "webhook URL not configured, denying by default");
            return Ok(ToolApprovalResponse::No);
        }

        let request_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = tokio::sync::oneshot::channel();

        // Register pending request
        {
            let mut pending = self.pending.write().await;
            pending.insert(request_id.clone(), tx);
        }

        let payload = WebhookApprovalPayload {
            request_id: request_id.clone(),
            tool_name: req.tool_name.clone(),
            arguments: req.arguments.clone(),
            risk_level: format!("{:?}", req.risk_level),
            description: req.description.clone(),
            callback_url: self
                .config
                .callback_url_base
                .as_ref()
                .map(|base| format!("{}/approval/callback", base)),
        };

        // POST to webhook (fire and forget — we wait for the callback)
        let payload_json = serde_json::to_string(&payload).map_err(|e| {
            AttaError::Validation(format!("failed to serialize approval payload: {}", e))
        })?;

        info!(
            tool = %req.tool_name,
            request_id = %request_id,
            webhook = %self.config.webhook_url,
            "sending approval request to webhook"
        );

        // Fire-and-forget HTTP POST — we don't block on the response since
        // we wait for the callback via the oneshot channel below.
        let webhook_url = self.config.webhook_url.clone();
        let client = self.http_client.clone();
        tokio::spawn(async move {
            if let Err(e) = client
                .post(&webhook_url)
                .header("Content-Type", "application/json")
                .body(payload_json)
                .send()
                .await
            {
                warn!(webhook = %webhook_url, error = %e, "failed to POST approval request to webhook");
            }
        });

        // Wait for callback with timeout
        match tokio::time::timeout(self.config.timeout, rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => {
                let mut pending = self.pending.write().await;
                pending.remove(&request_id);
                Ok(ToolApprovalResponse::No)
            }
            Err(_) => {
                let mut pending = self.pending.write().await;
                pending.remove(&request_id);
                Err(AttaError::ApprovalTimeout {
                    tool: req.tool_name.clone(),
                    timeout_secs: self.config.timeout.as_secs(),
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
    async fn test_webhook_no_url_denies() {
        let backend = WebhookApprovalBackend::new(WebhookApprovalConfig::default());
        let result = backend.request_approval(&make_request("shell")).await;
        assert_eq!(result.unwrap(), ToolApprovalResponse::No);
    }

    #[tokio::test]
    async fn test_webhook_callback_approval() {
        let config = WebhookApprovalConfig {
            webhook_url: "http://localhost:8080/approval".to_string(),
            timeout: Duration::from_secs(5),
            ..Default::default()
        };
        let backend = Arc::new(WebhookApprovalBackend::new(config));
        let backend_clone = Arc::clone(&backend);

        // Pre-register a pending request and simulate callback
        let request_id = "test-req-123".to_string();
        let (tx, rx) = tokio::sync::oneshot::channel();
        {
            let mut pending = backend.pending.write().await;
            pending.insert(request_id.clone(), tx);
        }

        // Simulate callback
        backend_clone
            .handle_callback(WebhookApprovalCallback {
                request_id,
                decision: "yes".to_string(),
                requester: None,
            })
            .await
            .unwrap();

        let response = rx.await.unwrap();
        assert_eq!(response, ToolApprovalResponse::Yes);
    }

    #[tokio::test]
    async fn test_webhook_requester_validation() {
        let config = WebhookApprovalConfig {
            webhook_url: "http://localhost:8080/approval".to_string(),
            allowed_requesters: vec!["admin@example.com".to_string()],
            ..Default::default()
        };
        let backend = WebhookApprovalBackend::new(config);

        // Unknown requester should be rejected
        let result = backend
            .handle_callback(WebhookApprovalCallback {
                request_id: "test".to_string(),
                decision: "yes".to_string(),
                requester: Some("hacker@evil.com".to_string()),
            })
            .await;
        assert!(result.is_err());
    }
}
