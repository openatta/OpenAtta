//! Remote Agent Hub — 管理远程 Agent 的 WebSocket 连接
//!
//! 跟踪在线远程 Agent，支持向特定 Agent 或全体推送下行消息。

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, warn};

use atta_types::{DownstreamMsg, RemoteAgent};

/// 单个远程 Agent 的连接句柄
struct AgentConnection {
    sender: mpsc::UnboundedSender<String>,
    agent: RemoteAgent,
    #[allow(dead_code)]
    connected_at: DateTime<Utc>,
}

/// 远程 Agent 连接管理中心
pub struct RemoteAgentHub {
    connections: Arc<RwLock<HashMap<String, AgentConnection>>>,
}

impl RemoteAgentHub {
    /// 创建空 Hub
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 注册一个远程 Agent 连接，返回消息接收器
    pub async fn add_connection(
        &self,
        agent: RemoteAgent,
    ) -> mpsc::UnboundedReceiver<String> {
        let id = agent.id.clone();
        let (tx, rx) = mpsc::unbounded_channel();
        let conn = AgentConnection {
            sender: tx,
            agent,
            connected_at: Utc::now(),
        };
        self.connections.write().await.insert(id.clone(), conn);
        debug!(agent_id = %id, "remote agent connected");
        rx
    }

    /// 移除连接
    pub async fn remove_connection(&self, agent_id: &str) {
        self.connections.write().await.remove(agent_id);
        debug!(agent_id = %agent_id, "remote agent disconnected");
    }

    /// 向指定 Agent 发送下行消息
    pub async fn send_to(&self, agent_id: &str, msg: &DownstreamMsg) {
        let json = match serde_json::to_string(msg) {
            Ok(j) => j,
            Err(e) => {
                warn!(error = %e, "failed to serialize downstream message");
                return;
            }
        };

        let conns = self.connections.read().await;
        if let Some(conn) = conns.get(agent_id) {
            if conn.sender.send(json).is_err() {
                debug!(agent_id = %agent_id, "send to remote agent failed (disconnected)");
            }
        }
    }

    /// 向所有在线 Agent 广播紧急停止
    pub async fn broadcast_estop(&self, reason: &str) {
        let msg = DownstreamMsg::Estop {
            msg_id: uuid::Uuid::new_v4().to_string(),
            reason: reason.to_string(),
            scope: "all".to_string(),
        };
        let json = match serde_json::to_string(&msg) {
            Ok(j) => j,
            Err(e) => {
                warn!(error = %e, "failed to serialize estop message");
                return;
            }
        };

        let conns = self.connections.read().await;
        for (id, conn) in conns.iter() {
            if conn.sender.send(json.clone()).is_err() {
                debug!(agent_id = %id, "estop broadcast failed (disconnected)");
            }
        }
    }

    /// 列出所有在线的远程 Agent
    pub async fn list_online(&self) -> Vec<RemoteAgent> {
        self.connections
            .read()
            .await
            .values()
            .map(|c| c.agent.clone())
            .collect()
    }

    /// 检查指定 Agent 是否在线
    pub async fn is_online(&self, agent_id: &str) -> bool {
        self.connections.read().await.contains_key(agent_id)
    }

    /// 在线 Agent 数量
    pub async fn online_count(&self) -> usize {
        self.connections.read().await.len()
    }
}

impl Default for RemoteAgentHub {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::RemoteAgentStatus;

    fn test_agent(id: &str) -> RemoteAgent {
        RemoteAgent {
            id: id.to_string(),
            name: format!("test-{id}"),
            description: String::new(),
            version: "0.1.0".to_string(),
            capabilities: vec![],
            status: RemoteAgentStatus::Online,
            last_heartbeat: None,
            registered_at: Utc::now(),
            registered_by: "test".to_string(),
            token_expires_at: None,
        }
    }

    #[tokio::test]
    async fn test_add_and_remove_connection() {
        let hub = RemoteAgentHub::new();
        let agent = test_agent("a1");
        let _rx = hub.add_connection(agent).await;
        assert_eq!(hub.online_count().await, 1);
        assert!(hub.is_online("a1").await);

        hub.remove_connection("a1").await;
        assert_eq!(hub.online_count().await, 0);
        assert!(!hub.is_online("a1").await);
    }

    #[tokio::test]
    async fn test_list_online() {
        let hub = RemoteAgentHub::new();
        let _rx1 = hub.add_connection(test_agent("a1")).await;
        let _rx2 = hub.add_connection(test_agent("a2")).await;

        let online = hub.list_online().await;
        assert_eq!(online.len(), 2);
    }

    #[tokio::test]
    async fn test_send_to() {
        let hub = RemoteAgentHub::new();
        let mut rx = hub.add_connection(test_agent("a1")).await;

        let msg = DownstreamMsg::Ack {
            msg_id: "test".to_string(),
        };
        hub.send_to("a1", &msg).await;

        let received = rx.recv().await.unwrap();
        assert!(received.contains("ack"));
    }

    #[tokio::test]
    async fn test_broadcast_estop() {
        let hub = RemoteAgentHub::new();
        let mut rx1 = hub.add_connection(test_agent("a1")).await;
        let mut rx2 = hub.add_connection(test_agent("a2")).await;

        hub.broadcast_estop("test reason").await;

        let msg1 = rx1.recv().await.unwrap();
        let msg2 = rx2.recv().await.unwrap();
        assert!(msg1.contains("estop"));
        assert!(msg2.contains("test reason"));
    }

    #[tokio::test]
    async fn test_send_to_nonexistent_agent() {
        let hub = RemoteAgentHub::new();
        // Should not panic
        let msg = DownstreamMsg::Ack {
            msg_id: "test".to_string(),
        };
        hub.send_to("nonexistent", &msg).await;
    }

    #[tokio::test]
    async fn test_remove_nonexistent_connection() {
        let hub = RemoteAgentHub::new();
        // Should not panic
        hub.remove_connection("nonexistent").await;
        assert_eq!(hub.online_count().await, 0);
    }
}
