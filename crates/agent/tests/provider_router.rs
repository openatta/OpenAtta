//! TG14: RouterProvider hint-based routing integration tests
//!
//! Verifies that RouterProvider extracts `hint:xxx` from the last user message,
//! routes to the matching provider, and falls back to default when no match is found.

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use atta_agent::llm::{LlmProvider, LlmResponse, Message};
use atta_agent::provider::RouterProvider;
use common::mock_llm::MockLlmProvider;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn assert_text_response(resp: &LlmResponse, expected: &str) {
    match resp {
        LlmResponse::Message(text) => assert_eq!(text, expected),
        other => panic!("expected LlmResponse::Message(\"{expected}\"), got {other:?}"),
    }
}

/// Build a RouterProvider with the given default label, plus named route providers.
/// Each provider returns "from <label>" on chat.
fn build_router(default_label: &str, routes: &[(&str, &str)]) -> RouterProvider {
    let default = Arc::new(MockLlmProvider::with_model_id(
        default_label,
        vec![LlmResponse::Message(format!("from {default_label}"))],
    )) as Arc<dyn LlmProvider>;

    let route_map: HashMap<String, Arc<dyn LlmProvider>> = routes
        .iter()
        .map(|(hint, label)| {
            let p = Arc::new(MockLlmProvider::with_model_id(
                label,
                vec![LlmResponse::Message(format!("from {label}"))],
            )) as Arc<dyn LlmProvider>;
            (hint.to_string(), p)
        })
        .collect();

    RouterProvider::new(default, route_map)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// 1. A plain message with no hint uses the default provider.
#[tokio::test]
async fn test_no_hint_uses_default() {
    let router = build_router("default-model", &[("fast", "fast-model")]);

    let messages = vec![Message::User("hello world".to_string())];
    let resp = router.chat(&messages, &[]).await.unwrap();
    assert_text_response(&resp, "from default-model");
}

/// 2. A message with `hint:fast` routes to the "fast" provider.
#[tokio::test]
async fn test_hint_routes_to_matching() {
    let router = build_router("default-model", &[("fast", "fast-model")]);

    let messages = vec![Message::User("hint:fast help me please".to_string())];
    let resp = router.chat(&messages, &[]).await.unwrap();
    assert_text_response(&resp, "from fast-model");
}

/// 3. A hint that does not match any route falls back to default.
#[tokio::test]
async fn test_hint_not_found_uses_default() {
    let router = build_router("default-model", &[("fast", "fast-model")]);

    let messages = vec![Message::User("hint:unknown help me".to_string())];
    let resp = router.chat(&messages, &[]).await.unwrap();
    assert_text_response(&resp, "from default-model");
}

/// 4. Multiple routes are registered and each resolves independently.
#[tokio::test]
async fn test_multiple_routes() {
    // Each provider needs its own response queue, so build manually
    let default = Arc::new(MockLlmProvider::with_model_id(
        "default-model",
        vec![LlmResponse::Message("from default-model".to_string())],
    )) as Arc<dyn LlmProvider>;

    let fast = Arc::new(MockLlmProvider::with_model_id(
        "fast-model",
        vec![LlmResponse::Message("from fast-model".to_string())],
    )) as Arc<dyn LlmProvider>;

    let quality = Arc::new(MockLlmProvider::with_model_id(
        "quality-model",
        vec![LlmResponse::Message("from quality-model".to_string())],
    )) as Arc<dyn LlmProvider>;

    let cheap = Arc::new(MockLlmProvider::with_model_id(
        "cheap-model",
        vec![LlmResponse::Message("from cheap-model".to_string())],
    )) as Arc<dyn LlmProvider>;

    let mut routes = HashMap::new();
    routes.insert("fast".to_string(), fast as Arc<dyn LlmProvider>);
    routes.insert("quality".to_string(), quality as Arc<dyn LlmProvider>);
    routes.insert("cheap".to_string(), cheap as Arc<dyn LlmProvider>);

    let router = RouterProvider::new(default, routes);

    // Test quality route
    let messages = vec![Message::User(
        "hint:quality analyze this deeply".to_string(),
    )];
    let resp = router.chat(&messages, &[]).await.unwrap();
    assert_text_response(&resp, "from quality-model");
}

/// 5. `model_info()` returns the default provider's model info.
#[tokio::test]
async fn test_model_info_returns_default() {
    let default = Arc::new(MockLlmProvider::with_model_id(
        "my-default",
        vec![LlmResponse::Message("ok".to_string())],
    )) as Arc<dyn LlmProvider>;

    let router = RouterProvider::new(default, HashMap::new());
    let info = router.model_info();

    assert_eq!(info.model_id, "my-default");
    assert_eq!(info.provider, "mock");
}

/// 6. Only the last user message's hint is considered; hints in earlier messages are ignored.
#[tokio::test]
async fn test_hint_in_last_user_message_only() {
    let router = build_router("default-model", &[("fast", "fast-model")]);

    let messages = vec![
        // Earlier user message has hint:fast
        Message::User("hint:fast do something quickly".to_string()),
        Message::Assistant("Sure, here's the quick version.".to_string()),
        // Latest user message has no hint
        Message::User("thanks, now summarize it".to_string()),
    ];

    let resp = router.chat(&messages, &[]).await.unwrap();
    // Should use default because the last user message has no hint
    assert_text_response(&resp, "from default-model");
}
