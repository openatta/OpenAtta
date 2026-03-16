//! Security API handlers

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};

use atta_security::SecurityPolicy;

use atta_types::{Action, Resource, ResourceType};

use crate::middleware::CurrentUser;
use crate::server::response::{check_authz, error_response, ApiResponse};
use crate::server::AppState;

/// Get current security policy
pub async fn get_security_policy(State(state): State<AppState>, _user: CurrentUser) -> impl IntoResponse {
    let policy = state.security_policy.read().await.clone();
    (StatusCode::OK, Json(ApiResponse { data: policy })).into_response()
}

/// Update security policy
pub async fn update_security_policy(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(new_policy): Json<SecurityPolicy>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Update, &Resource::all(ResourceType::Secret)).await {
        return resp;
    }

    // Validate basic constraints
    if new_policy.max_calls_per_minute == 0 {
        return error_response(
            StatusCode::BAD_REQUEST,
            &atta_types::AttaError::Validation("max_calls_per_minute must be > 0".to_string()),
        );
    }

    let mut policy = state.security_policy.write().await;
    *policy = new_policy;
    let updated = policy.clone();
    drop(policy);

    (StatusCode::OK, Json(ApiResponse { data: updated })).into_response()
}
