//! System API handlers

use std::time::SystemTime;

use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

pub async fn health_check() -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

pub async fn system_config() -> impl IntoResponse {
    // Return sanitized system config (no secrets)
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "mode": "desktop",
            "version": env!("CARGO_PKG_VERSION"),
        })),
    )
        .into_response()
}

/// GET /api/v1/system/metrics
///
/// Returns basic system metrics as a JSON object.
pub async fn metrics() -> impl IntoResponse {
    let uptime_secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "mode": "desktop",
            "rust_version": option_env!("CARGO_PKG_RUST_VERSION").unwrap_or("unknown"),
            "uptime_timestamp": uptime_secs,
            "status": "running",
        })),
    )
        .into_response()
}
