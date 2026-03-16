//! Audit API handlers

use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};

use atta_audit::AuditFilter;

use crate::middleware::CurrentUser;
use crate::server::response::{error_response, ApiResponse};
use crate::server::AppState;

/// Query audit entries with optional filters
pub async fn query_audit(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(filter): Query<AuditFilter>,
) -> impl IntoResponse {
    match state.audit.query(&filter).await {
        Ok(entries) => (StatusCode::OK, Json(ApiResponse { data: entries })).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

/// Export audit entries (JSON or CSV)
pub async fn export_audit(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(params): Query<ExportParams>,
) -> impl IntoResponse {
    let filter = AuditFilter {
        actor_id: params.actor_id,
        action: params.action,
        resource_type: params.resource_type,
        from: params.from,
        to: params.to,
        limit: params.limit.unwrap_or(10000),
        ..Default::default()
    };

    let entries = match state.audit.query(&filter).await {
        Ok(e) => e,
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    };

    let format = params.format.as_deref().unwrap_or("json");

    if format == "csv" {
        let mut csv = String::from("id,timestamp,actor_type,actor_id,action,resource_type,resource_id,correlation_id,outcome\n");
        for entry in &entries {
            let outcome = match &entry.outcome {
                atta_audit::AuditOutcome::Success => "success",
                atta_audit::AuditOutcome::Denied => "denied",
                atta_audit::AuditOutcome::Failed { .. } => "failed",
            };
            csv.push_str(&format!(
                "{},{},{},{},{},{},{},{},{}\n",
                entry.id,
                entry.timestamp.to_rfc3339(),
                entry.actor.actor_type.as_str(),
                entry.actor.id,
                entry.action,
                entry.resource.entity_type.as_str(),
                entry.resource.id,
                entry.correlation_id,
                outcome,
            ));
        }
        (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "text/csv"),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"audit.csv\"",
                ),
            ],
            csv,
        )
            .into_response()
    } else {
        (StatusCode::OK, Json(ApiResponse { data: entries })).into_response()
    }
}

#[derive(serde::Deserialize)]
pub struct ExportParams {
    pub format: Option<String>,
    pub actor_id: Option<String>,
    pub action: Option<String>,
    pub resource_type: Option<String>,
    pub from: Option<chrono::DateTime<chrono::Utc>>,
    pub to: Option<chrono::DateTime<chrono::Utc>>,
    pub limit: Option<usize>,
}
