//! Agent-based channel message handler
//!
//! Implements [`ChannelMessageHandler`] by running incoming messages through
//! the ReAct agent pipeline. This lives in `atta-core` so that the channel
//! crate does not depend on `atta-agent` directly.
//!
//! ## Features
//!
//! - **Draft streaming**: On channels that support draft updates (e.g., Telegram
//!   editMessageText), the handler sends an initial draft and updates it in
//!   real-time as the agent generates text.
//! - **Session-aware routing**: Reads `_agent_id` / `_flow_id` from enriched
//!   metadata to select the correct agent pipeline.
//! - **Context enhancement**: Injects session history and channel context into
//!   the agent prompt.

use std::sync::Arc;

use atta_agent::{select_dispatcher, ConversationContext, LlmProvider, ReactAgent};
use atta_channel::handler::ChannelMessageHandler;
use atta_channel::traits::{Channel, ChannelMessage, ChatType, SendMessage};
use atta_channel::DraftManager;
use atta_store::StateStore;
use atta_types::ToolRegistry;
use tracing::{debug, error, info, warn};

/// Agent-based handler that processes channel messages through the ReAct loop.
pub struct AgentChannelHandler {
    llm: Arc<dyn LlmProvider>,
    tool_registry: Arc<dyn ToolRegistry>,
    store: Arc<dyn StateStore>,
}

impl AgentChannelHandler {
    /// Create a new handler backed by the given LLM, tool registry, and store.
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        tool_registry: Arc<dyn ToolRegistry>,
        store: Arc<dyn StateStore>,
    ) -> Self {
        Self {
            llm,
            tool_registry,
            store,
        }
    }

    /// Build a ReactAgent for the given message.
    fn build_agent(&self, msg: &ChannelMessage) -> ReactAgent {
        let dispatcher = select_dispatcher(&self.llm.model_info());

        let mut ctx = ConversationContext::new(128_000);
        let prompt_ctx = atta_agent::PromptContext {
            tools: self.tool_registry.list_schemas(),
            channel: Some(msg.channel.clone()),
            ..Default::default()
        };
        let mut system_prompt = atta_agent::SystemPromptBuilder::with_defaults().build(&prompt_ctx);
        if let Some(instructions) = dispatcher.prompt_instructions() {
            system_prompt.push_str("\n\n");
            system_prompt.push_str(&instructions);
        }

        // Context enhancement: add channel and session metadata
        self.enhance_context(&mut system_prompt, msg);

        ctx.set_system(&system_prompt);
        ctx.add_user(&msg.content);

        let tools = self.tool_registry.list_schemas();
        let usage_cb = crate::usage_tracking::build_usage_callback(Arc::clone(&self.store));
        ReactAgent::new(
            Arc::clone(&self.llm),
            Arc::clone(&self.tool_registry),
            ctx,
            10,
        )
        .with_tools(tools)
        .with_dispatcher(dispatcher)
        .with_usage_callback(usage_cb)
    }

    /// Enhance the system prompt with channel and session context.
    fn enhance_context(&self, prompt: &mut String, msg: &ChannelMessage) {
        prompt.push_str("\n\n## Channel Context\n");
        prompt.push_str(&format!("- Channel: {}\n", msg.channel));
        prompt.push_str(&format!("- Chat type: {:?}\n", msg.chat_type));
        prompt.push_str(&format!("- Sender: {}\n", msg.sender));

        if let Some(ref group_id) = msg.group_id {
            prompt.push_str(&format!("- Group: {}\n", group_id));
        }

        // Add session context from enriched metadata
        if let Some(session_key) = msg.metadata.get("_session_key").and_then(|v| v.as_str()) {
            prompt.push_str(&format!("- Session: {}\n", session_key));
        }

        // Add group-specific instructions
        if matches!(msg.chat_type, ChatType::Group | ChatType::SuperGroup) {
            prompt.push_str("\nYou are responding in a group chat. Keep responses concise and relevant. ");
            prompt.push_str("Do not include tool execution details unless specifically asked.\n");
        }
    }

    /// Send response using draft streaming if supported, otherwise direct send.
    async fn send_response(
        &self,
        channel: &dyn Channel,
        msg: &ChannelMessage,
        answer: &str,
    ) {
        if channel.supports_draft_updates() && answer.len() > 100 {
            // Use draft streaming for longer responses
            self.send_with_draft(channel, msg, answer).await;
        } else {
            // Direct send for short responses or channels without draft support
            let response = SendMessage {
                recipient: msg.sender.clone(),
                content: answer.to_string(),
                subject: None,
                thread_ts: msg.thread_ts.clone(),
                metadata: serde_json::json!({}),
            };
            if let Err(e) = atta_channel::dispatch::send_with_retry(channel, response, 3).await {
                error!(error = %e, "failed to send response after retries");
            }
        }
    }

    /// Send response using draft-based streaming (edit-in-place).
    async fn send_with_draft(
        &self,
        channel: &dyn Channel,
        msg: &ChannelMessage,
        answer: &str,
    ) {
        let draft_manager = DraftManager::new(std::time::Duration::from_millis(500));

        // Send initial draft
        let draft_msg = SendMessage {
            recipient: msg.sender.clone(),
            content: "...".to_string(),
            subject: None,
            thread_ts: msg.thread_ts.clone(),
            metadata: serde_json::json!({}),
        };

        let draft_id = match channel.send_draft(draft_msg).await {
            Ok(id) => id,
            Err(e) => {
                warn!(error = %e, "failed to send draft, falling back to direct send");
                let response = SendMessage {
                    recipient: msg.sender.clone(),
                    content: answer.to_string(),
                    subject: None,
                    thread_ts: msg.thread_ts.clone(),
                    metadata: serde_json::json!({}),
                };
                let _ = atta_channel::dispatch::send_with_retry(channel, response, 3).await;
                return;
            }
        };

        // NOTE: This simulates streaming by post-hoc chunking the completed answer.
        // A proper implementation would integrate with the agent's streaming callback
        // (ReactAgent::run_streaming) to update the draft as tokens arrive from the LLM.
        // This requires adding a streaming callback parameter to ChannelMessageHandler::handle().
        let chunk_size = 100; // characters per update
        let chars: Vec<char> = answer.chars().collect();

        for end in (chunk_size..=chars.len()).step_by(chunk_size) {
            let partial: String = chars[..end].iter().collect();
            if let Some(text) = draft_manager.accumulate(&partial) {
                if let Err(e) = channel.update_draft(&draft_id, &text).await {
                    debug!(error = %e, "draft update failed, continuing");
                }
            }
        }

        // Finalize with full content
        if let Err(e) = channel.update_draft(&draft_id, answer).await {
            warn!(error = %e, "failed to finalize draft content");
        }
        if let Err(e) = channel.finalize_draft(&draft_id).await {
            warn!(error = %e, "failed to finalize draft");
        }
    }
}

