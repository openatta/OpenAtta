//! Approval API handlers

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use atta_types::{Action, ApprovalFilter, ApprovalStatus, AttaError, Resource, ResourceType};

use crate::middleware::CurrentUser;
use crate::server::response::{check_authz, error_response};
use crate::server::AppState;

#[derive(Debug, Deserialize)]
pub struct ListApprovalsQuery {
    pub status: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

pub async fn list_approvals(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(query): Query<ListApprovalsQuery>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Read, &Resource::all(ResourceType::Approval)).await {
        return resp;
    }

    let filter = ApprovalFilter {
        status: query.status.as_deref().and_then(parse_approval_status),
        approver_role: None,
        task_id: None,
        limit: query.limit.unwrap_or(20),
        offset: query.offset.unwrap_or(0),
    };

    match state.store.list_approvals(&filter).await {
        Ok(approvals) => (StatusCode::OK, Json(approvals)).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

pub async fn get_approval(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Read, &Resource::new(ResourceType::Approval, &id)).await {
        return resp;
    }

    let approval_id = match Uuid::parse_str(&id) {
        Ok(uuid) => uuid,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &AttaError::Validation(format!("invalid approval id: {}", id)),
            );
        }
    };

    match state.store.get_approval(&approval_id).await {
        Ok(Some(approval)) => (StatusCode::OK, Json(approval)).into_response(),
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            &AttaError::NotFound {
                entity_type: "approval".to_string(),
                id,
            },
        ),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

#[derive(Debug, Deserialize)]
pub struct ApprovalAction {
    pub comment: Option<String>,
    pub reason: Option<String>,
    pub feedback: Option<String>,
}

pub async fn approve(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(action): Json<ApprovalAction>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Approve, &Resource::new(ResourceType::Approval, &id)).await {
        return resp;
    }

    let approval_id = match Uuid::parse_str(&id) {
        Ok(uuid) => uuid,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &AttaError::Validation(format!("invalid approval id: {}", id)),
            );
        }
    };

    let actor = user.actor;
    let comment = resolve_comment(&action);

    match state
        .store
        .update_approval_status(&approval_id, ApprovalStatus::Approved, &actor, comment.as_deref())
        .await
    {
        Ok(()) => {
            // Trigger flow advance after approval
            if let Ok(Some(approval)) = state.store.get_approval(&approval_id).await {
                let task_id = approval.task_id;
                if let Err(e) = state.flow_engine.advance_by_id(&task_id).await {
                    tracing::warn!(task_id = %task_id, error = %e, "failed to advance after approval");
                }
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "approved"})),
            )
                .into_response()
        }
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

pub async fn deny(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(action): Json<ApprovalAction>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Approve, &Resource::new(ResourceType::Approval, &id)).await {
        return resp;
    }

    let approval_id = match Uuid::parse_str(&id) {
        Ok(uuid) => uuid,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &AttaError::Validation(format!("invalid approval id: {}", id)),
            );
        }
    };

    let actor = user.actor;
    let comment = resolve_comment(&action);

    match state
        .store
        .update_approval_status(&approval_id, ApprovalStatus::Denied, &actor, comment.as_deref())
        .await
    {
        Ok(()) => {
            // Trigger flow advance after denial
            if let Ok(Some(approval)) = state.store.get_approval(&approval_id).await {
                let task_id = approval.task_id;
                if let Err(e) = state.flow_engine.advance_by_id(&task_id).await {
                    tracing::warn!(task_id = %task_id, error = %e, "failed to advance after denial");
                }
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "denied"})),
            )
                .into_response()
        }
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

pub async fn request_changes(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(action): Json<ApprovalAction>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Approve, &Resource::new(ResourceType::Approval, &id)).await {
        return resp;
    }

    let approval_id = match Uuid::parse_str(&id) {
        Ok(uuid) => uuid,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &AttaError::Validation(format!("invalid approval id: {}", id)),
            );
        }
    };

    let actor = user.actor;
    let comment = resolve_comment(&action);

    match state
        .store
        .update_approval_status(&approval_id, ApprovalStatus::RequestChanges, &actor, comment.as_deref())
        .await
    {
        Ok(()) => {
            // Trigger flow advance after request changes
            if let Ok(Some(approval)) = state.store.get_approval(&approval_id).await {
                let task_id = approval.task_id;
                if let Err(e) = state.flow_engine.advance_by_id(&task_id).await {
                    tracing::warn!(task_id = %task_id, error = %e, "failed to advance after request_changes");
                }
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "changes_requested"})),
            )
                .into_response()
        }
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

/// Extract comment from the approval action, preferring `comment` over `reason` over `feedback`.
fn resolve_comment(action: &ApprovalAction) -> Option<String> {
    action
        .comment
        .clone()
        .or_else(|| action.reason.clone())
        .or_else(|| action.feedback.clone())
}

fn parse_approval_status(s: &str) -> Option<ApprovalStatus> {
    match s {
        "pending" => Some(ApprovalStatus::Pending),
        "approved" => Some(ApprovalStatus::Approved),
        "denied" => Some(ApprovalStatus::Denied),
        "request_changes" => Some(ApprovalStatus::RequestChanges),
        "expired" => Some(ApprovalStatus::Expired),
        _ => None,
    }
}
