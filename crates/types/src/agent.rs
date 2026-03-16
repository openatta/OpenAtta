//! Agent 相关类型

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::node::ResourceLimits;
use crate::skill::SkillDef;
use crate::tool::ToolDef;

/// Agent 执行规格
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpec {
    pub task_id: Uuid,
    pub agent_type: String,
    pub skill: SkillDef,
    pub tools: Vec<ToolDef>,
    pub resource_limits: ResourceLimits,
}

/// Agent 运行状态
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Created,
    Starting,
    Running,
    Completing,
    Completed { output: serde_json::Value },
    Error { error: String, retries: u32 },
    Retrying { attempt: u32 },
    Timeout,
    Cancelled,
}

/// Agent 间通信事件
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum AgentEvent {
    Output {
        task_id: Uuid,
        agent_type: String,
        data: serde_json::Value,
    },
    AssistRequest {
        from_task_id: Uuid,
        requested_agent_type: String,
        input: serde_json::Value,
    },
    StatusChanged {
        task_id: Uuid,
        status: AgentStatus,
    },
}
