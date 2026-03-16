# Client-Server Architecture

Version: v0.1.0

OpenAtta 采用 client-server 架构，三个独立二进制各司其职。

## 二进制

| 二进制 | Crate | 职责 |
|--------|-------|------|
| `attaos` | `crates/server` | 核心服务守护进程：HTTP API + WebUI + Agent 执行 |
| `attacli` | `crates/cli` | 轻量 CLI 客户端：通过 HTTP/SSE 与 attaos 通信 |
| `attash` | `apps/shell/src-tauri` | 桌面 Shell：Tauri v2 WebView + 原生系统托盘 + 自动更新 |

## 架构图

```
┌──────────────────────────────────────────────────┐
│                    attaos (server)                │
│                                                  │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐  │
│  │ HTTP API │  │  WebUI   │  │ CoreCoordinator│  │
│  │ (axum)   │  │ (embed)  │  │  (event loop) │  │
│  └────┬─────┘  └──────────┘  └───────┬───────┘  │
│       │                              │           │
│  ┌────┴──────────────────────────────┴───────┐   │
│  │           AppState (DI container)          │   │
│  │  Store │ Bus │ LLM │ Tools │ Flows │ Skills│   │
│  └────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────┘
        ▲                       ▲
        │ HTTP + SSE            │ HTTP (WebView)
        │                       │
   ┌────┴────┐             ┌────┴────┐
   │ attacli │             │  attash │
   │  (CLI)  │             │ (Tauri) │
   └─────────┘             └─────────┘
```

## 通信协议

| 协议 | 用途 | 端点 |
|------|------|------|
| **REST** | CRUD 操作 | `GET/POST/PUT/DELETE /api/v1/*` |
| **SSE** | 流式聊天 | `POST /api/v1/chat` |
| **WebSocket** | 实时事件推送 | `/api/v1/ws`（带认证验证） |

- **CRUD 操作**：标准 REST API（`GET /api/v1/tasks`、`POST /api/v1/skills` 等），所有端点均通过 `check_authz` 进行权限检查
- **流式聊天**：`POST /api/v1/chat` → SSE 流（`ChatEvent` 事件）
- **实时事件**：WebSocket（`/api/v1/ws`）— 任务状态变更、Agent Delta 等，升级前验证认证

## 自动启动

attacli 和 attash 在首次使用时自动检测并启动 attaos 服务：

1. 健康检查 `GET /api/v1/health`
2. 若失败，spawn `attaos --port {port} --skip-update-check`
3. 轮询等待就绪（最多 15 秒）

## 端口协调

| 优先级 | 来源 |
|--------|------|
| 1 | CLI 参数 `--port` / `--url` |
| 2 | 环境变量 `ATTA_PORT` / `ATTA_URL` |
| 3 | 默认值 `3000` |

## Chat SSE 协议

请求：
```json
POST /api/v1/chat
{
  "message": "用户消息",
  "skill_id": "optional-skill-id"
}
```

SSE 事件（`ChatEvent`）：
```
data: {"type":"thinking","data":{"iteration":1}}
data: {"type":"text_delta","data":{"delta":"Hello"}}
data: {"type":"tool_start","data":{"tool_name":"web_search","call_id":"abc"}}
data: {"type":"tool_complete","data":{"tool_name":"web_search","call_id":"abc","duration_ms":1200}}
data: {"type":"done","data":{"iterations":2}}
```

## Desktop vs Enterprise

两个版本使用相同的进程架构（attaos + attacli + attash），
通过 Cargo features 切换基础设施：

| Feature | Desktop | Enterprise |
|---------|---------|------------|
| EventBus | InProcBus | NatsBus |
| StateStore | SqliteStore | PostgresStore |
| Authz | AllowAll | RBACAuthz |
| AuditSink | NoopAudit | AuditStore |

Features 仅在 `atta-server` crate 上设置：
```bash
cargo build -p atta-server --features desktop   # 默认
cargo build -p atta-server --features enterprise
```

## CORS

服务端通过 `tower-http::cors::CorsLayer` 配置跨域策略，允许 attash WebView 和外部客户端正常访问 API。

## 安全

- 所有 API 端点通过 `check_authz` 进行权限验证
- WebSocket 升级前进行认证检查
- Task 操作使用乐观锁（version 字段）防止并发冲突
- 子进程环境变量通过白名单（`ATTA_HOME`、`ATTA_PORT`、`ATTA_LOG`、`ATTA_LOG_LEVEL`、`ATTA_DATA_DIR`）进行清洗
