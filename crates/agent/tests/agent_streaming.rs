//! TG11 — Streaming delta event tests
//!
//! Verifies that `ReactAgent::run_streaming()` emits the correct sequence
//! of `AgentStreamEvent` deltas: Thinking, ClearProgress, TextChunk,
//! ToolStart, ToolComplete, ToolError, and Done.

mod common;

use std::sync::Arc;

use atta_agent::react::{AgentDelta, AgentStreamEvent};
use tokio::sync::mpsc;

use common::builders::build_agent_default;
use common::fixtures::{make_tool_call, text_response, tool_response};
use common::mock_llm::MockLlmProvider;
use common::mock_tools::{echo_tool_def, failing_tool_def, CountingRegistry, SimpleRegistry};

/// Collect all AgentStreamEvents from a channel receiver
async fn collect_events(mut rx: mpsc::Receiver<AgentStreamEvent>) -> Vec<AgentStreamEvent> {
    let mut events = Vec::new();
    while let Some(evt) = rx.recv().await {
        events.push(evt);
    }
    events
}

/// Extract only the Delta variants from a stream event list
fn deltas(events: &[AgentStreamEvent]) -> Vec<AgentDelta> {
    events
        .iter()
        .filter_map(|e| match e {
            AgentStreamEvent::Delta(d) => Some(d.clone()),
            _ => None,
        })
        .collect()
}

// ────────────────────────────────────────────────────────────────
// 1. Streaming text produces Thinking + ClearProgress + TextChunk(s) + Done
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_streaming_text_produces_deltas() {
    let llm = Arc::new(MockLlmProvider::text("Hello streaming!"));
    let registry = Arc::new(SimpleRegistry::empty());
    let mut agent = build_agent_default(llm, registry, "Stream test");

    let (tx, rx) = mpsc::channel(128);

    let result = agent
        .run_streaming(tx)
        .await
        .expect("streaming should succeed");
    assert_eq!(result["answer"], "Hello streaming!");

    let events = collect_events(rx).await;
    let ds = deltas(&events);

    // Must see: Thinking, ClearProgress, at least one TextChunk, Done
    assert!(
        ds.iter().any(|d| matches!(d, AgentDelta::Thinking { .. })),
        "should emit Thinking"
    );
    assert!(
        ds.iter().any(|d| matches!(d, AgentDelta::ClearProgress)),
        "should emit ClearProgress"
    );
    assert!(
        ds.iter().any(|d| matches!(d, AgentDelta::TextChunk { .. })),
        "should emit at least one TextChunk"
    );
    assert!(
        ds.iter().any(|d| matches!(d, AgentDelta::Done { .. })),
        "should emit Done"
    );
}

// ────────────────────────────────────────────────────────────────
// 2. Streaming tool produces ToolStart + ToolComplete events
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_streaming_tool_produces_start_complete() {
    let call = make_tool_call("echo", serde_json::json!({"message": "stream"}));
    let llm = Arc::new(MockLlmProvider::new(vec![
        tool_response(vec![call]),
        text_response("Tool stream done."),
    ]));
    let registry = Arc::new(CountingRegistry::new(vec![echo_tool_def()]));
    let mut agent = build_agent_default(llm, registry, "Stream tool test");

    let (tx, rx) = mpsc::channel(128);
    let _ = agent
        .run_streaming(tx)
        .await
        .expect("streaming should succeed");

    let ds = deltas(&collect_events(rx).await);

    let tool_starts: Vec<_> = ds
        .iter()
        .filter(|d| matches!(d, AgentDelta::ToolStart { .. }))
        .collect();
    let tool_completes: Vec<_> = ds
        .iter()
        .filter(|d| matches!(d, AgentDelta::ToolComplete { .. }))
        .collect();

    assert_eq!(tool_starts.len(), 1, "should have 1 ToolStart");
    assert_eq!(tool_completes.len(), 1, "should have 1 ToolComplete");

    // Verify the tool name
    if let AgentDelta::ToolStart { tool_name, .. } = &tool_starts[0] {
        assert_eq!(tool_name, "echo");
    }
}

// ────────────────────────────────────────────────────────────────
// 3. Streaming tool error produces ToolError delta
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_streaming_tool_error_produces_error_delta() {
    let call = make_tool_call("failing_tool", serde_json::json!({}));
    let llm = Arc::new(MockLlmProvider::new(vec![
        tool_response(vec![call]),
        text_response("Error handled."),
    ]));
    let registry =
        Arc::new(CountingRegistry::new(vec![failing_tool_def()]).with_failing("failing_tool"));
    let mut agent = build_agent_default(llm, registry, "Stream error test");

    let (tx, rx) = mpsc::channel(128);
    let _ = agent
        .run_streaming(tx)
        .await
        .expect("streaming should succeed");

    let ds = deltas(&collect_events(rx).await);

    let tool_errors: Vec<_> = ds
        .iter()
        .filter(|d| matches!(d, AgentDelta::ToolError { .. }))
        .collect();

    assert_eq!(tool_errors.len(), 1, "should have 1 ToolError");

    if let AgentDelta::ToolError {
        tool_name, error, ..
    } = &tool_errors[0]
    {
        assert_eq!(tool_name, "failing_tool");
        assert!(!error.is_empty(), "error message should not be empty");
    }
}

