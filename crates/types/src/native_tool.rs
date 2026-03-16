//! Native Rust tool trait
//!
//! Tools implemented directly in Rust (as opposed to MCP servers).

use crate::tool::RiskLevel;
use crate::AttaError;

/// Native Rust tool trait
///
/// Implement this for tools that run directly in the AttaOS process.
/// These are registered into the ToolRegistry with a `Native` binding.
#[async_trait::async_trait]
pub trait NativeTool: Send + Sync + 'static {
    /// Tool name (unique identifier)
    fn name(&self) -> &str;

    /// Human-readable description
    fn description(&self) -> &str;

    /// JSON Schema for the tool's parameters
    fn parameters_schema(&self) -> serde_json::Value;

    /// Risk level classification
    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }

    /// Execute the tool with the given arguments
    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, AttaError>;
}
