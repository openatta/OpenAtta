//! Pre/Post execution hook system for tool calls
//!
//! Provides a pipeline of hooks that can inspect, modify, or block tool
//! invocations before and after execution.

use atta_types::AttaError;
use serde_json::Value;

/// Outcome of a pre-execution hook
pub enum HookOutcome {
    /// Allow the tool call to proceed as-is
    Allow,
    /// Allow but replace the arguments with modified values
    ModifyArgs(Value),
    /// Block the tool call with a reason
    Block(String),
}

/// A hook that can intercept tool execution and LLM calls
#[async_trait::async_trait]
pub trait ToolHook: Send + Sync + 'static {
    /// Called before a tool is executed.
    /// Return `Allow` to proceed, `ModifyArgs` to change arguments, or `Block` to reject.
    async fn before_execute(&self, tool_name: &str, args: &Value)
        -> Result<HookOutcome, AttaError>;

    /// Called after a tool is executed.
    /// Return `Some(value)` to replace the result, or `None` to keep the original.
    async fn after_execute(
        &self,
        tool_name: &str,
        args: &Value,
        result: &Result<Value, AttaError>,
    ) -> Result<Option<Value>, AttaError>;

    /// Called before an LLM request is sent.
    /// Return `Allow` to proceed, `ModifyArgs` with modified messages, or `Block` to reject.
    /// Default implementation allows all LLM calls.
    async fn before_llm_call(&self, _messages: &[Value]) -> Result<HookOutcome, AttaError> {
        Ok(HookOutcome::Allow)
    }

    /// Called after an LLM response is received.
    /// Return `Some(value)` to replace the response, or `None` to keep the original.
    /// Default implementation passes through.
    async fn after_llm_call(&self, _response: &Value) -> Result<Option<Value>, AttaError> {
        Ok(None)
    }
}

/// A chain of hooks executed in order
pub struct HookChain {
    hooks: Vec<Box<dyn ToolHook>>,
}

impl HookChain {
    /// Create an empty hook chain
    pub fn new() -> Self {
        Self { hooks: Vec::new() }
    }

    /// Add a hook to the chain
    pub fn add(&mut self, hook: Box<dyn ToolHook>) {
        self.hooks.push(hook);
    }

    /// Run all before-hooks in order.
    ///
    /// Returns the final outcome and the (possibly modified) arguments.
    /// If any hook returns `Block`, stops immediately.
    /// If multiple hooks return `ModifyArgs`, they chain (each sees the previous modification).
    pub async fn run_before(
        &self,
        tool_name: &str,
        args: &Value,
    ) -> Result<(HookOutcome, Value), AttaError> {
        let mut current_args = args.clone();

        for hook in &self.hooks {
            match hook.before_execute(tool_name, &current_args).await? {
                HookOutcome::Block(reason) => {
                    return Ok((HookOutcome::Block(reason), current_args));
                }
                HookOutcome::ModifyArgs(new_args) => {
                    current_args = new_args;
                }
                HookOutcome::Allow => {}
            }
        }

        Ok((HookOutcome::Allow, current_args))
    }

    /// Run all after-hooks in order.
    ///
    /// Returns `Some(value)` if any hook wants to replace the result.
    /// The last hook to return `Some` wins.
    pub async fn run_after(
        &self,
        tool_name: &str,
        args: &Value,
        result: &Result<Value, AttaError>,
    ) -> Result<Option<Value>, AttaError> {
        let mut replacement = None;

        for hook in &self.hooks {
            if let Some(new_value) = hook.after_execute(tool_name, args, result).await? {
                replacement = Some(new_value);
            }
        }

        Ok(replacement)
    }

    /// Run all before-LLM hooks in order.
    ///
    /// Returns the final outcome and (possibly modified) messages.
    /// If any hook returns `Block`, stops immediately.
    pub async fn run_before_llm(
        &self,
        messages: &[Value],
    ) -> Result<(HookOutcome, Vec<Value>), AttaError> {
        let mut current_msgs = messages.to_vec();

        for hook in &self.hooks {
            match hook.before_llm_call(&current_msgs).await? {
                HookOutcome::Block(reason) => {
                    return Ok((HookOutcome::Block(reason), current_msgs));
                }
                HookOutcome::ModifyArgs(new_msgs) => {
                    if let Some(arr) = new_msgs.as_array() {
                        current_msgs = arr.clone();
                    }
                }
                HookOutcome::Allow => {}
            }
        }

        Ok((HookOutcome::Allow, current_msgs))
    }

    /// Run all after-LLM hooks in order.
    ///
    /// Returns `Some(value)` if any hook wants to replace the response.
    pub async fn run_after_llm(&self, response: &Value) -> Result<Option<Value>, AttaError> {
        let mut replacement = None;

        for hook in &self.hooks {
            if let Some(new_value) = hook.after_llm_call(response).await? {
                replacement = Some(new_value);
            }
        }

        Ok(replacement)
    }
}

