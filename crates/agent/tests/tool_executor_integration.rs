//! TG12 — Tool execution strategy integration tests
//!
//! Tests the `tool_executor` module: parallel vs sequential execution,
//! call-id matching, error handling, and `result_to_string` formatting.

mod common;

use std::sync::Arc;

use atta_agent::llm::ToolCall;
use atta_agent::tool_executor;
use atta_types::{AttaError, ToolRegistry};

use common::mock_tools::{echo_tool_def, high_risk_tool_def, CountingRegistry, SimpleRegistry};

/// Helper to build a ToolCall with a given name and ID.
fn tc(name: &str, id: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        name: name.to_string(),
        arguments: serde_json::json!({}),
    }
}

// ────────────────────────────────────────────────────────────────
// 1. Parallel execution for multiple low-risk tools
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_parallel_execution_multiple_low_risk() {
    let registry: Arc<dyn ToolRegistry> = Arc::new(CountingRegistry::new(vec![echo_tool_def()]));

    let calls = vec![tc("echo", "c1"), tc("echo", "c2"), tc("echo", "c3")];

    // Verify the strategy selects parallel
    assert!(
        tool_executor::should_execute_parallel(&calls, registry.as_ref()),
        "3 low-risk tools should execute in parallel"
    );

    let results = tool_executor::execute_tools(&calls, Arc::clone(&registry)).await;
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|r| r.result.is_ok()));
}

// ────────────────────────────────────────────────────────────────
// 2. Sequential execution with high-risk tool
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_sequential_execution_with_high_risk() {
    let registry: Arc<dyn ToolRegistry> = Arc::new(CountingRegistry::new(vec![
        echo_tool_def(),
        high_risk_tool_def(),
    ]));

    let calls = vec![tc("echo", "c1"), tc("dangerous_tool", "c2")];

    // High-risk tool present -> should NOT execute in parallel
    assert!(
        !tool_executor::should_execute_parallel(&calls, registry.as_ref()),
        "presence of high-risk tool should prevent parallel execution"
    );

    let results = tool_executor::execute_tools(&calls, Arc::clone(&registry)).await;
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.result.is_ok()));
}

// ────────────────────────────────────────────────────────────────
// 3. Single call is not parallel
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_single_call_not_parallel() {
    let registry: Arc<dyn ToolRegistry> = Arc::new(CountingRegistry::new(vec![echo_tool_def()]));
    let calls = vec![tc("echo", "c1")];

    assert!(
        !tool_executor::should_execute_parallel(&calls, registry.as_ref()),
        "single call should not be parallel"
    );
}

// ────────────────────────────────────────────────────────────────
// 4. Empty calls returns empty results
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_empty_calls_returns_empty() {
    let registry: Arc<dyn ToolRegistry> = Arc::new(SimpleRegistry::empty());
    let calls: Vec<ToolCall> = vec![];

    let results = tool_executor::execute_tools(&calls, registry).await;
    assert!(results.is_empty(), "no calls should yield no results");
}

// ────────────────────────────────────────────────────────────────
// 5. Results match call IDs
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_results_match_call_ids() {
    let registry: Arc<dyn ToolRegistry> = Arc::new(CountingRegistry::new(vec![echo_tool_def()]));
    let calls = vec![tc("echo", "alpha"), tc("echo", "beta"), tc("echo", "gamma")];

    let results = tool_executor::execute_tools(&calls, Arc::clone(&registry)).await;

    // Collect result IDs
    let result_ids: Vec<&str> = results.iter().map(|r| r.tool_call_id.as_str()).collect();
    // Each input call ID must appear in the results
    for call in &calls {
        assert!(
            result_ids.contains(&call.id.as_str()),
            "result should contain call_id '{}'",
            call.id
        );
    }
}

// ────────────────────────────────────────────────────────────────
// 6. Tool not found returns error in result
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_tool_not_found_returns_error() {
    // CountingRegistry with fail_tool set to "nonexistent".
    // When invoke("nonexistent", ...) is called, CountingRegistry returns an error.
    let registry: Arc<dyn ToolRegistry> =
        Arc::new(CountingRegistry::new(vec![echo_tool_def()]).with_failing("nonexistent"));
    let calls = vec![tc("nonexistent", "c_missing")];

    let results = tool_executor::execute_tools(&calls, Arc::clone(&registry)).await;
    assert_eq!(results.len(), 1);
    assert!(
        results[0].result.is_err(),
        "invoking a failing tool should produce an error"
    );
}

// ────────────────────────────────────────────────────────────────
// 7. result_to_string for Ok value
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_result_to_string_ok() {
    let ok_result: Result<serde_json::Value, AttaError> =
        Ok(serde_json::json!({"status": "success"}));
    let text = tool_executor::result_to_string(&ok_result);

    // Should be the JSON stringified value
    assert!(text.contains("success"), "ok text should contain 'success'");
    // Verify it's valid JSON
    let parsed: serde_json::Value =
        serde_json::from_str(&text).expect("result_to_string Ok should be valid JSON");
    assert_eq!(parsed["status"], "success");
}

// ────────────────────────────────────────────────────────────────
// 8. result_to_string for Err value
// ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_result_to_string_err() {
    let err_result: Result<serde_json::Value, AttaError> =
        Err(AttaError::ToolNotFound("missing_tool".to_string()));
    let text = tool_executor::result_to_string(&err_result);

    // Should contain an "error" key
    let parsed: serde_json::Value =
        serde_json::from_str(&text).expect("result_to_string Err should be valid JSON");
    assert!(
        parsed.get("error").is_some(),
        "error result should have 'error' key"
    );
    let error_msg = parsed["error"].as_str().unwrap();
    assert!(
        error_msg.contains("missing_tool"),
        "error message should reference the tool name, got: {error_msg}"
    );
}
