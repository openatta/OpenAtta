//! Sub-agent delegation tool
//!
//! Delegates tasks to sub-agents with depth control, timeout,
//! and optional tool filtering.

use std::sync::Arc;
use std::time::Duration;

use atta_types::{AttaError, RiskLevel};
use serde_json::{json, Value};

/// Opaque sub-agent runner — implemented at the CLI/Core layer to avoid
/// circular dependency between atta-tools and atta-agent.
#[async_trait::async_trait]
pub trait SubAgentRunner: Send + Sync + 'static {
    /// Run a sub-agent with the given task, model, allowed tools, timeout, and depth.
    /// Returns the sub-agent's final output as JSON.
    async fn run(
        &self,
        task: &str,
        model: Option<&str>,
        allowed_tools: Option<&[String]>,
        timeout: Duration,
        depth: u32,
    ) -> Result<Value, AttaError>;
}

/// Configuration for delegation depth and timeout
pub struct DelegateConfig {
    /// Maximum delegation depth (default 3)
    pub max_depth: u32,
    /// Timeout for sub-agent execution (default 300s)
    pub timeout: Duration,
    /// Current depth in the delegation chain
    pub current_depth: u32,
}

impl Default for DelegateConfig {
    fn default() -> Self {
        Self {
            max_depth: 3,
            timeout: Duration::from_secs(300),
            current_depth: 0,
        }
    }
}

/// Delegation tool — delegates tasks to sub-agents
pub struct DelegationTool {
    config: DelegateConfig,
    runner: Option<Arc<dyn SubAgentRunner>>,
}

impl DelegationTool {
    /// Create a new DelegationTool with the given configuration
    pub fn new(config: DelegateConfig) -> Self {
        Self {
            config,
            runner: None,
        }
    }

    /// Attach a sub-agent runner for actual delegation
    pub fn with_runner(mut self, runner: Arc<dyn SubAgentRunner>) -> Self {
        self.runner = Some(runner);
        self
    }
}

impl Default for DelegationTool {
    fn default() -> Self {
        Self::new(DelegateConfig::default())
    }
}

#[async_trait::async_trait]
impl atta_types::NativeTool for DelegationTool {
    fn name(&self) -> &str {
        "atta-delegation"
    }

    fn description(&self) -> &str {
        "Delegate a sub-task to another agent for parallel or specialized processing"
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Medium
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "Task description for the sub-agent"
                },
                "model": {
                    "type": "string",
                    "description": "LLM model to use for the sub-agent (optional)"
                },
                "allowed_tools": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Tools available to the sub-agent (empty = all)"
                }
            },
            "required": ["task"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let task = args["task"]
            .as_str()
            .ok_or_else(|| AttaError::Validation("'task' is required".into()))?;

        // Depth check
        if self.config.current_depth >= self.config.max_depth {
            return Err(AttaError::SecurityViolation(format!(
                "delegation depth limit reached ({}/{})",
                self.config.current_depth, self.config.max_depth
            )));
        }

        let model = args["model"].as_str();
        let allowed_tools: Option<Vec<String>> = args["allowed_tools"].as_array().map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        });

        // If no runner configured, return placeholder
        let Some(ref runner) = self.runner else {
            return Ok(json!({
                "status": "delegated",
                "task": task,
                "depth": self.config.current_depth,
                "max_depth": self.config.max_depth,
                "timeout_secs": self.config.timeout.as_secs(),
                "message": "delegation requires SubAgentRunner to be configured"
            }));
        };

        // Run the sub-agent with depth+1
        let result = runner
            .run(
                task,
                model,
                allowed_tools.as_deref(),
                self.config.timeout,
                self.config.current_depth + 1,
            )
            .await?;

        Ok(json!({
            "status": "completed",
            "task": task,
            "depth": self.config.current_depth + 1,
            "result": result,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn test_delegation_name() {
        let tool = DelegationTool::default();
        assert_eq!(tool.name(), "atta-delegation");
    }

    #[tokio::test]
    async fn test_delegation_placeholder() {
        let tool = DelegationTool::default();
        let result = tool
            .execute(json!({"task": "research topic X"}))
            .await
            .unwrap();
        assert_eq!(result["status"], "delegated");
        assert_eq!(result["task"], "research topic X");
        assert_eq!(result["depth"], 0);
    }

    #[tokio::test]
    async fn test_delegation_missing_task() {
        let tool = DelegationTool::default();
        let result = tool.execute(json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delegation_depth_limit() {
        let config = DelegateConfig {
            max_depth: 2,
            current_depth: 2,
            ..Default::default()
        };
        let tool = DelegationTool::new(config);
        let result = tool.execute(json!({"task": "do something"})).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("depth limit"));
    }

    #[tokio::test]
    async fn test_delegation_within_depth() {
        let config = DelegateConfig {
            max_depth: 3,
            current_depth: 1,
            ..Default::default()
        };
        let tool = DelegationTool::new(config);
        let result = tool.execute(json!({"task": "sub-task"})).await.unwrap();
        assert_eq!(result["depth"], 1);
        assert_eq!(result["max_depth"], 3);
    }
}
