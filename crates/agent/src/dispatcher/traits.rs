//! ToolDispatcher trait

use crate::llm::{LlmResponse, Message, ToolCall};

/// Result of dispatching an LLM response
#[derive(Debug, Clone)]
pub enum DispatchResult {
    /// LLM produced a final text answer
    FinalAnswer(String),
    /// LLM requested tool calls
    ToolCalls(Vec<ToolCall>),
}

/// Result of a single tool execution
pub struct ToolCallResult {
    /// Tool call ID
    pub call_id: String,
    /// Tool name
    pub name: String,
    /// Execution result (success text or error text)
    pub output: String,
    /// Whether the call succeeded
    pub success: bool,
}

/// Dispatches tool calls between the agent and LLM
///
/// Abstracts away whether the model uses native tool calling or XML-based format.
pub trait ToolDispatcher: Send + Sync {
    /// Parse an LLM response into dispatch result
    fn parse_response(&self, response: &LlmResponse) -> DispatchResult;

    /// Format tool execution results as messages for the conversation
    fn format_results(&self, results: &[ToolCallResult]) -> Vec<Message>;

    /// Generate tool-calling format instructions for the system prompt.
    /// Returns `None` for native dispatchers (no extra instructions needed).
    fn prompt_instructions(&self) -> Option<String>;

    /// Whether to send structured tool specifications to the provider.
    /// Native dispatchers return `true`, XML dispatchers return `false`.
    fn should_send_tool_specs(&self) -> bool;
}
