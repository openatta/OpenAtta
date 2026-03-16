//! Tool API handlers

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};

use atta_types::{Action, AttaError, Resource, ResourceType};

use crate::middleware::CurrentUser;
use crate::server::response::{check_authz, error_response};
use crate::server::AppState;

pub async fn list_tools(State(state): State<AppState>, user: CurrentUser) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Read, &Resource::all(ResourceType::Tool)).await {
        return resp;
    }
    let schemas = state.tool_registry.list_schemas();
    (StatusCode::OK, Json(schemas)).into_response()
}

pub async fn get_tool(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Read, &Resource::new(ResourceType::Tool, &name)).await {
        return resp;
    }
    match state.tool_registry.get_schema(&name) {
        Some(schema) => (StatusCode::OK, Json(schema)).into_response(),
        None => error_response(StatusCode::NOT_FOUND, &AttaError::ToolNotFound(name)),
    }
}

/// POST /api/v1/tools/{name}/test
///
/// Test a tool by invoking it with the provided JSON arguments.
pub async fn test_tool(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(name): Path<String>,
    Json(args): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Execute, &Resource::new(ResourceType::Tool, &name)).await {
        return resp;
    }
    // Verify the tool exists first
    if state.tool_registry.get(&name).is_none() {
        return error_response(StatusCode::NOT_FOUND, &AttaError::ToolNotFound(name));
    }

    // Invoke the tool with the provided arguments
    match state.tool_registry.invoke(&name, &args).await {
        Ok(result) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "tool": name,
                "success": true,
                "result": result,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "tool": name,
                "success": false,
                "error": e.to_string(),
            })),
        )
            .into_response(),
    }
}
