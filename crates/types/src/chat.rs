//! Client ↔ Server 共享的 Chat 类型
//!
//! `ChatRequest` 由客户端（attacli / attash）发送，
//! `ChatEvent` 由服务端通过 SSE 流式返回。

use serde::{Deserialize, Serialize};

/// 客户端发送的聊天请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    /// 用户消息内容
    pub message: String,

    /// 可选的 Skill ID（指定使用哪个 Skill 处理）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_id: Option<String>,

    /// 可选的 Flow ID（直接启动指定 Flow，跳过意图识别）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow_id: Option<String>,

    /// 可选的 Task ID（绑定到已有 Task，用于 Flow 中 Gate 审批等交互）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

/// 服务端通过 SSE 返回的聊天事件
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ChatEvent {
    /// Agent 正在思考
    #[serde(rename = "thinking")]
    Thinking { iteration: u32 },

    /// 文本增量
    #[serde(rename = "text_delta")]
    TextDelta { delta: String },

    /// 工具开始执行
    #[serde(rename = "tool_start")]
    ToolStart { tool_name: String, call_id: String },

    /// 工具执行完成
    #[serde(rename = "tool_complete")]
    ToolComplete {
        tool_name: String,
        call_id: String,
        duration_ms: u64,
    },

    /// 工具执行失败
    #[serde(rename = "tool_error")]
    ToolError {
        tool_name: String,
        call_id: String,
        error: String,
    },

    /// 执行完成
    #[serde(rename = "done")]
    Done { iterations: u32 },

    /// 错误
    #[serde(rename = "error")]
    Error { message: String },
}
