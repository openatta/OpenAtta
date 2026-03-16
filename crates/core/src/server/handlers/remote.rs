//! Remote Agent API handlers
//!
//! WebSocket 端点 + REST 管理 API。

use axum::{
    extract::{
        ws::{self, WebSocket},
        Path, Query, State, WebSocketUpgrade,
    },
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tracing::{debug, info, warn};

use atta_audit::{AuditEntry, AuditOutcome};
use atta_types::{
    Action, AttaError, DownstreamMsg, EntityRef, RegisterRemoteAgentRequest,
    RegisterRemoteAgentResponse, RemoteAgent, RemoteAgentStatus, Resource, ResourceType,
    UpstreamMsg,
};

use crate::middleware::CurrentUser;
use crate::server::response::{check_authz, error_response};
use crate::server::AppState;

// ── WebSocket 端点 ──

#[derive(Debug, Deserialize)]
pub struct WsTokenQuery {
    pub token: Option<String>,
}

/// WebSocket 升级端点
///
/// GET /api/v1/remote/ws?token=aat_xxx
pub async fn remote_ws_upgrade(
    State(state): State<AppState>,
    Query(params): Query<WsTokenQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let token = match params.token {
        Some(t) => t,
        None => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                &AttaError::Validation("missing token query parameter".into()),
            );
        }
    };

    let token_hash = hash_token(&token);

    // Validate token against store
    match state.store.get_remote_agent_by_token(&token_hash).await {
        Ok(Some(agent)) => {
            // Check token expiry
            if let Some(expires) = agent.token_expires_at {
                if expires < chrono::Utc::now() {
                    return error_response(
                        StatusCode::UNAUTHORIZED,
                        &AttaError::Validation("agent token has expired".into()),
                    );
                }
            }
            ws.on_upgrade(move |socket| handle_remote_ws(state, token_hash, socket))
                .into_response()
        }
        Ok(None) => error_response(
            StatusCode::UNAUTHORIZED,
            &AttaError::Validation("invalid agent token".into()),
        ),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

/// 处理远程 Agent WebSocket 连接
async fn handle_remote_ws(state: AppState, token_hash: String, socket: WebSocket) {
    let (mut sender, mut receiver) = socket.split();

    // Look up agent by token
    let agent = match state.store.get_remote_agent_by_token(&token_hash).await {
        Ok(Some(a)) => a,
        Ok(None) => {
            warn!("remote agent token not found after upgrade");
            return;
        }
        Err(e) => {
            warn!(error = %e, "failed to lookup remote agent");
            return;
        }
    };

    let agent_id = agent.id.clone();
    info!(agent_id = %agent_id, agent_name = %agent.name, "remote agent WebSocket connected");

    // Mark as online
    if let Err(e) = state
        .store
        .update_remote_agent_status(&agent_id, &RemoteAgentStatus::Online)
        .await
    {
        warn!(error = %e, "failed to update remote agent status");
    }

    // Register in hub
    let mut hub_rx = state.remote_agent_hub.add_connection(agent).await;

    // Forward hub messages (downstream) to WebSocket
    let agent_id_send = agent_id.clone();
    let send_task = tokio::spawn(async move {
        while let Some(msg) = hub_rx.recv().await {
            if sender.send(ws::Message::Text(msg)).await.is_err() {
                debug!(agent_id = %agent_id_send, "WebSocket send failed");
                break;
            }
        }
    });

    // Receive from WebSocket (upstream) and process
    let state_recv = state.clone();
    let agent_id_recv = agent_id.clone();
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                ws::Message::Text(text) => {
                    handle_upstream_message(&state_recv, &agent_id_recv, &text).await;
                }
                ws::Message::Close(_) => break,
                _ => {} // Ignore binary, ping/pong handled by axum
            }
        }
    });

    // Wait for either side to close
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }

    // Cleanup
    state.remote_agent_hub.remove_connection(&agent_id).await;
    if let Err(e) = state
        .store
        .update_remote_agent_status(&agent_id, &RemoteAgentStatus::Offline)
        .await
    {
        warn!(error = %e, "failed to mark remote agent offline");
    }
    info!(agent_id = %agent_id, "remote agent WebSocket disconnected");
}

