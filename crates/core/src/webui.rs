//! WebUI static file serving from filesystem
//!
//! Serves the Vue 3 SPA from `$ATTA_HOME/lib/webui/` directory.
//! Returns 404 JSON when the directory is absent or has no `index.html`.

use std::path::PathBuf;

use axum::{
    http::StatusCode,
    response::{IntoResponse, Json},
    Router,
};

/// Build a router for WebUI static file serving.
///
/// - If `webui_dir` is `Some` and contains `index.html`, serves files with SPA fallback.
/// - Otherwise, all routes return 404 JSON.
pub fn webui_routes(webui_dir: Option<PathBuf>) -> Router {
    match webui_dir {
        Some(dir) if dir.join("index.html").exists() => {
            tracing::info!(dir = %dir.display(), "serving WebUI from filesystem");

            // Serve static files with SPA fallback (index.html for unmatched paths)
            let serve_dir = tower_http::services::ServeDir::new(&dir)
                .not_found_service(tower_http::services::ServeFile::new(dir.join("index.html")));

            Router::new().fallback_service(serve_dir)
        }
        _ => {
            tracing::info!("WebUI not installed — returning 404 for UI requests");
            Router::new().fallback(webui_not_found)
        }
    }
}

/// Handler that returns 404 JSON when WebUI is not installed
async fn webui_not_found() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "WebUI not installed" })),
    )
}
