//! Tool execution strategies
//!
//! Provides parallel and sequential tool execution. When LLM returns multiple
//! tool calls, they can be executed concurrently (if no high-risk tools are involved)
//! or sequentially.

use std::sync::Arc;
use std::time::Duration;

use atta_types::{AttaError, RiskLevel, ToolRegistry};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::llm::ToolCall;

/// Default tool execution timeout (120 seconds)
const DEFAULT_TOOL_TIMEOUT: Duration = Duration::from_secs(120);

/// Configuration for tool execution
pub struct ToolExecutionConfig {
    /// Per-tool timeout (default 120s)
    pub tool_timeout: Duration,
    /// Optional cancellation token for cooperative cancellation
    pub cancel_token: Option<CancellationToken>,
    /// Maximum repeated identical calls before aborting (0=unlimited, default 3)
    pub max_repeated_calls: u32,
    /// Optional hook chain for pre/post execution hooks
    pub hooks: Option<Arc<crate::hooks::HookChain>>,
    /// Maximum output size in characters (0=unlimited, default 100_000)
    pub max_output_chars: usize,
}

/// Default maximum output size (100K characters)
const DEFAULT_MAX_OUTPUT_CHARS: usize = 100_000;

impl Default for ToolExecutionConfig {
    fn default() -> Self {
        Self {
            tool_timeout: DEFAULT_TOOL_TIMEOUT,
            cancel_token: None,
            max_repeated_calls: 3,
            hooks: None,
            max_output_chars: DEFAULT_MAX_OUTPUT_CHARS,
        }
    }
}

/// Result of a single tool call
pub struct ToolResult {
    pub tool_call_id: String,
    pub tool_name: String,
    pub result: Result<serde_json::Value, AttaError>,
}

/// Check if tool calls can be executed in parallel.
///
/// Returns `true` when there are >1 calls and none have `High` risk level.
pub fn should_execute_parallel(calls: &[ToolCall], registry: &dyn ToolRegistry) -> bool {
    if calls.len() <= 1 {
        return false;
    }

    // If any tool has High risk, execute sequentially (may need approval)
    !calls.iter().any(|tc| {
        registry
            .get(&tc.name)
            .map(|def| def.risk_level == RiskLevel::High)
            .unwrap_or(false)
    })
}

/// Execute tool calls in parallel using `futures::future::join_all`
///
/// Requires an `Arc<dyn ToolRegistry>` to share across spawned futures.
pub async fn execute_tools_parallel(
    calls: &[ToolCall],
    registry: Arc<dyn ToolRegistry>,
) -> Vec<ToolResult> {
    info!(count = calls.len(), "executing tools in parallel");

    let futures: Vec<_> = calls
        .iter()
        .map(|tc| {
            let name = tc.name.clone();
            let id = tc.id.clone();
            let args = tc.arguments.clone();
            let reg = Arc::clone(&registry);
            async move {
                let result = reg.invoke(&name, &args).await;
                ToolResult {
                    tool_call_id: id,
                    tool_name: name,
                    result,
                }
            }
        })
        .collect();

    futures::future::join_all(futures).await
}

/// Execute tool calls sequentially (original behavior)
pub async fn execute_tools_sequential(
    calls: &[ToolCall],
    registry: &dyn ToolRegistry,
) -> Vec<ToolResult> {
    let mut results = Vec::with_capacity(calls.len());

    for tc in calls {
        info!(tool = %tc.name, id = %tc.id, "executing tool call");
        let result = registry.invoke(&tc.name, &tc.arguments).await;
        results.push(ToolResult {
            tool_call_id: tc.id.clone(),
            tool_name: tc.name.clone(),
            result,
        });
    }

    results
}

/// Execute tools with automatic parallel/sequential selection
pub async fn execute_tools(calls: &[ToolCall], registry: Arc<dyn ToolRegistry>) -> Vec<ToolResult> {
    if should_execute_parallel(calls, registry.as_ref()) {
        execute_tools_parallel(calls, registry).await
    } else {
        execute_tools_sequential(calls, registry.as_ref()).await
    }
}

