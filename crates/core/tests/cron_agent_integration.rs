//! Integration tests: CronEngine and AgentRegistry
//!
//! Tests the CronEngine scheduling lifecycle and AgentRegistry
//! spawn/list/pause/resume/terminate operations using a real SQLite store.

use std::sync::Arc;

use atta_bus::InProcBus;
use atta_core::agent_registry::{AgentRegistry, AgentStatus};
use atta_core::cron_engine::CronEngine;
use atta_store::SqliteStore;
use atta_types::{CronJob, CronScheduler, SubAgentRegistry};
use chrono::Utc;

fn temp_db_path() -> String {
    format!(
        "/tmp/atta_cron_test_{}.db",
        uuid::Uuid::new_v4().to_string().replace('-', "")
    )
}

// ─── CronEngine Tests ───────────────────────────────────────────────

#[tokio::test]
async fn test_cron_schedule_and_list() {
    let store: Arc<dyn atta_store::StateStore> =
        Arc::new(SqliteStore::open(&temp_db_path()).await.unwrap());
    let bus: Arc<dyn atta_bus::EventBus> = Arc::new(InProcBus::new());
    let engine = CronEngine::new(store, bus);

    let job = CronJob {
        id: uuid::Uuid::new_v4().to_string(),
        name: "test-job".to_string(),
        schedule: "0 */5 * * * *".to_string(), // every 5 minutes
        command: "echo hello".to_string(),
        config: serde_json::Value::Object(Default::default()),
        enabled: true,
        created_by: "test".to_string(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_run_at: None,
        next_run_at: None,
    };

    let scheduled = engine.schedule(job).await.unwrap();
    assert!(scheduled.next_run_at.is_some(), "next_run_at should be set");

    let jobs = engine.list(None).await.unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].name, "test-job");
}

