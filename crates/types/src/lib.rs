//! AttaOS 共享类型定义
//!
//! 所有 crate 共享的类型、枚举、错误定义。

pub mod agent;
pub mod agent_trait;
pub mod approval;
pub mod auth;
pub mod chat;
pub mod cron;
pub mod cron_trait;
pub mod error;
pub mod event;
pub mod flow;
pub mod flow_runner;
pub mod id;
pub mod mcp;
pub mod memory;
pub mod native_tool;
pub mod node;
pub mod package;
pub mod plugin;
pub mod serde_util;
pub mod skill;
pub mod task;
pub mod tool;
pub mod remote_agent;
pub mod usage;

pub use agent::{AgentEvent, AgentSpec, AgentStatus};
pub use agent_trait::SubAgentRegistry;
pub use approval::{ApprovalContext, ApprovalFilter, ApprovalRequest, ApprovalStatus};
pub use auth::{Action, Actor, ActorType, AuthzDecision, Resource, ResourceType, Role};
pub use chat::{ChatEvent, ChatRequest};
pub use cron::{CronJob, CronRun, CronRunStatus};
pub use cron_trait::CronScheduler;
pub use error::{AgentError, AttaError, LlmError, RuntimeError};
pub use event::{EntityRef, EventEnvelope};
pub use flow::{
    BranchDef, CondExpr, ErrorPolicy, GateDef, JoinStrategy, NotifyChannel, OnEnterAction,
};
pub use flow::{FlowDef, FlowState, StateDef, StateTransition, StateType, TransitionDef};
pub use mcp::{McpServerConfig, McpTransport};
pub use memory::MemoryType;
pub use native_tool::NativeTool;
pub use node::{ExecutionStatus, NodeCapacity, NodeInfo, NodeStatus, ResourceLimits};
pub use package::{
    InstalledPackage, Manifest, PackageRecord, PackageSource, PackageType, ResolvedDep,
    ServiceAccount,
};
pub use plugin::{PluginManifest, PluginSpec};
pub use skill::SkillDef;
pub use task::{Task, TaskFilter, TaskStatus};
pub use tool::{RiskLevel, ToolBinding, ToolDef, ToolRegistry, ToolSchema};
pub use usage::{ModelUsage, TokenUsage, UsageDaily, UsageRecord, UsageSummary};
pub use flow_runner::FlowRunner;
pub use remote_agent::{
    DownstreamMsg, RegisterRemoteAgentRequest, RegisterRemoteAgentResponse, RemoteAgent,
    RemoteAgentStatus, RemoteEvent, UpstreamMsg, WsFrame,
};