/// Invoke a single tool with timeout and optional cancellation
async fn invoke_with_timeout(
    registry: &dyn ToolRegistry,
    tool_name: &str,
    args: &serde_json::Value,
    config: &ToolExecutionConfig,
) -> Result<serde_json::Value, AttaError> {
    let fut = registry.invoke(tool_name, args);

    match &config.cancel_token {
        Some(token) => {
            tokio::select! {
                result = tokio::time::timeout(config.tool_timeout, fut) => {
                    result.map_err(|_| AttaError::Agent(atta_types::AgentError::Timeout(config.tool_timeout)))?
                }
                _ = token.cancelled() => {
                    Err(AttaError::SecurityViolation("tool execution cancelled".to_string()))
                }
            }
        }
        None => tokio::time::timeout(config.tool_timeout, fut)
            .await
            .map_err(|_| AttaError::Agent(atta_types::AgentError::Timeout(config.tool_timeout)))?,
    }
}

/// Truncate a JSON value if its string representation exceeds max_chars.
/// Returns the value unchanged if max_chars is 0 (unlimited).
fn truncate_output(value: serde_json::Value, max_chars: usize) -> serde_json::Value {
    if max_chars == 0 {
        return value;
    }
    let s = value.to_string();
    if s.len() <= max_chars {
        return value;
    }
    // Truncate and return as a wrapper with the truncated content
    let truncated = &s[..max_chars];
    serde_json::json!({
        "output": truncated,
        "truncated": true,
        "original_length": s.len(),
        "max_length": max_chars,
    })
}

/// Invoke a tool with optional hooks and timeout
async fn invoke_with_hooks_and_timeout(
    registry: &dyn ToolRegistry,
    tool_name: &str,
    args: &serde_json::Value,
    config: &ToolExecutionConfig,
) -> Result<serde_json::Value, AttaError> {
    let mut effective_args = args.clone();

    // Pre-execution hooks
    if let Some(ref hooks) = config.hooks {
        let (outcome, new_args) = hooks.run_before(tool_name, &effective_args).await?;
        match outcome {
            crate::hooks::HookOutcome::Block(reason) => {
                return Err(AttaError::SecurityViolation(format!(
                    "hook blocked tool '{}': {}",
                    tool_name, reason
                )));
            }
            crate::hooks::HookOutcome::ModifyArgs(_) => {
                effective_args = new_args;
            }
            crate::hooks::HookOutcome::Allow => {}
        }
    }

    let result = invoke_with_timeout(registry, tool_name, &effective_args, config).await;

    // Post-execution hooks
    if let Some(ref hooks) = config.hooks {
        if let Some(replacement) = hooks.run_after(tool_name, &effective_args, &result).await? {
            return Ok(truncate_output(replacement, config.max_output_chars));
        }
    }

    // Apply output truncation
    result.map(|v| truncate_output(v, config.max_output_chars))
}

/// Execute tool calls with configuration (timeout, cancellation, loop detection, hooks)
pub async fn execute_tools_configured(
    calls: &[ToolCall],
    registry: Arc<dyn ToolRegistry>,
    config: &ToolExecutionConfig,
    mut loop_detector: Option<&mut LoopDetector>,
) -> Vec<ToolResult> {
    // Loop detection: record calls + check progress
    if let Some(ref mut detector) = loop_detector {
        for tc in calls {
            if let Err(e) = detector.record(&tc.name, &tc.arguments) {
                return vec![ToolResult {
                    tool_call_id: tc.id.clone(),
                    tool_name: tc.name.clone(),
                    result: Err(e),
                }];
            }
        }
        if let Err(e) = detector.check_progress() {
            return vec![ToolResult {
                tool_call_id: calls[0].id.clone(),
                tool_name: calls[0].name.clone(),
                result: Err(e),
            }];
        }
    }

    if should_execute_parallel(calls, registry.as_ref()) {
        // Parallel execution with timeout
        let futures: Vec<_> = calls
            .iter()
            .map(|tc| {
                let name = tc.name.clone();
                let id = tc.id.clone();
                let args = tc.arguments.clone();
                let reg = Arc::clone(&registry);
                let timeout = config.tool_timeout;
                let cancel = config.cancel_token.clone();
                let hooks = config.hooks.clone();
                let max_output = config.max_output_chars;
                async move {
                    let cfg = ToolExecutionConfig {
                        tool_timeout: timeout,
                        cancel_token: cancel,
                        max_repeated_calls: 0, // already checked above
                        hooks,
                        max_output_chars: max_output,
                    };
                    let result =
                        invoke_with_hooks_and_timeout(reg.as_ref(), &name, &args, &cfg).await;
                    ToolResult {
                        tool_call_id: id,
                        tool_name: name,
                        result,
                    }
                }
            })
            .collect();
        let results = futures::future::join_all(futures).await;
        // Record outputs for progress detection
        if let Some(detector) = loop_detector {
            for tr in &results {
                if let Ok(ref v) = tr.result {
                    detector.record_output(v);
                }
            }
        }
        results
    } else {
        // Sequential execution with timeout
        let mut results = Vec::with_capacity(calls.len());
        for tc in calls {
            info!(tool = %tc.name, id = %tc.id, "executing tool call (configured)");
            let result =
                invoke_with_hooks_and_timeout(registry.as_ref(), &tc.name, &tc.arguments, config)
                    .await;
            // Record output for progress detection
            if let Some(ref mut detector) = loop_detector {
                if let Ok(ref v) = result {
                    detector.record_output(v);
                }
            }
            results.push(ToolResult {
                tool_call_id: tc.id.clone(),
                tool_name: tc.name.clone(),
                result,
            });
        }
        results
    }
}

