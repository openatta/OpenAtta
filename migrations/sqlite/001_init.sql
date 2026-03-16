-- AttaOS SQLite 初始化 Schema
-- 基于 docs/design-database.md

-- ── 核心表 ──

-- 任务
CREATE TABLE IF NOT EXISTS tasks (
    id              TEXT PRIMARY KEY,
    flow_id         TEXT NOT NULL,
    current_state   TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'running',
    error_message   TEXT,
    state_data      TEXT NOT NULL DEFAULT '{}',
    input           TEXT NOT NULL,
    output          TEXT,
    created_by      TEXT NOT NULL,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    completed_at    TEXT
);

CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_flow ON tasks(flow_id);
CREATE INDEX IF NOT EXISTS idx_tasks_created_by ON tasks(created_by, created_at);

-- Flow 运行状态
CREATE TABLE IF NOT EXISTS flow_states (
    task_id         TEXT PRIMARY KEY,
    current_state   TEXT NOT NULL,
    history         TEXT NOT NULL DEFAULT '[]',
    retry_count     INTEGER NOT NULL DEFAULT 0,
    updated_at      TEXT NOT NULL
);

-- Flow 模板定义
CREATE TABLE IF NOT EXISTS flow_defs (
    id              TEXT PRIMARY KEY,
    version         TEXT NOT NULL,
    name            TEXT,
    description     TEXT,
    definition      TEXT NOT NULL,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

-- Skill 定义
CREATE TABLE IF NOT EXISTS skill_defs (
    id              TEXT PRIMARY KEY,
    version         TEXT NOT NULL,
    name            TEXT,
    definition      TEXT NOT NULL,
    risk_level      TEXT NOT NULL DEFAULT 'low',
    tags            TEXT NOT NULL DEFAULT '[]',
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

-- Tool 注册
CREATE TABLE IF NOT EXISTS tool_defs (
    name            TEXT PRIMARY KEY,
    description     TEXT NOT NULL,
    plugin_name     TEXT,
    mcp_server      TEXT,
    risk_level      TEXT NOT NULL DEFAULT 'low',
    parameters      TEXT NOT NULL,
    returns_schema  TEXT,
    enabled         INTEGER NOT NULL DEFAULT 1,
    created_at      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tools_plugin ON tool_defs(plugin_name);
CREATE INDEX IF NOT EXISTS idx_tools_risk ON tool_defs(risk_level);

-- 已安装插件
CREATE TABLE IF NOT EXISTS plugins (
    name            TEXT PRIMARY KEY,
    version         TEXT NOT NULL,
    description     TEXT,
    author          TEXT,
    organization    TEXT,
    permissions     TEXT NOT NULL DEFAULT '[]',
    status          TEXT NOT NULL DEFAULT 'enabled',
    manifest        TEXT NOT NULL,
    signature_valid INTEGER NOT NULL DEFAULT 0,
    installed_at    TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

-- MCP Server 注册
CREATE TABLE IF NOT EXISTS mcp_servers (
    name            TEXT PRIMARY KEY,
    description     TEXT,
    transport       TEXT NOT NULL,
    url             TEXT,
    command         TEXT,
    args            TEXT NOT NULL DEFAULT '[]',
    auth_config     TEXT,
    status          TEXT NOT NULL DEFAULT 'disconnected',
    tools_count     INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL,
    last_connected  TEXT
);

-- 已安装包记录
CREATE TABLE IF NOT EXISTS packages (
    name            TEXT NOT NULL,
    version         TEXT NOT NULL,
    package_type    TEXT NOT NULL,
    installed_at    TEXT NOT NULL,
    installed_by    TEXT NOT NULL,
    PRIMARY KEY (name, version)
);

-- ── Enterprise 表（Desktop 也创建，仅企业版使用） ──

-- 服务账号
CREATE TABLE IF NOT EXISTS service_accounts (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL UNIQUE,
    api_key_hash    TEXT NOT NULL UNIQUE,
    roles           TEXT NOT NULL DEFAULT '[]',
    created_at      TEXT NOT NULL,
    enabled         INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS idx_sa_api_key ON service_accounts(api_key_hash);

-- 角色绑定
CREATE TABLE IF NOT EXISTS role_bindings (
    id              TEXT PRIMARY KEY,
    actor_id        TEXT NOT NULL,
    actor_type      TEXT NOT NULL,
    role            TEXT NOT NULL,
    granted_by      TEXT NOT NULL,
    granted_at      TEXT NOT NULL,
    expires_at      TEXT,
    UNIQUE(actor_id, role)
);

CREATE INDEX IF NOT EXISTS idx_roles_actor ON role_bindings(actor_id);
CREATE INDEX IF NOT EXISTS idx_roles_role ON role_bindings(role);

-- 审批记录
CREATE TABLE IF NOT EXISTS approvals (
    id              TEXT PRIMARY KEY,
    task_id         TEXT NOT NULL,
    requested_by    TEXT NOT NULL,
    approver_role   TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending',
    context         TEXT NOT NULL,
    resolved_by     TEXT,
    resolved_at     TEXT,
    comment         TEXT,
    timeout_at      TEXT NOT NULL,
    created_at      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_approvals_status ON approvals(status);
CREATE INDEX IF NOT EXISTS idx_approvals_task ON approvals(task_id);

-- 节点注册
CREATE TABLE IF NOT EXISTS nodes (
    id              TEXT PRIMARY KEY,
    hostname        TEXT NOT NULL,
    labels          TEXT NOT NULL DEFAULT '[]',
    status          TEXT NOT NULL DEFAULT 'online',
    total_memory    INTEGER NOT NULL DEFAULT 0,
    available_memory INTEGER NOT NULL DEFAULT 0,
    running_agents  INTEGER NOT NULL DEFAULT 0,
    max_concurrent  INTEGER NOT NULL DEFAULT 4,
    last_heartbeat  TEXT NOT NULL,
    registered_at   TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_nodes_status ON nodes(status);

-- 审计日志
CREATE TABLE IF NOT EXISTS audit_log (
    id              TEXT PRIMARY KEY,
    timestamp       TEXT NOT NULL,
    actor_type      TEXT NOT NULL,
    actor_id        TEXT NOT NULL,
    action          TEXT NOT NULL,
    resource_type   TEXT NOT NULL,
    resource_id     TEXT,
    correlation_id  TEXT NOT NULL,
    outcome         TEXT NOT NULL,
    detail          TEXT NOT NULL DEFAULT '{}',
    client_ip       TEXT,
    user_agent      TEXT
);

CREATE INDEX IF NOT EXISTS idx_audit_time ON audit_log(timestamp);
CREATE INDEX IF NOT EXISTS idx_audit_actor ON audit_log(actor_id, timestamp);
CREATE INDEX IF NOT EXISTS idx_audit_resource ON audit_log(resource_type, resource_id);
CREATE INDEX IF NOT EXISTS idx_audit_correlation ON audit_log(correlation_id);
CREATE INDEX IF NOT EXISTS idx_audit_action ON audit_log(action);

-- 密钥存储
CREATE TABLE IF NOT EXISTS secrets (
    key             TEXT PRIMARY KEY,
    value           BLOB NOT NULL,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);
