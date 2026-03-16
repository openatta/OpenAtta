//! TG10 — Agent loop robustness & edge-case tests
//!
//! Exercises the ReAct loop under boundary conditions: empty arguments,
//! iteration limits, tool failures, unicode inputs, nested JSON,
//! large results, and many concurrent tool calls.

mod common;

use std::sync::Arc;

use atta_agent::context::ConversationContext;
use atta_agent::llm::LlmResponse;
use atta_agent::react::ReactAgent;
use atta_types::{AgentError, AttaError};

use atta_types::ToolRegistry;

use common::builders::build_agent_default;
use common::fixtures::{make_tool_call, text_response, tool_response};
use common::mock_llm::MockLlmProvider;
use common::mock_tools::{echo_tool_def, failing_tool_def, CountingRegistry};

// ────────────────────────────────────────────────────────────────
// 1. Empty arguments handled
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_empty_arguments_handled() {
    let call = make_tool_call("echo", serde_json::json!({}));
    let llm = Arc::new(MockLlmProvider::new(vec![
        tool_response(vec![call]),
        text_response("Handled empty args."),
    ]));
    let registry = Arc::new(CountingRegistry::new(vec![echo_tool_def()]));
    let count = registry.invocation_count.clone();

    let mut agent = build_agent_default(llm, registry, "Empty args test");

    let result = agent.run().await.expect("agent should succeed");
    assert_eq!(result["answer"], "Handled empty args.");
    assert_eq!(*count.lock().unwrap(), 1);
}

// ────────────────────────────────────────────────────────────────
// 2. Max iterations enforced (default = 20)
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_max_iterations_enforced() {
    // Provide 30 tool responses; only 20 should execute (default max)
    let call = make_tool_call("echo", serde_json::json!({"message": "loop"}));
    let responses: Vec<LlmResponse> = (0..30).map(|_| tool_response(vec![call.clone()])).collect();
    let llm = Arc::new(MockLlmProvider::new(responses));
    let registry = Arc::new(CountingRegistry::new(vec![echo_tool_def()]));
    let count = registry.invocation_count.clone();

    let mut agent = build_agent_default(llm, registry, "Loop many times");

    let err = agent.run().await.expect_err("should hit max iterations");
    match err {
        AttaError::Agent(AgentError::MaxIterations(20)) => {}
        other => panic!("expected MaxIterations(20), got: {other:?}"),
    }
    assert!(
        *count.lock().unwrap() <= 20,
        "at most 20 invocations allowed, got {}",
        *count.lock().unwrap()
    );
}

// ────────────────────────────────────────────────────────────────
// 3. Max iterations custom limit = 5
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_max_iterations_custom_limit() {
    let call = make_tool_call("echo", serde_json::json!({"message": "x"}));
    let responses: Vec<LlmResponse> = (0..10).map(|_| tool_response(vec![call.clone()])).collect();
    let llm = Arc::new(MockLlmProvider::new(responses));
    let registry = Arc::new(CountingRegistry::new(vec![echo_tool_def()]));
    let count = registry.invocation_count.clone();

    let mut ctx = ConversationContext::new(128_000);
    ctx.set_system("Test agent.");
    ctx.add_user("Custom limit 5.");

    let reg_trait: Arc<dyn ToolRegistry> = registry;
    let tools = reg_trait.list_schemas();
    let mut agent = ReactAgent::new(llm, reg_trait, ctx, 5).with_tools(tools);

    let err = agent.run().await.expect_err("should hit max iterations");
    match err {
        AttaError::Agent(AgentError::MaxIterations(5)) => {}
        other => panic!("expected MaxIterations(5), got: {other:?}"),
    }
    assert!(
        *count.lock().unwrap() <= 5,
        "at most 5 invocations, got {}",
        *count.lock().unwrap()
    );
}

// ────────────────────────────────────────────────────────────────
// 4. Failing tool recovery
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_failing_tool_recovery() {
    // Tool fails → error in context → LLM sees error → returns text
    let call = make_tool_call("failing_tool", serde_json::json!({}));
    let llm = Arc::new(MockLlmProvider::new(vec![
        tool_response(vec![call]),
        text_response("Recovered after failure."),
    ]));
    let registry =
        Arc::new(CountingRegistry::new(vec![failing_tool_def()]).with_failing("failing_tool"));

    let mut agent = build_agent_default(llm, registry, "Fail then recover");

    let result = agent.run().await.expect("agent should recover");
    assert_eq!(result["answer"], "Recovered after failure.");
}

