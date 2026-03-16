//! Native tool dispatcher — pass-through for models with built-in tool calling

use crate::llm::{LlmResponse, Message};

use super::traits::{DispatchResult, ToolCallResult, ToolDispatcher};

/// Dispatcher for models that natively support tool calling (e.g., GPT-4, Claude)
pub struct NativeToolDispatcher;

impl ToolDispatcher for NativeToolDispatcher {
    fn parse_response(&self, response: &LlmResponse) -> DispatchResult {
        match response {
            LlmResponse::Message(text) => DispatchResult::FinalAnswer(text.clone()),
            LlmResponse::ToolCalls(calls) => DispatchResult::ToolCalls(calls.clone()),
        }
    }

    fn format_results(&self, results: &[ToolCallResult]) -> Vec<Message> {
        results
            .iter()
            .map(|r| Message::ToolResult {
                tool_call_id: r.call_id.clone(),
                content: r.output.clone(),
            })
            .collect()
    }

    fn prompt_instructions(&self) -> Option<String> {
        None
    }

    fn should_send_tool_specs(&self) -> bool {
        true
    }
}
