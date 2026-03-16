//! Reliable Provider with failover chain
//!
//! Wraps multiple [`LlmProvider`] instances and tries them in priority order.
//! Supports error classification: auth errors skip, rate limits backoff,
//! context window exceeded tries next provider, request failures retry.

use std::sync::Arc;
use std::time::{Duration, Instant};

use atta_types::{AttaError, LlmError, ToolSchema};
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::llm::{ChatOptions, LlmProvider, LlmResponse, LlmResult, Message, ModelInfo};

/// Backoff state for a single provider
struct BackoffState {
    /// When the backoff expires (None = not in backoff)
    until: Option<Instant>,
    /// Current backoff duration
    duration: Duration,
}

impl BackoffState {
    fn new() -> Self {
        Self {
            until: None,
            duration: Duration::from_secs(1),
        }
    }

    fn is_available(&self) -> bool {
        match self.until {
            None => true,
            Some(until) => Instant::now() >= until,
        }
    }

    fn enter_backoff(&mut self) {
        self.until = Some(Instant::now() + self.duration);
        // Exponential backoff: 1s → 2s → 4s → ... → 60s cap
        self.duration = (self.duration * 2).min(Duration::from_secs(60));
    }

    fn reset(&mut self) {
        self.until = None;
        self.duration = Duration::from_secs(1);
    }
}

/// A provider entry in the failover chain
struct ProviderEntry {
    provider: Arc<dyn LlmProvider>,
    backoff: Arc<Mutex<BackoffState>>,
}

/// Reliable Provider with fallback chain
///
/// Tries providers in priority order. On failure:
/// - `AuthError` → skip to next (don't retry)
/// - `RateLimited` → enter backoff, try next
/// - `ContextWindowExceeded` → try next provider (may have larger window)
/// - `RequestFailed` → enter backoff, try next
pub struct ReliableProvider {
    providers: Vec<ProviderEntry>,
}

impl ReliableProvider {
    /// Create a new ReliableProvider from a list of providers (ordered by priority)
    pub fn new(providers: Vec<Arc<dyn LlmProvider>>) -> Self {
        let entries = providers
            .into_iter()
            .map(|p| ProviderEntry {
                provider: p,
                backoff: Arc::new(Mutex::new(BackoffState::new())),
            })
            .collect();
        Self { providers: entries }
    }

    /// Classify error and decide action
    fn should_try_next(err: &AttaError) -> bool {
        matches!(
            err,
            AttaError::Llm(LlmError::AuthError(_))
                | AttaError::Llm(LlmError::RateLimited { .. })
                | AttaError::Llm(LlmError::ContextWindowExceeded { .. })
                | AttaError::Llm(LlmError::RequestFailed(_))
        )
    }

    fn should_backoff(err: &AttaError) -> bool {
        matches!(
            err,
            AttaError::Llm(LlmError::RateLimited { .. })
                | AttaError::Llm(LlmError::RequestFailed(_))
        )
    }
}

