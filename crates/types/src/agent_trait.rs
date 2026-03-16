//! Sub-agent registry trait
//!
//! Shared trait for sub-agent management, used by both core and tools crates.

/// Sub-agent registry trait for tool integration
#[async_trait::async_trait]
pub trait SubAgentRegistry: Send + Sync + 'static {
    /// Spawn a sub-agent with the given task description, returns agent ID
    async fn spawn_task(&self, task: String) -> String;
    /// List all agents as JSON
    async fn list_json(&self) -> serde_json::Value;
    /// Pause an agent
    async fn pause(&self, id: &str) -> Result<(), String>;
    /// Resume an agent
    async fn resume(&self, id: &str) -> Result<(), String>;
    /// Terminate an agent
    async fn terminate(&self, id: &str) -> Result<(), String>;
}
