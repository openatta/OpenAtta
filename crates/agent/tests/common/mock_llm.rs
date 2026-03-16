//! Mock LLM providers for integration testing

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use atta_agent::llm::{ChatOptions, LlmProvider, LlmResponse, Message, ModelInfo, ToolCall};
use atta_types::{AttaError, ToolSchema};

/// Mock LLM provider that returns scripted responses in FIFO order.
///
/// Panics if more calls are made than responses are available.
pub struct MockLlmProvider {
    responses: Mutex<VecDeque<LlmResponse>>,
    model: ModelInfo,
}

impl MockLlmProvider {
    pub fn new(responses: Vec<LlmResponse>) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from(responses)),
            model: ModelInfo {
                model_id: "mock-model".to_string(),
                context_window: 128_000,
                supports_tools: true,
                provider: "mock".to_string(),
                supports_streaming: true,
            },
        }
    }

    /// Create a provider with a custom model ID
    pub fn with_model_id(model_id: &str, responses: Vec<LlmResponse>) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from(responses)),
            model: ModelInfo {
                model_id: model_id.to_string(),
                context_window: 128_000,
                supports_tools: true,
                provider: "mock".to_string(),
                supports_streaming: true,
            },
        }
    }

    /// Create a provider that returns a single text message
    pub fn text(msg: &str) -> Self {
        Self::new(vec![LlmResponse::Message(msg.to_string())])
    }

    /// Create a provider that returns tool calls then a final text
    pub fn tool_then_text(calls: Vec<ToolCall>, final_text: &str) -> Self {
        Self::new(vec![
            LlmResponse::ToolCalls(calls),
            LlmResponse::Message(final_text.to_string()),
        ])
    }

    /// How many responses remain
    pub fn remaining(&self) -> usize {
        self.responses.lock().unwrap().len()
    }
}

#[async_trait::async_trait]
impl LlmProvider for MockLlmProvider {
    async fn chat(
        &self,
        _messages: &[Message],
        _tools: &[ToolSchema],
    ) -> Result<LlmResponse, AttaError> {
        let mut guard = self.responses.lock().unwrap();
        Ok(guard
            .pop_front()
            .expect("MockLlmProvider: no more scripted responses"))
    }

    async fn chat_with_options(
        &self,
        messages: &[Message],
        tools: &[ToolSchema],
        _options: &ChatOptions,
    ) -> Result<LlmResponse, AttaError> {
        self.chat(messages, tools).await
    }

    fn model_info(&self) -> ModelInfo {
        self.model.clone()
    }
}

/// Recording LLM provider that captures all requests while returning scripted responses.
pub struct RecordingLlmProvider {
    inner: MockLlmProvider,
    recorded: Arc<Mutex<Vec<Vec<Message>>>>,
}

impl RecordingLlmProvider {
    pub fn new(responses: Vec<LlmResponse>) -> (Self, Arc<Mutex<Vec<Vec<Message>>>>) {
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let provider = Self {
            inner: MockLlmProvider::new(responses),
            recorded: recorded.clone(),
        };
        (provider, recorded)
    }
}

#[async_trait::async_trait]
impl LlmProvider for RecordingLlmProvider {
    async fn chat(
        &self,
        messages: &[Message],
        tools: &[ToolSchema],
    ) -> Result<LlmResponse, AttaError> {
        self.recorded.lock().unwrap().push(messages.to_vec());
        self.inner.chat(messages, tools).await
    }

    async fn chat_with_options(
        &self,
        messages: &[Message],
        tools: &[ToolSchema],
        _options: &ChatOptions,
    ) -> Result<LlmResponse, AttaError> {
        self.chat(messages, tools).await
    }

    fn model_info(&self) -> ModelInfo {
        self.inner.model_info()
    }
}

/// The kind of error a FailingLlmProvider should produce.
#[derive(Debug, Clone)]
pub enum FailKind {
    RateLimited,
    AuthError,
}

/// Failing LLM provider — always returns an error
pub struct FailingLlmProvider {
    kind: FailKind,
    model: ModelInfo,
}

impl FailingLlmProvider {
    pub fn rate_limited() -> Self {
        Self {
            kind: FailKind::RateLimited,
            model: ModelInfo {
                model_id: "fail-model".to_string(),
                context_window: 128_000,
                supports_tools: true,
                provider: "fail".to_string(),
                supports_streaming: false,
            },
        }
    }

    pub fn auth_error() -> Self {
        Self {
            kind: FailKind::AuthError,
            model: ModelInfo {
                model_id: "fail-model".to_string(),
                context_window: 128_000,
                supports_tools: true,
                provider: "fail".to_string(),
                supports_streaming: false,
            },
        }
    }

    /// Create a FailingLlmProvider with a custom model ID (useful for identifying providers in failover)
    pub fn with_model_id(kind: FailKind, model_id: &str) -> Self {
        Self {
            kind,
            model: ModelInfo {
                model_id: model_id.to_string(),
                context_window: 128_000,
                supports_tools: true,
                provider: "fail".to_string(),
                supports_streaming: false,
            },
        }
    }

    fn make_error(&self) -> AttaError {
        match &self.kind {
            FailKind::RateLimited => AttaError::Llm(atta_types::LlmError::RateLimited {
                retry_after_secs: 1,
            }),
            FailKind::AuthError => {
                AttaError::Llm(atta_types::LlmError::AuthError("invalid key".to_string()))
            }
        }
    }
}

#[async_trait::async_trait]
impl LlmProvider for FailingLlmProvider {
    async fn chat(
        &self,
        _messages: &[Message],
        _tools: &[ToolSchema],
    ) -> Result<LlmResponse, AttaError> {
        Err(self.make_error())
    }

    fn model_info(&self) -> ModelInfo {
        self.model.clone()
    }
}
