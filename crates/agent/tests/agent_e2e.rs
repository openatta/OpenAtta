//! TG9 — Full ReAct cycle integration tests
//!
//! Verifies end-to-end agent execution through the ReactAgent,
//! including text responses, tool call cycles, multi-step chains,
//! error handling, and max-iteration enforcement.

mod common;

use std::sync::Arc;

use atta_agent::context::ConversationContext;
use atta_agent::llm::LlmResponse;
use atta_agent::react::ReactAgent;
use atta_types::{AgentError, AttaError};

use atta_types::ToolRegistry;

use common::builders::build_agent_default;
use common::fixtures::{make_tool_call, text_response, tool_response};
use common::mock_llm::{MockLlmProvider, RecordingLlmProvider};
use common::mock_tools::{echo_tool_def, failing_tool_def, CountingRegistry, SimpleRegistry};

// ────────────────────────────────────────────────────────────────
// 1. Simple text response
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_simple_text_response() {
    let llm = Arc::new(MockLlmProvider::text("Hello, user!"));
    let registry = Arc::new(SimpleRegistry::empty());
    let mut agent = build_agent_default(llm, registry, "Hi");

    let result = agent.run().await.expect("agent should succeed");

    assert_eq!(result["answer"], "Hello, user!");
}

// ────────────────────────────────────────────────────────────────
// 2. Single tool call cycle
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_single_tool_call_cycle() {
    let tool_call = make_tool_call("echo", serde_json::json!({"message": "ping"}));
    let llm = Arc::new(MockLlmProvider::tool_then_text(
        vec![tool_call],
        "Done calling echo.",
    ));
    let registry = Arc::new(CountingRegistry::new(vec![echo_tool_def()]));
    let count = registry.invocation_count.clone();

    let mut agent = build_agent_default(llm, registry, "Call echo");

    let result = agent.run().await.expect("agent should succeed");

    assert_eq!(result["answer"], "Done calling echo.");
    assert_eq!(
        *count.lock().unwrap(),
        1,
        "echo tool should be invoked once"
    );
}

// ────────────────────────────────────────────────────────────────
// 3. Multi-step tool chain (2 tool calls then final text)
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_multi_step_tool_chain() {
    let call_1 = make_tool_call("echo", serde_json::json!({"message": "step1"}));
    let call_2 = make_tool_call("echo", serde_json::json!({"message": "step2"}));
    let responses = vec![
        tool_response(vec![call_1]),
        tool_response(vec![call_2]),
        text_response("All done."),
    ];
    let llm = Arc::new(MockLlmProvider::new(responses));
    let registry = Arc::new(CountingRegistry::new(vec![echo_tool_def()]));
    let count = registry.invocation_count.clone();

    let mut agent = build_agent_default(llm, registry, "Do two things");

    let result = agent.run().await.expect("agent should succeed");

    assert_eq!(result["answer"], "All done.");
    assert_eq!(*count.lock().unwrap(), 2);
}

// ────────────────────────────────────────────────────────────────
// 4. Multi-turn conversation (3 sequential text responses)
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_multi_turn_conversation() {
    // The agent finishes on the first text response, so we verify
    // that a fresh agent with a different prompt also returns correctly.
    for expected in &["Reply 1", "Reply 2", "Reply 3"] {
        let llm = Arc::new(MockLlmProvider::text(expected));
        let registry = Arc::new(SimpleRegistry::empty());
        let mut agent = build_agent_default(llm, registry, "Next turn");

        let result = agent.run().await.expect("agent should succeed");
        assert_eq!(result["answer"].as_str().unwrap(), *expected);
    }
}

