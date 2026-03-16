//! Shared API response types

use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;

use atta_types::{Action, AttaError, AuthzDecision, Resource};

use crate::middleware::CurrentUser;
use crate::server::AppState;

/// Unified API success response wrapper
#[derive(Debug, Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub data: T,
}

/// List response with pagination metadata
#[derive(Debug, Serialize)]
pub struct ListResponse<T: Serialize> {
    pub items: Vec<T>,
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
}

/// Error response body
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: ErrorDetail,
}

/// Structured error detail
#[derive(Debug, Serialize)]
pub struct ErrorDetail {
    pub code: String,
    pub message: String,
}

/// Build a JSON error response from an AttaError
pub fn error_response(status: StatusCode, error: &AttaError) -> axum::response::Response {
    let code = match error {
        AttaError::NotFound { .. }
        | AttaError::FlowNotFound(_)
        | AttaError::SkillNotFound(_)
        | AttaError::ToolNotFound(_) => "not_found",
        AttaError::Validation(_) => "validation_error",
        _ => "internal_error",
    };

    (
        status,
        Json(ErrorResponse {
            error: ErrorDetail {
                code: code.to_string(),
                message: error.to_string(),
            },
        }),
    )
        .into_response()
}

/// Check authorization and return 403 Forbidden if denied.
pub async fn check_authz(
    state: &AppState,
    user: &CurrentUser,
    action: Action,
    resource: &Resource,
) -> Result<(), axum::response::Response> {
    match state.authz.check(&user.actor, action, resource).await {
        Ok(AuthzDecision::Allow) => Ok(()),
        Ok(AuthzDecision::Deny { reason }) => Err(error_response(
            StatusCode::FORBIDDEN,
            &AttaError::PermissionDenied { permission: reason },
        )),
        Ok(AuthzDecision::RequireApproval { approver_role }) => Err(error_response(
            StatusCode::FORBIDDEN,
            &AttaError::PermissionDenied {
                permission: format!("requires approval from {approver_role}"),
            },
        )),
        Err(e) => Err(error_response(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    }
}

/// Build a simple string error response for unimplemented features
pub fn not_implemented(feature: &str) -> axum::response::Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(ErrorResponse {
            error: ErrorDetail {
                code: "not_implemented".to_string(),
                message: format!("{feature} is not yet implemented"),
            },
        }),
    )
        .into_response()
}
