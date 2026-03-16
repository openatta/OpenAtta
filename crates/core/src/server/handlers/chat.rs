//! Chat SSE handler
//!
//! `POST /api/v1/chat` — 接收 `ChatRequest`，构建 ReactAgent，
//! 通过 SSE 流式返回 `ChatEvent`。

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Json;
use futures::stream::Stream;
use tokio::sync::mpsc;

use atta_agent::react::{AgentDelta, AgentStreamEvent};
use atta_agent::{ConversationContext, ReactAgent};
use atta_types::{ChatEvent, ChatRequest};

use crate::middleware::CurrentUser;
use super::super::AppState;

/// POST /api/v1/chat → SSE stream
pub async fn chat_sse(
    State(state): State<AppState>,
    _user: CurrentUser,
    Json(req): Json<ChatRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel::<ChatEvent>(256);

    // Spawn agent task
    tokio::spawn(async move {
        if let Err(e) = run_chat_agent(state, req, tx.clone()).await {
            let _ = tx
                .send(ChatEvent::Error {
                    message: e.to_string(),
                })
                .await;
        }
    });

    // Convert mpsc receiver into SSE stream
    let stream = async_stream::stream! {
        let mut rx = rx;
        while let Some(event) = rx.recv().await {
            match serde_json::to_string(&event) {
                Ok(data) => yield Ok(Event::default().data(data)),
                Err(e) => {
                    tracing::warn!(error = %e, "failed to serialize SSE event, skipping");
                    continue;
                }
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Build and run a ReactAgent, forwarding deltas as ChatEvent
async fn run_chat_agent(
    state: AppState,
    req: ChatRequest,
    chat_tx: mpsc::Sender<ChatEvent>,
) -> Result<(), atta_types::AttaError> {
    let llm = Arc::clone(&state.llm);
    let tool_registry = Arc::clone(&state.tool_registry);

    // Build system prompt (optionally from skill)
    let mut system_prompt = String::from(
        "You are AttaOS, an AI assistant. Answer the user's question helpfully and concisely.",
    );
    let mut tools = tool_registry.list_schemas();

    if let Some(skill_id) = &req.skill_id {
        if let Some(skill) = state.skill_registry.get(skill_id) {
            let variables = serde_json::json!({"input": req.message});
            system_prompt = crate::skill_engine::build_skill_system_prompt(&skill, &variables);
            tools = crate::skill_engine::filter_tools_for_skill(tool_registry.as_ref(), &skill);
        }
    }

    // Select dispatcher based on model capabilities
    let dispatcher = atta_agent::select_dispatcher(&llm.model_info());
    if let Some(instructions) = dispatcher.prompt_instructions() {
        system_prompt.push_str("\n\n");
        system_prompt.push_str(&instructions);
    }

    let mut ctx = ConversationContext::new(128_000);
    ctx.set_system(&system_prompt);
    ctx.add_user(&req.message);

    // Build usage callback to persist token usage to the store
    let usage_cb = crate::usage_tracking::build_usage_callback(Arc::clone(&state.store));

    let mut agent = ReactAgent::new(llm, tool_registry, ctx, 10)
        .with_tools(tools)
        .with_dispatcher(dispatcher)
        .with_usage_callback(usage_cb);

    // Set up streaming delta channel
    let (delta_tx, mut delta_rx) = mpsc::channel::<AgentStreamEvent>(256);

    // Forward agent deltas → ChatEvent
    let chat_tx_clone = chat_tx.clone();
    tokio::spawn(async move {
        while let Some(event) = delta_rx.recv().await {
            let chat_event = match event {
                AgentStreamEvent::Delta(delta) => delta_to_chat_event(delta),
                AgentStreamEvent::StreamChunk(chunk) => {
                    if let atta_agent::StreamChunk::TextDelta { delta } = chunk {
                        Some(ChatEvent::TextDelta { delta })
                    } else {
                        None
                    }
                }
            };
            if let Some(ce) = chat_event {
                if chat_tx_clone.send(ce).await.is_err() {
                    break;
                }
            }
        }
    });

    let _result = agent.run_streaming(delta_tx).await?;
    Ok(())
}

/// Convert AgentDelta to ChatEvent
fn delta_to_chat_event(delta: AgentDelta) -> Option<ChatEvent> {
    match delta {
        AgentDelta::Thinking { iteration } => Some(ChatEvent::Thinking { iteration }),
        AgentDelta::TextChunk { .. } => None, // TextDelta already sent via StreamChunk
        AgentDelta::ToolStart { tool_name, call_id } => {
            Some(ChatEvent::ToolStart { tool_name, call_id })
        }
        AgentDelta::ToolComplete {
            tool_name,
            call_id,
            duration_ms,
        } => Some(ChatEvent::ToolComplete {
            tool_name,
            call_id,
            duration_ms,
        }),
        AgentDelta::ToolError {
            tool_name,
            call_id,
            error,
        } => Some(ChatEvent::ToolError {
            tool_name,
            call_id,
            error,
        }),
        AgentDelta::Done { iterations } => Some(ChatEvent::Done { iterations }),
        AgentDelta::ClearProgress => None,
        AgentDelta::ApprovalPending { .. } | AgentDelta::ApprovalGranted { .. } => None,
    }
}
