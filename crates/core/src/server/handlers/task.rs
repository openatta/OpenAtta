//! Task API handlers

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use atta_types::{Action, AttaError, EntityRef, EventEnvelope, Resource, ResourceType, TaskFilter, TaskStatus};

use crate::middleware::CurrentUser;
use crate::server::response::{check_authz, error_response};
use crate::server::AppState;

#[derive(Debug, Deserialize)]
pub struct CreateTaskRequest {
    pub flow_id: String,
    pub input: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct ListTasksQuery {
    pub status: Option<String>,
    pub flow_id: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

pub async fn list_tasks(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(query): Query<ListTasksQuery>,
) -> impl IntoResponse {
    let status = query.status.as_deref().and_then(parse_task_status);

    // Data isolation: Admin/Owner see all, others see only their own
    let created_by = if user.roles.iter().any(|r| {
        matches!(
            r,
            atta_types::Role::Owner | atta_types::Role::Admin
        )
    }) {
        None
    } else {
        Some(user.actor.id.clone())
    };

    let filter = TaskFilter {
        status,
        flow_id: query.flow_id,
        created_by,
        limit: query.limit.unwrap_or(20),
        offset: query.offset.unwrap_or(0),
    };

    match state.store.list_tasks(&filter).await {
        Ok(tasks) => (StatusCode::OK, Json(tasks)).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

pub async fn create_task(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<CreateTaskRequest>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Create, &Resource::all(ResourceType::Task)).await {
        return resp;
    }

    // Validate input size (max 1MB)
    let input_size = req.input.to_string().len();
    if input_size > 1_048_576 {
        return error_response(
            StatusCode::BAD_REQUEST,
            &AttaError::Validation(format!(
                "task input exceeds maximum size ({}B > 1MB)",
                input_size
            )),
        );
    }

    let actor = user.actor;

    match state
        .flow_engine
        .create_task(&req.flow_id, req.input, actor)
        .await
    {
        Ok(task) => (StatusCode::CREATED, Json(task)).into_response(),
        Err(e) => match &e {
            AttaError::FlowNotFound(_) => error_response(StatusCode::NOT_FOUND, &e),
            AttaError::Validation(_) => error_response(StatusCode::BAD_REQUEST, &e),
            _ => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
        },
    }
}

pub async fn get_task(State(state): State<AppState>, user: CurrentUser, Path(id): Path<String>) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Read, &Resource::new(ResourceType::Task, &id)).await {
        return resp;
    }

    let task_id = match Uuid::parse_str(&id) {
        Ok(uuid) => uuid,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &AttaError::Validation(format!("invalid task id: {}", id)),
            );
        }
    };

    match state.store.get_task(&task_id).await {
        Ok(Some(task)) => (StatusCode::OK, Json(task)).into_response(),
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            &AttaError::NotFound {
                entity_type: "task".to_string(),
                id: id.to_string(),
            },
        ),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

pub async fn delete_task(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Delete, &Resource::new(ResourceType::Task, &id)).await {
        return resp;
    }

    let task_id = match Uuid::parse_str(&id) {
        Ok(uuid) => uuid,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &AttaError::Validation(format!("invalid task id: {}", id)),
            );
        }
    };

    match state.store.get_task(&task_id).await {
        Ok(Some(task)) => match &task.status {
            TaskStatus::Completed | TaskStatus::Failed { .. } | TaskStatus::Cancelled => {}
            _ => {
                return error_response(
                    StatusCode::CONFLICT,
                    &AttaError::Validation(
                        "can only delete completed, failed, or cancelled tasks".to_string(),
                    ),
                );
            }
        },
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                &AttaError::NotFound {
                    entity_type: "task".to_string(),
                    id: id.to_string(),
                },
            );
        }
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }

    match state
        .store
        .update_task_status(&task_id, TaskStatus::Cancelled)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

pub async fn cancel_task(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Update, &Resource::new(ResourceType::Task, &id)).await {
        return resp;
    }

    let task_id = match Uuid::parse_str(&id) {
        Ok(uuid) => uuid,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &AttaError::Validation(format!("invalid task id: {}", id)),
            );
        }
    };

    match state
        .store
        .update_task_status(&task_id, TaskStatus::Cancelled)
        .await
    {
        Ok(()) => {
            match EventEnvelope::new(
                "atta.task.cancelled",
                EntityRef::task(&task_id),
                user.actor,
                task_id,
                serde_json::json!({"task_id": task_id}),
            ) {
                Ok(envelope) => {
                    if let Err(e) = state
                        .bus
                        .publish("atta.task.cancelled", envelope)
                        .await
                    {
                        tracing::warn!(task_id = %task_id, error = %e, "failed to publish task.cancelled event");
                    }
                }
                Err(e) => {
                    tracing::warn!(task_id = %task_id, error = %e, "failed to create task.cancelled event");
                }
            }

            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "cancelled"})),
            )
                .into_response()
        }
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

fn parse_task_status(s: &str) -> Option<TaskStatus> {
    match s {
        "running" => Some(TaskStatus::Running),
        "waiting_approval" => Some(TaskStatus::WaitingApproval),
        "completed" => Some(TaskStatus::Completed),
        "failed" => Some(TaskStatus::Failed {
            error: String::new(),
        }),
        "cancelled" => Some(TaskStatus::Cancelled),
        _ => None,
    }
}