/// 处理上行消息
async fn handle_upstream_message(state: &AppState, agent_id: &str, text: &str) {
    let msg: UpstreamMsg = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            debug!(agent_id = %agent_id, error = %e, "invalid upstream message");
            return;
        }
    };

    match msg {
        UpstreamMsg::EventBatch { msg_id, events } => {
            debug!(
                agent_id = %agent_id,
                event_count = events.len(),
                "received event batch"
            );

            // Convert remote events → EventEnvelope and publish to bus + audit
            for event in &events {
                let envelope = match atta_types::EventEnvelope::new(
                    &event.event_type,
                    atta_types::EntityRef::new(
                        atta_types::ResourceType::Node,
                        format!("remote:{agent_id}"),
                    ),
                    atta_types::Actor {
                        actor_type: atta_types::ActorType::Agent,
                        id: agent_id.to_string(),
                    },
                    event
                        .correlation_id
                        .as_deref()
                        .and_then(|s| uuid::Uuid::parse_str(s).ok())
                        .unwrap_or_else(uuid::Uuid::new_v4),
                    &event.payload,
                ) {
                    Ok(env) => env,
                    Err(e) => {
                        warn!(error = %e, "failed to create event envelope for remote event");
                        continue;
                    }
                };

                if let Err(e) = state
                    .bus
                    .publish(&event.event_type, envelope.clone())
                    .await
                {
                    warn!(error = %e, "failed to publish remote event to bus");
                }

                // Forward to WebSocket hub for WebUI
                state.ws_hub.broadcast(&envelope);
            }

            // Send ACK
            let ack = DownstreamMsg::Ack { msg_id };
            state.remote_agent_hub.send_to(agent_id, &ack).await;
        }
        UpstreamMsg::Register { msg_id, .. } => {
            // Agent already registered via REST API; just confirm
            let resp = DownstreamMsg::Registered {
                msg_id,
                agent_id: agent_id.to_string(),
            };
            state.remote_agent_hub.send_to(agent_id, &resp).await;
        }
        UpstreamMsg::Deregister { msg_id, reason } => {
            info!(agent_id = %agent_id, reason = %reason, "remote agent deregistering");
            let ack = DownstreamMsg::Ack { msg_id };
            state.remote_agent_hub.send_to(agent_id, &ack).await;
            // Connection will close naturally
        }
    }

    // Update heartbeat on any message
    if let Err(e) = state.store.update_remote_agent_heartbeat(agent_id).await {
        debug!(error = %e, "failed to update heartbeat");
    }
}

// ── REST API 端点 ──

