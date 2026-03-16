//! Start Flow tool — creates a new Task from a FlowDef
//!
//! This tool allows agents to trigger flows programmatically during
//! a conversation. The FlowRunner is late-bound at the Core layer.

use std::sync::Arc;

use atta_types::{AttaError, FlowRunner, RiskLevel};
use serde_json::{json, Value};

/// Tool for starting a flow and creating a task.
pub struct StartFlowTool {
    runner: Option<Arc<dyn FlowRunner>>,
}

impl StartFlowTool {
    /// Create a new StartFlowTool (initially unbound).
    pub fn new() -> Self {
        Self { runner: None }
    }

    /// Attach a FlowRunner for actual execution.
    pub fn with_runner(mut self, runner: Arc<dyn FlowRunner>) -> Self {
        self.runner = Some(runner);
        self
    }

    /// Replace the runner (for late-binding).
    pub fn set_runner(&mut self, runner: Arc<dyn FlowRunner>) {
        self.runner = Some(runner);
    }
}

impl Default for StartFlowTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl atta_types::NativeTool for StartFlowTool {
    fn name(&self) -> &str {
        "atta-start-flow"
    }

    fn description(&self) -> &str {
        "Start a workflow (Flow) by creating a new Task. Returns the task ID for tracking progress. \
         Use `list_flows` action to see available flows first."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["start", "list_flows"],
                    "description": "Action to perform: 'start' creates a new task, 'list_flows' lists available flows"
                },
                "flow_id": {
                    "type": "string",
                    "description": "Flow ID to start (required for 'start' action)"
                },
                "input": {
                    "type": "object",
                    "description": "Input data for the flow (passed as task input)"
                }
            },
            "required": ["action"]
        })
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Medium
    }

    async fn execute(&self, args: Value) -> Result<Value, AttaError> {
        let runner = self.runner.as_ref().ok_or_else(|| {
            AttaError::Validation("FlowRunner not configured".into())
        })?;

        let action = args["action"]
            .as_str()
            .unwrap_or("start");

        match action {
            "list_flows" => {
                let flows = runner.list_flows().await?;
                let list: Vec<Value> = flows
                    .into_iter()
                    .map(|(id, name)| {
                        json!({
                            "id": id,
                            "name": name.unwrap_or_default()
                        })
                    })
                    .collect();
                Ok(json!({ "flows": list }))
            }
            "start" => {
                let flow_id = args["flow_id"]
                    .as_str()
                    .ok_or_else(|| {
                        AttaError::Validation("flow_id is required for 'start' action".into())
                    })?;

                let input = args.get("input").cloned().unwrap_or(json!({}));

                let task = runner
                    .start_flow(flow_id, input, atta_types::Actor::system())
                    .await?;

                Ok(json!({
                    "task_id": task.id.to_string(),
                    "flow_id": task.flow_id,
                    "status": task.status.as_str(),
                    "current_state": task.current_state,
                    "message": format!("Flow '{}' started, task ID: {}", flow_id, task.id)
                }))
            }
            other => Err(AttaError::Validation(
                format!("Unknown action: {other}"),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::NativeTool;

    #[test]
    fn tool_metadata() {
        let tool = StartFlowTool::new();
        assert_eq!(tool.name(), "atta-start-flow");
        assert_eq!(tool.risk_level(), RiskLevel::Medium);
        assert!(tool.description().contains("workflow"));

        let schema = tool.parameters_schema();
        assert!(schema["properties"]["action"].is_object());
        assert!(schema["properties"]["flow_id"].is_object());
    }
}
