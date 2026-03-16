//! Memory API handlers
//!
//! Search, get, and delete memory entries via REST API.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use atta_memory::SearchOptions;
use atta_types::{Action, AttaError, Resource, ResourceType};

use crate::middleware::CurrentUser;
use crate::server::response::{check_authz, error_response};
use crate::server::AppState;

/// Query parameters for memory search
#[derive(Debug, Deserialize)]
pub struct MemorySearchQuery {
    /// Search query (semantic + full-text)
    pub query: String,
    /// Maximum results to return (default: 10)
    pub limit: Option<usize>,
    /// Filter by memory type (comma-separated: knowledge,episodic,procedural,semantic)
    pub memory_types: Option<String>,
    /// Minimum relevance score (0.0 - 1.0)
    pub min_score: Option<f32>,
}

/// GET /api/v1/memory/search?query=...&limit=...
pub async fn search_memory(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(params): Query<MemorySearchQuery>,
) -> impl IntoResponse {
    let memory_types = params.memory_types.as_ref().map(|s| {
        s.split(',')
            .filter_map(|t| serde_json::from_str(&format!("\"{t}\"")).ok())
            .collect()
    });

    let options = SearchOptions {
        limit: params.limit.unwrap_or(10),
        memory_types,
        min_score: params.min_score,
        ..Default::default()
    };

    match state.memory_store.search(&params.query, &options).await {
        Ok(results) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "count": results.len(),
                "results": results,
            })),
        )
            .into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

/// GET /api/v1/memory/{id}
pub async fn get_memory(
    State(state): State<AppState>,
    _user: CurrentUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &AttaError::Validation(format!("invalid UUID: {e}")),
            );
        }
    };

    match state.memory_store.get(&id).await {
        Ok(Some(entry)) => (StatusCode::OK, Json(entry)).into_response(),
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            &AttaError::NotFound {
                entity_type: "memory".to_string(),
                id: id.to_string(),
            },
        ),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

/// DELETE /api/v1/memory/{id}
pub async fn delete_memory(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Delete, &Resource::new(ResourceType::Tool, &id)).await {
        return resp;
    }

    let id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &AttaError::Validation(format!("invalid UUID: {e}")),
            );
        }
    };

    match state.memory_store.delete(&id).await {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "deleted",
                "id": id.to_string(),
            })),
        )
            .into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}
