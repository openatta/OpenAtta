//! Channel API handlers

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use atta_types::{Action, AttaError, Resource, ResourceType};

use crate::middleware::CurrentUser;
use crate::server::response::{check_authz, error_response, ApiResponse};
use crate::server::AppState;

/// Channel info returned by the list endpoint
#[derive(Debug, serde::Serialize)]
pub struct ChannelInfo {
    pub name: String,
    pub healthy: bool,
}

/// List running channels from the registry
pub async fn list_channels(State(state): State<AppState>, user: CurrentUser) -> axum::response::Response {
    if let Err(resp) = check_authz(&state, &user, Action::Read, &Resource::all(ResourceType::Channel)).await {
        return resp;
    }

    let names = state.channel_registry.list().await;
    let mut channels = Vec::with_capacity(names.len());

    for name in names {
        let healthy = if let Some(ch) = state.channel_registry.get(&name).await {
            ch.health_check().await.is_ok()
        } else {
            false
        };
        channels.push(ChannelInfo { name, healthy });
    }

    Json(ApiResponse { data: channels }).into_response()
}

/// Health check for a specific channel
pub async fn channel_health(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(name): Path<String>,
) -> axum::response::Response {
    if let Err(resp) = check_authz(&state, &user, Action::Read, &Resource::new(ResourceType::Channel, &name)).await {
        return resp;
    }

    let Some(channel) = state.channel_registry.get(&name).await else {
        return error_response(
            StatusCode::NOT_FOUND,
            &AttaError::NotFound {
                entity_type: "channel".to_string(),
                id: name,
            },
        );
    };

    match channel.health_check().await {
        Ok(()) => (
            StatusCode::OK,
            Json(ApiResponse {
                data: serde_json::json!({ "name": name, "status": "healthy" }),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiResponse {
                data: serde_json::json!({ "name": name, "status": "unhealthy", "error": e.to_string() }),
            }),
        )
            .into_response(),
    }
}

/// Receive an incoming webhook payload and push it into the channel.
///
/// Verifies the webhook signature via the channel's `verify_webhook_signature`
/// method before processing. Returns 401 Unauthorized if verification fails.
pub async fn receive_webhook(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let Some(channel) = state.channel_registry.get(&name).await else {
        return error_response(
            StatusCode::NOT_FOUND,
            &AttaError::NotFound {
                entity_type: "channel".to_string(),
                id: name,
            },
        );
    };

    // --- Webhook signature verification ---
    let header_map: HashMap<String, String> = headers
        .iter()
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|val| (k.as_str().to_lowercase(), val.to_string()))
        })
        .collect();

    match channel.verify_webhook_signature(&header_map, &body) {
        Ok(true) => {} // Valid
        Ok(false) => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                &AttaError::Validation("invalid webhook signature".to_string()),
            );
        }
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &AttaError::Other(anyhow::anyhow!("signature verification error: {e}")),
            );
        }
    }

    // Parse the body as a ChannelMessage
    let msg: atta_channel::ChannelMessage = match serde_json::from_slice(&body) {
        Ok(m) => m,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &AttaError::Validation(format!("invalid webhook payload: {e}")),
            );
        }
    };

    // Only webhook channels accept HTTP pushes
    if channel.name() != "webhook" && !channel.name().starts_with("webhook") {
        return error_response(
            StatusCode::BAD_REQUEST,
            &AttaError::Validation(format!(
                "channel '{}' does not accept webhook pushes",
                name
            )),
        );
    }

    let send_msg = atta_channel::SendMessage {
        recipient: msg.sender,
        content: msg.content,
        subject: None,
        thread_ts: msg.thread_ts,
        metadata: msg.metadata,
    };

    match channel.send(send_msg).await {
        Ok(()) => (StatusCode::ACCEPTED, Json(ApiResponse { data: "ok" })).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

/// Add a new channel at runtime
pub async fn add_channel(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(config): Json<atta_channel::ChannelConfig>,
) -> axum::response::Response {
    if let Err(resp) = check_authz(&state, &user, Action::Create, &Resource::all(ResourceType::Channel)).await {
        return resp;
    }

    let channel = match atta_channel::create_channel(&config) {
        Ok(ch) => ch,
        Err(e) => {
            return error_response(StatusCode::BAD_REQUEST, &e);
        }
    };

    let name = channel.name().to_string();

    // Check if a channel with the same name already exists
    if state.channel_registry.get(&name).await.is_some() {
        return error_response(
            StatusCode::CONFLICT,
            &AttaError::AlreadyExists {
                entity_type: "channel".to_string(),
                id: name,
            },
        );
    }

    state.channel_registry.insert(name.clone(), channel).await;

    (
        StatusCode::CREATED,
        Json(ApiResponse {
            data: serde_json::json!({ "name": name, "status": "created" }),
        }),
    )
        .into_response()
}

/// Remove a channel at runtime
pub async fn remove_channel(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(name): Path<String>,
) -> axum::response::Response {
    if let Err(resp) = check_authz(&state, &user, Action::Delete, &Resource::new(ResourceType::Channel, &name)).await {
        return resp;
    }

    match state.channel_registry.remove(&name).await {
        Some(_) => (
            StatusCode::OK,
            Json(ApiResponse {
                data: serde_json::json!({ "name": name, "status": "removed" }),
            }),
        )
            .into_response(),
        None => error_response(
            StatusCode::NOT_FOUND,
            &AttaError::NotFound {
                entity_type: "channel".to_string(),
                id: name,
            },
        ),
    }
}

/// Update a channel configuration (remove old + create new)
pub async fn update_channel(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(name): Path<String>,
    Json(config): Json<atta_channel::ChannelConfig>,
) -> axum::response::Response {
    if let Err(resp) = check_authz(&state, &user, Action::Update, &Resource::new(ResourceType::Channel, &name)).await {
        return resp;
    }

    // Verify the channel exists
    if state.channel_registry.get(&name).await.is_none() {
        return error_response(
            StatusCode::NOT_FOUND,
            &AttaError::NotFound {
                entity_type: "channel".to_string(),
                id: name,
            },
        );
    }

    // Create new channel from config
    let channel = match atta_channel::create_channel(&config) {
        Ok(ch) => ch,
        Err(e) => {
            return error_response(StatusCode::BAD_REQUEST, &e);
        }
    };

    // Remove old and insert new
    state.channel_registry.remove(&name).await;
    let new_name = channel.name().to_string();
    state
        .channel_registry
        .insert(new_name.clone(), channel)
        .await;

    (
        StatusCode::OK,
        Json(ApiResponse {
            data: serde_json::json!({ "name": new_name, "status": "updated" }),
        }),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Session management endpoints
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SessionListQuery {
    pub limit: Option<usize>,
}

/// List active sessions
pub async fn list_sessions(
    State(state): State<AppState>,
    _user: CurrentUser,
) -> axum::response::Response {
    let Some(ref router) = state.session_router else {
        return (
            StatusCode::OK,
            Json(ApiResponse { data: Vec::<String>::new() }),
        )
            .into_response();
    };

    let sessions = router.list_sessions().await;
    (StatusCode::OK, Json(ApiResponse { data: sessions })).into_response()
}

/// Get session configuration
pub async fn get_session(
    State(state): State<AppState>,
    _user: CurrentUser,
    Path(key): Path<String>,
) -> axum::response::Response {
    let Some(ref router) = state.session_router else {
        return error_response(
            StatusCode::NOT_FOUND,
            &AttaError::NotFound {
                entity_type: "session".to_string(),
                id: key,
            },
        );
    };

    match router.get_config(&key).await {
        Some(config) => (StatusCode::OK, Json(ApiResponse { data: config })).into_response(),
        None => error_response(
            StatusCode::NOT_FOUND,
            &AttaError::NotFound {
                entity_type: "session".to_string(),
                id: key,
            },
        ),
    }
}

/// Update session configuration
pub async fn update_session(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(key): Path<String>,
    Json(config): Json<atta_channel::SessionConfig>,
) -> axum::response::Response {
    if let Err(resp) = check_authz(&state, &user, Action::Update, &Resource::new(ResourceType::Channel, &key)).await {
        return resp;
    }

    let Some(ref router) = state.session_router else {
        return error_response(
            StatusCode::NOT_FOUND,
            &AttaError::Validation("session router not configured".to_string()),
        );
    };

    router.set_config(&key, config.clone()).await;
    (StatusCode::OK, Json(ApiResponse { data: config })).into_response()
}

/// Delete session configuration (revert to defaults)
pub async fn delete_session(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(key): Path<String>,
) -> axum::response::Response {
    if let Err(resp) = check_authz(&state, &user, Action::Delete, &Resource::new(ResourceType::Channel, &key)).await {
        return resp;
    }

    let Some(ref router) = state.session_router else {
        return StatusCode::NO_CONTENT.into_response();
    };

    router.remove_config(&key).await;
    StatusCode::NO_CONTENT.into_response()
}

// ---------------------------------------------------------------------------
// ACP (Agent Control Plane) takeover endpoints
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct TakeoverRequest {
    pub operator_id: String,
    pub reason: Option<String>,
}

/// Start human takeover for a session
pub async fn start_takeover(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(key): Path<String>,
    Json(req): Json<TakeoverRequest>,
) -> axum::response::Response {
    if let Err(resp) = check_authz(&state, &user, Action::Update, &Resource::new(ResourceType::Channel, &key)).await {
        return resp;
    }

    let Some(ref router) = state.session_router else {
        return error_response(
            StatusCode::BAD_REQUEST,
            &AttaError::Validation("session router not configured".to_string()),
        );
    };

    let config = router
        .start_takeover(&key, &req.operator_id, req.reason)
        .await;

    (StatusCode::OK, Json(ApiResponse { data: config })).into_response()
}

/// End human takeover for a session
pub async fn end_takeover(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(key): Path<String>,
) -> axum::response::Response {
    if let Err(resp) = check_authz(&state, &user, Action::Update, &Resource::new(ResourceType::Channel, &key)).await {
        return resp;
    }

    let Some(ref router) = state.session_router else {
        return error_response(
            StatusCode::BAD_REQUEST,
            &AttaError::Validation("session router not configured".to_string()),
        );
    };

    match router.end_takeover(&key).await {
        Some(config) => (StatusCode::OK, Json(ApiResponse { data: config })).into_response(),
        None => error_response(
            StatusCode::NOT_FOUND,
            &AttaError::NotFound {
                entity_type: "session".to_string(),
                id: key,
            },
        ),
    }
}

// ---------------------------------------------------------------------------
// Access control management endpoints
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct AccessControlUpdate {
    pub senders: Vec<String>,
}

/// Set the allowlist for a channel
pub async fn set_allowlist(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(name): Path<String>,
    Json(req): Json<AccessControlUpdate>,
) -> axum::response::Response {
    if let Err(resp) = check_authz(&state, &user, Action::Update, &Resource::new(ResourceType::Channel, &name)).await {
        return resp;
    }

    let Some(ref acl) = state.access_control else {
        return error_response(
            StatusCode::BAD_REQUEST,
            &AttaError::Validation("access control not configured".to_string()),
        );
    };

    acl.set_allowlist(&name, req.senders).await;
    (
        StatusCode::OK,
        Json(ApiResponse {
            data: serde_json::json!({ "channel": name, "status": "allowlist_updated" }),
        }),
    )
        .into_response()
}

/// Set the blocklist for a channel
pub async fn set_blocklist(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(name): Path<String>,
    Json(req): Json<AccessControlUpdate>,
) -> axum::response::Response {
    if let Err(resp) = check_authz(&state, &user, Action::Update, &Resource::new(ResourceType::Channel, &name)).await {
        return resp;
    }

    let Some(ref acl) = state.access_control else {
        return error_response(
            StatusCode::BAD_REQUEST,
            &AttaError::Validation("access control not configured".to_string()),
        );
    };

    acl.set_blocklist(&name, req.senders).await;
    (
        StatusCode::OK,
        Json(ApiResponse {
            data: serde_json::json!({ "channel": name, "status": "blocklist_updated" }),
        }),
    )
        .into_response()
}
