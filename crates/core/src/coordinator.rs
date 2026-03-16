//! CoreCoordinator — central event-driven orchestrator
//!
//! Subscribes to EventBus topics and orchestrates:
//! - Agent spawning when flow enters an Agent state
//! - Flow advancement on agent completion
//! - Error policy handling on agent failure

use std::sync::Arc;

use atta_agent::react::AgentStreamEvent;
use atta_agent::{ConversationContext, LlmProvider, ReactAgent};
use atta_bus::EventBus;
use atta_store::StateStore;
use atta_types::{AttaError, EventEnvelope, StateType, ToolRegistry};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::flow_engine::FlowEngine;
use crate::ws_hub::WsHub;

/// CoreCoordinator orchestrates the event loop
pub struct CoreCoordinator {
    bus: Arc<dyn EventBus>,
    store: Arc<dyn StateStore>,
    flow_engine: Arc<FlowEngine>,
    tool_registry: Arc<dyn ToolRegistry>,
    llm: Arc<dyn LlmProvider>,
    ws_hub: Arc<WsHub>,
}

impl CoreCoordinator {
    /// Create a new CoreCoordinator
    pub fn new(
        bus: Arc<dyn EventBus>,
        store: Arc<dyn StateStore>,
        flow_engine: Arc<FlowEngine>,
        tool_registry: Arc<dyn ToolRegistry>,
        llm: Arc<dyn LlmProvider>,
        ws_hub: Arc<WsHub>,
    ) -> Self {
        Self {
            bus,
            store,
            flow_engine,
            tool_registry,
            llm,
            ws_hub,
        }
    }

    /// Start the event loop — subscribes to all relevant topics and processes events.
    /// This spawns background tasks and returns immediately.
    pub async fn start(self: Arc<Self>) -> Result<(), AttaError> {
        // Subscribe to flow.advanced events
        let mut flow_stream = self.bus.subscribe("atta.flow.advanced").await?;
        let coordinator = Arc::clone(&self);
        tokio::spawn(async move {
            use futures::StreamExt;
            while let Some(event) = flow_stream.next().await {
                coordinator.ws_hub.broadcast(&event);
                if let Err(e) = coordinator.on_flow_advanced(&event).await {
                    warn!(error = %e, "on_flow_advanced handler error");
                }
            }
        });

        // Subscribe to agent.completed events
        let mut agent_completed_stream = self.bus.subscribe("atta.agent.completed").await?;
        let coordinator = Arc::clone(&self);
        tokio::spawn(async move {
            use futures::StreamExt;
            while let Some(event) = agent_completed_stream.next().await {
                coordinator.ws_hub.broadcast(&event);
                if let Err(e) = coordinator.on_agent_completed(&event).await {
                    warn!(error = %e, "on_agent_completed handler error");
                }
            }
        });

        // Subscribe to agent.error events
        let mut agent_error_stream = self.bus.subscribe("atta.agent.error").await?;
        let coordinator = Arc::clone(&self);
        tokio::spawn(async move {
            use futures::StreamExt;
            while let Some(event) = agent_error_stream.next().await {
                coordinator.ws_hub.broadcast(&event);
                if let Err(e) = coordinator.on_agent_error(&event).await {
                    warn!(error = %e, "on_agent_error handler error");
                }
            }
        });

        // Subscribe to task.created for broadcasting
        let mut task_created_stream = self.bus.subscribe("atta.task.created").await?;
        let coordinator = Arc::clone(&self);
        tokio::spawn(async move {
            use futures::StreamExt;
            while let Some(event) = task_created_stream.next().await {
                coordinator.ws_hub.broadcast(&event);
            }
        });

        // Subscribe to cron.triggered events for job execution
        let mut cron_stream = self.bus.subscribe("atta.cron.triggered").await?;
        let coordinator = Arc::clone(&self);
        tokio::spawn(async move {
            use futures::StreamExt;
            while let Some(event) = cron_stream.next().await {
                coordinator.ws_hub.broadcast(&event);
                if let Err(e) = coordinator.on_cron_triggered(&event).await {
                    warn!(error = %e, "on_cron_triggered handler error");
                }
            }
        });

        info!("CoreCoordinator event loop started");
        Ok(())
    }

    /// When a flow advances, check if we need to spawn an agent
    async fn on_flow_advanced(&self, event: &EventEnvelope) -> Result<(), AttaError> {
        let task_id_str = &event.entity.id;
        let task_id = match Uuid::parse_str(task_id_str) {
            Ok(id) => id,
            Err(_) => return Ok(()),
        };

        let to_state = event
            .payload
            .get("to")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| {
                warn!(event_type = %event.event_type, "missing 'to' field in event payload");
                ""
            });

