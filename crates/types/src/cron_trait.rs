//! Cron scheduler trait
//!
//! Shared trait for cron scheduling, used by both core and tools crates.

use crate::AttaError;

/// Cron scheduler trait for tool integration
#[async_trait::async_trait]
pub trait CronScheduler: Send + Sync + 'static {
    /// Schedule a new cron job
    async fn schedule_job(
        &self,
        name: &str,
        schedule: &str,
        command: &str,
    ) -> Result<serde_json::Value, AttaError>;
    /// List cron jobs
    async fn list_jobs(&self, status: Option<&str>) -> Result<serde_json::Value, AttaError>;
    /// Cancel/delete a cron job
    async fn cancel_job(&self, id: &str) -> Result<(), AttaError>;
    /// Trigger a cron job
    async fn trigger_job(&self, id: &str) -> Result<serde_json::Value, AttaError>;
    /// Get job run history
    async fn job_history(&self, id: &str, limit: usize) -> Result<serde_json::Value, AttaError>;
    /// Update a cron job
    async fn update_job(
        &self,
        id: &str,
        schedule: Option<&str>,
        enabled: Option<bool>,
    ) -> Result<serde_json::Value, AttaError>;
}