#[tokio::test]
async fn test_cron_schedule_invalid_expression() {
    let store: Arc<dyn atta_store::StateStore> =
        Arc::new(SqliteStore::open(&temp_db_path()).await.unwrap());
    let bus: Arc<dyn atta_bus::EventBus> = Arc::new(InProcBus::new());
    let engine = CronEngine::new(store, bus);

    let job = CronJob {
        id: uuid::Uuid::new_v4().to_string(),
        name: "bad-job".to_string(),
        schedule: "this is not cron".to_string(),
        command: "echo".to_string(),
        config: serde_json::Value::Object(Default::default()),
        enabled: true,
        created_by: "test".to_string(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_run_at: None,
        next_run_at: None,
    };

    let result = engine.schedule(job).await;
    assert!(result.is_err(), "invalid cron expression should fail");
}

#[tokio::test]
async fn test_cron_cancel() {
    let store: Arc<dyn atta_store::StateStore> =
        Arc::new(SqliteStore::open(&temp_db_path()).await.unwrap());
    let bus: Arc<dyn atta_bus::EventBus> = Arc::new(InProcBus::new());
    let engine = CronEngine::new(store, bus);

    let job = CronJob {
        id: uuid::Uuid::new_v4().to_string(),
        name: "cancel-me".to_string(),
        schedule: "0 * * * * *".to_string(),
        command: "echo".to_string(),
        config: serde_json::Value::Object(Default::default()),
        enabled: true,
        created_by: "test".to_string(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_run_at: None,
        next_run_at: None,
    };

    let scheduled = engine.schedule(job).await.unwrap();
    engine.cancel(&scheduled.id).await.unwrap();

    let jobs = engine.list(None).await.unwrap();
    assert_eq!(jobs.len(), 0);
}

#[tokio::test]
async fn test_cron_trigger_job() {
    let store: Arc<dyn atta_store::StateStore> =
        Arc::new(SqliteStore::open(&temp_db_path()).await.unwrap());
    let bus: Arc<dyn atta_bus::EventBus> = Arc::new(InProcBus::new());
    let engine = CronEngine::new(store, bus);

    let job = CronJob {
        id: uuid::Uuid::new_v4().to_string(),
        name: "trigger-me".to_string(),
        schedule: "0 * * * * *".to_string(),
        command: "echo triggered".to_string(),
        config: serde_json::Value::Object(Default::default()),
        enabled: true,
        created_by: "test".to_string(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_run_at: None,
        next_run_at: None,
    };

    let scheduled = engine.schedule(job).await.unwrap();
    let run = engine.trigger_job(&scheduled.id, "test").await.unwrap();

    assert_eq!(run.job_id, scheduled.id);
    assert_eq!(run.triggered_by, "test");
    // trigger_job returns immediately with Running status (async execution via event bus)
    assert_eq!(run.status, atta_types::CronRunStatus::Running);
    assert!(run.completed_at.is_none());

    // Check last_run_at was updated on the job
    let updated = engine.get(&scheduled.id).await.unwrap().unwrap();
    assert!(updated.last_run_at.is_some());
}

#[tokio::test]
async fn test_cron_update_schedule() {
    let store: Arc<dyn atta_store::StateStore> =
        Arc::new(SqliteStore::open(&temp_db_path()).await.unwrap());
    let bus: Arc<dyn atta_bus::EventBus> = Arc::new(InProcBus::new());
    let engine = CronEngine::new(store, bus);

    let job = CronJob {
        id: uuid::Uuid::new_v4().to_string(),
        name: "update-me".to_string(),
        schedule: "0 * * * * *".to_string(),
        command: "echo".to_string(),
        config: serde_json::Value::Object(Default::default()),
        enabled: true,
        created_by: "test".to_string(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_run_at: None,
        next_run_at: None,
    };

    let scheduled = engine.schedule(job).await.unwrap();

    // Update schedule
    let updated = engine
        .update(&scheduled.id, Some("0 */10 * * * *"), None)
        .await
        .unwrap();
    assert_eq!(updated.schedule, "0 */10 * * * *");

    // Disable
    let disabled = engine
        .update(&scheduled.id, None, Some(false))
        .await
        .unwrap();
    assert!(!disabled.enabled);
}

#[tokio::test]
async fn test_cron_scheduler_trait() {
    let store: Arc<dyn atta_store::StateStore> =
        Arc::new(SqliteStore::open(&temp_db_path()).await.unwrap());
    let bus: Arc<dyn atta_bus::EventBus> = Arc::new(InProcBus::new());
    let engine = CronEngine::new(store, bus);

    // Test via CronScheduler trait
    let result = engine
        .schedule_job("trait-job", "0 * * * * *", "echo hello")
        .await
        .unwrap();
    assert!(result.get("id").is_some());

    let list = engine.list_jobs(None).await.unwrap();
    let jobs = list.as_array().unwrap();
    assert_eq!(jobs.len(), 1);
}

// ─── AgentRegistry Tests ────────────────────────────────────────────

#[tokio::test]
async fn test_agent_spawn_and_list() {
    let registry = AgentRegistry::new();

    let id = registry
        .spawn("integration test task".to_string(), |cancel| async move {
            cancel.cancelled().await;
        })
        .await;

    let agents = registry.list().await;
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].id, id);
    assert_eq!(agents[0].status, AgentStatus::Running);
    assert_eq!(agents[0].task, "integration test task");

    registry.terminate(&id).await.unwrap();
}

#[tokio::test]
async fn test_agent_full_lifecycle() {
    let registry = AgentRegistry::new();

    // Spawn
    let id = registry
        .spawn("lifecycle test".to_string(), |cancel| async move {
            cancel.cancelled().await;
        })
        .await;

    // Get
    let agent = registry.get(&id).await.unwrap();
    assert_eq!(agent.status, AgentStatus::Running);

    // Pause
    registry.pause(&id).await.unwrap();
    let agent = registry.get(&id).await.unwrap();
    assert_eq!(agent.status, AgentStatus::Paused);

    // Resume
    registry.resume(&id).await.unwrap();
    let agent = registry.get(&id).await.unwrap();
    assert_eq!(agent.status, AgentStatus::Running);

    // Terminate
    registry.terminate(&id).await.unwrap();
    let agent = registry.get(&id).await.unwrap();
    assert_eq!(agent.status, AgentStatus::Terminated);

    // Cleanup
    registry.cleanup().await;
    let agents = registry.list().await;
    assert_eq!(agents.len(), 0);
}

#[tokio::test]
async fn test_agent_multiple_spawn() {
    let registry = AgentRegistry::new();

    let id1 = registry
        .spawn("task 1".to_string(), |c| async move { c.cancelled().await })
        .await;
    let id2 = registry
        .spawn("task 2".to_string(), |c| async move { c.cancelled().await })
        .await;
    let id3 = registry
        .spawn("task 3".to_string(), |c| async move { c.cancelled().await })
        .await;

    let agents = registry.list().await;
    assert_eq!(agents.len(), 3);

    // Terminate one
    registry.terminate(&id2).await.unwrap();
    let running: Vec<_> = registry
        .list()
        .await
        .into_iter()
        .filter(|a| a.status == AgentStatus::Running)
        .collect();
    assert_eq!(running.len(), 2);

    // Cleanup terminated
    registry.cleanup().await;
    assert_eq!(registry.list().await.len(), 2);

    // Terminate rest
    registry.terminate(&id1).await.unwrap();
    registry.terminate(&id3).await.unwrap();
}

#[tokio::test]
async fn test_agent_terminate_nonexistent() {
    let registry = AgentRegistry::new();
    let result = registry.terminate("nonexistent-id").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_agent_pause_not_running() {
    let registry = AgentRegistry::new();

    let id = registry
        .spawn("test".to_string(), |c| async move { c.cancelled().await })
        .await;

    // Pause it
    registry.pause(&id).await.unwrap();

    // Try to pause again — should fail
    let result = registry.pause(&id).await;
    assert!(result.is_err());

    registry.terminate(&id).await.unwrap();
}

#[tokio::test]
async fn test_agent_registry_trait() {
    let registry = AgentRegistry::new();

    // Test via SubAgentRegistry trait
    let id = registry.spawn_task("trait test task".to_string()).await;

    let list = registry.list_json().await;
    let agents = list.as_array().unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0]["id"], id);

    registry.terminate(&id).await.unwrap();
}

#[tokio::test]
async fn test_agent_auto_complete() {
    let registry = AgentRegistry::new();

    // Spawn an agent that completes immediately
    let id = registry
        .spawn("quick task".to_string(), |_cancel| async {
            // Complete immediately
        })
        .await;

    // Give the task time to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let agent = registry.get(&id).await.unwrap();
    assert_eq!(agent.status, AgentStatus::Completed);

    registry.cleanup().await;
    assert_eq!(registry.list().await.len(), 0);
}
