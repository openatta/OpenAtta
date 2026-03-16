# 用户管理 + 远程 Agent 接入 — 合并设计文档

> 两个特性共享认证基础设施，合并实施减少重复工作。

## 一、现状与改动范围

### 已有基础（可复用）

| 组件 | 位置 | 状态 |
|------|------|------|
| CurrentUser 中间件 | `crates/core/src/middleware.rs` | ✅ 完整，Desktop=Owner, OIDC/ApiKey 验证 |
| RBAC 引擎 | `crates/auth/src/rbac.rs` | ✅ 完整，6 角色权限矩阵 |
| AllowAll | `crates/auth/src/allow_all.rs` | ✅ Desktop 默认 |
| AuditSink | `crates/audit/` | ✅ NoopAudit(Desktop) / AuditStore(Enterprise) |
| WsHub | `crates/core/src/ws_hub.rs` | ✅ 广播模型，可参考 |
| TaskFilter.created_by | `crates/types/src/task.rs` | ✅ 字段存在，handler 传 None |
| service_accounts 表 | `migrations/sqlite/001_init.sql` | ✅ API Key 认证 |
| role_bindings 表 | `migrations/sqlite/001_init.sql` | ✅ RBAC 角色绑定 |

### 需要改动的文件

| 文件 | 改动内容 |
|------|----------|
| `migrations/sqlite/002_remote_agents.sql` | **新增** remote_agents 表 |
| `crates/types/src/remote_agent.rs` | **新增** RemoteAgent 类型、WS 消息类型 |
| `crates/types/src/lib.rs` | 导出新模块 |
| `crates/core/src/remote_agent_hub.rs` | **新增** RemoteAgentHub（管理 WS 连接） |
| `crates/core/src/server/handlers/remote.rs` | **新增** 远程 Agent WS handler + REST API |
| `crates/core/src/server/handlers/mod.rs` | 导出新模块 |
| `crates/core/src/server/mod.rs` | AppState 新增字段 + 路由 |
| `crates/core/src/server/handlers/task.rs` | 接入 CurrentUser + 数据隔离 |
| `crates/core/src/server/handlers/approval.rs` | 接入 CurrentUser |
| `crates/core/src/server/handlers/cron.rs` | 接入 CurrentUser |
| `crates/core/src/lib.rs` | 导出 remote_agent_hub |
| `crates/server/src/services.rs` | 初始化 RemoteAgentHub |
| `crates/store/src/traits.rs` | RemoteAgentStore trait |
| `crates/store/src/sqlite.rs` | RemoteAgentStore 实现 |

## 二、实施步骤

### Step 1: 数据库 — remote_agents 表

```sql
-- migrations/sqlite/002_remote_agents.sql

CREATE TABLE IF NOT EXISTS remote_agents (
    id              TEXT PRIMARY KEY,          -- ra_xxxx base58 ID
    name            TEXT NOT NULL,
    token_hash      TEXT NOT NULL UNIQUE,       -- SHA-256(aat_xxx)
    description     TEXT DEFAULT '',
    version         TEXT DEFAULT '0.1.0',
    capabilities    TEXT NOT NULL DEFAULT '[]', -- JSON array
    status          TEXT NOT NULL DEFAULT 'offline',  -- online/offline
    last_heartbeat  TEXT,
    registered_at   TEXT NOT NULL,
    registered_by   TEXT NOT NULL               -- actor_id
);

CREATE INDEX IF NOT EXISTS idx_ra_token ON remote_agents(token_hash);
CREATE INDEX IF NOT EXISTS idx_ra_status ON remote_agents(status);
```

### Step 2: 类型定义 — RemoteAgent + WS 消息

```rust
// crates/types/src/remote_agent.rs

/// 远程 Agent 注册信息
pub struct RemoteAgent {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub capabilities: Vec<String>,
    pub status: RemoteAgentStatus,
    pub last_heartbeat: Option<DateTime<Utc>>,
    pub registered_at: DateTime<Utc>,
    pub registered_by: String,
}

pub enum RemoteAgentStatus {
    Online,
    Offline,
}

/// WebSocket 消息帧
pub struct WsFrame {
    pub msg_type: String,
    pub msg_id: String,
    pub payload: serde_json::Value,
}

/// 上行消息类型
pub enum UpstreamMsg {
    Register { agent_name, agent_version, description, capabilities },
    EventBatch { events: Vec<RemoteEvent> },
    Deregister { reason },
}

/// 远程事件
pub struct RemoteEvent {
    pub event_type: String,
    pub timestamp: DateTime<Utc>,
    pub correlation_id: Option<String>,
    pub payload: serde_json::Value,
}

/// 下行消息类型
pub enum DownstreamMsg {
    Registered { agent_id },
    Estop { reason, scope },
    PolicyUpdate { policy },
    ResourceUpdated { resource_type, id },
    Ack { msg_id },
}
```

### Step 3: RemoteAgentHub

