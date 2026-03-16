//! Usage tracking utilities
//!
//! Provides a reusable usage callback factory for recording LLM token
//! usage to the StateStore.

use std::sync::Arc;

use atta_agent::react::UsageCallback;
use atta_store::StateStore;
use atta_types::usage::UsageRecord;

/// Build a [`UsageCallback`] that persists usage records to the given store.
///
/// The callback spawns a background task per LLM call to avoid blocking
/// the agent's ReAct loop.
pub fn build_usage_callback(store: Arc<dyn StateStore>) -> UsageCallback {
    Box::new(move |model: &str, usage: &atta_types::TokenUsage| {
        let store = Arc::clone(&store);
        let model = model.to_string();
        let input = usage.input_tokens;
        let output = usage.output_tokens;
        let total = usage.total();
        let cost = estimate_cost(&model, input, output);
        tokio::spawn(async move {
            let record = UsageRecord {
                id: uuid::Uuid::new_v4().to_string(),
                task_id: None,
                model,
                input_tokens: input,
                output_tokens: output,
                total_tokens: total,
                cost_usd: cost,
                created_at: chrono::Utc::now(),
            };
            if let Err(e) = atta_store::UsageStore::record_usage(store.as_ref(), &record).await {
                tracing::warn!(error = %e, "failed to record usage");
            }
        });
    })
}

/// Build a [`UsageCallback`] that persists usage records with a task ID.
pub fn build_usage_callback_with_task(
    store: Arc<dyn StateStore>,
    task_id: String,
) -> UsageCallback {
    Box::new(move |model: &str, usage: &atta_types::TokenUsage| {
        let store = Arc::clone(&store);
        let model = model.to_string();
        let task_id = task_id.clone();
        let input = usage.input_tokens;
        let output = usage.output_tokens;
        let total = usage.total();
        let cost = estimate_cost(&model, input, output);
        tokio::spawn(async move {
            let record = UsageRecord {
                id: uuid::Uuid::new_v4().to_string(),
                task_id: Some(task_id),
                model,
                input_tokens: input,
                output_tokens: output,
                total_tokens: total,
                cost_usd: cost,
                created_at: chrono::Utc::now(),
            };
            if let Err(e) = atta_store::UsageStore::record_usage(store.as_ref(), &record).await {
                tracing::warn!(error = %e, "failed to record usage");
            }
        });
    })
}

/// Estimate USD cost based on model and token counts.
///
/// Pricing is approximate and based on public pricing as of early 2025.
/// Returns 0.0 for unknown models.
pub fn estimate_cost(model: &str, input_tokens: u64, output_tokens: u64) -> f64 {
    let (input_price, output_price) = match model {
        // OpenAI
        m if m.starts_with("gpt-4o-mini") => (0.15 / 1_000_000.0, 0.60 / 1_000_000.0),
        m if m.starts_with("gpt-4o") => (2.50 / 1_000_000.0, 10.0 / 1_000_000.0),
        m if m.starts_with("gpt-4-turbo") => (10.0 / 1_000_000.0, 30.0 / 1_000_000.0),
        m if m.starts_with("gpt-4") => (30.0 / 1_000_000.0, 60.0 / 1_000_000.0),
        m if m.starts_with("gpt-3.5") => (0.50 / 1_000_000.0, 1.50 / 1_000_000.0),
        m if m.starts_with("o1-mini") => (3.0 / 1_000_000.0, 12.0 / 1_000_000.0),
        m if m.starts_with("o1") => (15.0 / 1_000_000.0, 60.0 / 1_000_000.0),
        // Anthropic
        m if m.contains("claude-3-5-sonnet") || m.contains("claude-sonnet-4") => {
            (3.0 / 1_000_000.0, 15.0 / 1_000_000.0)
        }
        m if m.contains("claude-3-5-haiku") => (0.80 / 1_000_000.0, 4.0 / 1_000_000.0),
        m if m.contains("claude-3-opus") || m.contains("claude-opus-4") => {
            (15.0 / 1_000_000.0, 75.0 / 1_000_000.0)
        }
        // DeepSeek
        m if m.contains("deepseek") => (0.27 / 1_000_000.0, 1.10 / 1_000_000.0),
        _ => (0.0, 0.0),
    };

    (input_tokens as f64 * input_price) + (output_tokens as f64 * output_price)
}
