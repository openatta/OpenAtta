//! XML-based tool dispatcher for models without native tool calling support

use crate::llm::{LlmResponse, Message, ToolCall};

use super::traits::{DispatchResult, ToolCallResult, ToolDispatcher};

/// Dispatcher that uses XML tags for tool calling with non-native models
///
/// Expects LLM output in the format:
/// ```xml
/// <tool_call>{"name": "tool_name", "arguments": {...}}</tool_call>
/// ```
///
/// Returns results as:
/// ```xml
/// <tool_result name="tool_name" status="ok">
/// result content
/// </tool_result>
/// ```
pub struct XmlToolDispatcher {
    /// Counter for generating call IDs
    call_counter: std::sync::atomic::AtomicU64,
}

impl XmlToolDispatcher {
    /// Create a new XML tool dispatcher
    pub fn new() -> Self {
        Self {
            call_counter: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Generate a unique call ID
    fn next_call_id(&self) -> String {
        let id = self
            .call_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        format!("xml_call_{id}")
    }

    /// Parse tool calls from XML-tagged text
    fn extract_tool_calls(&self, text: &str) -> Vec<ToolCall> {
        let mut calls = Vec::new();

        // Normalize various tag formats
        let normalized = text
            .replace("<toolcall>", "<tool_call>")
            .replace("</toolcall>", "</tool_call>")
            .replace("<tool-call>", "<tool_call>")
            .replace("</tool-call>", "</tool_call>")
            .replace("<invoke>", "<tool_call>")
            .replace("</invoke>", "</tool_call>");

        let mut search_from = 0;
        while let Some(start) = normalized[search_from..].find("<tool_call>") {
            let abs_start = search_from + start + "<tool_call>".len();
            if let Some(end) = normalized[abs_start..].find("</tool_call>") {
                let abs_end = abs_start + end;
                let json_str = normalized[abs_start..abs_end].trim();

                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str) {
                    let name = parsed
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let arguments = parsed
                        .get("arguments")
                        .cloned()
                        .unwrap_or(serde_json::json!({}));

                    if !name.is_empty() {
                        calls.push(ToolCall {
                            id: self.next_call_id(),
                            name,
                            arguments,
                        });
                    }
                }

                search_from = abs_end + "</tool_call>".len();
            } else {
                break;
            }
        }

        calls
    }
}

impl Default for XmlToolDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolDispatcher for XmlToolDispatcher {
    fn parse_response(&self, response: &LlmResponse) -> DispatchResult {
        match response {
            LlmResponse::Message(text) => {
                let calls = self.extract_tool_calls(text);
                if calls.is_empty() {
                    // Strip any residual XML tags from the final answer
                    DispatchResult::FinalAnswer(text.clone())
                } else {
                    DispatchResult::ToolCalls(calls)
                }
            }
            // If the model somehow returns native tool calls, handle them
            LlmResponse::ToolCalls(calls) => DispatchResult::ToolCalls(calls.clone()),
        }
    }

    fn format_results(&self, results: &[ToolCallResult]) -> Vec<Message> {
        let mut xml_parts = Vec::new();
        for r in results {
            let status = if r.success { "ok" } else { "error" };
            xml_parts.push(format!(
                "<tool_result name=\"{}\" status=\"{}\">\n{}\n</tool_result>",
                r.name, status, r.output
            ));
        }

        vec![Message::User(xml_parts.join("\n\n"))]
    }

    fn prompt_instructions(&self) -> Option<String> {
        Some(
            "## Tool Calling Format
When you need to use a tool, output a tool call in this XML format:

<tool_call>{\"name\": \"tool_name\", \"arguments\": {\"param\": \"value\"}}</tool_call>

You may call multiple tools by including multiple <tool_call> blocks.
After each tool call, you will receive the result in this format:

<tool_result name=\"tool_name\" status=\"ok\">
result content
</tool_result>

When you have your final answer, output it as plain text without any tool_call tags."
                .to_string(),
        )
    }

    fn should_send_tool_specs(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_single_tool_call() {
        let dispatcher = XmlToolDispatcher::new();
        let text = r#"I'll read the file. <tool_call>{"name": "file_read", "arguments": {"path": "/tmp/test.txt"}}</tool_call>"#;

        let calls = dispatcher.extract_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "file_read");
        assert_eq!(calls[0].arguments["path"], "/tmp/test.txt");
    }

    #[test]
    fn test_extract_multiple_tool_calls() {
        let dispatcher = XmlToolDispatcher::new();
        let text = r#"<tool_call>{"name": "a", "arguments": {}}</tool_call> some text <tool_call>{"name": "b", "arguments": {}}</tool_call>"#;

        let calls = dispatcher.extract_tool_calls(text);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "a");
        assert_eq!(calls[1].name, "b");
    }

    #[test]
    fn test_extract_normalized_tags() {
        let dispatcher = XmlToolDispatcher::new();
        let text = r#"<toolcall>{"name": "test", "arguments": {}}</toolcall>"#;

        let calls = dispatcher.extract_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "test");
    }

    #[test]
    fn test_no_tool_calls_returns_empty() {
        let dispatcher = XmlToolDispatcher::new();
        let text = "Just a regular response with no tools.";
        let calls = dispatcher.extract_tool_calls(text);
        assert!(calls.is_empty());
    }

    #[test]
    fn test_parse_response_with_tool_calls() {
        let dispatcher = XmlToolDispatcher::new();
        let response = LlmResponse::Message(
            r#"Let me check. <tool_call>{"name": "file_read", "arguments": {"path": "/tmp"}}</tool_call>"#.to_string(),
        );

        match dispatcher.parse_response(&response) {
            DispatchResult::ToolCalls(calls) => {
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].name, "file_read");
            }
            _ => panic!("expected ToolCalls"),
        }
    }

    #[test]
    fn test_parse_response_final_answer() {
        let dispatcher = XmlToolDispatcher::new();
        let response = LlmResponse::Message("The answer is 42.".to_string());

        match dispatcher.parse_response(&response) {
            DispatchResult::FinalAnswer(text) => {
                assert_eq!(text, "The answer is 42.");
            }
            _ => panic!("expected FinalAnswer"),
        }
    }

    #[test]
    fn test_format_results() {
        let dispatcher = XmlToolDispatcher::new();
        let results = vec![ToolCallResult {
            call_id: "1".to_string(),
            name: "file_read".to_string(),
            output: "file contents".to_string(),
            success: true,
        }];

        let messages = dispatcher.format_results(&results);
        assert_eq!(messages.len(), 1);
        match &messages[0] {
            Message::User(text) => {
                assert!(text.contains("tool_result"));
                assert!(text.contains("file_read"));
                assert!(text.contains("file contents"));
            }
            _ => panic!("expected User message"),
        }
    }
}