/// POST /api/v1/remote/agents — 注册新远程 Agent
pub async fn register_remote_agent(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<RegisterRemoteAgentRequest>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Register, &Resource::all(ResourceType::RemoteAgent)).await {
        return resp;
    }

    let agent_id = format!("ra_{}", atta_types::id::new_id());
    let token = format!("aat_{}", generate_token());
    let token_hash = hash_token(&token);

    let token_expires_at = req.token_ttl_hours.map(|hours| {
        chrono::Utc::now() + chrono::Duration::hours(hours as i64)
    });

    let agent = RemoteAgent {
        id: agent_id.clone(),
        name: req.name,
        description: req.description,
        version: "0.1.0".to_string(),
        capabilities: req.capabilities,
        status: RemoteAgentStatus::Offline,
        last_heartbeat: None,
        registered_at: chrono::Utc::now(),
        registered_by: user.actor.id.clone(),
        token_expires_at,
    };

    match state.store.register_remote_agent(&agent, &token_hash).await {
        Ok(()) => {
            info!(agent_id = %agent_id, "remote agent registered");
            // Audit log
            let entry = AuditEntry {
                id: uuid::Uuid::new_v4(),
                timestamp: chrono::Utc::now(),
                actor: user.actor.clone(),
                action: "remote_agent.register".to_string(),
                resource: EntityRef::new(atta_types::ResourceType::RemoteAgent, &agent_id),
                correlation_id: uuid::Uuid::new_v4(),
                outcome: AuditOutcome::Success,
                detail: serde_json::json!({"agent_id": &agent_id}),
            };
            if let Err(e) = state.audit.record(&entry).await {
                warn!(error = %e, "failed to record register audit entry");
            }
            (
                StatusCode::CREATED,
                Json(RegisterRemoteAgentResponse {
                    agent_id,
                    token,
                    registered_at: agent.registered_at,
                    token_expires_at,
                }),
            )
                .into_response()
        }
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

/// GET /api/v1/remote/agents — 列出远程 Agents
pub async fn list_remote_agents(
    State(state): State<AppState>,
    _user: CurrentUser,
) -> impl IntoResponse {
    match state.store.list_remote_agents().await {
        Ok(agents) => (StatusCode::OK, Json(agents)).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

/// GET /api/v1/remote/agents/{id}
pub async fn get_remote_agent(
    State(state): State<AppState>,
    _user: CurrentUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.store.get_remote_agent(&id).await {
        Ok(Some(mut agent)) => {
            // Enrich with real-time online status from hub
            if state.remote_agent_hub.is_online(&id).await {
                agent.status = RemoteAgentStatus::Online;
            }
            (StatusCode::OK, Json(agent)).into_response()
        }
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            &AttaError::NotFound {
                entity_type: "remote_agent".to_string(),
                id,
            },
        ),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

/// DELETE /api/v1/remote/agents/{id}
pub async fn delete_remote_agent(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Delete, &Resource::new(ResourceType::RemoteAgent, &id)).await {
        return resp;
    }

    // Disconnect if online
    if state.remote_agent_hub.is_online(&id).await {
        state.remote_agent_hub.remove_connection(&id).await;
    }

    match state.store.delete_remote_agent(&id).await {
        Ok(()) => {
            // Audit log
            let entry = AuditEntry {
                id: uuid::Uuid::new_v4(),
                timestamp: chrono::Utc::now(),
                actor: user.actor,
                action: "remote_agent.delete".to_string(),
                resource: EntityRef::new(atta_types::ResourceType::RemoteAgent, &id),
                correlation_id: uuid::Uuid::new_v4(),
                outcome: AuditOutcome::Success,
                detail: serde_json::json!({"agent_id": &id}),
            };
            if let Err(e) = state.audit.record(&entry).await {
                warn!(error = %e, "failed to record delete audit entry");
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

/// POST /api/v1/remote/agents/{id}/estop — 紧急停止
pub async fn estop_remote_agent(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Manage, &Resource::new(ResourceType::RemoteAgent, &id)).await {
        return resp;
    }

    // Verify agent exists
    match state.store.get_remote_agent(&id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                &AttaError::NotFound {
                    entity_type: "remote_agent".to_string(),
                    id: id.clone(),
                },
            );
        }
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }

    // Check if online
    if !state.remote_agent_hub.is_online(&id).await {
        return error_response(
            StatusCode::CONFLICT,
            &AttaError::Validation("agent is not online".to_string()),
        );
    }

    let msg = DownstreamMsg::Estop {
        msg_id: uuid::Uuid::new_v4().to_string(),
        reason: "admin triggered".to_string(),
        scope: "all".to_string(),
    };

    state.remote_agent_hub.send_to(&id, &msg).await;

    // Audit log
    let entry = AuditEntry {
        id: uuid::Uuid::new_v4(),
        timestamp: chrono::Utc::now(),
        actor: user.actor,
        action: "remote_agent.estop".to_string(),
        resource: EntityRef::new(atta_types::ResourceType::RemoteAgent, &id),
        correlation_id: uuid::Uuid::new_v4(),
        outcome: AuditOutcome::Success,
        detail: serde_json::json!({"agent_id": id}),
    };
    if let Err(e) = state.audit.record(&entry).await {
        warn!(error = %e, "failed to record estop audit entry");
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({"status": "estop_sent"})),
    )
        .into_response()
}

/// Token 轮转请求
#[derive(Debug, Deserialize)]
pub struct RotateTokenRequest {
    /// Token TTL in hours. None = never expires.
    pub token_ttl_hours: Option<u32>,
}

/// POST /api/v1/remote/agents/{id}/rotate-token — Rotate agent token
pub async fn rotate_remote_agent_token(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<RotateTokenRequest>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Manage, &Resource::new(ResourceType::RemoteAgent, &id)).await {
        return resp;
    }

    // Verify agent exists
    match state.store.get_remote_agent(&id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                &AttaError::NotFound {
                    entity_type: "remote_agent".to_string(),
                    id: id.clone(),
                },
            );
        }
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }

    let new_token = format!("aat_{}", generate_token());
    let new_hash = hash_token(&new_token);
    let expires_at = req.token_ttl_hours.map(|hours| {
        chrono::Utc::now() + chrono::Duration::hours(hours as i64)
    });

    match state
        .store
        .rotate_remote_agent_token(&id, &new_hash, expires_at)
        .await
    {
        Ok(()) => {
            // Disconnect the agent so it must reconnect with new token
            if state.remote_agent_hub.is_online(&id).await {
                state.remote_agent_hub.remove_connection(&id).await;
            }

            info!(agent_id = %id, "remote agent token rotated");
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "token": new_token,
                    "expires_at": expires_at,
                })),
            )
                .into_response()
        }
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

// ── 工具函数 ──

fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let hash = hasher.finalize();
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

fn generate_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