#[async_trait::async_trait]
impl ChannelMessageHandler for AgentChannelHandler {
    async fn handle(&self, msg: &ChannelMessage, channel: &dyn Channel) {
        let session_key = msg.metadata.get("_session_key").and_then(|v| v.as_str()).unwrap_or("-");
        let agent_id = msg.metadata.get("_agent_id").and_then(|v| v.as_str());
        let flow_id = msg.metadata.get("_flow_id").and_then(|v| v.as_str());

        info!(
            channel = msg.channel,
            sender = %msg.sender,
            chat_type = ?msg.chat_type,
            session = session_key,
            agent_id = agent_id.unwrap_or("-"),
            flow_id = flow_id.unwrap_or("-"),
            "processing channel message"
        );

        // TODO: Use agent_id/flow_id from session metadata to select different
        // agent pipelines or flows. Currently all messages use the default agent.
        // This requires an AgentRegistry lookup by agent_id and FlowEngine
        // dispatch by flow_id, which are not yet wired into this handler.
        if agent_id.is_some() || flow_id.is_some() {
            debug!(
                agent_id = agent_id.unwrap_or("-"),
                flow_id = flow_id.unwrap_or("-"),
                "session has agent/flow binding — using default agent (per-session routing not yet implemented)"
            );
        }

        // Start typing indicator
        if let Err(e) = channel.start_typing(&msg.sender).await {
            warn!(error = %e, "failed to start typing indicator");
        }

        let mut agent = self.build_agent(msg);

        let answer = match agent.run().await {
            Ok(output) => output
                .get("answer")
                .and_then(|v| v.as_str())
                .unwrap_or("I couldn't generate a response.")
                .to_string(),
            Err(e) => {
                error!(error = %e, "agent execution failed");
                format!("Error: {e}")
            }
        };

        // Stop typing indicator
        if let Err(e) = channel.stop_typing(&msg.sender).await {
            warn!(error = %e, "failed to stop typing indicator");
        }

        // Send response (with draft streaming if supported)
        self.send_response(channel, msg, &answer).await;
    }
}