// ────────────────────────────────────────────────────────────────
// 5. Mixed success and failure tools
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_mixed_success_and_failure() {
    let echo_call = make_tool_call("echo", serde_json::json!({"message": "works"}));
    let fail_call = make_tool_call("failing_tool", serde_json::json!({}));
    // Both calls in a single response
    let llm = Arc::new(MockLlmProvider::new(vec![
        tool_response(vec![echo_call, fail_call]),
        text_response("Mixed results processed."),
    ]));
    let registry = Arc::new(
        CountingRegistry::new(vec![echo_tool_def(), failing_tool_def()])
            .with_failing("failing_tool"),
    );
    let count = registry.invocation_count.clone();

    let mut agent = build_agent_default(llm, registry, "Mixed tools");

    let result = agent.run().await.expect("agent should succeed");
    assert_eq!(result["answer"], "Mixed results processed.");
    // Both tools should have been invoked (echo succeeds, failing_tool fails)
    assert_eq!(*count.lock().unwrap(), 2);
}

// ────────────────────────────────────────────────────────────────
// 6. Unicode tool arguments
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_unicode_tool_arguments() {
    let call = make_tool_call(
        "echo",
        serde_json::json!({"message": "こんにちは世界 🌍🎉"}),
    );
    let llm = Arc::new(MockLlmProvider::new(vec![
        tool_response(vec![call]),
        text_response("Unicode processed."),
    ]));
    let registry = Arc::new(CountingRegistry::new(vec![echo_tool_def()]));

    let mut agent = build_agent_default(llm, registry, "Unicode test");

    let result = agent.run().await.expect("agent should handle unicode");
    assert_eq!(result["answer"], "Unicode processed.");
}

// ────────────────────────────────────────────────────────────────
// 7. Nested JSON arguments
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_nested_json_arguments() {
    let nested_args = serde_json::json!({
        "query": "{\"nested\": true}",
        "options": {
            "depth": 3,
            "filters": ["a", "b"]
        }
    });
    let call = make_tool_call("echo", nested_args);
    let llm = Arc::new(MockLlmProvider::new(vec![
        tool_response(vec![call]),
        text_response("Nested JSON works."),
    ]));
    let registry = Arc::new(CountingRegistry::new(vec![echo_tool_def()]));

    let mut agent = build_agent_default(llm, registry, "Nested JSON test");

    let result = agent.run().await.expect("agent should handle nested JSON");
    assert_eq!(result["answer"], "Nested JSON works.");
}

// ────────────────────────────────────────────────────────────────
// 8. Tool then immediate text (only 2 LLM calls)
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_tool_then_immediate_text() {
    let call = make_tool_call("echo", serde_json::json!({"message": "fast"}));
    let llm = Arc::new(MockLlmProvider::new(vec![
        tool_response(vec![call]),
        text_response("Immediate text."),
    ]));
    let registry = Arc::new(CountingRegistry::new(vec![echo_tool_def()]));

    let mut agent = build_agent_default(llm, registry, "Quick cycle");

    let result = agent.run().await.expect("agent should succeed");
    assert_eq!(result["answer"], "Immediate text.");
    // The MockLlmProvider should have exactly 0 remaining responses
    // (both were consumed — one for tool call, one for text)
}

// ────────────────────────────────────────────────────────────────
// 9. Large tool result (10 KB)
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_large_tool_result() {
    // The echo tool returns the message; we send a large message.
    let large_message = "x".repeat(10_240); // 10 KB
    let call = make_tool_call("echo", serde_json::json!({"message": large_message}));
    let llm = Arc::new(MockLlmProvider::new(vec![
        tool_response(vec![call]),
        text_response("Processed large result."),
    ]));
    let registry = Arc::new(CountingRegistry::new(vec![echo_tool_def()]));

    let mut agent = build_agent_default(llm, registry, "Large result test");

    let result = agent
        .run()
        .await
        .expect("agent should handle large results");
    assert_eq!(result["answer"], "Processed large result.");
}

// ────────────────────────────────────────────────────────────────
// 10. Many tool calls in single response (5 calls)
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_many_tool_calls_in_single_response() {
    let calls: Vec<_> = (0..5)
        .map(|i| atta_agent::llm::ToolCall {
            id: format!("tc_{i}"),
            name: "echo".to_string(),
            arguments: serde_json::json!({"message": format!("msg_{i}")}),
        })
        .collect();
    let llm = Arc::new(MockLlmProvider::new(vec![
        tool_response(calls),
        text_response("All five done."),
    ]));
    let registry = Arc::new(CountingRegistry::new(vec![echo_tool_def()]));
    let count = registry.invocation_count.clone();

    let mut agent = build_agent_default(llm, registry, "Five tools at once");

    let result = agent.run().await.expect("agent should succeed");
    assert_eq!(result["answer"], "All five done.");
    assert_eq!(
        *count.lock().unwrap(),
        5,
        "all 5 tool calls should be invoked"
    );
}
