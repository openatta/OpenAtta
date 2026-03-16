//! Diagnostics API handlers

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Serialize;

use atta_types::{Action, Resource, ResourceType};

use crate::middleware::CurrentUser;
use crate::server::response::{check_authz, ApiResponse};
use crate::server::AppState;

#[derive(Debug, Serialize)]
pub struct DiagResult {
    pub severity: String,
    pub category: String,
    pub message: String,
}

/// POST /api/v1/diagnostics/run
pub async fn run_diagnostics(
    State(state): State<AppState>,
    user: CurrentUser,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Read, &Resource::all(ResourceType::Node)).await {
        return resp;
    }

    let mut results = Vec::new();

    // Check database connectivity
    match state.store.list_tasks(&atta_types::TaskFilter {
        status: None,
        flow_id: None,
        created_by: None,
        limit: 1,
        offset: 0,
    }).await {
        Ok(_) => results.push(DiagResult {
            severity: "ok".to_string(),
            category: "database".to_string(),
            message: "SQLite connection OK".to_string(),
        }),
        Err(e) => results.push(DiagResult {
            severity: "error".to_string(),
            category: "database".to_string(),
            message: format!("Database error: {e}"),
        }),
    }

    // Check cron engine
    if state.cron_engine.is_some() {
        results.push(DiagResult {
            severity: "ok".to_string(),
            category: "cron".to_string(),
            message: "Cron engine running".to_string(),
        });
    } else {
        results.push(DiagResult {
            severity: "warn".to_string(),
            category: "cron".to_string(),
            message: "Cron engine not initialized".to_string(),
        });
    }

    // Check channels
    let channel_names = state.channel_registry.list().await;
    if channel_names.is_empty() {
        results.push(DiagResult {
            severity: "ok".to_string(),
            category: "channels".to_string(),
            message: "No channels configured".to_string(),
        });
    } else {
        for name in &channel_names {
            if let Some(ch) = state.channel_registry.get(name).await {
                match ch.health_check().await {
                    Ok(()) => results.push(DiagResult {
                        severity: "ok".to_string(),
                        category: "channels".to_string(),
                        message: format!("{name}: connected"),
                    }),
                    Err(e) => results.push(DiagResult {
                        severity: "error".to_string(),
                        category: "channels".to_string(),
                        message: format!("{name}: {e}"),
                    }),
                }
            }
        }
    }

    // Check MCP servers
    let mcp_names = state.mcp_registry.list_servers().await;
    if mcp_names.is_empty() {
        results.push(DiagResult {
            severity: "ok".to_string(),
            category: "mcp".to_string(),
            message: "No MCP servers registered".to_string(),
        });
    } else {
        results.push(DiagResult {
            severity: "ok".to_string(),
            category: "mcp".to_string(),
            message: format!("{} MCP server(s) registered", mcp_names.len()),
        });
    }

    // System info
    results.push(DiagResult {
        severity: "ok".to_string(),
        category: "system".to_string(),
        message: format!("AttaOS v{} running", env!("CARGO_PKG_VERSION")),
    });

    (StatusCode::OK, Json(ApiResponse { data: results })).into_response()
}
