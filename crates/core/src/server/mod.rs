//! axum HTTP 服务器
//!
//! REST API + WebSocket 端点。
//! Handler 按领域分组在 `handlers/` 子模块中。

pub mod handlers;
pub mod response;
pub mod ws;

use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    http::{header, Method},
    routing::{get, post},
    Router,
};
use tower_http::cors::CorsLayer;

use tokio::sync::RwLock;

use atta_agent::LlmProvider;
use atta_audit::AuditSink;
use atta_auth::Authz;
use atta_bus::EventBus;
use atta_mcp::McpRegistry;
use atta_memory::MemoryStore;
use atta_security::SecurityPolicy;
use atta_store::StateStore;
use atta_types::ToolRegistry;

use crate::agent_registry::AgentRegistry;
use crate::cron_engine::CronEngine;
use crate::flow_engine::FlowEngine;
use crate::log_broadcast::LogBroadcast;
use crate::middleware::AuthMode;
use crate::remote_agent_hub::RemoteAgentHub;
use crate::skill_engine::SkillRegistry;
use crate::webui::webui_routes;
use crate::ws_hub::WsHub;

/// 应用共享状态
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<dyn StateStore>,
    pub bus: Arc<dyn EventBus>,
    pub authz: Arc<dyn Authz>,
    pub audit: Arc<dyn AuditSink>,
    pub flow_engine: Arc<FlowEngine>,
    pub tool_registry: Arc<dyn ToolRegistry>,
    pub llm: Arc<dyn LlmProvider>,
    pub ws_hub: Arc<WsHub>,
    pub skill_registry: Arc<SkillRegistry>,
    pub mcp_registry: Arc<McpRegistry>,
    /// Registry of running channel instances
    pub channel_registry: Arc<atta_channel::ChannelRegistry>,
    /// Memory store for agent long-term memory
    pub memory_store: Arc<dyn MemoryStore>,
    /// Dynamic security policy
    pub security_policy: Arc<RwLock<SecurityPolicy>>,
    /// Optional path to the WebUI dist directory
    pub webui_dir: Option<PathBuf>,
    /// Authentication mode
    pub auth_mode: AuthMode,
    /// Cron scheduling engine (late-initialized; None until main.rs injects it)
    pub cron_engine: Option<Arc<CronEngine>>,
    /// Registry of running sub-agent instances (late-initialized; None until main.rs injects it)
    pub agent_registry: Option<Arc<AgentRegistry>>,
    /// Broadcast channel for real-time log streaming
    pub log_broadcast: Arc<LogBroadcast>,
    /// Remote Agent connection hub
    pub remote_agent_hub: Arc<RemoteAgentHub>,
    /// Session router for channel message routing
    pub session_router: Option<Arc<atta_channel::SessionRouter>>,
    /// Access control policy for channel senders
    pub access_control: Option<Arc<atta_channel::AccessControlPolicy>>,
}

