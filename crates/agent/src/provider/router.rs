//! Router Provider with hint-based routing
//!
//! Routes LLM requests to different providers based on `hint:xxx` tags
//! extracted from the last user message.

use std::collections::HashMap;
use std::sync::Arc;

use atta_types::{AttaError, ToolSchema};

use crate::llm::{ChatOptions, LlmProvider, LlmResponse, LlmResult, Message, ModelInfo};

/// Hint-based router provider
///
/// Inspects the last user message for `hint:xxx` tags and routes
/// to the matching provider. Falls back to the default provider
/// if no hint matches.
pub struct RouterProvider {
    routes: HashMap<String, Arc<dyn LlmProvider>>,
    default: Arc<dyn LlmProvider>,
}

impl RouterProvider {
    /// Create a new RouterProvider
    ///
    /// # Arguments
    /// * `default` - Default provider when no hint matches
    /// * `routes` - Map of hint name → provider (e.g., "fast" → OpenAI, "quality" → Anthropic)
    pub fn new(
        default: Arc<dyn LlmProvider>,
        routes: HashMap<String, Arc<dyn LlmProvider>>,
    ) -> Self {
        Self { routes, default }
    }

    /// Extract hint from the last user message.
    /// Looks for patterns like `hint:fast`, `hint:quality` etc.
    fn extract_hint(messages: &[Message]) -> Option<String> {
        // Find the last user message
        let last_user = messages.iter().rev().find_map(|m| match m {
            Message::User(content) => Some(content.as_str()),
            _ => None,
        })?;

        // Look for hint:xxx pattern
        for word in last_user.split_whitespace() {
            if let Some(hint) = word.strip_prefix("hint:") {
                let hint = hint.trim_end_matches(|c: char| !c.is_alphanumeric());
                if !hint.is_empty() {
                    return Some(hint.to_lowercase());
                }
            }
        }

        None
    }

    /// Select the provider based on hint
    fn select_provider(&self, messages: &[Message]) -> &dyn LlmProvider {
        if let Some(hint) = Self::extract_hint(messages) {
            if let Some(provider) = self.routes.get(&hint) {
                tracing::info!(hint = %hint, "routing to hint-matched provider");
                return provider.as_ref();
            }
            tracing::debug!(hint = %hint, "no provider for hint, using default");
        }
        self.default.as_ref()
    }
}

#[async_trait::async_trait]
impl LlmProvider for RouterProvider {
    async fn chat(
        &self,
        messages: &[Message],
        tools: &[ToolSchema],
    ) -> Result<LlmResponse, AttaError> {
        self.select_provider(messages).chat(messages, tools).await
    }

    async fn chat_with_usage(
        &self,
        messages: &[Message],
        tools: &[ToolSchema],
        options: &ChatOptions,
    ) -> Result<LlmResult, AttaError> {
        self.select_provider(messages)
            .chat_with_usage(messages, tools, options)
            .await
    }

    fn model_info(&self) -> ModelInfo {
        self.default.model_info()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockProvider(String);

    #[async_trait::async_trait]
    impl LlmProvider for MockProvider {
        async fn chat(
            &self,
            _messages: &[Message],
            _tools: &[ToolSchema],
        ) -> Result<LlmResponse, AttaError> {
            Ok(LlmResponse::Message(format!("from {}", self.0)))
        }
        fn model_info(&self) -> ModelInfo {
            ModelInfo {
                model_id: self.0.clone(),
                context_window: 128_000,
                supports_tools: true,
                provider: self.0.clone(),
                supports_streaming: false,
            }
        }
    }

    #[test]
    fn test_extract_hint() {
        let messages = vec![Message::User("hello hint:fast please".to_string())];
        assert_eq!(RouterProvider::extract_hint(&messages), Some("fast".into()));
    }

    #[test]
    fn test_extract_hint_none() {
        let messages = vec![Message::User("hello world".to_string())];
        assert_eq!(RouterProvider::extract_hint(&messages), None);
    }

    #[test]
    fn test_extract_hint_last_user() {
        let messages = vec![
            Message::User("hint:quality first".to_string()),
            Message::Assistant("ok".to_string()),
            Message::User("hint:fast second".to_string()),
        ];
        assert_eq!(RouterProvider::extract_hint(&messages), Some("fast".into()));
    }

    #[tokio::test]
    async fn test_routes_to_hint() {
        let default: Arc<dyn LlmProvider> = Arc::new(MockProvider("default".into()));
        let fast: Arc<dyn LlmProvider> = Arc::new(MockProvider("fast-model".into()));

        let mut routes = HashMap::new();
        routes.insert("fast".to_string(), fast);

        let router = RouterProvider::new(default, routes);
        let messages = vec![Message::User("hint:fast do something".to_string())];

        let result = router.chat(&messages, &[]).await.unwrap();
        match result {
            LlmResponse::Message(text) => assert_eq!(text, "from fast-model"),
            _ => panic!("expected Message"),
        }
    }

    #[tokio::test]
    async fn test_routes_to_default() {
        let default: Arc<dyn LlmProvider> = Arc::new(MockProvider("default".into()));

        let router = RouterProvider::new(default, HashMap::new());
        let messages = vec![Message::User("no hint here".to_string())];

        let result = router.chat(&messages, &[]).await.unwrap();
        match result {
            LlmResponse::Message(text) => assert_eq!(text, "from default"),
            _ => panic!("expected Message"),
        }
    }
}
