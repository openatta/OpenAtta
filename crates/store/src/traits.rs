//! StateStore trait 定义
//!
//! 状态存储的统一抽象。Desktop 使用 SqliteStore，Enterprise 使用 PostgresStore。
//! 所有状态变更通过此 trait 进行持久化。
//!
//! StateStore 是一个 super-trait，由以下子 trait 组合而成：
//! - [`TaskStore`] — 任务 CRUD 及状态合并
//! - [`FlowStore`] — Flow 定义与运行状态
//! - [`RegistryStore`] — Plugin / Tool / Skill 注册
//! - [`PackageStore`] — 包管理
//! - [`ServiceAccountStore`] — Service Account 查询
//! - [`NodeStore`] — 节点管理（Enterprise）
//! - [`ApprovalStore`] — 审批管理
//! - [`McpStore`] — MCP Server 注册
//! - [`CronStore`] — Cron 调度
//! - [`RbacStore`] — 角色绑定（RBAC）
//! - [`UsageStore`] — LLM 用量追踪

use chrono::{DateTime, Utc};
use uuid::Uuid;

use atta_types::package::ServiceAccount;
use atta_types::usage::{UsageDaily, UsageRecord, UsageSummary};
use atta_types::{
    Actor, ApprovalFilter, ApprovalRequest, ApprovalStatus, AttaError, CronJob, CronRun, FlowDef,
    FlowState, McpServerConfig, NodeInfo, NodeStatus, PackageRecord, PluginManifest, SkillDef,
    StateTransition, Task, TaskFilter, TaskStatus, ToolDef,
};

// ── Task CRUD ──

/// 任务存储抽象
///
/// 负责任务的创建、查询、状态更新等 CRUD 操作，
/// 以及 state_data 的 JSON merge patch。
#[async_trait::async_trait]
pub trait TaskStore: Send + Sync {
    /// 创建新任务
    async fn create_task(&self, task: &Task) -> Result<(), AttaError>;

    /// 根据 ID 查询任务
    async fn get_task(&self, id: &Uuid) -> Result<Option<Task>, AttaError>;

    /// 更新任务状态
    async fn update_task_status(&self, id: &Uuid, status: TaskStatus) -> Result<(), AttaError>;

    /// 按条件列出任务
    async fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, AttaError>;

    /// 合并 task 的 state_data（JSON merge patch）
    async fn merge_task_state_data(
        &self,
        task_id: &Uuid,
        data: serde_json::Value,
    ) -> Result<(), AttaError>;
}

// ── Flow ──

/// Flow 存储抽象
///
/// 负责 Flow 定义模板和 Flow 运行状态的持久化，
/// 以及创建任务并初始化 Flow 状态、推进任务状态等复合事务操作。
#[async_trait::async_trait]
pub trait FlowStore: Send + Sync {
    /// 保存 Flow 定义模板
    async fn save_flow_def(&self, flow: &FlowDef) -> Result<(), AttaError>;

    /// 根据 ID 查询 Flow 定义
    async fn get_flow_def(&self, id: &str) -> Result<Option<FlowDef>, AttaError>;

    /// 保存 Flow 运行状态
    async fn save_flow_state(&self, task_id: &Uuid, state: &FlowState) -> Result<(), AttaError>;

    /// 根据 task_id 查询 Flow 运行状态
    async fn get_flow_state(&self, task_id: &Uuid) -> Result<Option<FlowState>, AttaError>;

    /// 列出所有 Flow 定义
    async fn list_flow_defs(&self) -> Result<Vec<FlowDef>, AttaError>;

    /// 删除 Flow 定义
    async fn delete_flow_def(&self, id: &str) -> Result<(), AttaError>;

    /// 列出所有 Skill 定义
    async fn list_skill_defs(&self) -> Result<Vec<SkillDef>, AttaError>;

    /// 创建任务并初始化 Flow 状态（事务）
    async fn create_task_with_flow(
        &self,
        task: &Task,
        flow_state: &FlowState,
    ) -> Result<(), AttaError>;

    /// 推进任务状态（更新 task status + flow state + 记录 transition，事务）
    ///
    /// `expected_version` enables optimistic locking: if the current version in
    /// the store does not match, the call returns `AttaError::Conflict`.
    async fn advance_task(
        &self,
        task_id: &Uuid,
        new_status: TaskStatus,
        new_state: &str,
        transition: &StateTransition,
        expected_version: u64,
    ) -> Result<(), AttaError>;
}

// ── Plugin/Tool/Skill 注册 ──

/// 注册表存储抽象
///
/// 负责 Plugin、Tool、Skill 的注册、注销和查询。
#[async_trait::async_trait]
pub trait RegistryStore: Send + Sync {
    /// 注册插件
    async fn register_plugin(&self, manifest: &PluginManifest) -> Result<(), AttaError>;

