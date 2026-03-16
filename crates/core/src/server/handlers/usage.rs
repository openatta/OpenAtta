//! Usage & Cost API handlers
//!
//! Queries real usage data from the StateStore's UsageStore implementation.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::{Duration, Utc};
use serde::Deserialize;

use super::super::AppState;
use crate::middleware::CurrentUser;
use crate::server::response::ApiResponse;

#[derive(Debug, Deserialize)]
pub struct SummaryQuery {
    pub period: Option<String>,
}

/// Parse period string (e.g. "7d", "30d", "90d") into a Duration.
/// Defaults to 30 days.
fn parse_period(period: &str) -> Duration {
    let trimmed = period.trim_end_matches('d');
    trimmed
        .parse::<i64>()
        .ok()
        .map(Duration::days)
        .unwrap_or_else(|| Duration::days(30))
}

/// GET /api/v1/usage/summary
pub async fn usage_summary(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(query): Query<SummaryQuery>,
) -> impl IntoResponse {
    let period = query.period.unwrap_or_else(|| "30d".to_string());
    let duration = parse_period(&period);
    let since = Utc::now() - duration;

    match state.store.get_usage_summary(since).await {
        Ok(mut summary) => {
            summary.period = period;
            (StatusCode::OK, Json(ApiResponse { data: summary })).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to get usage summary");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct DailyQuery {
    pub start: Option<String>,
    pub end: Option<String>,
}

/// GET /api/v1/usage/daily
pub async fn usage_daily(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(query): Query<DailyQuery>,
) -> impl IntoResponse {
    let end = query
        .end
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);

    let start = query
        .start
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|| end - Duration::days(30));

    match state.store.get_usage_daily(start, end).await {
        Ok(daily) => (StatusCode::OK, Json(ApiResponse { data: daily })).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "failed to get daily usage");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    }
}

/// GET /api/v1/usage/by-model
pub async fn usage_by_model(State(state): State<AppState>, _user: CurrentUser) -> impl IntoResponse {
    let since = Utc::now() - Duration::days(30);

    match state.store.get_usage_summary(since).await {
        Ok(summary) => {
            (StatusCode::OK, Json(ApiResponse { data: summary.by_model })).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to get usage by model");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    }
}

/// GET /api/v1/usage/export
pub async fn usage_export(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(query): Query<DailyQuery>,
) -> impl IntoResponse {
    let end = query
        .end
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);

    let start = query
        .start
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|| end - Duration::days(30));

    match state.store.get_usage_daily(start, end).await {
        Ok(daily) => {
            let mut csv = String::from("date,tokens,cost_usd,input_tokens,output_tokens\n");
            for row in &daily {
                csv.push_str(&format!(
                    "{},{},{:.6},{},{}\n",
                    row.date, row.tokens, row.cost_usd, row.input_tokens, row.output_tokens
                ));
            }
            (
                StatusCode::OK,
                [
                    ("content-type", "text/csv"),
                    (
                        "content-disposition",
                        "attachment; filename=\"usage.csv\"",
                    ),
                ],
                csv,
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to export usage");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    }
}
