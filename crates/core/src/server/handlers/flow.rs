//! Flow API handlers

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};

use atta_types::{Action, AttaError, FlowDef, Resource, ResourceType};

use crate::flow_engine::FlowEngine;
use crate::middleware::CurrentUser;
use crate::server::response::{check_authz, error_response};
use crate::server::AppState;

pub async fn list_flows(State(state): State<AppState>, user: CurrentUser) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Read, &Resource::all(ResourceType::Flow)).await {
        return resp;
    }
    match state.store.list_flow_defs().await {
        Ok(flows) => (StatusCode::OK, Json(flows)).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

pub async fn create_flow(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(mut flow_def): Json<FlowDef>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Create, &Resource::all(ResourceType::Flow)).await {
        return resp;
    }
    if let Err(e) = FlowEngine::validate_flow_def(&flow_def) {
        return error_response(StatusCode::BAD_REQUEST, &e);
    }

    // API-created flows are always "imported"
    flow_def.source = "imported".to_string();

    if let Err(e) = state.store.save_flow_def(&flow_def).await {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e);
    }

    if let Err(e) = state.flow_engine.register_flow_def(flow_def.clone()) {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e);
    }

    (StatusCode::CREATED, Json(flow_def)).into_response()
}

pub async fn get_flow(State(state): State<AppState>, user: CurrentUser, Path(id): Path<String>) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Read, &Resource::new(ResourceType::Flow, &id)).await {
        return resp;
    }
    match state.flow_engine.get_flow_def(&id) {
        Ok(flow) => (StatusCode::OK, Json(flow)).into_response(),
        Err(e) => match &e {
            AttaError::FlowNotFound(_) => error_response(StatusCode::NOT_FOUND, &e),
            _ => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
        },
    }
}

pub async fn update_flow(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(mut flow_def): Json<FlowDef>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Update, &Resource::new(ResourceType::Flow, &id)).await {
        return resp;
    }
    if let Err(e) = FlowEngine::validate_flow_def(&flow_def) {
        return error_response(StatusCode::BAD_REQUEST, &e);
    }

    // Verify flow exists and preserve source
    let existing = match state.store.get_flow_def(&id).await {
        Ok(Some(f)) => f,
        Ok(None) => {
            return error_response(StatusCode::NOT_FOUND, &AttaError::FlowNotFound(id));
        }
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    };

    flow_def.source = existing.source;
    flow_def.id = id;

    if let Err(e) = state.store.save_flow_def(&flow_def).await {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e);
    }

    if let Err(e) = state.flow_engine.register_flow_def(flow_def.clone()) {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e);
    }

    (StatusCode::OK, Json(flow_def)).into_response()
}

pub async fn delete_flow(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Delete, &Resource::new(ResourceType::Flow, &id)).await {
        return resp;
    }
    // Only imported flows can be deleted
    match state.store.get_flow_def(&id).await {
        Ok(Some(flow)) => {
            if flow.source != "imported" {
                return error_response(
                    StatusCode::FORBIDDEN,
                    &AttaError::Validation("cannot delete builtin flow".to_string()),
                );
            }
        }
        Ok(None) => {
            return error_response(StatusCode::NOT_FOUND, &AttaError::FlowNotFound(id));
        }
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }

    if let Err(e) = state.store.delete_flow_def(&id).await {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e);
    }

    StatusCode::NO_CONTENT.into_response()
}
