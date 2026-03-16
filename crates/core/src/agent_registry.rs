//! Agent Registry
//!
//! 跟踪和管理运行中的 sub-agent 实例。
//! 提供 spawn/list/pause/resume/terminate 操作。

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

/// Agent 句柄，用于跟踪和管理单个 agent
struct AgentHandle {
    /// Agent 唯一 ID
    id: String,
    /// 任务描述
    task: String,
    /// 当前状态
    status: AgentStatus,
    /// 创建时间
    created_at: DateTime<Utc>,
    /// 取消令牌
    cancel_token: CancellationToken,
    /// 异步任务句柄
    join_handle: Option<JoinHandle<()>>,
}

/// Agent 状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Running,
    Paused,
    Completed,
    Failed,
    Terminated,
}

/// Agent 摘要信息（对外暴露）
#[derive(Debug, Clone, Serialize)]
pub struct AgentSummary {
    pub id: String,
    pub task: String,
    pub status: AgentStatus,
    pub created_at: DateTime<Utc>,
    pub elapsed_ms: u64,
}

/// Agent Registry — 管理运行中的 sub-agent
pub struct AgentRegistry {
    agents: Arc<Mutex<HashMap<String, AgentHandle>>>,
}

impl AgentRegistry {
    /// 创建新的 Agent Registry
    pub fn new() -> Self {
        Self {
            agents: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Spawn 一个新的 sub-agent
    ///
    /// 返回 agent ID。caller 提供一个 async closure 作为 agent 的执行体。
    pub async fn spawn<F, Fut>(&self, task: String, f: F) -> String
    where
        F: FnOnce(CancellationToken) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let id = uuid::Uuid::new_v4().to_string();
        let cancel_token = CancellationToken::new();
        let token_clone = cancel_token.clone();

        let agents = self.agents.clone();
        let id_clone = id.clone();

        let join_handle = tokio::spawn(async move {
            f(token_clone).await;
            // Mark as completed when done
            let mut map = agents.lock().await;
            if let Some(handle) = map.get_mut(&id_clone) {
                if handle.status == AgentStatus::Running {
                    handle.status = AgentStatus::Completed;
                }
            }
        });

        let handle = AgentHandle {
            id: id.clone(),
            task,
            status: AgentStatus::Running,
            created_at: Utc::now(),
            cancel_token,
            join_handle: Some(join_handle),
        };

        self.agents.lock().await.insert(id.clone(), handle);
        info!(agent_id = %id, "sub-agent spawned");

        id
    }

    /// 列出所有 agent
    pub async fn list(&self) -> Vec<AgentSummary> {
        let agents = self.agents.lock().await;
        let now = Utc::now();
        agents
            .values()
            .map(|h| AgentSummary {
                id: h.id.clone(),
                task: h.task.clone(),
                status: h.status.clone(),
                created_at: h.created_at,
                elapsed_ms: (now - h.created_at).num_milliseconds().max(0) as u64,
            })
            .collect()
    }

    /// 获取单个 agent 信息
    pub async fn get(&self, id: &str) -> Option<AgentSummary> {
        let agents = self.agents.lock().await;
        let now = Utc::now();
        agents.get(id).map(|h| AgentSummary {
            id: h.id.clone(),
            task: h.task.clone(),
            status: h.status.clone(),
            created_at: h.created_at,
            elapsed_ms: (now - h.created_at).num_milliseconds().max(0) as u64,
        })
    }

    /// 终止 agent
    pub async fn terminate(&self, id: &str) -> Result<(), String> {
        let mut agents = self.agents.lock().await;
        if let Some(handle) = agents.get_mut(id) {
            handle.cancel_token.cancel();
            handle.status = AgentStatus::Terminated;
            if let Some(jh) = handle.join_handle.take() {
                jh.abort();
            }
            info!(agent_id = %id, "sub-agent terminated");
            Ok(())
        } else {
            Err(format!("agent '{}' not found", id))
        }
    }

    /// 暂停 agent（标记状态，实际暂停需要 agent 协作检查）
    pub async fn pause(&self, id: &str) -> Result<(), String> {
        let mut agents = self.agents.lock().await;
        if let Some(handle) = agents.get_mut(id) {
            if handle.status == AgentStatus::Running {
                handle.status = AgentStatus::Paused;
                debug!(agent_id = %id, "sub-agent paused");
                Ok(())
            } else {
                Err(format!("agent '{}' is not running", id))
            }
        } else {
            Err(format!("agent '{}' not found", id))
        }
    }

    /// 恢复 agent
    pub async fn resume(&self, id: &str) -> Result<(), String> {
        let mut agents = self.agents.lock().await;
        if let Some(handle) = agents.get_mut(id) {
            if handle.status == AgentStatus::Paused {
                handle.status = AgentStatus::Running;
                debug!(agent_id = %id, "sub-agent resumed");
                Ok(())
            } else {
                Err(format!("agent '{}' is not paused", id))
            }
        } else {
            Err(format!("agent '{}' not found", id))
        }
    }

    /// 清理已完成/已终止的 agent
    pub async fn cleanup(&self) {
        let mut agents = self.agents.lock().await;
        agents.retain(|_, h| matches!(h.status, AgentStatus::Running | AgentStatus::Paused));
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Implement the SubAgentRegistry trait from atta-types for integration
#[async_trait::async_trait]
impl atta_types::SubAgentRegistry for AgentRegistry {
    async fn spawn_task(&self, task: String) -> String {
        self.spawn(task, |cancel| async move {
            cancel.cancelled().await;
        })
        .await
    }

    async fn list_json(&self) -> serde_json::Value {
        let agents = self.list().await;
        serde_json::to_value(agents).unwrap_or_default()
    }

    async fn pause(&self, id: &str) -> Result<(), String> {
        AgentRegistry::pause(self, id).await
    }

    async fn resume(&self, id: &str) -> Result<(), String> {
        AgentRegistry::resume(self, id).await
    }

    async fn terminate(&self, id: &str) -> Result<(), String> {
        AgentRegistry::terminate(self, id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_spawn_and_list() {
        let registry = AgentRegistry::new();
        let id = registry
            .spawn("test task".to_string(), |_cancel| async {
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
            })
            .await;

        let agents = registry.list().await;
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].id, id);
        assert_eq!(agents[0].status, AgentStatus::Running);

        // Cleanup
        registry.terminate(&id).await.unwrap();
    }

    #[tokio::test]
    async fn test_terminate() {
        let registry = AgentRegistry::new();
        let id = registry
            .spawn("test".to_string(), |cancel| async move {
                cancel.cancelled().await;
            })
            .await;

        registry.terminate(&id).await.unwrap();

        let agent = registry.get(&id).await.unwrap();
        assert_eq!(agent.status, AgentStatus::Terminated);
    }

    #[tokio::test]
    async fn test_pause_resume() {
        let registry = AgentRegistry::new();
        let id = registry
            .spawn("test".to_string(), |_| async {
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
            })
            .await;

        registry.pause(&id).await.unwrap();
        assert_eq!(registry.get(&id).await.unwrap().status, AgentStatus::Paused);

        registry.resume(&id).await.unwrap();
        assert_eq!(
            registry.get(&id).await.unwrap().status,
            AgentStatus::Running
        );

        registry.terminate(&id).await.unwrap();
    }
}
