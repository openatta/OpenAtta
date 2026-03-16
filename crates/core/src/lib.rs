//! AttaOS 控制面核心
//!
//! 本 crate 包含 AttaOS 的控制面组件：
//!
//! - [`FlowEngine`] — Flow 状态机引擎，管理 FlowDef 模板并驱动 Task 状态推进
//! - [`ConditionEvaluator`] — 条件求值器，解析和求值 Flow 转移条件表达式
//! - [`DefaultToolRegistry`] — ToolRegistry 默认实现
//! - [`server`] — axum HTTP 服务器骨架（REST API 路由和 handler）
//!
//! # 架构
//!
//! Core 层位于分层模型的中心：
//! ```text
//! Client → Core → Flow → Agent → Skill → Tool → MCP
//! ```
//!
//! Core 通过 4 个核心 trait（EventBus / StateStore / Authz / AuditSink）
//! 与基础设施层交互，实现 Desktop/Enterprise 双版本切换。

pub mod agent_registry;
pub mod builtin_tools;
pub mod channel_handler;
pub mod condition;
pub mod coordinator;
pub mod cron_engine;
pub mod flow_engine;
pub mod log_broadcast;
pub mod middleware;
pub mod remote_agent_hub;
pub mod server;
pub mod skill_engine;
pub mod tool_registry;
pub mod usage_tracking;
pub mod webui;
pub mod ws_hub;

pub use condition::ConditionEvaluator;
pub use flow_engine::FlowEngine;
pub use server::{api_router, AppState};
pub use tool_registry::DefaultToolRegistry;

// Re-export ToolRegistry trait from atta-types for convenience
pub use atta_types::ToolRegistry;
