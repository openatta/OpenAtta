//! TG13: ReliableProvider failover integration tests
//!
//! Verifies that ReliableProvider correctly tries providers in priority order,
//! falls back on RateLimit/AuthError, and returns the last error when all fail.

mod common;

use std::sync::Arc;

use atta_agent::llm::{LlmProvider, LlmResponse, Message};
use atta_agent::provider::ReliableProvider;
use atta_types::{AttaError, LlmError};
use common::mock_llm::{FailKind, FailingLlmProvider, MockLlmProvider};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn user_msg(text: &str) -> Vec<Message> {
    vec![Message::User(text.to_string())]
}

fn assert_text_response(resp: &LlmResponse, expected: &str) {
    match resp {
        LlmResponse::Message(text) => assert_eq!(text, expected),
        other => panic!("expected LlmResponse::Message(\"{expected}\"), got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// 1. When the first provider succeeds, no fallback occurs.
#[tokio::test]
async fn test_first_provider_succeeds() {
    let primary = Arc::new(MockLlmProvider::text("primary answer")) as Arc<dyn LlmProvider>;
    let secondary = Arc::new(MockLlmProvider::text("secondary answer")) as Arc<dyn LlmProvider>;

    let reliable = ReliableProvider::new(vec![primary, secondary]);
    let resp = reliable.chat(&user_msg("hello"), &[]).await.unwrap();
    assert_text_response(&resp, "primary answer");
}

/// 2. A rate-limited first provider causes fallback to second.
#[tokio::test]
async fn test_rate_limited_falls_back() {
    let first = Arc::new(FailingLlmProvider::rate_limited()) as Arc<dyn LlmProvider>;
    let second = Arc::new(MockLlmProvider::text("backup response")) as Arc<dyn LlmProvider>;

    let reliable = ReliableProvider::new(vec![first, second]);
    let resp = reliable.chat(&user_msg("hello"), &[]).await.unwrap();
    assert_text_response(&resp, "backup response");
}

/// 3. An auth-error first provider causes fallback to second.
#[tokio::test]
async fn test_auth_error_falls_back() {
    let first = Arc::new(FailingLlmProvider::auth_error()) as Arc<dyn LlmProvider>;
    let second = Arc::new(MockLlmProvider::text("fallback ok")) as Arc<dyn LlmProvider>;

    let reliable = ReliableProvider::new(vec![first, second]);
    let resp = reliable.chat(&user_msg("hello"), &[]).await.unwrap();
    assert_text_response(&resp, "fallback ok");
}

/// 4. When all providers fail, the last error is returned.
#[tokio::test]
async fn test_all_fail_returns_error() {
    let first = Arc::new(FailingLlmProvider::auth_error()) as Arc<dyn LlmProvider>;
    let second = Arc::new(FailingLlmProvider::auth_error()) as Arc<dyn LlmProvider>;

    let reliable = ReliableProvider::new(vec![first, second]);
    let err = reliable.chat(&user_msg("hello"), &[]).await.unwrap_err();

    assert!(
        matches!(err, AttaError::Llm(LlmError::AuthError(_))),
        "expected AuthError, got {err:?}"
    );
}

/// 5. A single-provider chain succeeds normally.
#[tokio::test]
async fn test_single_provider_success() {
    let only = Arc::new(MockLlmProvider::text("only one")) as Arc<dyn LlmProvider>;

    let reliable = ReliableProvider::new(vec![only]);
    let resp = reliable.chat(&user_msg("hi"), &[]).await.unwrap();
    assert_text_response(&resp, "only one");
}

/// 6. A single failing provider yields an error immediately.
#[tokio::test]
async fn test_single_provider_failure() {
    let only = Arc::new(FailingLlmProvider::auth_error()) as Arc<dyn LlmProvider>;

    let reliable = ReliableProvider::new(vec![only]);
    let err = reliable.chat(&user_msg("hi"), &[]).await.unwrap_err();

    assert!(
        matches!(err, AttaError::Llm(LlmError::AuthError(_))),
        "expected AuthError, got {err:?}"
    );
}

/// 7. Three-provider chain where first two fail, third succeeds.
#[tokio::test]
async fn test_three_providers_first_two_fail() {
    let first = Arc::new(FailingLlmProvider::with_model_id(
        FailKind::RateLimited,
        "provider-1",
    )) as Arc<dyn LlmProvider>;
    let second = Arc::new(FailingLlmProvider::with_model_id(
        FailKind::AuthError,
        "provider-2",
    )) as Arc<dyn LlmProvider>;
    let third = Arc::new(MockLlmProvider::text("third provider saved us")) as Arc<dyn LlmProvider>;

    let reliable = ReliableProvider::new(vec![first, second, third]);
    let resp = reliable.chat(&user_msg("help"), &[]).await.unwrap();
    assert_text_response(&resp, "third provider saved us");
}

/// 8. `model_info()` returns the first provider's model info.
#[tokio::test]
async fn test_model_info_uses_first_provider() {
    let primary = Arc::new(MockLlmProvider::with_model_id(
        "gpt-4o",
        vec![LlmResponse::Message("ok".to_string())],
    )) as Arc<dyn LlmProvider>;
    let secondary = Arc::new(MockLlmProvider::with_model_id(
        "claude-3-opus",
        vec![LlmResponse::Message("ok".to_string())],
    )) as Arc<dyn LlmProvider>;

    let reliable = ReliableProvider::new(vec![primary, secondary]);
    let info = reliable.model_info();

    assert_eq!(info.model_id, "gpt-4o");
}
