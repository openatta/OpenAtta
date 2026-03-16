//! FlowRunner trait for starting flows from tools
//!
//! Defined in atta-types to avoid circular dependencies between atta-tools
//! and atta-core. The core layer implements this trait and late-binds it
//! into the start_flow tool.

use crate::auth::Actor;
use crate::error::AttaError;
use crate::task::Task;

/// Opaque flow runner — implemented at the Core layer.
///
/// This trait allows the `start_flow` tool to create tasks without depending
/// on `atta-core` directly.
#[async_trait::async_trait]
pub trait FlowRunner: Send + Sync + 'static {
    /// Create a new task for the given flow.
    ///
    /// Returns the created Task with its base58 ID.
    async fn start_flow(
        &self,
        flow_id: &str,
        input: serde_json::Value,
        actor: Actor,
    ) -> Result<Task, AttaError>;

    /// List available flow IDs and their names.
    async fn list_flows(&self) -> Result<Vec<(String, Option<String>)>, AttaError>;
}
