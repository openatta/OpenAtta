//! AttaOS Agent 执行器
//!
//! 提供 Agent 执行所需的核心抽象：
//! - [`llm::LlmProvider`] — LLM 交互接口
//! - [`context::ConversationContext`] — 对话上下文管理
//! - [`react::ReactAgent`] — ReAct 循环执行引擎
//! - [`provider`] — LLM Provider 实现（OpenAI、Anthropic、Reliable、Router）
//! - [`tool_executor`] — 并行/串行工具执行
//! - [`research`] — 研究阶段迷你 agent

pub mod background;
pub mod context;
pub mod dispatcher;
pub mod hooks;
pub mod llm;
pub mod prompt;
pub mod provider;
pub mod react;
pub mod research;
pub mod tool_executor;

pub use context::ConversationContext;
pub use dispatcher::{select_dispatcher, DispatchResult, ToolDispatcher};
pub use llm::{
    ChatOptions, LlmProvider, LlmResponse, LlmResult, LlmStream, Message, ModelInfo, StreamChunk,
    ThinkingLevel, ToolCall,
};
pub use prompt::{
    GuardAction, GuardCategory, PromptContext, PromptGuard, PromptSection, SkillsPromptMode,
    SystemPromptBuilder,
};
pub use provider::{
    AnthropicProvider, DeepSeekProvider, OpenAiProvider, ReliableProvider, RouterProvider,
};
pub use react::{ReactAgent, UsageCallback};
