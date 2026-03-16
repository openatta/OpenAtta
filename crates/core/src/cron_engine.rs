//! Cron 调度引擎
//!
//! 管理定时任务的调度、取消、触发和历史查询。
//! 使用 cron 表达式解析和 tokio 定时器实现分钟级精度的调度。

use std::sync::Arc;

use atta_bus::EventBus;
use atta_store::StateStore;
use atta_types::{AttaError, CronJob, CronRun, CronRunStatus, EventEnvelope};
use chrono::Utc;
use cron::Schedule;
use tracing::{debug, error, info, warn};

/// Cron 调度引擎
pub struct CronEngine {
    store: Arc<dyn StateStore>,
    /// Event bus for publishing cron execution events
    bus: Arc<dyn EventBus>,
    /// 取消令牌，用于停止调度循环
    cancel: tokio_util::sync::CancellationToken,
}

impl CronEngine {
    /// 创建新的 Cron 引擎
    pub fn new(store: Arc<dyn StateStore>, bus: Arc<dyn EventBus>) -> Self {
        Self {
            store,
            bus,
            cancel: tokio_util::sync::CancellationToken::new(),
        }
    }

    /// 启动调度循环（后台 tick 检查）
    pub fn start(self: &Arc<Self>) {
        let engine = Arc::clone(self);
        tokio::spawn(async move {
            info!("CronEngine started");
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if let Err(e) = engine.tick().await {
                            error!(error = %e, "CronEngine tick error");
                        }
                    }
                    _ = engine.cancel.cancelled() => {
                        info!("CronEngine stopped");
                        break;
                    }
                }
            }
        });
    }

    /// 停止调度引擎
    pub fn stop(&self) {
        self.cancel.cancel();
    }

    /// 每分钟 tick：检查并执行到期的 cron jobs
    async fn tick(&self) -> Result<(), AttaError> {
        let now = Utc::now();
        let jobs = self.store.list_cron_jobs(Some("active")).await?;

        for job in &jobs {
            // Check if this job should run now
            if let Some(next_run) = &job.next_run_at {
                if *next_run <= now {
                    debug!(job_id = %job.id, name = %job.name, "cron job due, triggering");
                    if let Err(e) = self.trigger_job(&job.id, "scheduler").await {
                        warn!(job_id = %job.id, error = %e, "failed to trigger cron job");
                    }
                }
            }
        }

        Ok(())
    }

    /// 调度新 cron 任务
    pub async fn schedule(&self, job: CronJob) -> Result<CronJob, AttaError> {
        // Validate cron expression
        let schedule: Schedule = job.schedule.parse().map_err(|e| {
            AttaError::Validation(format!("invalid cron expression '{}': {}", job.schedule, e))
        })?;

        // Calculate next run time
        let next_run = schedule.upcoming(Utc).next();

        let mut job = job;
        job.next_run_at = next_run;

        self.store.save_cron_job(&job).await?;
        info!(job_id = %job.id, name = %job.name, schedule = %job.schedule, "cron job scheduled");

        Ok(job)
    }

    /// 取消（删除）cron 任务
    pub async fn cancel(&self, job_id: &str) -> Result<(), AttaError> {
        self.store.delete_cron_job(job_id).await?;
        info!(job_id = %job_id, "cron job cancelled");
        Ok(())
    }

    /// 列出 cron 任务
    pub async fn list(&self, status: Option<&str>) -> Result<Vec<CronJob>, AttaError> {
        self.store.list_cron_jobs(status).await
    }

    /// 获取单个 cron 任务
    pub async fn get(&self, job_id: &str) -> Result<Option<CronJob>, AttaError> {
        self.store.get_cron_job(job_id).await
    }

    /// 手动触发 cron 任务
    pub async fn trigger_job(
        &self,
        job_id: &str,
        triggered_by: &str,
    ) -> Result<CronRun, AttaError> {
        let job = self
            .store
            .get_cron_job(job_id)
            .await?
            .ok_or_else(|| AttaError::Validation(format!("cron job '{}' not found", job_id)))?;

        let run_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();

        let run = CronRun {
            id: run_id.clone(),
            job_id: job_id.to_string(),
            status: CronRunStatus::Running,
            started_at: now,
            completed_at: None,
            output: None,
            error: None,
            triggered_by: triggered_by.to_string(),
        };

        self.store.save_cron_run(&run).await?;

        // Update last_run_at and next_run_at
        let mut updated_job = job.clone();
        updated_job.last_run_at = Some(now);
        updated_job.updated_at = now;

        // Calculate next run time
        if let Ok(schedule) = job.schedule.parse::<Schedule>() {
            updated_job.next_run_at = schedule.upcoming(Utc).next();
        }

        self.store.save_cron_job(&updated_job).await?;

        info!(job_id = %job_id, run_id = %run_id, "cron job triggered");

        // Publish cron.triggered event — CoreCoordinator will handle execution
        let event = EventEnvelope::new(
            "atta.cron.triggered",
            atta_types::EntityRef::task(&uuid::Uuid::new_v4()),
            atta_types::Actor::system(),
            uuid::Uuid::new_v4(),
            serde_json::json!({
                "job_id": job.id,
                "run_id": run_id,
                "command": job.command,
                "config": job.config,
                "triggered_by": triggered_by,
            }),
        )?;

        // Spawn async execution so trigger_job returns immediately
        let store = Arc::clone(&self.store);
        let bus = Arc::clone(&self.bus);
        let job_id_owned = job_id.to_string();
        let run_id_owned = run_id.clone();
        let triggered_by_owned = triggered_by.to_string();

        tokio::spawn(async move {
            // Publish the event for CoreCoordinator to pick up
            if let Err(e) = bus.publish("atta.cron.triggered", event).await {
                error!(job_id = %job_id_owned, error = %e, "failed to publish cron.triggered event");

                // Mark run as failed if we can't even publish
                let failed_run = CronRun {
                    id: run_id_owned,
                    job_id: job_id_owned.clone(),
                    status: CronRunStatus::Failed,
                    started_at: now,
                    completed_at: Some(Utc::now()),
                    output: None,
                    error: Some(format!("failed to publish execution event: {e}")),
                    triggered_by: triggered_by_owned,
                };
                if let Err(e) = store.save_cron_run(&failed_run).await {
                    error!(job_id = %job_id_owned, error = %e, "failed to persist cron run failure");
                }
            }
        });

        // Return the run record (status=Running; will be updated async)
        Ok(CronRun {
            id: run_id,
            job_id: job_id.to_string(),
            status: CronRunStatus::Running,
            started_at: now,
            completed_at: None,
            output: None,
            error: None,
            triggered_by: triggered_by.to_string(),
        })
    }

    /// 查询任务运行历史
    pub async fn history(&self, job_id: &str, limit: usize) -> Result<Vec<CronRun>, AttaError> {
        self.store.list_cron_runs(job_id, limit).await
    }

    /// 更新 cron 任务的调度或启用状态
    pub async fn update(
        &self,
        job_id: &str,
        schedule: Option<&str>,
        enabled: Option<bool>,
    ) -> Result<CronJob, AttaError> {
        let mut job = self
            .store
            .get_cron_job(job_id)
            .await?
            .ok_or_else(|| AttaError::Validation(format!("cron job '{}' not found", job_id)))?;

        if let Some(new_schedule) = schedule {
            // Validate the new cron expression
            let sched: Schedule = new_schedule.parse().map_err(|e| {
                AttaError::Validation(format!("invalid cron expression '{}': {}", new_schedule, e))
            })?;
            job.schedule = new_schedule.to_string();
            job.next_run_at = sched.upcoming(Utc).next();
        }

        if let Some(en) = enabled {
            job.enabled = en;
        }

        job.updated_at = Utc::now();
        self.store.save_cron_job(&job).await?;

        info!(job_id = %job_id, "cron job updated");
        Ok(job)
    }
}