/// 构建 API 路由器
pub fn api_router(state: AppState) -> Router {
    let webui_dir = state.webui_dir.clone();
    let auth_mode = state.auth_mode.clone();
    let store_ext = state.store.clone();

    let api = Router::new()
        // System
        .route("/api/v1/health", get(handlers::system::health_check))
        .route("/api/v1/system/health", get(handlers::system::health_check))
        .route(
            "/api/v1/system/config",
            get(handlers::system::system_config),
        )
        .route("/api/v1/system/metrics", get(handlers::system::metrics))
        // Tasks
        .route(
            "/api/v1/tasks",
            get(handlers::task::list_tasks).post(handlers::task::create_task),
        )
        .route(
            "/api/v1/tasks/{id}",
            get(handlers::task::get_task).delete(handlers::task::delete_task),
        )
        .route(
            "/api/v1/tasks/{id}/cancel",
            post(handlers::task::cancel_task),
        )
        // Flows
        .route(
            "/api/v1/flows",
            get(handlers::flow::list_flows).post(handlers::flow::create_flow),
        )
        .route(
            "/api/v1/flows/{id}",
            get(handlers::flow::get_flow)
                .put(handlers::flow::update_flow)
                .delete(handlers::flow::delete_flow),
        )
        // Skills
        .route(
            "/api/v1/skills",
            get(handlers::skill::list_skills).post(handlers::skill::create_skill),
        )
        .route(
            "/api/v1/skills/{id}",
            get(handlers::skill::get_skill)
                .put(handlers::skill::update_skill)
                .delete(handlers::skill::delete_skill),
        )
        // Tools
        .route("/api/v1/tools", get(handlers::tool::list_tools))
        .route("/api/v1/tools/{name}", get(handlers::tool::get_tool))
        .route("/api/v1/tools/{name}/test", post(handlers::tool::test_tool))
        // MCP Servers
        .route(
            "/api/v1/mcp/servers",
            get(handlers::mcp::list_mcp_servers).post(handlers::mcp::register_mcp_server),
        )
        .route(
            "/api/v1/mcp/servers/{name}",
            get(handlers::mcp::get_mcp_server).delete(handlers::mcp::unregister_mcp_server),
        )
        .route(
            "/api/v1/mcp/servers/{name}/connect",
            post(handlers::mcp::connect_mcp_server),
        )
        .route(
            "/api/v1/mcp/servers/{name}/disconnect",
            post(handlers::mcp::disconnect_mcp_server),
        )
        // Packages
        .route(
            "/api/v1/packages/install",
            post(handlers::package::install_package),
        )
        .route(
            "/api/v1/packages/{pkg_type}/{name}/upgrade",
            post(handlers::package::upgrade_package),
        )
        .route(
            "/api/v1/packages/{pkg_type}/{name}/dependencies",
            get(handlers::package::package_dependencies),
        )
        // Agents
        .route("/api/v1/agents", get(handlers::agents::list_agents))
        .route("/api/v1/agents/{id}", get(handlers::agents::get_agent))
        .route(
            "/api/v1/agents/{id}/pause",
            post(handlers::agents::pause_agent),
        )
        .route(
            "/api/v1/agents/{id}/resume",
            post(handlers::agents::resume_agent),
        )
        .route(
            "/api/v1/agents/{id}/terminate",
            post(handlers::agents::terminate_agent),
        )
        // Nodes (Enterprise)
        .route("/api/v1/nodes", get(handlers::node::list_nodes))
        .route("/api/v1/nodes/{id}", get(handlers::node::get_node))
        .route("/api/v1/nodes/{id}/drain", post(handlers::node::drain_node))
        .route(
            "/api/v1/nodes/{id}/resume",
            post(handlers::node::resume_node),
        )
        // Approvals (Enterprise)
        .route("/api/v1/approvals", get(handlers::approval::list_approvals))
        .route(
            "/api/v1/approvals/{id}",
            get(handlers::approval::get_approval),
        )
        .route(
            "/api/v1/approvals/{id}/approve",
            post(handlers::approval::approve),
        )
        .route(
            "/api/v1/approvals/{id}/deny",
            post(handlers::approval::deny),
        )
        .route(
            "/api/v1/approvals/{id}/request-changes",
            post(handlers::approval::request_changes),
        )
        // Channels
        .route(
            "/api/v1/channels",
            get(handlers::channel::list_channels).post(handlers::channel::add_channel),
        )
        .route(
            "/api/v1/channels/{name}",
            axum::routing::put(handlers::channel::update_channel)
                .delete(handlers::channel::remove_channel),
        )
        .route(
            "/api/v1/channels/{name}/health",
            get(handlers::channel::channel_health),
        )
        .route(
            "/api/v1/channels/webhook/{name}",
            post(handlers::channel::receive_webhook),
        )
        // Channel sessions
        .route(
            "/api/v1/channels/sessions",
            get(handlers::channel::list_sessions),
        )
        .route(
            "/api/v1/channels/sessions/{key}",
            get(handlers::channel::get_session)
                .put(handlers::channel::update_session)
                .delete(handlers::channel::delete_session),
        )
        // ACP (human takeover)
        .route(
            "/api/v1/channels/sessions/{key}/takeover",
            post(handlers::channel::start_takeover),
        )
        .route(
            "/api/v1/channels/sessions/{key}/takeover/end",
            post(handlers::channel::end_takeover),
        )
        // Channel access control
        .route(
            "/api/v1/channels/{name}/allowlist",
            axum::routing::put(handlers::channel::set_allowlist),
        )
        .route(
            "/api/v1/channels/{name}/blocklist",
            axum::routing::put(handlers::channel::set_blocklist),
        )
        // Memory
        .route(
            "/api/v1/memory/search",
            get(handlers::memory::search_memory),
        )
        .route(
            "/api/v1/memory/{id}",
            get(handlers::memory::get_memory).delete(handlers::memory::delete_memory),
        )
        // Security
        .route(
            "/api/v1/security/policy",
            get(handlers::security::get_security_policy)
                .put(handlers::security::update_security_policy),
        )
        // Audit (Enterprise)
        .route("/api/v1/audit", get(handlers::audit::query_audit))
        .route("/api/v1/audit/export", get(handlers::audit::export_audit))
        // Cron Jobs
        .route(
            "/api/v1/cron/jobs",
            get(handlers::cron::list_jobs).post(handlers::cron::create_job),
        )
        .route(
            "/api/v1/cron/jobs/{id}",
            get(handlers::cron::get_job)
                .put(handlers::cron::update_job)
                .delete(handlers::cron::delete_job),
        )
        .route(
            "/api/v1/cron/jobs/{id}/trigger",
            post(handlers::cron::trigger_job),
        )
        .route(
            "/api/v1/cron/jobs/{id}/runs",
            get(handlers::cron::list_runs),
        )
        .route("/api/v1/cron/status", get(handlers::cron::cron_status))
        // Usage & Cost
        .route("/api/v1/usage/summary", get(handlers::usage::usage_summary))
        .route("/api/v1/usage/daily", get(handlers::usage::usage_daily))
        .route("/api/v1/usage/by-model", get(handlers::usage::usage_by_model))
        .route("/api/v1/usage/export", get(handlers::usage::usage_export))
        // Logs
        .route("/api/v1/logs/stream", get(handlers::logs::log_stream))
        .route("/api/v1/logs/recent", get(handlers::logs::recent_logs))
        // Diagnostics
        .route("/api/v1/diagnostics/run", post(handlers::diagnostics::run_diagnostics))
        // Chat (SSE streaming)
        .route("/api/v1/chat", post(handlers::chat::chat_sse))
        // Remote Agents
        .route(
            "/api/v1/remote/agents",
            get(handlers::remote::list_remote_agents).post(handlers::remote::register_remote_agent),
        )
        .route(
            "/api/v1/remote/agents/{id}",
            get(handlers::remote::get_remote_agent).delete(handlers::remote::delete_remote_agent),
        )
        .route(
            "/api/v1/remote/agents/{id}/estop",
            post(handlers::remote::estop_remote_agent),
        )
        .route(
            "/api/v1/remote/agents/{id}/rotate-token",
            post(handlers::remote::rotate_remote_agent_token),
        )
        .route("/api/v1/remote/ws", get(handlers::remote::remote_ws_upgrade))
        // WebSocket
        .route("/api/v1/ws", get(ws::ws_upgrade))
        .layer(axum::Extension(auth_mode))
        .layer(axum::Extension(store_ext))
        .layer(
            CorsLayer::new()
                .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS])
                .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION, header::ACCEPT])
                .allow_origin(tower_http::cors::Any),
        )
        .with_state(state);

    // Merge WebUI routes as fallback (must be last — catches all unmatched routes)
    api.merge(webui_routes(webui_dir))
}