    /// 注销插件
    async fn unregister_plugin(&self, name: &str) -> Result<(), AttaError>;

    /// 列出所有已注册插件
    async fn list_plugins(&self) -> Result<Vec<PluginManifest>, AttaError>;

    /// 注册 Tool
    async fn register_tool(&self, tool: &ToolDef) -> Result<(), AttaError>;

    /// 列出所有已注册 Tool
    async fn list_tools(&self) -> Result<Vec<ToolDef>, AttaError>;

    /// 注册 Skill
    async fn register_skill(&self, skill: &SkillDef) -> Result<(), AttaError>;

    /// 列出所有已注册 Skill
    async fn list_skills(&self) -> Result<Vec<SkillDef>, AttaError>;

    /// 根据名称查询 Tool
    async fn get_tool(&self, name: &str) -> Result<Option<ToolDef>, AttaError>;

    /// 根据 ID 查询 Skill
    async fn get_skill(&self, id: &str) -> Result<Option<SkillDef>, AttaError>;

    /// 根据名称查询 Plugin
    async fn get_plugin(&self, name: &str) -> Result<Option<PluginManifest>, AttaError>;

    /// 删除 Skill
    async fn delete_skill(&self, id: &str) -> Result<(), AttaError>;
}

// ── 包管理 ──

/// 包存储抽象
///
/// 负责已安装包的注册和查询。
#[async_trait::async_trait]
pub trait PackageStore: Send + Sync {
    /// 注册已安装包
    async fn register_package(&self, pkg: &PackageRecord) -> Result<(), AttaError>;

    /// 根据名称查询已安装包
    async fn get_package(&self, name: &str) -> Result<Option<PackageRecord>, AttaError>;
}

// ── Service Account ──

/// Service Account 存储抽象
///
/// 负责根据 API Key Hash 查询 Service Account。
#[async_trait::async_trait]
pub trait ServiceAccountStore: Send + Sync {
    /// 根据 API Key Hash 查询 Service Account
    async fn get_service_account_by_key(
        &self,
        api_key_hash: &str,
    ) -> Result<Option<ServiceAccount>, AttaError>;
}

// ── Node 管理（Enterprise） ──

/// 节点存储抽象
///
/// 负责节点信息的增删改查，主要用于 Enterprise 版的多节点管理。
#[async_trait::async_trait]
pub trait NodeStore: Send + Sync {
    /// 更新/插入节点信息（心跳时调用）
    async fn upsert_node(&self, node_info: &NodeInfo) -> Result<(), AttaError>;

    /// 根据 ID 查询节点信息
    async fn get_node(&self, node_id: &str) -> Result<Option<NodeInfo>, AttaError>;

    /// 列出所有节点
    async fn list_nodes(&self) -> Result<Vec<NodeInfo>, AttaError>;

    /// 列出指定时间之后有心跳的节点
    async fn list_nodes_after(&self, cutoff: DateTime<Utc>) -> Result<Vec<NodeInfo>, AttaError>;

    /// 更新节点状态
    async fn update_node_status(&self, node_id: &str, status: NodeStatus) -> Result<(), AttaError>;
}

// ── 审批管理 ──

/// 审批存储抽象
///
/// 负责审批请求的保存、查询和状态更新。
#[async_trait::async_trait]
pub trait ApprovalStore: Send + Sync {
    /// 保存审批请求
    async fn save_approval(&self, approval: &ApprovalRequest) -> Result<(), AttaError>;

    /// 根据 ID 查询审批请求
    async fn get_approval(&self, id: &Uuid) -> Result<Option<ApprovalRequest>, AttaError>;

    /// 按条件列出审批请求
    async fn list_approvals(
        &self,
        filter: &ApprovalFilter,
    ) -> Result<Vec<ApprovalRequest>, AttaError>;

    /// 更新审批状态
    async fn update_approval_status(
        &self,
        id: &Uuid,
        status: ApprovalStatus,
        resolved_by: &Actor,
        comment: Option<&str>,
    ) -> Result<(), AttaError>;
}

// ── MCP 注册 ──

/// MCP Server 存储抽象
///
/// 负责 MCP Server 配置的注册和查询。
#[async_trait::async_trait]
pub trait McpStore: Send + Sync {
    /// 注册 MCP Server
    async fn register_mcp(&self, mcp_def: &McpServerConfig) -> Result<(), AttaError>;

    /// 列出所有已注册 MCP Server
    async fn list_mcp_servers(&self) -> Result<Vec<McpServerConfig>, AttaError>;

