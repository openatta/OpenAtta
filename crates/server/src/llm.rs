//! LLM provider initialization
//!
//! Detects available API keys and builds the appropriate [`LlmProvider`]
//! (single provider or [`ReliableProvider`] with failover chain).

use std::sync::Arc;

use anyhow::{Context, Result};

use atta_agent::{
    AnthropicProvider, DeepSeekProvider, LlmProvider, ModelInfo, OpenAiProvider, ReliableProvider,
};

/// Detect and create LLM provider from env vars.
///
/// When multiple API keys are set, builds a [`ReliableProvider`] with failover chain
/// (Anthropic -> OpenAI -> DeepSeek). Otherwise uses a single provider directly.
pub(crate) fn build_llm_provider() -> Result<Arc<dyn LlmProvider>> {
    let has_anthropic = std::env::var("ANTHROPIC_API_KEY").is_ok();
    let has_openai = std::env::var("OPENAI_API_KEY").is_ok();
    let has_deepseek = std::env::var("DEEPSEEK_API_KEY").is_ok();

    let mut providers: Vec<Arc<dyn LlmProvider>> = Vec::new();

    if has_anthropic {
        let provider =
            AnthropicProvider::from_env().context("failed to create Anthropic provider")?;
        providers.push(Arc::new(provider));
    }
    if has_openai {
        let provider = OpenAiProvider::from_env().context("failed to create OpenAI provider")?;
        providers.push(Arc::new(provider));
    }
    if has_deepseek {
        let provider =
            DeepSeekProvider::from_env().context("failed to create DeepSeek provider")?;
        providers.push(Arc::new(provider));
    }

    match providers.len() {
        0 => {
            tracing::warn!(
                "no LLM API key found (ANTHROPIC_API_KEY, OPENAI_API_KEY, or DEEPSEEK_API_KEY). Agent execution will fail."
            );
            Ok(Arc::new(NoopLlmProvider))
        }
        1 => {
            let provider = providers.into_iter().next().unwrap();
            tracing::info!(
                model = %provider.model_info().model_id,
                provider = %provider.model_info().provider,
                "using single LLM provider"
            );
            Ok(provider)
        }
        n => {
            let model_ids: Vec<String> = providers
                .iter()
                .map(|p| p.model_info().model_id.clone())
                .collect();
            tracing::info!(
                providers = ?model_ids,
                "using ReliableProvider with {n} providers"
            );
            Ok(Arc::new(ReliableProvider::new(providers)))
        }
    }
}

/// Stub LLM provider when no API key is configured
struct NoopLlmProvider;

#[async_trait::async_trait]
impl LlmProvider for NoopLlmProvider {
    async fn chat(
        &self,
        _messages: &[atta_agent::Message],
        _tools: &[atta_types::ToolSchema],
    ) -> Result<atta_agent::LlmResponse, atta_types::AttaError> {
        Err(atta_types::AttaError::Llm(atta_types::LlmError::AuthError(
            "no LLM provider configured — set ANTHROPIC_API_KEY, OPENAI_API_KEY, or DEEPSEEK_API_KEY".to_string(),
        )))
    }

    fn model_info(&self) -> ModelInfo {
        ModelInfo {
            model_id: "none".to_string(),
            context_window: 0,
            supports_tools: false,
            provider: "none".to_string(),
            supports_streaming: false,
        }
    }
}
