//! MCP Server management API handlers

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde_json::json;

use atta_types::{Action, McpServerConfig, McpTransport, Resource, ResourceType};

use crate::middleware::CurrentUser;
use crate::server::response::check_authz;
use crate::server::AppState;

/// GET /api/v1/mcp/servers — list all registered MCP server names
pub async fn list_mcp_servers(State(state): State<AppState>, user: CurrentUser) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Read, &Resource::all(ResourceType::Mcp)).await {
        return resp;
    }
    let servers = state.mcp_registry.list_servers().await;
    (StatusCode::OK, Json(json!({ "servers": servers }))).into_response()
}

/// GET /api/v1/mcp/servers/{name} — get server info including its tools
pub async fn get_mcp_server(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Read, &Resource::new(ResourceType::Mcp, &name)).await {
        return resp;
    }
    let client = match state.mcp_registry.get(&name).await {
        Some(c) => c,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": format!("MCP server '{}' not found", name) })),
            )
                .into_response();
        }
    };

    match client.list_tools().await {
        Ok(tools) => {
            let tool_list: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "description": t.description,
                        "input_schema": t.input_schema,
                    })
                })
                .collect();

            (
                StatusCode::OK,
                Json(json!({
                    "name": name,
                    "tools": tool_list,
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("failed to list tools: {}", e) })),
        )
            .into_response(),
    }
}

/// POST /api/v1/mcp/servers — register a new MCP server
pub async fn register_mcp_server(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(config): Json<McpServerConfig>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Create, &Resource::all(ResourceType::Mcp)).await {
        return resp;
    }
    match config.transport {
        McpTransport::Stdio => {
            let command = match &config.command {
                Some(cmd) => cmd.clone(),
                None => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": "stdio transport requires 'command' field" })),
                    )
                        .into_response();
                }
            };

            match atta_mcp::StdioMcpClient::spawn(&config.name, &command, &config.args).await {
                Ok(client) => {
                    state.mcp_registry.add(&config.name, Arc::new(client)).await;
                    (
                        StatusCode::CREATED,
                        Json(json!({
                            "name": config.name,
                            "transport": "stdio",
                            "status": "registered",
                        })),
                    )
                        .into_response()
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": format!("failed to spawn MCP server: {}", e) })),
                )
                    .into_response(),
            }
        }
        McpTransport::Sse => {
            let url = match &config.url {
                Some(u) => u.clone(),
                None => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": "SSE transport requires 'url' field" })),
                    )
                        .into_response();
                }
            };

            let client = atta_mcp::SseMcpClient::new(&config.name, url);
            state.mcp_registry.add(&config.name, Arc::new(client)).await;
            (
                StatusCode::CREATED,
                Json(json!({
                    "name": config.name,
                    "transport": "sse",
                    "status": "registered",
                })),
            )
                .into_response()
        }
    }
}

/// DELETE /api/v1/mcp/servers/{name} — unregister an MCP server
pub async fn unregister_mcp_server(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Delete, &Resource::new(ResourceType::Mcp, &name)).await {
        return resp;
    }
    match state.mcp_registry.remove(&name).await {
        Some(_) => {
            // Also remove from persistent store
            let _ = state.store.unregister_mcp(&name).await;
            (
                StatusCode::OK,
                Json(json!({ "name": name, "status": "removed" })),
            )
                .into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("MCP server '{}' not found", name) })),
        )
            .into_response(),
    }
}

/// POST /api/v1/mcp/servers/{name}/connect — ping the server to verify connection
pub async fn connect_mcp_server(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Execute, &Resource::new(ResourceType::Mcp, &name)).await {
        return resp;
    }
    let client = match state.mcp_registry.get(&name).await {
        Some(c) => c,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": format!("MCP server '{}' not found", name) })),
            )
                .into_response();
        }
    };

    match client.ping().await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "name": name, "status": "connected" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("ping failed: {}", e) })),
        )
            .into_response(),
    }
}

/// POST /api/v1/mcp/servers/{name}/disconnect — remove server from registry
pub async fn disconnect_mcp_server(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Execute, &Resource::new(ResourceType::Mcp, &name)).await {
        return resp;
    }
    match state.mcp_registry.remove(&name).await {
        Some(_) => (
            StatusCode::OK,
            Json(json!({ "name": name, "status": "disconnected" })),
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("MCP server '{}' not found", name) })),
        )
            .into_response(),
    }
}
