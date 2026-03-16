//! Approval manager — routes approval requests to backends

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use atta_types::AttaError;
use tokio::sync::RwLock;
use tracing::{info, warn};

use super::types::{ToolApprovalRequest, ToolApprovalResponse};

/// Backend that handles approval prompts
#[async_trait::async_trait]
pub trait ApprovalBackend: Send + Sync {
    /// Request approval from the user/operator
    async fn request_approval(
        &self,
        req: &ToolApprovalRequest,
    ) -> Result<ToolApprovalResponse, AttaError>;
}

/// Manages tool approval with session-scoped "Always" allowlist
pub struct ApprovalManager {
    backend: Arc<dyn ApprovalBackend>,
    /// Tools approved with "Always" for this session
    session_allowlist: RwLock<HashSet<String>>,
    /// Timeout for approval requests
    timeout: Duration,
}

impl ApprovalManager {
    /// Create a new approval manager
    pub fn new(backend: Arc<dyn ApprovalBackend>) -> Self {
        Self {
            backend,
            session_allowlist: RwLock::new(HashSet::new()),
            timeout: Duration::from_secs(120),
        }
    }

    /// Create with a custom timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Check if a tool is in the session allowlist, otherwise request approval
    pub async fn check_and_approve(&self, req: &ToolApprovalRequest) -> Result<(), AttaError> {
        // Check session allowlist first
        {
            let allowlist = self.session_allowlist.read().await;
            if allowlist.contains(&req.tool_name) {
                info!(
                    tool = %req.tool_name,
                    "tool approved via session allowlist"
                );
                return Ok(());
            }
        }

        // Request approval with timeout
        let response = tokio::time::timeout(self.timeout, self.backend.request_approval(req))
            .await
            .map_err(|_| AttaError::ApprovalTimeout {
                tool: req.tool_name.clone(),
                timeout_secs: self.timeout.as_secs(),
            })??;

        match response {
            ToolApprovalResponse::Yes => {
                info!(tool = %req.tool_name, "tool call approved (one-time)");
                Ok(())
            }
            ToolApprovalResponse::Always => {
                info!(tool = %req.tool_name, "tool added to session allowlist");
                let mut allowlist = self.session_allowlist.write().await;
                allowlist.insert(req.tool_name.clone());
                Ok(())
            }
            ToolApprovalResponse::No => {
                warn!(tool = %req.tool_name, "tool call denied by user");
                Err(AttaError::ApprovalDenied {
                    tool: req.tool_name.clone(),
                })
            }
        }
    }

    /// Clear the session allowlist
    pub async fn clear_allowlist(&self) {
        let mut allowlist = self.session_allowlist.write().await;
        allowlist.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::RiskLevel;

    struct MockBackend {
        response: ToolApprovalResponse,
    }

    #[async_trait::async_trait]
    impl ApprovalBackend for MockBackend {
        async fn request_approval(
            &self,
            _req: &ToolApprovalRequest,
        ) -> Result<ToolApprovalResponse, AttaError> {
            Ok(self.response.clone())
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
    async fn test_approve_yes() {
        let backend = Arc::new(MockBackend {
            response: ToolApprovalResponse::Yes,
        });
        let manager = ApprovalManager::new(backend);
        let result = manager.check_and_approve(&make_request("shell")).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_approve_no() {
        let backend = Arc::new(MockBackend {
            response: ToolApprovalResponse::No,
        });
        let manager = ApprovalManager::new(backend);
        let result = manager.check_and_approve(&make_request("shell")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_approve_always_adds_to_allowlist() {
        let backend = Arc::new(MockBackend {
            response: ToolApprovalResponse::Always,
        });
        let manager = ApprovalManager::new(backend);

        // First call goes through backend
        let result = manager.check_and_approve(&make_request("shell")).await;
        assert!(result.is_ok());

        // Second call should use allowlist (backend is not called)
        let result = manager.check_and_approve(&make_request("shell")).await;
        assert!(result.is_ok());
    }
}