/// Execute tool calls with streaming delta events
///
/// Emits `ToolStart`, `ToolComplete`, and `ToolError` deltas via the provided channel.
/// Uses timeout, cancellation, loop detection, and hooks from config.
pub async fn execute_tools_with_deltas(
    calls: &[ToolCall],
    registry: Arc<dyn ToolRegistry>,
    config: &ToolExecutionConfig,
    delta_tx: &tokio::sync::mpsc::Sender<crate::react::AgentStreamEvent>,
    mut loop_detector: Option<&mut LoopDetector>,
) -> Vec<ToolResult> {
    use crate::react::{AgentDelta, AgentStreamEvent};
    use std::time::Instant;

    // Loop detection: record calls + check progress
    if let Some(ref mut detector) = loop_detector {
        for tc in calls {
            if let Err(e) = detector.record(&tc.name, &tc.arguments) {
                let _ = delta_tx
                    .send(AgentStreamEvent::Delta(AgentDelta::ToolError {
                        tool_name: tc.name.clone(),
                        call_id: tc.id.clone(),
                        error: e.to_string(),
                    }))
                    .await;
                return vec![ToolResult {
                    tool_call_id: tc.id.clone(),
                    tool_name: tc.name.clone(),
                    result: Err(e),
                }];
            }
        }
        if let Err(e) = detector.check_progress() {
            let _ = delta_tx
                .send(AgentStreamEvent::Delta(AgentDelta::ToolError {
                    tool_name: calls[0].name.clone(),
                    call_id: calls[0].id.clone(),
                    error: e.to_string(),
                }))
                .await;
            return vec![ToolResult {
                tool_call_id: calls[0].id.clone(),
                tool_name: calls[0].name.clone(),
                result: Err(e),
            }];
        }
    }

    // Always sequential when emitting deltas (to maintain event ordering)
    let mut results = Vec::with_capacity(calls.len());
    for tc in calls {
        // Emit ToolStart
        let _ = delta_tx
            .send(AgentStreamEvent::Delta(AgentDelta::ToolStart {
                tool_name: tc.name.clone(),
                call_id: tc.id.clone(),
            }))
            .await;

        let start = Instant::now();
        let result =
            invoke_with_hooks_and_timeout(registry.as_ref(), &tc.name, &tc.arguments, config).await;
        let duration_ms = start.elapsed().as_millis() as u64;

        // Record output for progress detection
        if let Some(ref mut detector) = loop_detector {
            if let Ok(ref v) = result {
                detector.record_output(v);
            }
        }

        // Emit ToolComplete or ToolError
        match &result {
            Ok(_) => {
                let _ = delta_tx
                    .send(AgentStreamEvent::Delta(AgentDelta::ToolComplete {
                        tool_name: tc.name.clone(),
                        call_id: tc.id.clone(),
                        duration_ms,
                    }))
                    .await;
            }
            Err(e) => {
                let _ = delta_tx
                    .send(AgentStreamEvent::Delta(AgentDelta::ToolError {
                        tool_name: tc.name.clone(),
                        call_id: tc.id.clone(),
                        error: e.to_string(),
                    }))
                    .await;
            }
        }

        results.push(ToolResult {
            tool_call_id: tc.id.clone(),
            tool_name: tc.name.clone(),
            result,
        });
    }

    results
}

