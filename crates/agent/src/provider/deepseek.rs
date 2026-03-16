//! DeepSeek API provider
//!
//! Implements [`LlmProvider`] for DeepSeek's chat completions API.
//! DeepSeek is fully compatible with the OpenAI API format.

use atta_types::{AttaError, LlmError, ToolSchema};
use tracing::debug;

use crate::llm::{
    ChatOptions, LlmProvider, LlmResponse, LlmResult, LlmStream, Message, ModelInfo, StreamChunk,
    ThinkingLevel, ToolCall,
};
use atta_types::TokenUsage;

/// DeepSeek API provider
pub struct DeepSeekProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl DeepSeekProvider {
    /// Create a new DeepSeek provider
    pub fn new(api_key: String, model: String, base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            model,
            base_url,
        }
    }

    /// Create from environment variables
    ///
    /// - `DEEPSEEK_API_KEY` — required
    /// - `DEEPSEEK_MODEL` — defaults to "deepseek-chat"
    /// - `DEEPSEEK_BASE_URL` — defaults to "https://api.deepseek.com/v1"
    pub fn from_env() -> Result<Self, AttaError> {
        let api_key = std::env::var("DEEPSEEK_API_KEY").map_err(|_| {
            AttaError::Llm(LlmError::AuthError(
                "DEEPSEEK_API_KEY environment variable not set".to_string(),
            ))
        })?;
        let model = std::env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| "deepseek-chat".to_string());
        let base_url = std::env::var("DEEPSEEK_BASE_URL")
            .unwrap_or_else(|_| "https://api.deepseek.com/v1".to_string());

        Ok(Self::new(api_key, model, base_url))
    }

    /// Convert our Message enum to OpenAI-compatible message format
    fn format_messages(messages: &[Message]) -> Vec<serde_json::Value> {
        messages
            .iter()
            .map(|msg| match msg {
                Message::System(content) => serde_json::json!({
                    "role": "system",
                    "content": content,
                }),
                Message::User(content) => serde_json::json!({
                    "role": "user",
                    "content": content,
                }),
                Message::Assistant(content) => serde_json::json!({
                    "role": "assistant",
                    "content": content,
                }),
                Message::AssistantToolCalls(calls) => {
                    let tool_calls: Vec<serde_json::Value> = calls
                        .iter()
                        .map(|tc| {
                            serde_json::json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": tc.name,
                                    "arguments": tc.arguments.to_string(),
                                }
                            })
                        })
                        .collect();
                    serde_json::json!({
                        "role": "assistant",
                        "tool_calls": tool_calls,
                    })
                }
                Message::ToolResult {
                    tool_call_id,
                    content,
                } => serde_json::json!({
                    "role": "tool",
                    "tool_call_id": tool_call_id,
                    "content": content,
                }),
            })
            .collect()
    }

    /// Convert our ToolSchema to OpenAI-compatible function calling format
    fn format_tools(tools: &[ToolSchema]) -> Vec<serde_json::Value> {
        use super::schema::{clean_tool_schema, CleaningStrategy};
        tools
            .iter()
            .map(|t| {
                let cleaned_params = clean_tool_schema(&t.parameters, &CleaningStrategy::OpenAi);
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": cleaned_params,
                    }
                })
            })
            .collect()
    }

    /// Parse the API response (OpenAI-compatible format)
    fn parse_response(body: &serde_json::Value) -> Result<LlmResponse, AttaError> {
        let choice = body.get("choices").and_then(|c| c.get(0)).ok_or_else(|| {
            AttaError::Llm(LlmError::InvalidResponse(
                "no choices in response".to_string(),
            ))
        })?;

        let message = choice.get("message").ok_or_else(|| {
            AttaError::Llm(LlmError::InvalidResponse(
                "no message in choice".to_string(),
            ))
        })?;

        // Check for tool calls first
        if let Some(tool_calls) = message.get("tool_calls").and_then(|v| v.as_array()) {
            let calls: Result<Vec<ToolCall>, AttaError> = tool_calls
                .iter()
                .map(|tc| {
                    let id = tc
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let function = tc.get("function").ok_or_else(|| {
                        AttaError::Llm(LlmError::InvalidResponse(
                            "missing function in tool_call".to_string(),
                        ))
                    })?;
                    let name = function
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let arguments_str = function
                        .get("arguments")
                        .and_then(|v| v.as_str())
                        .unwrap_or("{}");
                    let arguments: serde_json::Value =
                        serde_json::from_str(arguments_str).unwrap_or(serde_json::json!({}));

                    Ok(ToolCall {
                        id,
                        name,
                        arguments,
                    })
                })
                .collect();

            return Ok(LlmResponse::ToolCalls(calls?));
        }

        // Otherwise, it's a text message
        let content = message
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        Ok(LlmResponse::Message(content))
    }

    /// Build the request body, shared between chat / chat_stream / chat_with_options
    fn build_request_body(&self, messages: &[Message], tools: &[ToolSchema]) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": Self::format_messages(messages),
        });
        if !tools.is_empty() {
            body["tools"] = serde_json::Value::Array(Self::format_tools(tools));
        }
        body
    }

    /// Extract token usage from the API response body (OpenAI-compatible format)
    fn parse_usage(body: &serde_json::Value) -> Option<TokenUsage> {
        let usage = body.get("usage")?;
        let input = usage.get("prompt_tokens")?.as_u64()?;
        let output = usage.get("completion_tokens")?.as_u64()?;
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
impl LlmProvider for DeepSeekProvider {
    async fn chat(
        &self,
        messages: &[Message],
        tools: &[ToolSchema],
    ) -> Result<LlmResponse, AttaError> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = self.build_request_body(messages, tools);

        debug!(model = %self.model, messages = messages.len(), tools = tools.len(), "DeepSeek chat request");

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
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

        debug!("DeepSeek response received");
        Self::parse_response(&response_body)
    }

    async fn chat_stream(
        &self,
        messages: &[Message],
        tools: &[ToolSchema],
    ) -> Result<LlmStream, AttaError> {
        let url = format!("{}/chat/completions", self.base_url);
        let mut body = self.build_request_body(messages, tools);
        body["stream"] = serde_json::json!(true);

        debug!(model = %self.model, "DeepSeek streaming chat request");

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
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

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<StreamChunk, AttaError>>(128);

        tokio::spawn(async move {
            use futures::StreamExt;
            let mut byte_stream = response.bytes_stream();
            let mut line_buf = String::new();

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
                    if line == "data: [DONE]" {
                        let _ = tx.send(Ok(StreamChunk::Done)).await;
                        return;
                    }
                    if let Some(data) = line.strip_prefix("data: ") {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                            if let Some(choices) = json.get("choices").and_then(|c| c.as_array()) {
                                for choice in choices {
                                    let Some(delta) = choice.get("delta") else {
                                        continue;
                                    };

                                    if let Some(content) =
                                        delta.get("content").and_then(|v| v.as_str())
                                    {
                                        if !content.is_empty() {
                                            let _ = tx
                                                .send(Ok(StreamChunk::TextDelta {
                                                    delta: content.to_string(),
                                                }))
                                                .await;
                                        }
                                    }

                                    if let Some(tool_calls) =
                                        delta.get("tool_calls").and_then(|v| v.as_array())
                                    {
                                        for tc in tool_calls {
                                            let index = tc
                                                .get("index")
                                                .and_then(|v| v.as_u64())
                                                .unwrap_or(0)
                                                as usize;
                                            let id = tc
                                                .get("id")
                                                .and_then(|v| v.as_str())
                                                .map(|s| s.to_string());
                                            let function = tc.get("function");
                                            let name = function
                                                .and_then(|f| f.get("name"))
                                                .and_then(|v| v.as_str())
                                                .map(|s| s.to_string());
                                            let args = function
                                                .and_then(|f| f.get("arguments"))
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");

                                            let _ = tx
                                                .send(Ok(StreamChunk::ToolCallDelta {
                                                    index,
                                                    id,
                                                    name,
                                                    arguments_delta: args.to_string(),
                                                }))
                                                .await;
                                        }
                                    }
                                }
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
        let url = format!("{}/chat/completions", self.base_url);
        let mut body = self.build_request_body(messages, tools);

        // Apply thinking level → reasoning_effort
        match &options.thinking_level {
            ThinkingLevel::Low => {
                body["reasoning_effort"] = serde_json::json!("low");
            }
            ThinkingLevel::Medium => {
                body["reasoning_effort"] = serde_json::json!("medium");
            }
            ThinkingLevel::High | ThinkingLevel::Extended(_) => {
                body["reasoning_effort"] = serde_json::json!("high");
            }
        }

        // Apply temperature
        if let Some(temp) = options.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        debug!(model = %self.model, "DeepSeek chat_with_options request");

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
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
        let url = format!("{}/chat/completions", self.base_url);
        let mut body = self.build_request_body(messages, tools);

        match &options.thinking_level {
            ThinkingLevel::Low => {
                body["reasoning_effort"] = serde_json::json!("low");
            }
            ThinkingLevel::Medium => {
                body["reasoning_effort"] = serde_json::json!("medium");
            }
            ThinkingLevel::High | ThinkingLevel::Extended(_) => {
                body["reasoning_effort"] = serde_json::json!("high");
            }
        }
        if let Some(temp) = options.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        debug!(model = %self.model, "DeepSeek chat_with_usage request");

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
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
            context_window: 64_000,
            supports_tools: true,
            provider: "deepseek".to_string(),
            supports_streaming: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_messages() {
        let messages = vec![
            Message::System("You are helpful.".to_string()),
            Message::User("Hello".to_string()),
            Message::Assistant("Hi!".to_string()),
        ];
        let formatted = DeepSeekProvider::format_messages(&messages);
        assert_eq!(formatted.len(), 3);
        assert_eq!(formatted[0]["role"], "system");
        assert_eq!(formatted[1]["role"], "user");
        assert_eq!(formatted[2]["role"], "assistant");
    }

    #[test]
    fn test_format_tools() {
        let tools = vec![ToolSchema {
            name: "search".to_string(),
            description: "Search the web".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                }
            }),
        }];
        let formatted = DeepSeekProvider::format_tools(&tools);
        assert_eq!(formatted.len(), 1);
        assert_eq!(formatted[0]["type"], "function");
        assert_eq!(formatted[0]["function"]["name"], "search");
    }

    #[test]
    fn test_parse_text_response() {
        let body = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Hello, world!"
                }
            }]
        });
        let result = DeepSeekProvider::parse_response(&body).unwrap();
        match result {
            LlmResponse::Message(text) => assert_eq!(text, "Hello, world!"),
            _ => panic!("expected Message"),
        }
    }

    #[test]
    fn test_parse_tool_call_response() {
        let body = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_123",
                        "type": "function",
                        "function": {
                            "name": "search",
                            "arguments": "{\"query\":\"rust\"}"
                        }
                    }]
                }
            }]
        });
        let result = DeepSeekProvider::parse_response(&body).unwrap();
        match result {
            LlmResponse::ToolCalls(calls) => {
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].id, "call_123");
                assert_eq!(calls[0].name, "search");
                assert_eq!(calls[0].arguments, serde_json::json!({"query": "rust"}));
            }
            _ => panic!("expected ToolCalls"),
        }
    }
}
