//! LLM Provider trait 与相关类型
//!
//! 定义与大语言模型交互的抽象接口。不同 LLM 后端（OpenAI、Anthropic 等）
//! 实现 [`LlmProvider`] trait 即可接入 Agent 执行循环。

use std::pin::Pin;

use atta_types::{AttaError, TokenUsage, ToolSchema};
use futures::Stream;
use serde::{Deserialize, Serialize};

/// 对话消息（类型化枚举）
///
/// 替代原来的 `ChatMessage { role, content }` 扁平结构，
/// 使用带标签的枚举精确表达每种消息角色及其内容结构。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role", content = "content")]
pub enum Message {
    /// System prompt
    #[serde(rename = "system")]
    System(String),
    /// 用户消息
    #[serde(rename = "user")]
    User(String),
    /// Assistant 纯文本回复
    #[serde(rename = "assistant")]
    Assistant(String),
    /// Assistant 发起的 tool 调用
    #[serde(rename = "tool_calls")]
    AssistantToolCalls(Vec<ToolCall>),
    /// Tool 执行结果
    #[serde(rename = "tool")]
    ToolResult {
        /// 对应的 ToolCall ID
        tool_call_id: String,
        /// 执行结果文本
        content: String,
    },
}

/// 模型元信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    /// 模型标识（如 "gpt-4o"、"claude-3-opus"）
    pub model_id: String,
    /// 上下文窗口大小（token 数）
    pub context_window: usize,
    /// 是否支持 tool calling
    pub supports_tools: bool,
    /// 提供者名称（如 "openai"、"anthropic"）
    pub provider: String,
    /// 是否支持流式输出
    pub supports_streaming: bool,
}

/// LLM 返回的 tool 调用
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// 调用 ID（用于关联 tool result）
    pub id: String,
    /// Tool 名称
    pub name: String,
    /// 调用参数（JSON 对象）
    pub arguments: serde_json::Value,
}

/// LLM 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LlmResponse {
    /// 纯文本回复
    Message(String),
    /// 请求调用一个或多个 Tool
    ToolCalls(Vec<ToolCall>),
}

/// LLM response paired with token usage information
#[derive(Debug, Clone)]
pub struct LlmResult {
    /// The response content
    pub response: LlmResponse,
    /// Token usage from the API call (if available)
    pub usage: Option<TokenUsage>,
}

// ── Streaming types ──

/// 流式输出块
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamChunk {
    /// 文本增量
    TextDelta { delta: String },
    /// Tool call 增量
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments_delta: String,
    },
    /// 流结束
    Done,
}

/// 流式 LLM 响应流
pub type LlmStream = Pin<Box<dyn Stream<Item = Result<StreamChunk, AttaError>> + Send>>;

// ── Thinking Level ──

/// LLM 思考深度配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ThinkingLevel {
    /// 低思考深度 (Anthropic: 无 thinking / OpenAI: reasoning_effort=low)
    Low,
    /// 中等思考深度（默认）
    Medium,
    /// 高思考深度 (OpenAI: reasoning_effort=high)
    High,
    /// 扩展思考 (Anthropic: thinking.budget_tokens=N)
    Extended(u32),
}

impl Default for ThinkingLevel {
    fn default() -> Self {
        Self::Medium
    }
}

/// Chat 调用选项
#[derive(Debug, Clone)]
pub struct ChatOptions {
    /// 思考深度
    pub thinking_level: ThinkingLevel,
    /// 温度参数
    pub temperature: Option<f32>,
}

impl Default for ChatOptions {
    fn default() -> Self {
        Self {
            thinking_level: ThinkingLevel::Medium,
            temperature: None,
        }
    }
}

/// LLM 提供者 trait
///
/// 所有 LLM 后端（OpenAI、Anthropic、本地模型等）统一实现此接口，
/// Agent 执行器通过此 trait 与模型交互。
#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync + 'static {
    /// 发送对话消息与可用 tool 列表，获取 LLM 响应
    async fn chat(
        &self,
        messages: &[Message],
        tools: &[ToolSchema],
    ) -> Result<LlmResponse, AttaError>;

    /// 流式发送对话消息与可用 tool 列表
    ///
    /// 默认实现将非流式 `chat()` 结果包装为单个 `TextDelta` + `Done`。
    async fn chat_stream(
        &self,
        messages: &[Message],
        tools: &[ToolSchema],
    ) -> Result<LlmStream, AttaError> {
        let response = self.chat(messages, tools).await?;
        let stream = async_stream::stream! {
            match response {
                LlmResponse::Message(text) => {
                    yield Ok(StreamChunk::TextDelta { delta: text });
                    yield Ok(StreamChunk::Done);
                }
                LlmResponse::ToolCalls(calls) => {
                    for (i, tc) in calls.into_iter().enumerate() {
                        yield Ok(StreamChunk::ToolCallDelta {
                            index: i,
                            id: Some(tc.id),
                            name: Some(tc.name),
                            arguments_delta: tc.arguments.to_string(),
                        });
                    }
                    yield Ok(StreamChunk::Done);
                }
            }
        };
        Ok(Box::pin(stream))
    }

    /// 带选项的流式 chat 调用
    ///
    /// 默认实现忽略 options，委托 `chat_stream()`。
    async fn chat_stream_with_options(
        &self,
        messages: &[Message],
        tools: &[ToolSchema],
        _options: &ChatOptions,
    ) -> Result<LlmStream, AttaError> {
        self.chat_stream(messages, tools).await
    }

    /// 带选项的 chat 调用
    ///
    /// 默认实现忽略 options，委托 `chat()`。
    async fn chat_with_options(
        &self,
        messages: &[Message],
        tools: &[ToolSchema],
        _options: &ChatOptions,
    ) -> Result<LlmResponse, AttaError> {
        self.chat(messages, tools).await
    }

    /// 带 usage 信息的 chat 调用
    ///
    /// 返回 LlmResult（包含 response + token usage）。
    /// 默认实现委托 `chat_with_options()` 并返回 None usage。
    async fn chat_with_usage(
        &self,
        messages: &[Message],
        tools: &[ToolSchema],
        options: &ChatOptions,
    ) -> Result<LlmResult, AttaError> {
        let response = self.chat_with_options(messages, tools, options).await?;
        Ok(LlmResult {
            response,
            usage: None,
        })
    }

    /// 返回模型元信息
    fn model_info(&self) -> ModelInfo;
}