// ────────────────────────────────────────────────────────────────
// 4. Streaming Done has iteration count
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_streaming_done_has_iteration_count() {
    let call = make_tool_call("echo", serde_json::json!({"message": "x"}));
    let llm = Arc::new(MockLlmProvider::new(vec![
        tool_response(vec![call]),
        text_response("Done at iteration 2."),
    ]));
    let registry = Arc::new(CountingRegistry::new(vec![echo_tool_def()]));
    let mut agent = build_agent_default(llm, registry, "Iteration count test");

    let (tx, rx) = mpsc::channel(128);
    let _ = agent
        .run_streaming(tx)
        .await
        .expect("streaming should succeed");

    let ds = deltas(&collect_events(rx).await);

    let done_events: Vec<_> = ds
        .iter()
        .filter_map(|d| match d {
            AgentDelta::Done { iterations } => Some(*iterations),
            _ => None,
        })
        .collect();

    assert_eq!(done_events.len(), 1, "should have exactly 1 Done");
    assert_eq!(
        done_events[0], 2,
        "Done should report iteration 2 (1 tool + 1 text)"
    );
}

// ────────────────────────────────────────────────────────────────
// 5. Thinking emitted per iteration
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_streaming_thinking_emitted_per_iteration() {
    let call = make_tool_call("echo", serde_json::json!({"message": "y"}));
    let llm = Arc::new(MockLlmProvider::new(vec![
        tool_response(vec![call.clone()]),
        tool_response(vec![call]),
        text_response("After 3 iterations."),
    ]));
    let registry = Arc::new(CountingRegistry::new(vec![echo_tool_def()]));
    let mut agent = build_agent_default(llm, registry, "Thinking per iter");

    let (tx, rx) = mpsc::channel(128);
    let _ = agent
        .run_streaming(tx)
        .await
        .expect("streaming should succeed");

    let ds = deltas(&collect_events(rx).await);
    let thinking_events: Vec<u32> = ds
        .iter()
        .filter_map(|d| match d {
            AgentDelta::Thinking { iteration } => Some(*iteration),
            _ => None,
        })
        .collect();

    // 3 iterations: tool, tool, text
    assert_eq!(thinking_events.len(), 3, "should emit Thinking 3 times");
    assert_eq!(thinking_events, vec![1, 2, 3]);
}

// ────────────────────────────────────────────────────────────────
// 6. TextChunk texts reconstruct answer
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_streaming_text_chunks_reconstruct_answer() {
    let answer_text = "This is a complete answer that should be chunked and reconstructable.";
    let llm = Arc::new(MockLlmProvider::text(answer_text));
    let registry = Arc::new(SimpleRegistry::empty());
    let mut agent = build_agent_default(llm, registry, "Reconstruct test");

    let (tx, rx) = mpsc::channel(128);
    let result = agent
        .run_streaming(tx)
        .await
        .expect("streaming should succeed");

    let ds = deltas(&collect_events(rx).await);
    let reconstructed: String = ds
        .iter()
        .filter_map(|d| match d {
            AgentDelta::TextChunk { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(
        reconstructed,
        result["answer"].as_str().unwrap(),
        "concatenated TextChunks should equal the final answer"
    );
}

// ────────────────────────────────────────────────────────────────
// 7. Multi-tool streaming deltas
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_streaming_multi_tool_deltas() {
    let call_a = make_tool_call("echo", serde_json::json!({"message": "a"}));
    let call_b = atta_agent::llm::ToolCall {
        id: "tc_echo_b".to_string(),
        name: "echo".to_string(),
        arguments: serde_json::json!({"message": "b"}),
    };
    let llm = Arc::new(MockLlmProvider::new(vec![
        tool_response(vec![call_a, call_b]),
        text_response("Both tools streamed."),
    ]));
    let registry = Arc::new(CountingRegistry::new(vec![echo_tool_def()]));
    let mut agent = build_agent_default(llm, registry, "Multi tool stream");

    let (tx, rx) = mpsc::channel(128);
    let _ = agent
        .run_streaming(tx)
        .await
        .expect("streaming should succeed");

    let ds = deltas(&collect_events(rx).await);

    let tool_starts = ds
        .iter()
        .filter(|d| matches!(d, AgentDelta::ToolStart { .. }))
        .count();
    let tool_completes = ds
        .iter()
        .filter(|d| matches!(d, AgentDelta::ToolComplete { .. }))
        .count();

    assert_eq!(tool_starts, 2, "should have 2 ToolStart events");
    assert_eq!(tool_completes, 2, "should have 2 ToolComplete events");
}

// ────────────────────────────────────────────────────────────────
// 8. Streaming returns same value as run()
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_streaming_returns_same_value_as_run() {
    let answer = "Consistent answer.";

    // Non-streaming run
    let llm1 = Arc::new(MockLlmProvider::text(answer));
    let reg1 = Arc::new(SimpleRegistry::empty());
    let mut agent1 = build_agent_default(llm1, reg1, "Consistency test");
    let result_run = agent1.run().await.expect("run should succeed");

    // Streaming run with identical provider
    let llm2 = Arc::new(MockLlmProvider::text(answer));
    let reg2 = Arc::new(SimpleRegistry::empty());
    let mut agent2 = build_agent_default(llm2, reg2, "Consistency test");
    let (tx, _rx) = mpsc::channel(128);
    let result_stream = agent2
        .run_streaming(tx)
        .await
        .expect("streaming should succeed");

    assert_eq!(
        result_run, result_stream,
        "run() and run_streaming() should return the same JSON value"
    );
}
