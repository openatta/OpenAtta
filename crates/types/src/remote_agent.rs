//! 远程 Agent 类型定义
//!
//! 远程 Agent 通过 WebSocket 接入 AttaOS，上报事件，接收指令。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 远程 Agent 注册信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteAgent {
    /// 唯一 ID（`ra_` 前缀 + base58）
    pub id: String,
    /// 名称
    pub name: String,
    /// 描述
    pub description: String,
    /// Agent 版本
    pub version: String,
    /// 能力标签
    pub capabilities: Vec<String>,
    /// 在线状态
    pub status: RemoteAgentStatus,
    /// 最后心跳时间
    pub last_heartbeat: Option<DateTime<Utc>>,
    /// 注册时间
    pub registered_at: DateTime<Utc>,
    /// 注册者 actor_id
    pub registered_by: String,
    /// Token 过期时间（None = 永不过期）
    pub token_expires_at: Option<DateTime<Utc>>,
}

/// 远程 Agent 状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteAgentStatus {
    Online,
    Offline,
}

/// WebSocket 消息帧（上行 + 下行统一格式）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsFrame {
    pub msg_type: String,
    pub msg_id: String,
    pub payload: serde_json::Value,
}

/// 上行消息：远程 Agent → AttaOS
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "msg_type", rename_all = "snake_case")]
pub enum UpstreamMsg {
    /// 注册
    Register {
        msg_id: String,
        agent_name: String,
        agent_version: String,
        #[serde(default)]
        description: String,
        #[serde(default)]
        capabilities: Vec<String>,
    },
    /// 事件批量上报
    EventBatch {
        msg_id: String,
        events: Vec<RemoteEvent>,
    },
    /// 注销
    Deregister {
        msg_id: String,
        #[serde(default)]
        reason: String,
    },
}

/// 远程 Agent 上报的事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteEvent {
    /// 事件类型（如 `agent.task.started`）
    pub event_type: String,
    /// 事件时间
    pub timestamp: DateTime<Utc>,
    /// 关联 ID（用于追踪链路）
    #[serde(default)]
    pub correlation_id: Option<String>,
    /// 事件负载
    #[serde(default)]
    pub payload: serde_json::Value,
}

/// 下行消息：AttaOS → 远程 Agent
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "msg_type", rename_all = "snake_case")]
pub enum DownstreamMsg {
    /// 注册确认
    Registered {
        msg_id: String,
        agent_id: String,
    },
    /// 紧急停止
    Estop {
        msg_id: String,
        reason: String,
        #[serde(default = "default_scope")]
        scope: String,
    },
    /// 安全策略更新
    PolicyUpdate {
        msg_id: String,
        policy: serde_json::Value,
    },
    /// 资源变更通知
    ResourceUpdated {
        msg_id: String,
        resource_type: String,
        id: String,
    },
    /// 事件确认
    Ack {
        msg_id: String,
    },
    /// 错误
    Error {
        msg_id: String,
        error: String,
    },
}

fn default_scope() -> String {
    "all".to_string()
}

/// 注册远程 Agent 的请求（REST API 用）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRemoteAgentRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Token TTL in hours (default: 720 = 30 days). None = never expires.
    pub token_ttl_hours: Option<u32>,
}

/// 注册响应（包含明文 token，仅此一次）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRemoteAgentResponse {
    pub agent_id: String,
    pub token: String,
    pub registered_at: DateTime<Utc>,
    pub token_expires_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_msg_register_serde() {
        let msg = UpstreamMsg::Register {
            msg_id: "uuid-1".to_string(),
            agent_name: "test-agent".to_string(),
            agent_version: "1.0.0".to_string(),
            description: "A test agent".to_string(),
            capabilities: vec!["prd".to_string()],
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: UpstreamMsg = serde_json::from_str(&json).unwrap();
        match back {
            UpstreamMsg::Register { agent_name, .. } => {
                assert_eq!(agent_name, "test-agent");
            }
            _ => panic!("expected Register"),
        }
    }

    #[test]
    fn downstream_msg_estop_serde() {
        let msg = DownstreamMsg::Estop {
            msg_id: "uuid-2".to_string(),
            reason: "admin triggered".to_string(),
            scope: "all".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: DownstreamMsg = serde_json::from_str(&json).unwrap();
        match back {
            DownstreamMsg::Estop { reason, .. } => {
                assert_eq!(reason, "admin triggered");
            }
            _ => panic!("expected Estop"),
        }
    }

    #[test]
    fn remote_event_serde() {
        let event = RemoteEvent {
            event_type: "agent.task.started".to_string(),
            timestamp: Utc::now(),
            correlation_id: Some("corr-1".to_string()),
            payload: serde_json::json!({"task_id": "t-1"}),
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: RemoteEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.event_type, "agent.task.started");
    }

    #[test]
    fn remote_agent_status_serde() {
        let json = serde_json::to_string(&RemoteAgentStatus::Online).unwrap();
        assert_eq!(json, r#""online""#);
        let back: RemoteAgentStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, RemoteAgentStatus::Online);
    }

    #[test]
    fn test_remote_agent_with_token_expiry() {
        let agent = RemoteAgent {
            id: "ra_test".to_string(),
            name: "test".to_string(),
            description: String::new(),
            version: "0.1.0".to_string(),
            capabilities: vec!["data".to_string()],
            status: RemoteAgentStatus::Online,
            last_heartbeat: Some(chrono::Utc::now()),
            registered_at: chrono::Utc::now(),
            registered_by: "owner".to_string(),
            token_expires_at: Some(chrono::Utc::now() + chrono::Duration::hours(24)),
        };
        let json = serde_json::to_string(&agent).unwrap();
        let back: RemoteAgent = serde_json::from_str(&json).unwrap();
        assert!(back.token_expires_at.is_some());
        assert_eq!(back.status, RemoteAgentStatus::Online);
    }

    #[test]
    fn test_register_request_with_ttl() {
        let json = r#"{"name": "agent1", "capabilities": [], "token_ttl_hours": 720}"#;
        let req: RegisterRemoteAgentRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.token_ttl_hours, Some(720));
    }

    #[test]
    fn test_register_request_without_ttl() {
        let json = r#"{"name": "agent1"}"#;
        let req: RegisterRemoteAgentRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.token_ttl_hours, None);
    }
}