        // Load task and flow def to check if target state is Agent type
        let task = match self.store.get_task(&task_id).await? {
            Some(t) => t,
            None => return Ok(()),
        };

        let flow_def = match self.flow_engine.get_flow_def(&task.flow_id) {
            Ok(f) => f,
            Err(_) => return Ok(()),
        };

        let state_def = match flow_def.states.get(to_state) {
            Some(s) => s,
            None => return Ok(()),
        };

        if state_def.state_type != StateType::Agent {
            return Ok(());
        }

        info!(
            task_id = %task_id,
            state = to_state,
            agent = state_def.agent.as_deref().unwrap_or("default"),
            "spawning agent for Agent state"
        );

        // Publish agent assigned event
        self.bus
            .publish(
                "atta.agent.assigned",
                EventEnvelope::agent_assigned(
                    &task_id,
                    state_def.agent.as_deref().unwrap_or("react"),
                )?,
            )
            .await?;

        // Build system prompt
        let prompt_ctx = atta_agent::PromptContext {
            tools: self.tool_registry.list_schemas(),
            current_state: Some(to_state.to_string()),
            ..Default::default()
        };
        let mut system_prompt = atta_agent::SystemPromptBuilder::with_defaults().build(&prompt_ctx);

        // Select dispatcher based on model capabilities
        let dispatcher = atta_agent::select_dispatcher(&self.llm.model_info());
        if let Some(instructions) = dispatcher.prompt_instructions() {
            system_prompt.push_str("\n\n");
            system_prompt.push_str(&instructions);
        }

        // Build user message from task input
        let user_message =
            serde_json::to_string_pretty(&task.input).unwrap_or_else(|_| task.input.to_string());

        // Spawn agent
        let llm = Arc::clone(&self.llm);
        let tool_registry = Arc::clone(&self.tool_registry);
        let store = Arc::clone(&self.store);
        let bus = Arc::clone(&self.bus);
        let tools = tool_registry.list_schemas();

        let usage_store = Arc::clone(&self.store);
        tokio::spawn(async move {
            let mut ctx = ConversationContext::new(128_000);
            ctx.set_system(&system_prompt);
            ctx.add_user(&user_message);

            let usage_cb = crate::usage_tracking::build_usage_callback_with_task(
                Arc::clone(&usage_store),
                task_id.to_string(),
            );
            let mut agent = ReactAgent::new(llm, tool_registry, ctx, 10)
                .with_tools(tools)
                .with_dispatcher(dispatcher)
                .with_usage_callback(usage_cb);

            // Set up streaming delta channel
            let (delta_tx, mut delta_rx) = tokio::sync::mpsc::channel::<AgentStreamEvent>(256);

            // Forward deltas to event bus
            let delta_bus = Arc::clone(&bus);
            tokio::spawn(async move {
                while let Some(event) = delta_rx.recv().await {
                    if let Ok(payload) = serde_json::to_value(&event) {
                        if let Ok(envelope) = EventEnvelope::agent_delta(&task_id, &payload) {
                            let _ = delta_bus
                                .publish("atta.agent.delta", envelope)
                                .await;
                        }
                    }
                }
            });

            let result = agent.run_streaming(delta_tx).await;

            match result {
                Ok(output) => {
                    info!(task_id = %task_id, "agent completed successfully");

                    // Merge output into state_data
                    let patch = serde_json::json!({
                        "agent_output": output,
                        "agent_status": "completed",
                    });
                    if let Err(e) = store.merge_task_state_data(&task_id, patch).await {
                        error!(task_id = %task_id, error = %e, "failed to merge agent output");
                    }

                    // Publish completion event
                    match EventEnvelope::agent_completed(&task_id, &output, 0) {
                        Ok(envelope) => {
                            if let Err(e) = bus
                                .publish("atta.agent.completed", envelope)
                                .await
                            {
                                error!(task_id = %task_id, error = %e, "failed to publish agent.completed event");
                            }
                        }
                        Err(e) => {
                            error!(task_id = %task_id, error = %e, "failed to create agent.completed event");
                        }
                    }
                }
                Err(e) => {
                    warn!(task_id = %task_id, error = %e, "agent execution failed");

                    let patch = serde_json::json!({
                        "agent_status": "error",
                        "error": e.to_string(),
                    });
                    if let Err(e) = store.merge_task_state_data(&task_id, patch).await {
                        error!(task_id = %task_id, error = %e, "failed to merge agent error state");
                    }

                    match EventEnvelope::agent_error(&task_id, &e.to_string()) {
                        Ok(envelope) => {
                            if let Err(e) = bus
                                .publish("atta.agent.error", envelope)
                                .await
                            {
                                error!(task_id = %task_id, error = %e, "failed to publish agent.error event");
                            }
                        }
                        Err(ee) => {
                            error!(task_id = %task_id, error = %ee, "failed to create agent.error event");
                        }
                    }
                }
            }
        });