```rust
// crates/core/src/remote_agent_hub.rs

/// 管理远程 Agent WebSocket 连接
pub struct RemoteAgentHub {
    /// agent_id → WsSender
    agents: Arc<RwLock<HashMap<String, AgentConnection>>>,
}

struct AgentConnection {
    sender: mpsc::UnboundedSender<String>,
    agent: RemoteAgent,
    connected_at: DateTime<Utc>,
}

impl RemoteAgentHub {
    pub fn new() -> Self;
    pub async fn register(&self, agent: RemoteAgent) -> mpsc::UnboundedReceiver<String>;
    pub async fn unregister(&self, agent_id: &str);
    pub async fn send_to(&self, agent_id: &str, msg: DownstreamMsg);
    pub async fn broadcast_estop(&self, reason: &str);
    pub async fn list_online(&self) -> Vec<RemoteAgent>;
    pub async fn agent_count(&self) -> usize;
}
```

### Step 4: WebSocket Handler

```rust
// crates/core/src/server/handlers/remote.rs

/// WebSocket 升级端点
/// GET /api/v1/remote/ws?token=aat_xxx
pub async fn remote_ws_upgrade(
    State(state): State<AppState>,
    Query(params): Query<WsTokenQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse

/// 处理 WebSocket 连接
async fn handle_remote_ws(state: AppState, token_hash: String, socket: WebSocket) {
    // 1. 等待 register 消息
    // 2. 验证 token → 查 remote_agents 表
    // 3. 注册到 RemoteAgentHub
    // 4. 双向消息循环：
    //    上行：event_batch → audit + bus
    //    下行：estop/policy → 转发
    // 5. 断开时标记 offline
}

/// REST API 端点
/// GET /api/v1/remote/agents — 列出远程 Agent
/// POST /api/v1/remote/agents — 注册新 Agent（生成 token）
/// DELETE /api/v1/remote/agents/{id} — 注销
/// POST /api/v1/remote/agents/{id}/estop — 紧急停止
```

### Step 5: Handler 接入 CurrentUser

改动模式统一：在需要 Actor 的 handler 中增加 `CurrentUser` 提取。

```rust
// 改动前：
pub async fn create_task(
    State(state): State<AppState>,
    Json(req): Json<CreateTaskRequest>,
) -> impl IntoResponse {
    let actor = Actor::user("anonymous");
    ...
}

// 改动后：
pub async fn create_task(
    State(state): State<AppState>,
    user: CurrentUser,                    // ← 新增
    Json(req): Json<CreateTaskRequest>,
) -> impl IntoResponse {
    let actor = user.actor;               // ← 使用真实用户
    ...
}
```

需要改动的 handler：

| Handler | 函数 | 改动点 |
|---------|------|--------|
| task.rs | create_task | Actor::user("anonymous") → user.actor |
| task.rs | list_tasks | created_by: None → 按角色隔离 |
| approval.rs | approve / deny / request_changes | Actor::user("anonymous") → user.actor |
| cron.rs | create_job | "user".to_string() → user.actor.id |

### Step 6: AppState + 路由

```rust
// AppState 新增：
pub remote_agent_hub: Arc<RemoteAgentHub>,

// 路由新增：
.route("/api/v1/remote/ws", get(handlers::remote::remote_ws_upgrade))
.route("/api/v1/remote/agents",
    get(handlers::remote::list_remote_agents)
    .post(handlers::remote::register_remote_agent))
.route("/api/v1/remote/agents/{id}",
    get(handlers::remote::get_remote_agent)
    .delete(handlers::remote::delete_remote_agent))
.route("/api/v1/remote/agents/{id}/estop",
    post(handlers::remote::estop_remote_agent))
```

## 三、Store trait 扩展

```rust
// crates/store/src/traits.rs 新增

#[async_trait]
pub trait RemoteAgentStore: Send + Sync {
    async fn register_remote_agent(&self, agent: &RemoteAgent) -> Result<(), AttaError>;
    async fn get_remote_agent(&self, id: &str) -> Result<Option<RemoteAgent>, AttaError>;
    async fn get_remote_agent_by_token(&self, token_hash: &str) -> Result<Option<RemoteAgent>, AttaError>;
    async fn list_remote_agents(&self) -> Result<Vec<RemoteAgent>, AttaError>;
    async fn update_remote_agent_status(&self, id: &str, status: RemoteAgentStatus) -> Result<(), AttaError>;
    async fn update_heartbeat(&self, id: &str) -> Result<(), AttaError>;
    async fn delete_remote_agent(&self, id: &str) -> Result<(), AttaError>;
}
```

## 四、安全设计

| 层面 | 实现 |
|------|------|
| Token 生成 | 注册时生成 `aat_` + 32 字节随机 base64，存储 SHA-256 哈希 |
| WS 认证 | 连接时 query param 或首条消息携带 token |
| 权限隔离 | 远程 Agent token 只能用于 `/api/v1/remote/*` + 资源只读 API |
| 事件标记 | 远程事件 audit_log.actor_type = 'remote_agent' |
| E-Stop | WebUI/API 触发 → RemoteAgentHub.send_to() → WS 下行 |

## 五、不改动的部分

- **EventBus trait**：远程事件通过 handler 写入，不改总线本身
- **RBAC 引擎**：已完整，本次只是接通调用
- **CurrentUser 中间件**：已完整，不需要改动
- **Store 层过滤器**：TaskFilter.created_by 已有，只需 handler 传值