/// Convert a tool result to a displayable string
pub fn result_to_string(result: &Result<serde_json::Value, AttaError>) -> String {
    match result {
        Ok(value) => value.to_string(),
        Err(e) => {
            warn!(error = %e, "tool call failed");
            serde_json::json!({ "error": e.to_string() }).to_string()
        }
    }
}

/// Multi-detector loop detection system.
///
/// Detects several loop patterns:
/// 1. **Exact repeat**: same tool+args called N times
/// 2. **Ping-pong**: A→B→A→B alternating pattern
/// 3. **Poll-no-progress**: recent outputs are all identical
pub struct LoopDetector {
    /// Exact-repeat counter: hash(name+args) → count
    call_counts: std::collections::HashMap<u64, u32>,
    max_repeats: u32,
    /// Sliding window of recent calls for pattern detection
    history: std::collections::VecDeque<(String, u64)>, // (tool_name, args_hash)
    /// Sliding window of recent output hashes for progress detection
    output_hashes: std::collections::VecDeque<u64>,
    /// Maximum history window size
    max_history: usize,
}

impl LoopDetector {
    /// Create a new LoopDetector with the given maximum repeat count
    pub fn new(max_repeats: u32) -> Self {
        Self {
            call_counts: std::collections::HashMap::new(),
            max_repeats,
            history: std::collections::VecDeque::with_capacity(30),
            output_hashes: std::collections::VecDeque::with_capacity(10),
            max_history: 30,
        }
    }

