//! Node management API handlers

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};

use atta_types::{Action, AttaError, EventEnvelope, NodeStatus, Resource, ResourceType};

use crate::middleware::CurrentUser;
use crate::server::response::{check_authz, error_response};
use crate::server::AppState;

pub async fn list_nodes(State(state): State<AppState>, _user: CurrentUser) -> impl IntoResponse {
    match state.store.list_nodes().await {
        Ok(nodes) => (StatusCode::OK, Json(nodes)).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

pub async fn get_node(State(state): State<AppState>, _user: CurrentUser, Path(id): Path<String>) -> impl IntoResponse {
    match state.store.get_node(&id).await {
        Ok(Some(node)) => (StatusCode::OK, Json(node)).into_response(),
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            &AttaError::NotFound {
                entity_type: "node".to_string(),
                id,
            },
        ),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

/// Drain a node — set status to Draining and publish event
pub async fn drain_node(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Manage, &Resource::new(ResourceType::Node, &id)).await {
        return resp;
    }

    // Verify node exists
    match state.store.get_node(&id).await {
        Ok(Some(node)) => {
            if node.status == NodeStatus::Draining {
                return (
                    StatusCode::OK,
                    Json(serde_json::json!({"status": "already_draining"})),
                )
                    .into_response();
            }
        }
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                &AttaError::NotFound {
                    entity_type: "node".to_string(),
                    id,
                },
            );
        }
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }

    if let Err(e) = state
        .store
        .update_node_status(&id, NodeStatus::Draining)
        .await
    {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e);
    }

    // Publish drain event
    if let Ok(event) = EventEnvelope::new(
        "atta.node.drain",
        atta_types::EntityRef::node(&id),
        user.actor,
        uuid::Uuid::new_v4(),
        serde_json::json!({"node_id": id}),
    ) {
        let _ = state.bus.publish("atta.node.drain", event).await;
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({"status": "draining"})),
    )
        .into_response()
}

/// Resume a node — set status to Online and publish event
pub async fn resume_node(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Manage, &Resource::new(ResourceType::Node, &id)).await {
        return resp;
    }

    // Verify node exists
    match state.store.get_node(&id).await {
        Ok(Some(node)) => {
            if node.status == NodeStatus::Online {
                return (
                    StatusCode::OK,
                    Json(serde_json::json!({"status": "already_online"})),
                )
                    .into_response();
            }
        }
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                &AttaError::NotFound {
                    entity_type: "node".to_string(),
                    id,
                },
            );
        }
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }

    if let Err(e) = state
        .store
        .update_node_status(&id, NodeStatus::Online)
        .await
    {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e);
    }

    // Publish resume event
    if let Ok(event) = EventEnvelope::new(
        "atta.node.resume",
        atta_types::EntityRef::node(&id),
        user.actor,
        uuid::Uuid::new_v4(),
        serde_json::json!({"node_id": id}),
    ) {
        let _ = state.bus.publish("atta.node.resume", event).await;
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({"status": "online"})),
    )
        .into_response()
}
