//! Anthropic API provider
//!
//! Implements [`LlmProvider`] for Anthropic's Messages API (v1/messages).

use atta_types::{AttaError, LlmError, ToolSchema};
use tracing::debug;

use crate::llm::{
    ChatOptions, LlmProvider, LlmResponse, LlmResult, LlmStream, Message, ModelInfo, StreamChunk,
    ThinkingLevel, ToolCall,
};
use atta_types::TokenUsage;

/// Anthropic API provider
pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
    max_tokens: u32,
}

impl AnthropicProvider {
    /// Create a new Anthropic provider
    pub fn new(api_key: String, model: String, base_url: String, max_tokens: u32) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            model,
            base_url,
            max_tokens,
        }
    }

    /// Create from environment variables
    ///
    /// - `ANTHROPIC_API_KEY` — required
    /// - `ANTHROPIC_MODEL` — defaults to "claude-sonnet-4-20250514"
    /// - `ANTHROPIC_BASE_URL` — defaults to "https://api.anthropic.com"
    /// - `ANTHROPIC_MAX_TOKENS` — defaults to 4096
    pub fn from_env() -> Result<Self, AttaError> {
        let api_key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
            AttaError::Llm(LlmError::AuthError(
                "ANTHROPIC_API_KEY environment variable not set".to_string(),
            ))
        })?;
        let model = std::env::var("ANTHROPIC_MODEL")
            .unwrap_or_else(|_| "claude-sonnet-4-20250514".to_string());
        let base_url = std::env::var("ANTHROPIC_BASE_URL")
            .unwrap_or_else(|_| "https://api.anthropic.com".to_string());
        let max_tokens = std::env::var("ANTHROPIC_MAX_TOKENS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(4096);

        Ok(Self::new(api_key, model, base_url, max_tokens))
    }

    /// Convert our Message enum to Anthropic API format.
    /// Returns (system_prompt, messages) because Anthropic takes system as a top-level param.
    fn format_messages(messages: &[Message]) -> (Option<String>, Vec<serde_json::Value>) {
        let mut system = None;
        let mut result = Vec::new();

        for msg in messages {
            match msg {
                Message::System(content) => {
                    system = Some(content.clone());
                }
                Message::User(content) => {
                    result.push(serde_json::json!({
                        "role": "user",
                        "content": content,
                    }));
                }
                Message::Assistant(content) => {
                    result.push(serde_json::json!({
                        "role": "assistant",
                        "content": [{ "type": "text", "text": content }],
                    }));
                }
                Message::AssistantToolCalls(calls) => {
                    let content: Vec<serde_json::Value> = calls
                        .iter()
                        .map(|tc| {
                            serde_json::json!({
                                "type": "tool_use",
                                "id": tc.id,
                                "name": tc.name,
                                "input": tc.arguments,
                            })
                        })
                        .collect();
                    result.push(serde_json::json!({
                        "role": "assistant",
                        "content": content,
                    }));
                }
                Message::ToolResult {
                    tool_call_id,
                    content,
                } => {
                    result.push(serde_json::json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": tool_call_id,
                            "content": content,
                        }],
                    }));
                }
            }
        }

        (system, result)
    }

    /// Convert our ToolSchema to Anthropic tool format
    fn format_tools(tools: &[ToolSchema]) -> Vec<serde_json::Value> {
        use super::schema::{clean_tool_schema, CleaningStrategy};
        tools
            .iter()
            .map(|t| {
                let cleaned_params = clean_tool_schema(&t.parameters, &CleaningStrategy::Anthropic);
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": cleaned_params,
                })
            })
            .collect()
    }

    /// Parse Anthropic API response
    fn parse_response(body: &serde_json::Value) -> Result<LlmResponse, AttaError> {
        let content = body
            .get("content")
            .and_then(|c| c.as_array())
            .ok_or_else(|| {
                AttaError::Llm(LlmError::InvalidResponse(
                    "no content array in response".to_string(),
                ))
            })?;

        let mut tool_calls = Vec::new();
        let mut text_parts = Vec::new();

        for block in content {
            let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match block_type {
                "text" => {
                    if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                        text_parts.push(text.to_string());
                    }
                }
                "tool_use" => {
                    let id = block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = block
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let arguments = block.get("input").cloned().unwrap_or(serde_json::json!({}));
                    tool_calls.push(ToolCall {
                        id,
                        name,
                        arguments,
                    });
                }
                _ => {
                    debug!(block_type, "ignoring unknown content block type");
                }
            }
        }

        if !tool_calls.is_empty() {
            Ok(LlmResponse::ToolCalls(tool_calls))
        } else {
            Ok(LlmResponse::Message(text_parts.join("")))
        }
    }

    /// Build the request body shared between chat/chat_stream
    fn build_request_body(&self, messages: &[Message], tools: &[ToolSchema]) -> serde_json::Value {
        let (system, formatted_messages) = Self::format_messages(messages);

        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "messages": formatted_messages,
        });

        if let Some(sys) = system {
            body["system"] = serde_json::Value::String(sys);
        }

        if !tools.is_empty() {
            body["tools"] = serde_json::Value::Array(Self::format_tools(tools));
        }

        body
    }

    /// Extract token usage from the Anthropic API response body
    fn parse_usage(body: &serde_json::Value) -> Option<TokenUsage> {
        let usage = body.get("usage")?;
        let input = usage.get("input_tokens")?.as_u64()?;
        let output = usage.get("output_tokens")?.as_u64()?;
        Some(TokenUsage {
            input_tokens: input,
            output_tokens: output,
        })
    }

    /// Handle HTTP error status codes
    fn handle_error_status(
        status: reqwest::StatusCode,
        response_text: &str,
        headers: &reqwest::header::HeaderMap,
    ) -> Option<AttaError> {
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Some(AttaError::Llm(LlmError::AuthError(
                "invalid API key".to_string(),
            )));
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = headers
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(60);
            return Some(AttaError::Llm(LlmError::RateLimited {
                retry_after_secs: retry_after,
            }));
        }
        if status.is_server_error() {
            return Some(AttaError::Llm(LlmError::RequestFailed(format!(
                "server error {}: {}",
                status, response_text
            ))));
        }
        if !status.is_success() {
            return Some(AttaError::Llm(LlmError::RequestFailed(format!(
                "HTTP {}: {}",
                status, response_text
            ))));
        }
        None
    }
}

