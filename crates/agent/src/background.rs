//! Background task manager for long-running tool executions
//!
//! Allows tools to be executed in the background while the agent continues.
//! The agent can poll for task status or cancel tasks.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use atta_types::AttaError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::{info, warn};

/// Status of a background task
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum TaskStatus {
    /// Task is currently running
    Running { tool_name: String, elapsed_ms: u64 },
    /// Task completed successfully
    Completed {
        tool_name: String,
        result: Value,
        duration_ms: u64,
    },
    /// Task failed with an error
    Failed {
        tool_name: String,
        error: String,
        duration_ms: u64,
    },
    /// Task was cancelled
    Cancelled { tool_name: String },
    /// Task not found
    NotFound,
}

/// Summary of a background task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSummary {
    pub id: String,
    pub tool_name: String,
    pub is_running: bool,
    pub elapsed_ms: u64,
}

/// Internal state for a background task
#[allow(dead_code)]
struct BackgroundTask {
    id: String,
    tool_name: String,
    handle: JoinHandle<Result<Value, AttaError>>,
    started_at: Instant,
    cancel_token: tokio_util::sync::CancellationToken,
}

/// Result stored after a background task completes
#[derive(Clone)]
enum CompletedResult {
    Ok(Value),
    Err(String),
    Cancelled,
}

/// Manages background tool executions
pub struct BackgroundTaskManager {
    /// Running tasks
    running: Arc<RwLock<HashMap<String, BackgroundTask>>>,
    /// Completed task results (kept for polling): (tool_name, result, duration_ms, completed_at)
    #[allow(clippy::type_complexity)]
    completed: Arc<RwLock<HashMap<String, (String, CompletedResult, u64, Instant)>>>,
    /// Counter for generating task IDs
    counter: Arc<std::sync::atomic::AtomicU64>,
}

