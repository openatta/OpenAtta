//! Interactive approval system for high-risk tool calls
//!
//! Provides session-scoped approval management with pluggable backends.

pub mod channel_bridge;
pub mod cli;
pub mod manager;
pub mod types;
pub mod webhook;
pub mod ws;

pub use manager::{ApprovalBackend, ApprovalManager};
pub use types::{ToolApprovalRequest, ToolApprovalResponse};
pub use webhook::{WebhookApprovalBackend, WebhookApprovalConfig};
pub use ws::WsApprovalBackend;