/// Implement the CronScheduler trait from atta-types for tool integration
#[async_trait::async_trait]
impl atta_types::CronScheduler for CronEngine {
    async fn schedule_job(
        &self,
        name: &str,
        schedule_expr: &str,
        command: &str,
    ) -> Result<serde_json::Value, AttaError> {
        let job = CronJob {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            schedule: schedule_expr.to_string(),
            command: command.to_string(),
            config: serde_json::Value::Object(Default::default()),
            enabled: true,
            created_by: "agent".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_run_at: None,
            next_run_at: None,
        };

        let result = self.schedule(job).await?;
        Ok(serde_json::to_value(result).unwrap_or_default())
    }

    async fn list_jobs(&self, status: Option<&str>) -> Result<serde_json::Value, AttaError> {
        let jobs = self.list(status).await?;
        Ok(serde_json::to_value(jobs).unwrap_or_default())
    }

    async fn cancel_job(&self, id: &str) -> Result<(), AttaError> {
        self.cancel(id).await
    }

    async fn trigger_job(&self, id: &str) -> Result<serde_json::Value, AttaError> {
        let run = self.trigger_job(id, "manual").await?;
        Ok(serde_json::to_value(run).unwrap_or_default())
    }

    async fn job_history(&self, id: &str, limit: usize) -> Result<serde_json::Value, AttaError> {
        let runs = self.history(id, limit).await?;
        Ok(serde_json::to_value(runs).unwrap_or_default())
    }

    async fn update_job(
        &self,
        id: &str,
        schedule: Option<&str>,
        enabled: Option<bool>,
    ) -> Result<serde_json::Value, AttaError> {
        let job = self.update(id, schedule, enabled).await?;
        Ok(serde_json::to_value(job).unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cron_expression_parsing() {
        // Valid cron expression
        let result = "0 */5 * * * *".parse::<Schedule>();
        assert!(result.is_ok());

        // Invalid cron expression
        let result = "invalid".parse::<Schedule>();
        assert!(result.is_err());
    }

    #[test]
    fn test_next_run_calculation() {
        let schedule: Schedule = "0 * * * * *".parse().unwrap(); // every minute
        let next = schedule.upcoming(Utc).next();
        assert!(next.is_some());
        assert!(next.unwrap() > Utc::now());
    }
}