impl BackgroundTaskManager {
    /// Create a new background task manager
    pub fn new() -> Self {
        Self {
            running: Arc::new(RwLock::new(HashMap::new())),
            completed: Arc::new(RwLock::new(HashMap::new())),
            counter: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }

    /// Spawn a tool execution as a background task.
    ///
    /// Returns the task ID for status polling.
    pub async fn spawn<F>(&self, tool_name: String, fut: F) -> String
    where
        F: std::future::Future<Output = Result<Value, AttaError>> + Send + 'static,
    {
        // Auto-evict stale completed results (older than 1 hour)
        self.evict_stale(std::time::Duration::from_secs(3600)).await;

        let id = format!(
            "bg-{}",
            self.counter
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let cancel_token = tokio_util::sync::CancellationToken::new();
        let cancel_clone = cancel_token.clone();
        let task_id = id.clone();
        let task_tool_name = tool_name.clone();

        let running_ref = Arc::clone(&self.running);
        let completed_ref = Arc::clone(&self.completed);

        let handle = tokio::spawn(async move {
            let result = tokio::select! {
                r = fut => r,
                _ = cancel_clone.cancelled() => {
                    Err(AttaError::SecurityViolation("background task cancelled".to_string()))
                }
            };

            // Move from running to completed
            let duration_ms = {
                let mut running = running_ref.write().await;
                if let Some(task) = running.remove(&task_id) {
                    task.started_at.elapsed().as_millis() as u64
                } else {
                    0
                }
            };

            let completed_result = match &result {
                Ok(v) => CompletedResult::Ok(v.clone()),
                Err(e) if e.to_string().contains("cancelled") => CompletedResult::Cancelled,
                Err(e) => CompletedResult::Err(e.to_string()),
            };

            {
                let mut completed = completed_ref.write().await;
                completed.insert(
                    task_id.clone(),
                    (task_tool_name, completed_result, duration_ms, Instant::now()),
                );
            }

            result
        });

        info!(id = %id, tool = %tool_name, "spawned background task");

        let task = BackgroundTask {
            id: id.clone(),
            tool_name,
            handle,
            started_at: Instant::now(),
            cancel_token,
        };

        {
            let mut running = self.running.write().await;
            running.insert(id.clone(), task);
        }

        id
    }

    /// Get the status of a background task
    pub async fn status(&self, id: &str) -> TaskStatus {
        // Check running tasks first
        {
            let running = self.running.read().await;
            if let Some(task) = running.get(id) {
                return TaskStatus::Running {
                    tool_name: task.tool_name.clone(),
                    elapsed_ms: task.started_at.elapsed().as_millis() as u64,
                };
            }
        }

        // Check completed tasks
        {
            let completed = self.completed.read().await;
            if let Some((tool_name, result, duration_ms, _completed_at)) = completed.get(id) {
                return match result {
                    CompletedResult::Ok(v) => TaskStatus::Completed {
                        tool_name: tool_name.clone(),
                        result: v.clone(),
                        duration_ms: *duration_ms,
                    },
                    CompletedResult::Err(e) => TaskStatus::Failed {
                        tool_name: tool_name.clone(),
                        error: e.clone(),
                        duration_ms: *duration_ms,
                    },
                    CompletedResult::Cancelled => TaskStatus::Cancelled {
                        tool_name: tool_name.clone(),
                    },
                };
            }
        }

        TaskStatus::NotFound
    }

    /// Cancel a running background task
    pub async fn cancel(&self, id: &str) -> bool {
        let running = self.running.read().await;
        if let Some(task) = running.get(id) {
            task.cancel_token.cancel();
            info!(id = %id, tool = %task.tool_name, "cancelled background task");
            true
        } else {
            warn!(id = %id, "attempted to cancel non-running task");
            false
        }
    }

    /// List all tasks (running and recently completed)
    pub async fn list(&self) -> Vec<TaskSummary> {
        let mut summaries = Vec::new();

        {
            let running = self.running.read().await;
            for task in running.values() {
                summaries.push(TaskSummary {
                    id: task.id.clone(),
                    tool_name: task.tool_name.clone(),
                    is_running: true,
                    elapsed_ms: task.started_at.elapsed().as_millis() as u64,
                });
            }
        }

        {
            let completed = self.completed.read().await;
            for (id, (tool_name, _, duration_ms, _completed_at)) in completed.iter() {
                summaries.push(TaskSummary {
                    id: id.clone(),
                    tool_name: tool_name.clone(),
                    is_running: false,
                    elapsed_ms: *duration_ms,
                });
            }
        }

        summaries
    }

    /// Remove completed task results older than the given TTL.
    ///
    /// Returns the number of evicted entries.
    pub async fn evict_stale(&self, ttl: std::time::Duration) -> usize {
        let mut completed = self.completed.write().await;
        let before = completed.len();
        completed.retain(|_, (_, _, _, completed_at)| completed_at.elapsed() < ttl);
        before - completed.len()
    }
}

impl Default for BackgroundTaskManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_spawn_and_status() {
        let mgr = BackgroundTaskManager::new();
        let id = mgr
            .spawn("test_tool".to_string(), async {
                Ok(serde_json::json!({"result": "done"}))
            })
            .await;

        // Give it time to complete
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let status = mgr.status(&id).await;
        assert!(matches!(status, TaskStatus::Completed { .. }));
        if let TaskStatus::Completed { result, .. } = status {
            assert_eq!(result["result"], "done");
        }
    }

    #[tokio::test]
    async fn test_cancel() {
        let mgr = BackgroundTaskManager::new();
        let id = mgr
            .spawn("slow_tool".to_string(), async {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                Ok(serde_json::json!({"result": "should not reach"}))
            })
            .await;

        // Should be running
        let status = mgr.status(&id).await;
        assert!(matches!(status, TaskStatus::Running { .. }));

        // Cancel it
        assert!(mgr.cancel(&id).await);

        // Wait for cancellation to propagate
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let status = mgr.status(&id).await;
        assert!(matches!(
            status,
            TaskStatus::Cancelled { .. } | TaskStatus::Failed { .. }
        ));
    }

    #[tokio::test]
    async fn test_list() {
        let mgr = BackgroundTaskManager::new();
        let _id1 = mgr
            .spawn("tool_a".to_string(), async {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                Ok(serde_json::json!({}))
            })
            .await;
        let _id2 = mgr
            .spawn("tool_b".to_string(), async { Ok(serde_json::json!({})) })
            .await;

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let list = mgr.list().await;
        assert!(list.len() >= 2);
    }

    #[tokio::test]
    async fn test_not_found() {
        let mgr = BackgroundTaskManager::new();
        let status = mgr.status("nonexistent").await;
        assert!(matches!(status, TaskStatus::NotFound));
    }

    #[tokio::test]
    async fn test_evict_stale() {
        let mgr = BackgroundTaskManager::new();

        // Spawn two tasks that complete immediately
        let id1 = mgr
            .spawn("tool_1".to_string(), async {
                Ok(serde_json::json!({"r": 1}))
            })
            .await;
        let id2 = mgr
            .spawn("tool_2".to_string(), async {
                Ok(serde_json::json!({"r": 2}))
            })
            .await;

        // Wait for them to complete
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Both should be completed
        assert!(matches!(mgr.status(&id1).await, TaskStatus::Completed { .. }));
        assert!(matches!(mgr.status(&id2).await, TaskStatus::Completed { .. }));

        // Evict with a very large TTL — nothing should be evicted
        let evicted = mgr.evict_stale(std::time::Duration::from_secs(3600)).await;
        assert_eq!(evicted, 0);
        assert!(matches!(mgr.status(&id1).await, TaskStatus::Completed { .. }));

        // Evict with a zero TTL — everything should be evicted
        let evicted = mgr.evict_stale(std::time::Duration::ZERO).await;
        assert_eq!(evicted, 2);

        // Both should now be NotFound
        assert!(matches!(mgr.status(&id1).await, TaskStatus::NotFound));
        assert!(matches!(mgr.status(&id2).await, TaskStatus::NotFound));
    }
}