impl Default for HookChain {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AllowHook;

    #[async_trait::async_trait]
    impl ToolHook for AllowHook {
        async fn before_execute(&self, _: &str, _: &Value) -> Result<HookOutcome, AttaError> {
            Ok(HookOutcome::Allow)
        }
        async fn after_execute(
            &self,
            _: &str,
            _: &Value,
            _: &Result<Value, AttaError>,
        ) -> Result<Option<Value>, AttaError> {
            Ok(None)
        }
    }

    struct BlockHook {
        reason: String,
    }

    #[async_trait::async_trait]
    impl ToolHook for BlockHook {
        async fn before_execute(&self, _: &str, _: &Value) -> Result<HookOutcome, AttaError> {
            Ok(HookOutcome::Block(self.reason.clone()))
        }
        async fn after_execute(
            &self,
            _: &str,
            _: &Value,
            _: &Result<Value, AttaError>,
        ) -> Result<Option<Value>, AttaError> {
            Ok(None)
        }
    }

    struct ModifyHook;

    #[async_trait::async_trait]
    impl ToolHook for ModifyHook {
        async fn before_execute(&self, _: &str, _: &Value) -> Result<HookOutcome, AttaError> {
            Ok(HookOutcome::ModifyArgs(
                serde_json::json!({"modified": true}),
            ))
        }
        async fn after_execute(
            &self,
            _: &str,
            _: &Value,
            _: &Result<Value, AttaError>,
        ) -> Result<Option<Value>, AttaError> {
            Ok(Some(serde_json::json!({"replaced": true})))
        }
    }

    #[tokio::test]
    async fn test_hook_chain_allow() {
        let mut chain = HookChain::new();
        chain.add(Box::new(AllowHook));
        let (outcome, _) = chain
            .run_before("test", &serde_json::json!({}))
            .await
            .unwrap();
        assert!(matches!(outcome, HookOutcome::Allow));
    }

    #[tokio::test]
    async fn test_hook_chain_block() {
        let mut chain = HookChain::new();
        chain.add(Box::new(AllowHook));
        chain.add(Box::new(BlockHook {
            reason: "denied".into(),
        }));
        let (outcome, _) = chain
            .run_before("test", &serde_json::json!({}))
            .await
            .unwrap();
        assert!(matches!(outcome, HookOutcome::Block(_)));
    }

    #[tokio::test]
    async fn test_hook_chain_modify_args() {
        let mut chain = HookChain::new();
        chain.add(Box::new(ModifyHook));
        let (outcome, args) = chain
            .run_before("test", &serde_json::json!({}))
            .await
            .unwrap();
        assert!(matches!(outcome, HookOutcome::Allow));
        assert_eq!(args, serde_json::json!({"modified": true}));
    }

    #[tokio::test]
    async fn test_hook_chain_after_replace() {
        let mut chain = HookChain::new();
        chain.add(Box::new(ModifyHook));
        let result: Result<Value, AttaError> = Ok(serde_json::json!({"original": true}));
        let replacement = chain
            .run_after("test", &serde_json::json!({}), &result)
            .await
            .unwrap();
        assert_eq!(replacement, Some(serde_json::json!({"replaced": true})));
    }

    #[tokio::test]
    async fn test_llm_hook_default_allows() {
        let mut chain = HookChain::new();
        chain.add(Box::new(AllowHook));
        let msgs = vec![serde_json::json!({"role": "user", "content": "hello"})];
        let (outcome, _) = chain.run_before_llm(&msgs).await.unwrap();
        assert!(matches!(outcome, HookOutcome::Allow));
    }

    struct LlmBlockHook;

    #[async_trait::async_trait]
    impl ToolHook for LlmBlockHook {
        async fn before_execute(&self, _: &str, _: &Value) -> Result<HookOutcome, AttaError> {
            Ok(HookOutcome::Allow)
        }
        async fn after_execute(
            &self,
            _: &str,
            _: &Value,
            _: &Result<Value, AttaError>,
        ) -> Result<Option<Value>, AttaError> {
            Ok(None)
        }
        async fn before_llm_call(&self, _messages: &[Value]) -> Result<HookOutcome, AttaError> {
            Ok(HookOutcome::Block("llm call blocked".to_string()))
        }
    }

    #[tokio::test]
    async fn test_llm_hook_block() {
        let mut chain = HookChain::new();
        chain.add(Box::new(LlmBlockHook));
        let msgs = vec![serde_json::json!({"role": "user", "content": "hello"})];
        let (outcome, _) = chain.run_before_llm(&msgs).await.unwrap();
        assert!(matches!(outcome, HookOutcome::Block(_)));
    }
}
