//! Usage tracking types
//!
//! Token usage and cost tracking for LLM API calls.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A single LLM API call usage record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    /// Unique record ID
    pub id: String,
    /// Associated task ID (if any)
    pub task_id: Option<String>,
    /// Model identifier (e.g. "gpt-4o", "claude-sonnet-4-20250514")
    pub model: String,
    /// Number of input (prompt) tokens
    pub input_tokens: u64,
    /// Number of output (completion) tokens
    pub output_tokens: u64,
    /// Total tokens (input + output)
    pub total_tokens: u64,
    /// Estimated cost in USD
    pub cost_usd: f64,
    /// When the API call was made
    pub created_at: DateTime<Utc>,
}

/// Aggregated usage summary for a time period
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSummary {
    /// Total tokens across all calls
    pub total_tokens: u64,
    /// Total estimated cost in USD
    pub total_cost_usd: f64,
    /// Total input tokens
    pub input_tokens: u64,
    /// Total output tokens
    pub output_tokens: u64,
    /// Number of API requests
    pub request_count: u64,
    /// Breakdown by model
    pub by_model: Vec<ModelUsage>,
    /// Period description (e.g. "30d", "7d")
    pub period: String,
}

/// Per-model usage aggregation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelUsage {
    /// Model identifier
    pub model: String,
    /// Total tokens for this model
    pub tokens: u64,
    /// Total cost for this model
    pub cost_usd: f64,
    /// Number of requests to this model
    pub request_count: u64,
}

/// Daily usage aggregation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageDaily {
    /// Date string (YYYY-MM-DD)
    pub date: String,
    /// Total tokens for this day
    pub tokens: u64,
    /// Total cost for this day
    pub cost_usd: f64,
    /// Input tokens for this day
    pub input_tokens: u64,
    /// Output tokens for this day
    pub output_tokens: u64,
}

/// Token usage returned from an LLM API call
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Number of input (prompt) tokens
    pub input_tokens: u64,
    /// Number of output (completion) tokens
    pub output_tokens: u64,
}

impl TokenUsage {
    /// Total tokens (input + output)
    pub fn total(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}