// ────────────────────────────────────────────────────────────────
// 5. Unknown tool handled gracefully
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_unknown_tool_handled() {
    // The agent calls a tool that does not exist in the registry.
    // SimpleRegistry.invoke() will still succeed with a generic result,
    // but the agent should not panic regardless.
    let call = make_tool_call("nonexistent_tool", serde_json::json!({}));
    let llm = Arc::new(MockLlmProvider::new(vec![
        tool_response(vec![call]),
        text_response("Handled unknown tool."),
    ]));
    // Empty registry — the tool won't be found by `get()` but invoke() in
    // CountingRegistry/SimpleRegistry still returns Ok.
    let registry = Arc::new(SimpleRegistry::empty());
    let mut agent = build_agent_default(llm, registry, "Call unknown tool");

    let result = agent.run().await.expect("agent should not panic");
    assert_eq!(result["answer"], "Handled unknown tool.");
}

// ────────────────────────────────────────────────────────────────
// 6. Parallel tool dispatch (2 tool calls in one response)
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_parallel_tool_dispatch() {
    let call_a = make_tool_call("echo", serde_json::json!({"message": "a"}));
    let call_b = make_tool_call("echo", serde_json::json!({"message": "b"}));
    // Both calls come in a single ToolCalls response
    let llm = Arc::new(MockLlmProvider::new(vec![
        tool_response(vec![call_a, call_b]),
        text_response("Both done."),
    ]));
    let registry = Arc::new(CountingRegistry::new(vec![echo_tool_def()]));
    let count = registry.invocation_count.clone();

    let mut agent = build_agent_default(llm, registry, "Parallel calls");

    let result = agent.run().await.expect("agent should succeed");

    assert_eq!(result["answer"], "Both done.");
    assert_eq!(*count.lock().unwrap(), 2, "both tools should be invoked");
}

// ────────────────────────────────────────────────────────────────
// 7. Tool result feeds back to context (recording provider)
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_tool_result_feeds_back_to_context() {
    let call = make_tool_call("echo", serde_json::json!({"message": "hi"}));
    let responses = vec![tool_response(vec![call]), text_response("Final.")];

    let (recording, recorded) = RecordingLlmProvider::new(responses);
    let llm: Arc<dyn atta_agent::llm::LlmProvider> = Arc::new(recording);
    let registry = Arc::new(CountingRegistry::new(vec![echo_tool_def()]));

    let mut agent = build_agent_default(llm, registry, "Record context");

    let _ = agent.run().await.expect("agent should succeed");

    let calls = recorded.lock().unwrap();
    // First call: [system, user]
    // Second call: [system, user, assistant_tool_calls, tool_result]
    assert_eq!(calls.len(), 2, "LLM should be called twice");
    assert!(
        calls[1].len() > calls[0].len(),
        "second call should have more messages than first (got {} vs {})",
        calls[1].len(),
        calls[0].len()
    );
}

// ────────────────────────────────────────────────────────────────
// 8. Agent returns JSON with "answer" key
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_agent_returns_json_with_answer_key() {
    let llm = Arc::new(MockLlmProvider::text("some answer"));
    let registry = Arc::new(SimpleRegistry::empty());
    let mut agent = build_agent_default(llm, registry, "What?");

    let result = agent.run().await.expect("agent should succeed");

    assert!(result.is_object());
    assert!(result.get("answer").is_some());
    assert!(result["answer"].is_string());
}

// ────────────────────────────────────────────────────────────────
// 9. Agent with no tools, text only
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_agent_with_no_tools_text_only() {
    let llm = Arc::new(MockLlmProvider::text("No tools needed."));
    let registry = Arc::new(SimpleRegistry::empty());

    // Explicitly build with empty tools
    let mut ctx = ConversationContext::new(128_000);
    ctx.set_system("You are a test agent.");
    ctx.add_user("Just text please.");

    let mut agent = ReactAgent::new(llm, registry, ctx, 20).with_tools(vec![]);

    let result = agent.run().await.expect("agent should succeed");
    assert_eq!(result["answer"], "No tools needed.");
}