#[async_trait::async_trait]
impl LlmProvider for ReliableProvider {
    async fn chat(
        &self,
        messages: &[Message],
        tools: &[ToolSchema],
    ) -> Result<LlmResponse, AttaError> {
        let mut last_error: Option<AttaError> = None;

        for (i, entry) in self.providers.iter().enumerate() {
            let backoff = entry.backoff.lock().await;
            if !backoff.is_available() {
                info!(provider = i, "provider in backoff, skipping");
                continue;
            }
            drop(backoff);

            match entry.provider.chat(messages, tools).await {
                Ok(response) => {
                    // Reset backoff on success
                    entry.backoff.lock().await.reset();
                    return Ok(response);
                }
                Err(e) => {
                    warn!(
                        provider = i,
                        model = %entry.provider.model_info().model_id,
                        error = %e,
                        "provider failed, checking fallback"
                    );

                    if Self::should_backoff(&e) {
                        entry.backoff.lock().await.enter_backoff();
                    }

                    if Self::should_try_next(&e) {
                        last_error = Some(e);
                        continue;
                    }

                    return Err(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            AttaError::Llm(LlmError::RequestFailed(
                "all providers exhausted".to_string(),
            ))
        }))
    }

    async fn chat_with_usage(
        &self,
        messages: &[Message],
        tools: &[ToolSchema],
        options: &ChatOptions,
    ) -> Result<LlmResult, AttaError> {
        let mut last_error: Option<AttaError> = None;

        for (i, entry) in self.providers.iter().enumerate() {
            let backoff = entry.backoff.lock().await;
            if !backoff.is_available() {
                info!(provider = i, "provider in backoff, skipping");
                continue;
            }
            drop(backoff);

            match entry
                .provider
                .chat_with_usage(messages, tools, options)
                .await
            {
                Ok(result) => {
                    entry.backoff.lock().await.reset();
                    return Ok(result);
                }
                Err(e) => {
                    warn!(
                        provider = i,
                        model = %entry.provider.model_info().model_id,
                        error = %e,
                        "provider failed, checking fallback"
                    );

                    if Self::should_backoff(&e) {
                        entry.backoff.lock().await.enter_backoff();
                    }

                    if Self::should_try_next(&e) {
                        last_error = Some(e);
                        continue;
                    }

                    return Err(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            AttaError::Llm(LlmError::RequestFailed(
                "all providers exhausted".to_string(),
            ))
        }))
    }

    fn model_info(&self) -> ModelInfo {
        // Return the first provider's model info
        self.providers
            .first()
            .map(|e| e.provider.model_info())
            .unwrap_or(ModelInfo {
                model_id: "reliable".to_string(),
                context_window: 0,
                supports_tools: false,
                provider: "reliable".to_string(),
                supports_streaming: false,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock provider that always succeeds
    struct OkProvider(String);

    #[async_trait::async_trait]
    impl LlmProvider for OkProvider {
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

    /// Mock provider that always fails
    struct FailProvider(AttaError);

    impl FailProvider {
        fn rate_limited() -> Self {
            Self(AttaError::Llm(LlmError::RateLimited {
                retry_after_secs: 5,
            }))
        }
        fn auth_error() -> Self {
            Self(AttaError::Llm(LlmError::AuthError("bad key".into())))
        }
    }

    #[async_trait::async_trait]
    impl LlmProvider for FailProvider {
        async fn chat(
            &self,
            _messages: &[Message],
            _tools: &[ToolSchema],
        ) -> Result<LlmResponse, AttaError> {
            // Recreate the error since AttaError is not Clone
            match &self.0 {
                AttaError::Llm(LlmError::RateLimited { retry_after_secs }) => {
                    Err(AttaError::Llm(LlmError::RateLimited {
                        retry_after_secs: *retry_after_secs,
                    }))
                }
                AttaError::Llm(LlmError::AuthError(msg)) => {
                    Err(AttaError::Llm(LlmError::AuthError(msg.clone())))
                }
                _ => Err(AttaError::Llm(LlmError::RequestFailed(
                    "unknown".to_string(),
                ))),
            }
        }
        fn model_info(&self) -> ModelInfo {
            ModelInfo {
                model_id: "fail".to_string(),
                context_window: 0,
                supports_tools: false,
                provider: "fail".to_string(),
                supports_streaming: false,
            }
        }
    }

    #[tokio::test]
    async fn test_uses_first_provider() {
        let provider = ReliableProvider::new(vec![
            Arc::new(OkProvider("primary".into())),
            Arc::new(OkProvider("secondary".into())),
        ]);
        let result = provider.chat(&[], &[]).await.unwrap();
        match result {
            LlmResponse::Message(text) => assert_eq!(text, "from primary"),
            _ => panic!("expected Message"),
        }
    }

    #[tokio::test]
    async fn test_falls_back_on_rate_limit() {
        let provider = ReliableProvider::new(vec![
            Arc::new(FailProvider::rate_limited()),
            Arc::new(OkProvider("backup".into())),
        ]);
        let result = provider.chat(&[], &[]).await.unwrap();
        match result {
            LlmResponse::Message(text) => assert_eq!(text, "from backup"),
            _ => panic!("expected Message"),
        }
    }

    #[tokio::test]
    async fn test_falls_back_on_auth_error() {
        let provider = ReliableProvider::new(vec![
            Arc::new(FailProvider::auth_error()),
            Arc::new(OkProvider("backup".into())),
        ]);
        let result = provider.chat(&[], &[]).await.unwrap();
        match result {
            LlmResponse::Message(text) => assert_eq!(text, "from backup"),
            _ => panic!("expected Message"),
        }
    }

    #[tokio::test]
    async fn test_all_fail_returns_last_error() {
        let provider = ReliableProvider::new(vec![
            Arc::new(FailProvider::auth_error()),
            Arc::new(FailProvider::rate_limited()),
        ]);
        let err = provider.chat(&[], &[]).await.unwrap_err();
        assert!(matches!(err, AttaError::Llm(LlmError::RateLimited { .. })));
    }
}
