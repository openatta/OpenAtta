//! Skill API handlers

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;

use atta_types::{Action, AttaError, Resource, ResourceType};

use crate::middleware::CurrentUser;
use crate::server::response::{check_authz, error_response};
use crate::server::AppState;

#[derive(Debug, Deserialize)]
pub struct CreateSkillRequest {
    pub skill: serde_json::Value,
}

pub async fn list_skills(State(state): State<AppState>, user: CurrentUser) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Read, &Resource::all(ResourceType::Skill)).await {
        return resp;
    }
    match state.store.list_skills().await {
        Ok(skills) => (StatusCode::OK, Json(skills)).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

pub async fn create_skill(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<CreateSkillRequest>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Create, &Resource::all(ResourceType::Skill)).await {
        return resp;
    }
    let mut skill_def: atta_types::SkillDef = match serde_json::from_value(req.skill) {
        Ok(s) => s,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &AttaError::Validation(format!("invalid skill definition: {}", e)),
            );
        }
    };

    // API-created skills are always "imported"
    skill_def.source = "imported".to_string();

    match state.store.register_skill(&skill_def).await {
        Ok(()) => (StatusCode::CREATED, Json(skill_def)).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

pub async fn get_skill(State(state): State<AppState>, user: CurrentUser, Path(id): Path<String>) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Read, &Resource::new(ResourceType::Skill, &id)).await {
        return resp;
    }
    match state.store.get_skill(&id).await {
        Ok(Some(skill)) => (StatusCode::OK, Json(skill)).into_response(),
        Ok(None) => error_response(StatusCode::NOT_FOUND, &AttaError::SkillNotFound(id)),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

pub async fn update_skill(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<CreateSkillRequest>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Update, &Resource::new(ResourceType::Skill, &id)).await {
        return resp;
    }
    // Verify skill exists
    let existing = match state.store.get_skill(&id).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return error_response(StatusCode::NOT_FOUND, &AttaError::SkillNotFound(id));
        }
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    };

    let mut skill_def: atta_types::SkillDef = match serde_json::from_value(req.skill) {
        Ok(s) => s,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &AttaError::Validation(format!("invalid skill definition: {}", e)),
            );
        }
    };

    // Preserve source from existing
    skill_def.source = existing.source;
    skill_def.id = id;

    match state.store.register_skill(&skill_def).await {
        Ok(()) => (StatusCode::OK, Json(skill_def)).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

pub async fn delete_skill(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Delete, &Resource::new(ResourceType::Skill, &id)).await {
        return resp;
    }
    // Only imported skills can be deleted
    match state.store.get_skill(&id).await {
        Ok(Some(skill)) => {
            if skill.source != "imported" {
                return error_response(
                    StatusCode::FORBIDDEN,
                    &AttaError::Validation("cannot delete builtin skill".to_string()),
                );
            }
        }
        Ok(None) => {
            return error_response(StatusCode::NOT_FOUND, &AttaError::SkillNotFound(id));
        }
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }

    match state.store.delete_skill(&id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}
