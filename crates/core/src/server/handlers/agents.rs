//! Agent API handlers

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};

use atta_types::{Action, Resource, ResourceType};

use crate::middleware::CurrentUser;
use crate::server::response::check_authz;
use crate::server::AppState;

/// GET /api/v1/agents — List all running agents
pub async fn list_agents(State(state): State<AppState>, user: CurrentUser) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Read, &Resource::all(ResourceType::Task)).await {
        return resp;
    }

    match &state.agent_registry {
        Some(registry) => {
            let agents = registry.list().await;
            (StatusCode::OK, Json(agents)).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "agent registry not initialized"})),
        )
            .into_response(),
    }
}

/// GET /api/v1/agents/{id} — Get agent by ID
pub async fn get_agent(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Read, &Resource::new(ResourceType::Task, &id)).await {
        return resp;
    }

    match &state.agent_registry {
        Some(registry) => match registry.get(&id).await {
            Some(agent) => (StatusCode::OK, Json(agent)).into_response(),
            None => (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "agent not found"})),
            )
                .into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "agent registry not initialized"})),
        )
            .into_response(),
    }
}

/// POST /api/v1/agents/{id}/terminate — Terminate an agent
pub async fn terminate_agent(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Manage, &Resource::new(ResourceType::Task, &id)).await {
        return resp;
    }

    match &state.agent_registry {
        Some(registry) => match registry.terminate(&id).await {
            Ok(()) => StatusCode::NO_CONTENT.into_response(),
            Err(e) => (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": e})),
            )
                .into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "agent registry not initialized"})),
        )
            .into_response(),
    }
}

/// POST /api/v1/agents/{id}/pause — Pause an agent
pub async fn pause_agent(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Manage, &Resource::new(ResourceType::Task, &id)).await {
        return resp;
    }

    match &state.agent_registry {
        Some(registry) => match registry.pause(&id).await {
            Ok(()) => StatusCode::NO_CONTENT.into_response(),
            Err(e) => (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": e})),
            )
                .into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "agent registry not initialized"})),
        )
            .into_response(),
    }
}

/// POST /api/v1/agents/{id}/resume — Resume a paused agent
pub async fn resume_agent(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Manage, &Resource::new(ResourceType::Task, &id)).await {
        return resp;
    }

    match &state.agent_registry {
        Some(registry) => match registry.resume(&id).await {
            Ok(()) => StatusCode::NO_CONTENT.into_response(),
            Err(e) => (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": e})),
            )
                .into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "agent registry not initialized"})),
        )
            .into_response(),
    }
}