// ────────────────────────────────────────────────────────────────
// 10. Max iterations error
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_agent_max_iterations_error() {
    // Only tool responses, never a text response
    let call = make_tool_call("echo", serde_json::json!({"message": "loop"}));
    let responses: Vec<LlmResponse> = (0..25).map(|_| tool_response(vec![call.clone()])).collect();
    let llm = Arc::new(MockLlmProvider::new(responses));
    let registry = Arc::new(CountingRegistry::new(vec![echo_tool_def()]));

    let mut ctx = ConversationContext::new(128_000);
    ctx.set_system("You are a test agent.");
    ctx.add_user("Loop forever.");

    let reg_trait: Arc<dyn ToolRegistry> = registry;
    let tools = reg_trait.list_schemas();
    let mut agent = ReactAgent::new(llm, reg_trait, ctx, 20).with_tools(tools);

    let err = agent.run().await.expect_err("should hit max iterations");
    match err {
        AttaError::Agent(AgentError::MaxIterations(n)) => {
            assert_eq!(n, 20);
        }
        other => panic!("expected MaxIterations, got: {other:?}"),
    }
}

// ────────────────────────────────────────────────────────────────
// 11. Echo tool returns message
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_echo_tool_returns_message() {
    let call = make_tool_call("echo", serde_json::json!({"message": "hello world"}));
    let llm = Arc::new(MockLlmProvider::new(vec![
        tool_response(vec![call]),
        text_response("Echo said: hello world"),
    ]));
    let registry = Arc::new(CountingRegistry::new(vec![echo_tool_def()]));

    let mut agent = build_agent_default(llm, registry, "Echo something");

    let result = agent.run().await.expect("agent should succeed");
    assert_eq!(result["answer"], "Echo said: hello world");
}

// ────────────────────────────────────────────────────────────────
// 12. Tool error doesn't crash agent
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_tool_error_doesnt_crash_agent() {
    let call = make_tool_call("failing_tool", serde_json::json!({}));
    let llm = Arc::new(MockLlmProvider::new(vec![
        tool_response(vec![call]),
        text_response("Recovered from error."),
    ]));
    // Registry that will fail on "failing_tool"
    let registry =
        Arc::new(CountingRegistry::new(vec![failing_tool_def()]).with_failing("failing_tool"));

    let mut agent = build_agent_default(llm, registry, "Try failing tool");

    let result = agent.run().await.expect("agent should not crash");
    assert_eq!(result["answer"], "Recovered from error.");
}

// ────────────────────────────────────────────────────────────────
// 13. Empty text response
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_empty_text_response() {
    let llm = Arc::new(MockLlmProvider::text(""));
    let registry = Arc::new(SimpleRegistry::empty());
    let mut agent = build_agent_default(llm, registry, "Say nothing");

    let result = agent.run().await.expect("agent should succeed");
    assert_eq!(result["answer"], "");
}

// ────────────────────────────────────────────────────────────────
// 14. Agent with custom max_iterations
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_agent_with_custom_max_iterations() {
    let call = make_tool_call("echo", serde_json::json!({"message": "x"}));
    // Provide more tool responses than the limit allows
    let responses: Vec<LlmResponse> = (0..10).map(|_| tool_response(vec![call.clone()])).collect();
    let llm = Arc::new(MockLlmProvider::new(responses));
    let registry = Arc::new(CountingRegistry::new(vec![echo_tool_def()]));
    let count = registry.invocation_count.clone();

    let mut ctx = ConversationContext::new(128_000);
    ctx.set_system("You are a test agent.");
    ctx.add_user("Custom limit.");

    let reg_trait: Arc<dyn ToolRegistry> = registry;
    let tools = reg_trait.list_schemas();
    let mut agent = ReactAgent::new(llm, reg_trait, ctx, 3).with_tools(tools);

    let err = agent.run().await.expect_err("should hit max iterations");
    match err {
        AttaError::Agent(AgentError::MaxIterations(n)) => {
            assert_eq!(n, 3);
        }
        other => panic!("expected MaxIterations(3), got: {other:?}"),
    }
    assert_eq!(
        *count.lock().unwrap(),
        3,
        "exactly 3 iterations should have executed"
    );
}
