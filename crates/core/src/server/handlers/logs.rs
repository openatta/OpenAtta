//! Log streaming API handlers
//!
//! Provides an SSE endpoint that streams structured log entries to the UI
//! and a REST endpoint for recent log entries. Both are backed by the
//! [`LogBroadcast`] in [`AppState`].

use axum::{
    extract::State,
    http::StatusCode,
    response::{
        sse::{Event, Sse},
        IntoResponse,
    },
    Json,
};
use futures::stream::Stream;
use std::convert::Infallible;
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::middleware::CurrentUser;
use crate::server::response::ApiResponse;
use crate::server::AppState;

/// GET /api/v1/logs/stream — SSE endpoint for real-time log streaming
///
/// Subscribes to the broadcast channel in [`AppState`] and yields each
/// [`LogEntry`] as a server-sent event.
pub async fn log_stream(
    State(state): State<AppState>,
    _user: CurrentUser,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.log_broadcast.subscribe();

    let stream = BroadcastStream::new(rx).filter_map(|result| match result {
        Ok(entry) => match serde_json::to_string(&entry) {
            Ok(json) => Some(Ok(Event::default().data(json))),
            Err(_) => None,
        },
        // Skip lagged entries rather than terminating the stream.
        Err(_) => None,
    });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}

/// GET /api/v1/logs/recent — Returns recent log entries (newest first)
pub async fn recent_logs(State(state): State<AppState>, _user: CurrentUser) -> impl IntoResponse {
    let entries = state.log_broadcast.recent(100);
    (StatusCode::OK, Json(ApiResponse { data: entries })).into_response()
}