    /// Compute a hash for a tool call
    fn call_hash(tool_name: &str, args: &serde_json::Value) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        tool_name.hash(&mut hasher);
        args.to_string().hash(&mut hasher);
        hasher.finish()
    }

    /// Record a tool call. Returns error if any loop pattern is detected.
    pub fn record(&mut self, tool_name: &str, args: &serde_json::Value) -> Result<(), AttaError> {
        if self.max_repeats == 0 {
            return Ok(());
        }

        let key = Self::call_hash(tool_name, args);

        // Detector 1: Exact repeat
        let count = self.call_counts.entry(key).or_insert(0);
        *count += 1;

        if *count > self.max_repeats {
            warn!(
                tool = tool_name,
                count = *count,
                max = self.max_repeats,
                "loop detected: tool called too many times with same arguments"
            );
            return Err(AttaError::SecurityViolation(format!(
                "loop detected: tool '{}' called {} times with same arguments (max {})",
                tool_name, count, self.max_repeats
            )));
        }

        // Add to history window
        if self.history.len() >= self.max_history {
            self.history.pop_front();
        }
        self.history.push_back((tool_name.to_string(), key));

        // Detector 2: Ping-pong (ABAB pattern in last 4 entries)
        if self.history.len() >= 4 {
            let len = self.history.len();
            let a1 = &self.history[len - 4];
            let b1 = &self.history[len - 3];
            let a2 = &self.history[len - 2];
            let b2 = &self.history[len - 1];
            if a1.1 == a2.1 && b1.1 == b2.1 && a1.1 != b1.1 {
                warn!(
                    tool_a = %a1.0,
                    tool_b = %b1.0,
                    "loop detected: ping-pong pattern (A→B→A→B)"
                );
                return Err(AttaError::SecurityViolation(format!(
                    "loop detected: ping-pong pattern between '{}' and '{}'",
                    a1.0, b1.0
                )));
            }
        }

        Ok(())
    }

    /// Record a tool output for no-progress detection.
    /// Should be called after each tool invocation with the result.
    pub fn record_output(&mut self, output: &serde_json::Value) {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        output.to_string().hash(&mut hasher);
        let hash = hasher.finish();

        if self.output_hashes.len() >= 10 {
            self.output_hashes.pop_front();
        }
        self.output_hashes.push_back(hash);
    }

    /// Check if recent outputs show no progress (all identical).
    /// Returns error if the last 5+ outputs are the same.
    pub fn check_progress(&self) -> Result<(), AttaError> {
        if self.output_hashes.len() >= 5 {
            let last = self.output_hashes.back().unwrap();
            let all_same = self.output_hashes.iter().rev().take(5).all(|h| h == last);
            if all_same {
                warn!("loop detected: last 5 tool outputs are identical (no progress)");
                return Err(AttaError::SecurityViolation(
                    "loop detected: no progress — last 5 tool outputs are identical".to_string(),
                ));
            }
        }
        Ok(())
    }

    /// Reset all call counts and history
    pub fn reset(&mut self) {
        self.call_counts.clear();
        self.history.clear();
        self.output_hashes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::{ToolBinding, ToolDef, ToolSchema};
    use std::collections::HashMap;
    use std::sync::RwLock;

    /// Mock registry for testing
    struct MockRegistry {
        tools: RwLock<HashMap<String, ToolDef>>,
    }

    impl MockRegistry {
        fn new(tools: Vec<ToolDef>) -> Self {
            let map: HashMap<String, ToolDef> =
                tools.into_iter().map(|t| (t.name.clone(), t)).collect();
            Self {
                tools: RwLock::new(map),
            }
        }
    }

    #[async_trait::async_trait]
    impl ToolRegistry for MockRegistry {
        fn register(&self, tool: ToolDef) {
            self.tools.write().unwrap().insert(tool.name.clone(), tool);
        }
        fn unregister(&self, name: &str) {
            self.tools.write().unwrap().remove(name);
        }
        fn get(&self, name: &str) -> Option<ToolDef> {
            self.tools.read().unwrap().get(name).cloned()
        }
        fn get_schema(&self, name: &str) -> Option<ToolSchema> {
            self.get(name).map(|t| ToolSchema::from(&t))
        }
        fn list_schemas(&self) -> Vec<ToolSchema> {
            self.tools
                .read()
                .unwrap()
                .values()
                .map(ToolSchema::from)
                .collect()
        }
        fn list_all(&self) -> Vec<ToolDef> {
            self.tools.read().unwrap().values().cloned().collect()
        }
        async fn invoke(
            &self,
            tool_name: &str,
            _arguments: &serde_json::Value,
        ) -> Result<serde_json::Value, AttaError> {
            Ok(serde_json::json!({ "result": format!("ok from {}", tool_name) }))
        }
    }

    fn make_tool(name: &str, risk: RiskLevel) -> ToolDef {
        ToolDef {
            name: name.to_string(),
            description: "test".to_string(),
            binding: ToolBinding::Builtin {
                handler_name: name.to_string(),
            },
            risk_level: risk,
            parameters: serde_json::json!({}),
        }
    }

    fn make_call(name: &str) -> ToolCall {
        ToolCall {
            id: format!("call_{}", name),
            name: name.to_string(),
            arguments: serde_json::json!({}),
        }
    }

    #[test]
    fn test_should_parallel_single_call() {
        let reg = MockRegistry::new(vec![make_tool("a", RiskLevel::Low)]);
        let calls = vec![make_call("a")];
        assert!(!should_execute_parallel(&calls, &reg));
    }

    #[test]
    fn test_should_parallel_multiple_low_risk() {
        let reg = MockRegistry::new(vec![
            make_tool("a", RiskLevel::Low),
            make_tool("b", RiskLevel::Medium),
        ]);
        let calls = vec![make_call("a"), make_call("b")];
        assert!(should_execute_parallel(&calls, &reg));
    }

    #[test]
    fn test_should_not_parallel_high_risk() {
        let reg = MockRegistry::new(vec![
            make_tool("a", RiskLevel::Low),
            make_tool("b", RiskLevel::High),
        ]);
        let calls = vec![make_call("a"), make_call("b")];
        assert!(!should_execute_parallel(&calls, &reg));
    }

    #[tokio::test]
    async fn test_execute_parallel() {
        let reg: Arc<dyn ToolRegistry> = Arc::new(MockRegistry::new(vec![
            make_tool("a", RiskLevel::Low),
            make_tool("b", RiskLevel::Low),
        ]));
        let calls = vec![make_call("a"), make_call("b")];
        let results = execute_tools_parallel(&calls, reg).await;
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.result.is_ok()));
    }

    #[tokio::test]
    async fn test_execute_sequential() {
        let reg = MockRegistry::new(vec![
            make_tool("a", RiskLevel::Low),
            make_tool("b", RiskLevel::Low),
        ]);
        let calls = vec![make_call("a"), make_call("b")];
        let results = execute_tools_sequential(&calls, &reg).await;
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.result.is_ok()));
    }
}
