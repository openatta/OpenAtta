//! Approval types

use atta_types::RiskLevel;

/// Request for tool approval
#[derive(Debug, Clone)]
pub struct ToolApprovalRequest {
    /// Tool being invoked
    pub tool_name: String,
    /// Tool arguments
    pub arguments: serde_json::Value,
    /// Assessed risk level
    pub risk_level: RiskLevel,
    /// Human-readable description of what the tool will do
    pub description: String,
}

/// Response to an approval request
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolApprovalResponse {
    /// Allow this single invocation
    Yes,
    /// Deny this invocation
    No,
    /// Allow this tool for the remainder of the session
    Always,
}
