//! Cron API handlers

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;

use atta_types::{Action, AttaError, Resource, ResourceType};

use crate::middleware::CurrentUser;
use crate::server::response::{check_authz, error_response, ApiResponse};
use crate::server::AppState;

#[allow(clippy::result_large_err)]
fn get_engine(state: &AppState) -> Result<&std::sync::Arc<crate::cron_engine::CronEngine>, axum::response::Response> {
    state.cron_engine.as_ref().ok_or_else(|| {
        crate::server::response::not_implemented("Cron engine")
    })
}

#[derive(Debug, Deserialize)]
pub struct ListJobsQuery {
    pub enabled: Option<bool>,
}

/// GET /api/v1/cron/jobs
pub async fn list_jobs(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(query): Query<ListJobsQuery>,
) -> axum::response::Response {
    if let Err(resp) = check_authz(&state, &user, Action::Read, &Resource::all(ResourceType::Task)).await {
        return resp;
    }

    let engine = match get_engine(&state) {
        Ok(e) => e,
        Err(r) => return r,
    };

    let status = match query.enabled {
        Some(true) => Some("active"),
        Some(false) => Some("disabled"),
        None => None,
    };

    match engine.list(status).await {
        Ok(jobs) => (StatusCode::OK, Json(ApiResponse { data: jobs })).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateJobRequest {
    pub name: String,
    pub schedule: String,
    pub command: String,
    #[serde(default)]
    pub config: serde_json::Value,
}

/// POST /api/v1/cron/jobs
pub async fn create_job(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<CreateJobRequest>,
) -> axum::response::Response {
    let engine = match get_engine(&state) {
        Ok(e) => e,
        Err(r) => return r,
    };

    if let Err(resp) = check_authz(&state, &user, Action::Create, &Resource::all(ResourceType::Task)).await {
        return resp;
    }

    let job = atta_types::CronJob {
        id: uuid::Uuid::new_v4().to_string(),
        name: req.name,
        schedule: req.schedule,
        command: req.command,
        config: req.config,
        enabled: true,
        created_by: user.actor.id,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        last_run_at: None,
        next_run_at: None,
    };

    match engine.schedule(job).await {
        Ok(job) => (StatusCode::CREATED, Json(ApiResponse { data: job })).into_response(),
        Err(e) => match &e {
            AttaError::Validation(_) => error_response(StatusCode::BAD_REQUEST, &e),
            _ => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
        },
    }
}

/// GET /api/v1/cron/jobs/{id}
pub async fn get_job(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> axum::response::Response {
    if let Err(resp) = check_authz(&state, &user, Action::Read, &Resource::new(ResourceType::Task, &id)).await {
        return resp;
    }

    let engine = match get_engine(&state) {
        Ok(e) => e,
        Err(r) => return r,
    };

    match engine.get(&id).await {
        Ok(Some(job)) => (StatusCode::OK, Json(ApiResponse { data: job })).into_response(),
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            &AttaError::NotFound {
                entity_type: "cron_job".to_string(),
                id,
            },
        ),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateJobRequest {
    pub schedule: Option<String>,
    pub enabled: Option<bool>,
}

/// PUT /api/v1/cron/jobs/{id}
pub async fn update_job(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<UpdateJobRequest>,
) -> axum::response::Response {
    let engine = match get_engine(&state) {
        Ok(e) => e,
        Err(r) => return r,
    };

    if let Err(resp) = check_authz(&state, &user, Action::Update, &Resource::new(ResourceType::Task, &id)).await {
        return resp;
    }

    match engine.update(&id, req.schedule.as_deref(), req.enabled).await {
        Ok(job) => (StatusCode::OK, Json(ApiResponse { data: job })).into_response(),
        Err(e) => match &e {
            AttaError::Validation(_) => error_response(StatusCode::BAD_REQUEST, &e),
            _ => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
        },
    }
}

/// DELETE /api/v1/cron/jobs/{id}
pub async fn delete_job(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> axum::response::Response {
    let engine = match get_engine(&state) {
        Ok(e) => e,
        Err(r) => return r,
    };

    if let Err(resp) = check_authz(&state, &user, Action::Delete, &Resource::new(ResourceType::Task, &id)).await {
        return resp;
    }

    match engine.cancel(&id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

/// POST /api/v1/cron/jobs/{id}/trigger
pub async fn trigger_job(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> axum::response::Response {
    let engine = match get_engine(&state) {
        Ok(e) => e,
        Err(r) => return r,
    };

    if let Err(resp) = check_authz(&state, &user, Action::Execute, &Resource::new(ResourceType::Task, &id)).await {
        return resp;
    }

    match engine.trigger_job(&id, "manual").await {
        Ok(run) => (StatusCode::OK, Json(ApiResponse { data: run })).into_response(),
        Err(e) => match &e {
            AttaError::Validation(_) => error_response(StatusCode::NOT_FOUND, &e),
            _ => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
        },
    }
}

#[derive(Debug, Deserialize)]
pub struct RunsQuery {
    pub limit: Option<usize>,
}

/// GET /api/v1/cron/jobs/{id}/runs
pub async fn list_runs(
    State(state): State<AppState>,
    _user: CurrentUser,
    Path(id): Path<String>,
    Query(query): Query<RunsQuery>,
) -> axum::response::Response {
    let engine = match get_engine(&state) {
        Ok(e) => e,
        Err(r) => return r,
    };

    match engine.history(&id, query.limit.unwrap_or(20)).await {
        Ok(runs) => (StatusCode::OK, Json(ApiResponse { data: runs })).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

/// GET /api/v1/cron/status
pub async fn cron_status(
    State(state): State<AppState>,
    user: CurrentUser,
) -> axum::response::Response {
    if let Err(resp) = check_authz(&state, &user, Action::Read, &Resource::all(ResourceType::Task)).await {
        return resp;
    }

    let running = state.cron_engine.is_some();
    (
        StatusCode::OK,
        Json(ApiResponse {
            data: serde_json::json!({
                "running": running,
            }),
        }),
    )
        .into_response()
}
