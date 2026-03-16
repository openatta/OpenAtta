-- AttaOS PostgreSQL 初始化 Schema
-- Enterprise 版数据库 Schema，使用 Postgres 原生类型

-- ── 核心表 ──

-- 任务
CREATE TABLE IF NOT EXISTS tasks (
    id              UUID PRIMARY KEY,
    flow_id         TEXT NOT NULL,
    current_state   TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'running',
    error_message   TEXT,
    state_data      JSONB NOT NULL DEFAULT '{}',
    input           JSONB NOT NULL,
    output          JSONB,
    created_by      TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL,
    completed_at    TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_flow ON tasks(flow_id);
CREATE INDEX IF NOT EXISTS idx_tasks_created_by ON tasks(created_by, created_at);

-- Flow 运行状态
CREATE TABLE IF NOT EXISTS flow_states (
    task_id         UUID PRIMARY KEY,
    current_state   TEXT NOT NULL,
    history         JSONB NOT NULL DEFAULT '[]',
    retry_count     INTEGER NOT NULL DEFAULT 0,
    updated_at      TIMESTAMPTZ NOT NULL
);

-- Flow 模板定义
CREATE TABLE IF NOT EXISTS flow_defs (
    id              TEXT PRIMARY KEY,
    version         TEXT NOT NULL,
    name            TEXT,
    description     TEXT,
    definition      JSONB NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL
);

-- Skill 定义
CREATE TABLE IF NOT EXISTS skill_defs (
    id              TEXT PRIMARY KEY,
    version         TEXT NOT NULL,
    name            TEXT,
    definition      JSONB NOT NULL,
    risk_level      TEXT NOT NULL DEFAULT 'low',
    tags            JSONB NOT NULL DEFAULT '[]',
    created_at      TIMESTAMPTZ NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL
);

-- Tool 注册
CREATE TABLE IF NOT EXISTS tool_defs (
    name            TEXT PRIMARY KEY,
    description     TEXT NOT NULL,
    plugin_name     TEXT,
    mcp_server      TEXT,
    risk_level      TEXT NOT NULL DEFAULT 'low',
    parameters      JSONB NOT NULL,
    returns_schema  JSONB,
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL
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
    permissions     JSONB NOT NULL DEFAULT '[]',
    status          TEXT NOT NULL DEFAULT 'enabled',
    manifest        JSONB NOT NULL,
    signature_valid BOOLEAN NOT NULL DEFAULT FALSE,
    installed_at    TIMESTAMPTZ NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL
);

-- MCP Server 注册
CREATE TABLE IF NOT EXISTS mcp_servers (
    name            TEXT PRIMARY KEY,
    description     TEXT,
    transport       TEXT NOT NULL,
    url             TEXT,
    command         TEXT,
    args            JSONB NOT NULL DEFAULT '[]',
    auth_config     JSONB,
    status          TEXT NOT NULL DEFAULT 'disconnected',
    tools_count     INTEGER NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL,
    last_connected  TIMESTAMPTZ
);

-- 已安装包记录
CREATE TABLE IF NOT EXISTS packages (
    name            TEXT NOT NULL,
    version         TEXT NOT NULL,
    package_type    TEXT NOT NULL,
    installed_at    TIMESTAMPTZ NOT NULL,
    installed_by    TEXT NOT NULL,
    PRIMARY KEY (name, version)
);

-- ── Enterprise 表 ──

-- 服务账号
CREATE TABLE IF NOT EXISTS service_accounts (
    id              UUID PRIMARY KEY,
    name            TEXT NOT NULL UNIQUE,
    api_key_hash    TEXT NOT NULL UNIQUE,
    roles           JSONB NOT NULL DEFAULT '[]',
    created_at      TIMESTAMPTZ NOT NULL,
    enabled         BOOLEAN NOT NULL DEFAULT TRUE
);

CREATE INDEX IF NOT EXISTS idx_sa_api_key ON service_accounts(api_key_hash);

-- 角色绑定
CREATE TABLE IF NOT EXISTS role_bindings (
    id              UUID PRIMARY KEY,
    actor_id        TEXT NOT NULL,
    actor_type      TEXT NOT NULL,
    role            TEXT NOT NULL,
    granted_by      TEXT NOT NULL,
    granted_at      TIMESTAMPTZ NOT NULL,
    expires_at      TIMESTAMPTZ,
    UNIQUE(actor_id, role)
);

CREATE INDEX IF NOT EXISTS idx_roles_actor ON role_bindings(actor_id);
CREATE INDEX IF NOT EXISTS idx_roles_role ON role_bindings(role);

-- 审批记录
CREATE TABLE IF NOT EXISTS approvals (
    id              UUID PRIMARY KEY,
    task_id         UUID NOT NULL,
    requested_by    TEXT NOT NULL,
    approver_role   TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending',
    context         JSONB NOT NULL,
    resolved_by     TEXT,
    resolved_at     TIMESTAMPTZ,
    comment         TEXT,
    timeout_at      TIMESTAMPTZ NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_approvals_status ON approvals(status);
CREATE INDEX IF NOT EXISTS idx_approvals_task ON approvals(task_id);

-- 节点注册
CREATE TABLE IF NOT EXISTS nodes (
    id              TEXT PRIMARY KEY,
    hostname        TEXT NOT NULL,
    labels          JSONB NOT NULL DEFAULT '[]',
    status          TEXT NOT NULL DEFAULT 'online',
    total_memory    BIGINT NOT NULL DEFAULT 0,
    available_memory BIGINT NOT NULL DEFAULT 0,
    running_agents  INTEGER NOT NULL DEFAULT 0,
    max_concurrent  INTEGER NOT NULL DEFAULT 4,
    last_heartbeat  TIMESTAMPTZ NOT NULL,
    registered_at   TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_nodes_status ON nodes(status);

-- 审计日志
CREATE TABLE IF NOT EXISTS audit_log (
    id              UUID PRIMARY KEY,
    timestamp       TIMESTAMPTZ NOT NULL,
    actor_type      TEXT NOT NULL,
    actor_id        TEXT NOT NULL,
    action          TEXT NOT NULL,
    resource_type   TEXT NOT NULL,
    resource_id     TEXT,
    correlation_id  UUID NOT NULL,
    outcome         TEXT NOT NULL,
    detail          JSONB NOT NULL DEFAULT '{}',
    client_ip       TEXT,
    user_agent      TEXT
);

CREATE INDEX IF NOT EXISTS idx_audit_time ON audit_log(timestamp);
CREATE INDEX IF NOT EXISTS idx_audit_actor ON audit_log(actor_id, timestamp);
CREATE INDEX IF NOT EXISTS idx_audit_resource ON audit_log(resource_type, resource_id);
CREATE INDEX IF NOT EXISTS idx_audit_correlation ON audit_log(correlation_id);
CREATE INDEX IF NOT EXISTS idx_audit_action ON audit_log(action);

-- Cron jobs
CREATE TABLE IF NOT EXISTS cron_jobs (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    schedule        TEXT NOT NULL,
    command         TEXT NOT NULL,
    config          JSONB NOT NULL DEFAULT '{}',
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    created_by      TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL,
    last_run_at     TIMESTAMPTZ,
    next_run_at     TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS cron_runs (
    id              TEXT PRIMARY KEY,
    job_id          TEXT NOT NULL REFERENCES cron_jobs(id),
    status          TEXT NOT NULL DEFAULT 'running',
    started_at      TIMESTAMPTZ NOT NULL,
    completed_at    TIMESTAMPTZ,
    output          TEXT,
    error           TEXT,
    triggered_by    TEXT NOT NULL DEFAULT 'scheduler'
);

CREATE INDEX IF NOT EXISTS idx_cron_runs_job ON cron_runs(job_id, started_at);

-- 密钥存储
CREATE TABLE IF NOT EXISTS secrets (
    key             TEXT PRIMARY KEY,
    value           BYTEA NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL
);