#[async_trait::async_trait]
impl LlmProvider for AnthropicProvider {
    async fn chat(
        &self,
        messages: &[Message],
        tools: &[ToolSchema],
    ) -> Result<LlmResponse, AttaError> {
        let url = format!("{}/v1/messages", self.base_url);
        let body = self.build_request_body(messages, tools);

        debug!(model = %self.model, messages = messages.len(), tools = tools.len(), "Anthropic chat request");

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AttaError::Llm(LlmError::RequestFailed(e.to_string())))?;

        let status = response.status();
        let headers = response.headers().clone();
        let body_text = response.text().await.unwrap_or_default();

        if let Some(err) = Self::handle_error_status(status, &body_text, &headers) {
            return Err(err);
        }

        let response_body: serde_json::Value = serde_json::from_str(&body_text).map_err(|e| {
            AttaError::Llm(LlmError::InvalidResponse(format!(
                "failed to parse response JSON: {}",
                e
            )))
        })?;

        debug!("Anthropic response received");
        Self::parse_response(&response_body)
    }

    async fn chat_stream(
        &self,
        messages: &[Message],
        tools: &[ToolSchema],
    ) -> Result<LlmStream, AttaError> {
        let url = format!("{}/v1/messages", self.base_url);
        let mut body = self.build_request_body(messages, tools);
        body["stream"] = serde_json::json!(true);

        debug!(model = %self.model, "Anthropic streaming chat request");

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AttaError::Llm(LlmError::RequestFailed(e.to_string())))?;

        let status = response.status();
        let headers = response.headers().clone();

        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            if let Some(err) = Self::handle_error_status(status, &body_text, &headers) {
                return Err(err);
            }
            return Err(AttaError::Llm(LlmError::RequestFailed(format!(
                "HTTP {}",
                status
            ))));
        }

        // Spawn a task to read SSE bytes and send parsed chunks via channel
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<StreamChunk, AttaError>>(128);

        tokio::spawn(async move {
            use futures::StreamExt;
            let mut byte_stream = response.bytes_stream();
            let mut line_buf = String::new();
            let mut current_tool_index: usize = 0;

            while let Some(chunk_result) = byte_stream.next().await {
                let bytes = match chunk_result {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = tx
                            .send(Err(AttaError::Llm(LlmError::RequestFailed(e.to_string()))))
                            .await;
                        return;
                    }
                };

                line_buf.push_str(&String::from_utf8_lossy(&bytes));

                while let Some(pos) = line_buf.find('\n') {
                    let line = line_buf[..pos].to_string();
                    line_buf = line_buf[pos + 1..].to_string();

                    let line = line.trim();
                    if line.is_empty() || line.starts_with(':') {
                        continue;
                    }

                    if line == "event: message_stop" {
                        let _ = tx.send(Ok(StreamChunk::Done)).await;
                        return;
                    }

                    if let Some(data) = line.strip_prefix("data: ") {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                            let event_type =
                                json.get("type").and_then(|v| v.as_str()).unwrap_or("");

                            match event_type {
                                "content_block_start" => {
                                    if let Some(block) = json.get("content_block") {
                                        let block_type = block
                                            .get("type")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");
                                        if block_type == "tool_use" {
                                            let idx = json
                                                .get("index")
                                                .and_then(|v| v.as_u64())
                                                .unwrap_or(0)
                                                as usize;
                                            current_tool_index = idx;
                                            let id = block
                                                .get("id")
                                                .and_then(|v| v.as_str())
                                                .map(|s| s.to_string());
                                            let name = block
                                                .get("name")
                                                .and_then(|v| v.as_str())
                                                .map(|s| s.to_string());
                                            let _ = tx
                                                .send(Ok(StreamChunk::ToolCallDelta {
                                                    index: idx,
                                                    id,
                                                    name,
                                                    arguments_delta: String::new(),
                                                }))
                                                .await;
                                        }
                                    }
                                }
                                "content_block_delta" => {
                                    if let Some(delta) = json.get("delta") {
                                        let delta_type = delta
                                            .get("type")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");
                                        match delta_type {
                                            "text_delta" => {
                                                if let Some(text) =
                                                    delta.get("text").and_then(|v| v.as_str())
                                                {
                                                    if !text.is_empty() {
                                                        let _ = tx
                                                            .send(Ok(StreamChunk::TextDelta {
                                                                delta: text.to_string(),
                                                            }))
                                                            .await;
                                                    }
                                                }
                                            }
                                            "input_json_delta" => {
                                                if let Some(partial) = delta
                                                    .get("partial_json")
                                                    .and_then(|v| v.as_str())
                                                {
                                                    let _ = tx
                                                        .send(Ok(StreamChunk::ToolCallDelta {
                                                            index: current_tool_index,
                                                            id: None,
                                                            name: None,
                                                            arguments_delta: partial.to_string(),
                                                        }))
                                                        .await;
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                "message_stop" => {
                                    let _ = tx.send(Ok(StreamChunk::Done)).await;
                                    return;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }

            let _ = tx.send(Ok(StreamChunk::Done)).await;
        });

        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Box::pin(stream))
    }

    async fn chat_with_options(
        &self,
        messages: &[Message],
        tools: &[ToolSchema],
        options: &ChatOptions,
    ) -> Result<LlmResponse, AttaError> {
        let url = format!("{}/v1/messages", self.base_url);
        let mut body = self.build_request_body(messages, tools);

        // Apply thinking level
        if let ThinkingLevel::Extended(budget) = &options.thinking_level {
            body["thinking"] = serde_json::json!({
                "type": "enabled",
                "budget_tokens": budget,
            });
        }

        // Apply temperature
        if let Some(temp) = options.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        debug!(model = %self.model, "Anthropic chat_with_options request");

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AttaError::Llm(LlmError::RequestFailed(e.to_string())))?;

        let status = response.status();
        let headers = response.headers().clone();
        let body_text = response.text().await.unwrap_or_default();

        if let Some(err) = Self::handle_error_status(status, &body_text, &headers) {
            return Err(err);
        }

        let response_body: serde_json::Value = serde_json::from_str(&body_text).map_err(|e| {
            AttaError::Llm(LlmError::InvalidResponse(format!(
                "failed to parse response JSON: {}",
                e
            )))
        })?;

        Self::parse_response(&response_body)
    }

    async fn chat_with_usage(
        &self,
        messages: &[Message],
        tools: &[ToolSchema],
        options: &ChatOptions,
    ) -> Result<LlmResult, AttaError> {
        let url = format!("{}/v1/messages", self.base_url);
        let mut body = self.build_request_body(messages, tools);

        if let ThinkingLevel::Extended(budget) = &options.thinking_level {
            body["thinking"] = serde_json::json!({
                "type": "enabled",
                "budget_tokens": budget,
            });
        }
        if let Some(temp) = options.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        debug!(model = %self.model, "Anthropic chat_with_usage request");

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AttaError::Llm(LlmError::RequestFailed(e.to_string())))?;

        let status = response.status();
        let headers = response.headers().clone();
        let body_text = response.text().await.unwrap_or_default();

        if let Some(err) = Self::handle_error_status(status, &body_text, &headers) {
            return Err(err);
        }

        let response_body: serde_json::Value = serde_json::from_str(&body_text).map_err(|e| {
            AttaError::Llm(LlmError::InvalidResponse(format!(
                "failed to parse response JSON: {}",
                e
            )))
        })?;

        let llm_response = Self::parse_response(&response_body)?;
        let usage = Self::parse_usage(&response_body);

        Ok(LlmResult {
            response: llm_response,
            usage,
        })
    }

    fn model_info(&self) -> ModelInfo {
        ModelInfo {
            model_id: self.model.clone(),
            context_window: 200_000,
            supports_tools: true,
            provider: "anthropic".to_string(),
            supports_streaming: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_messages_system_extracted() {
        let messages = vec![
            Message::System("Be helpful.".to_string()),
            Message::User("Hello".to_string()),
        ];
        let (system, msgs) = AnthropicProvider::format_messages(&messages);
        assert_eq!(system.unwrap(), "Be helpful.");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
    }

    #[test]
    fn test_format_tools() {
        let tools = vec![ToolSchema {
            name: "search".to_string(),
            description: "Search".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        }];
        let formatted = AnthropicProvider::format_tools(&tools);
        assert_eq!(formatted.len(), 1);
        assert_eq!(formatted[0]["name"], "search");
        assert!(formatted[0].get("input_schema").is_some());
    }

    #[test]
    fn test_parse_text_response() {
        let body = serde_json::json!({
            "content": [{ "type": "text", "text": "Hello!" }],
            "stop_reason": "end_turn"
        });
        let result = AnthropicProvider::parse_response(&body).unwrap();
        match result {
            LlmResponse::Message(text) => assert_eq!(text, "Hello!"),
            _ => panic!("expected Message"),
        }
    }

    #[test]
    fn test_parse_tool_use_response() {
        let body = serde_json::json!({
            "content": [{
                "type": "tool_use",
                "id": "toolu_123",
                "name": "search",
                "input": { "query": "rust" }
            }],
            "stop_reason": "tool_use"
        });
        let result = AnthropicProvider::parse_response(&body).unwrap();
        match result {
            LlmResponse::ToolCalls(calls) => {
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].id, "toolu_123");
                assert_eq!(calls[0].name, "search");
            }
            _ => panic!("expected ToolCalls"),
        }
    }
}