        Ok(())
    }

    /// On agent completion, advance the flow
    async fn on_agent_completed(&self, event: &EventEnvelope) -> Result<(), AttaError> {
        let task_id = match Uuid::parse_str(&event.entity.id) {
            Ok(id) => id,
            Err(_) => return Ok(()),
        };

        self.flow_engine.advance_by_id(&task_id).await?;
        Ok(())
    }

    /// On agent error, try error policy / advance
    async fn on_agent_error(&self, event: &EventEnvelope) -> Result<(), AttaError> {
        let task_id = match Uuid::parse_str(&event.entity.id) {
            Ok(id) => id,
            Err(_) => return Ok(()),
        };

        if let Err(e) = self.flow_engine.advance_by_id(&task_id).await {
            warn!(
                task_id = %task_id,
                error = %e,
                "failed to advance flow after agent error"
            );
        }
        Ok(())
    }

    /// On cron.triggered — execute the cron job command via agent
    async fn on_cron_triggered(&self, event: &EventEnvelope) -> Result<(), AttaError> {
        let job_id = event
            .payload
            .get("job_id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| {
                warn!(event_type = %event.event_type, "missing 'job_id' field in event payload");
                ""
            });
        let run_id = event
            .payload
            .get("run_id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| {
                warn!(event_type = %event.event_type, "missing 'run_id' field in event payload");
                ""
            });
        let command = event
            .payload
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| {
                warn!(event_type = %event.event_type, "missing 'command' field in event payload");
                ""
            });

        if command.is_empty() {
            warn!(job_id = %job_id, "cron job has empty command, skipping");
            return Ok(());
        }

        info!(job_id = %job_id, run_id = %run_id, command = %command, "executing cron job");

        // Build a one-shot agent to execute the cron command
        let prompt_ctx = atta_agent::PromptContext {
            tools: self.tool_registry.list_schemas(),
            current_state: Some("cron_execution".to_string()),
            ..Default::default()
        };
        let mut system_prompt = atta_agent::SystemPromptBuilder::with_defaults().build(&prompt_ctx);

        let dispatcher = atta_agent::select_dispatcher(&self.llm.model_info());
        if let Some(instructions) = dispatcher.prompt_instructions() {
            system_prompt.push_str("\n\n");
            system_prompt.push_str(&instructions);
        }

        let llm = Arc::clone(&self.llm);
        let tool_registry = Arc::clone(&self.tool_registry);
        let store = Arc::clone(&self.store);
        let tools = tool_registry.list_schemas();
        let job_id_owned = job_id.to_string();
        let run_id_owned = run_id.to_string();
        let command_owned = command.to_string();

        let cron_usage_store = Arc::clone(&self.store);
        tokio::spawn(async move {
            let mut ctx = ConversationContext::new(128_000);
            ctx.set_system(&system_prompt);
            ctx.add_user(&command_owned);

            let usage_cb = crate::usage_tracking::build_usage_callback(Arc::clone(&cron_usage_store));
            let mut agent = ReactAgent::new(llm, tool_registry, ctx, 10)
                .with_tools(tools)
                .with_dispatcher(dispatcher)
                .with_usage_callback(usage_cb);

            let result = agent.run().await;

            let cron_run = match result {
                Ok(output) => {
                    info!(
                        job_id = %job_id_owned,
                        run_id = %run_id_owned,
                        "cron job completed successfully"
                    );
                    let output_str =
                        serde_json::to_string(&output).unwrap_or_else(|_| output.to_string());
                    atta_types::CronRun {
                        id: run_id_owned,
                        job_id: job_id_owned,
                        status: atta_types::CronRunStatus::Completed,
                        started_at: chrono::Utc::now(),
                        completed_at: Some(chrono::Utc::now()),
                        output: Some(output_str),
                        error: None,
                        triggered_by: "scheduler".to_string(),
                    }
                }
                Err(e) => {
                    warn!(
                        job_id = %job_id_owned,
                        run_id = %run_id_owned,
                        error = %e,
                        "cron job execution failed"
                    );
                    atta_types::CronRun {
                        id: run_id_owned,
                        job_id: job_id_owned,
                        status: atta_types::CronRunStatus::Failed,
                        started_at: chrono::Utc::now(),
                        completed_at: Some(chrono::Utc::now()),
                        output: None,
                        error: Some(e.to_string()),
                        triggered_by: "scheduler".to_string(),
                    }
                }
            };

            if let Err(e) = store.save_cron_run(&cron_run).await {
                error!(error = %e, "failed to save cron run result");
            }
        });

        Ok(())
    }
}