    /// 注销 MCP Server
    async fn unregister_mcp(&self, name: &str) -> Result<(), AttaError>;
}

// ── Cron 调度 ──

/// Cron 调度存储抽象
///
/// 负责 cron job 和 cron run 记录的增删改查。
#[async_trait::async_trait]
pub trait CronStore: Send + Sync {
    /// 保存 cron job
    async fn save_cron_job(&self, job: &CronJob) -> Result<(), AttaError>;

    /// 获取 cron job
    async fn get_cron_job(&self, id: &str) -> Result<Option<CronJob>, AttaError>;

    /// 列出 cron jobs
    async fn list_cron_jobs(&self, status: Option<&str>) -> Result<Vec<CronJob>, AttaError>;

    /// 删除 cron job
    async fn delete_cron_job(&self, id: &str) -> Result<(), AttaError>;

    /// 保存 cron run 记录
    async fn save_cron_run(&self, run: &CronRun) -> Result<(), AttaError>;

    /// 获取 cron job 的运行记录
    async fn list_cron_runs(&self, job_id: &str, limit: usize) -> Result<Vec<CronRun>, AttaError>;
}

// ── 角色绑定（RBAC） ──

/// RBAC 存储抽象
///
/// 负责 actor 角色的查询、绑定和解绑。
#[async_trait::async_trait]
pub trait RbacStore: Send + Sync {
    /// 查询 actor 的角色列表
    async fn get_roles_for_actor(&self, actor_id: &str)
        -> Result<Vec<atta_types::Role>, AttaError>;

    /// 绑定角色到 actor
    async fn bind_role(&self, actor_id: &str, role: &atta_types::Role) -> Result<(), AttaError>;

    /// 解绑 actor 的角色
    async fn unbind_role(&self, actor_id: &str, role: &atta_types::Role) -> Result<(), AttaError>;
}

// ── Usage 追踪 ──

/// Usage 存储抽象
///
/// 负责 LLM API 调用的 token 用量和费用记录、汇总查询。
#[async_trait::async_trait]
pub trait UsageStore: Send + Sync {
    /// 记录一条 usage
    async fn record_usage(&self, record: &UsageRecord) -> Result<(), AttaError>;

    /// 获取指定时间段的汇总
    async fn get_usage_summary(&self, since: DateTime<Utc>) -> Result<UsageSummary, AttaError>;

    /// 获取按日聚合的 usage
    async fn get_usage_daily(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<UsageDaily>, AttaError>;
}

// ── Remote Agent 存储 ──

/// Remote Agent 存储抽象
///
/// 负责远程 Agent 注册信息的 CRUD。
#[async_trait::async_trait]
pub trait RemoteAgentStore: Send + Sync {
    /// 注册远程 Agent
    async fn register_remote_agent(
        &self,
        agent: &atta_types::RemoteAgent,
        token_hash: &str,
    ) -> Result<(), AttaError>;

    /// 按 ID 查询远程 Agent
    async fn get_remote_agent(&self, id: &str) -> Result<Option<atta_types::RemoteAgent>, AttaError>;

    /// 按 token 哈希查询远程 Agent
    async fn get_remote_agent_by_token(
        &self,
        token_hash: &str,
    ) -> Result<Option<atta_types::RemoteAgent>, AttaError>;

    /// 列出所有远程 Agent
    async fn list_remote_agents(&self) -> Result<Vec<atta_types::RemoteAgent>, AttaError>;

    /// 更新远程 Agent 状态
    async fn update_remote_agent_status(
        &self,
        id: &str,
        status: &atta_types::RemoteAgentStatus,
    ) -> Result<(), AttaError>;

    /// 更新心跳时间
    async fn update_remote_agent_heartbeat(&self, id: &str) -> Result<(), AttaError>;

    /// 删除远程 Agent
    async fn delete_remote_agent(&self, id: &str) -> Result<(), AttaError>;

    /// Rotate a remote agent's token
    async fn rotate_remote_agent_token(
        &self,
        id: &str,
        new_token_hash: &str,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<(), AttaError>;
}

// ── StateStore super-trait ──

/// 状态存储抽象（super-trait）
///
/// 所有数据持久化操作的统一接口。Desktop 版使用 SQLite 实现，
/// Enterprise 版使用 PostgreSQL 实现。
///
/// 此 trait 组合了所有子 trait，使得 `Arc<dyn StateStore>` 可以访问
/// 全部存储方法。
pub trait StateStore:
    TaskStore
    + FlowStore
    + RegistryStore
    + PackageStore
    + ServiceAccountStore
    + NodeStore
    + ApprovalStore
    + McpStore
    + CronStore
    + RbacStore
    + UsageStore
    + RemoteAgentStore
    + Send
    + Sync
    + 'static
{
}
