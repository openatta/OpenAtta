# AttaOS

Rust 重写的 AI 操作系统 —— 让 Agent 像进程一样被调度、隔离、审计。

## 版本

| 版本 | 说明 |
|------|------|
| Desktop | 单机单用户，SQLite + 进程内总线，零外部依赖 |
| Enterprise | 多机多用户，Postgres + NATS JetStream + RBAC + 审计 |

> Desktop 是 Enterprise 的「单机 profile」—— 抽象不变，基础设施可替换。

## 二进制

| 二进制 | Crate | 职责 |
|--------|-------|------|
| `attaos` | `crates/server` | 核心服务守护进程：HTTP API + WebUI + Agent 执行 |
| `attacli` | `crates/cli` | 轻量 CLI 客户端：通过 HTTP/SSE 与 attaos 通信 |
| `attash` | `apps/shell/src-tauri` | 桌面 Shell：Tauri WebView + 原生系统托盘 + 自动更新 |

## 技术栈

- **语言**：Rust 2021 edition
- **异步运行时**：Tokio
- **HTTP**：axum
- **序列化**：serde / serde_json / serde_yaml
- **数据库**：sqlx（SQLite / Postgres 统一抽象）
- **日志**：tracing + tracing-subscriber
- **事件总线**：tokio mpsc/broadcast（Desktop） / async-nats（Enterprise）
- **错误处理**：thiserror + anyhow
- **CLI**：clap
- **Web UI**：Vue 3 + Vite + Pinia
- **桌面 Shell**：Tauri v2（WebView + 原生系统托盘）

## 项目结构

```
atta/
├── Cargo.toml              # workspace root
├── crates/
│   ├── server/             # attaos 守护进程（所有重量级依赖 + feature flags）
│   ├── cli/                # attacli 轻量 CLI 客户端（HTTP/SSE client）
│   ├── core/               # 控制面：调度、状态、Flow 引擎、HTTP 路由
│   ├── agent/              # Agent 执行器：ReAct 循环、LLM 交互
│   ├── bus/                # EventBus trait + InProcBus / NatsBus
│   ├── store/              # StateStore trait + SqliteStore / PostgresStore
│   ├── auth/               # Authz trait + AllowAll / RBACAuthz
│   ├── audit/              # AuditSink trait + NoopAudit / AuditStore
│   ├── memory/             # 记忆系统：向量 + FTS 混合搜索
│   ├── types/              # 共享类型：EventEnvelope、ChatEvent、Actor 等
│   ├── secret/             # 密钥管理
│   ├── mcp/                # MCP Server 管理
│   ├── channel/            # Channel（Terminal、Webhook 等）
│   ├── security/           # 安全策略、审批、E-Stop
│   └── tools/              # 原生 Tool 实现
├── apps/
│   └── shell/src-tauri/    # attash 桌面 Shell（Tauri + 自动更新）
├── webui/                  # Vue 3 前端工程
├── flows/                  # 内置 Flow 模板（YAML）
├── skills/                 # 内置 Skill 模板（YAML）
├── docs/                   # 技术文档
└── chatgpt/                # 早期参考文档（ChatGPT 生成）
```

## 构建 & 运行

```bash
# 构建 attaos 服务（Desktop 默认）
cargo build -p atta-server --features desktop

# 构建 attaos 服务（Enterprise）
cargo build -p atta-server --features enterprise

# 构建 attacli 客户端
cargo build -p atta-cli

# 构建 attash 桌面 Shell
cargo build -p atta-shell

# 运行 attaos 服务
cargo run -p atta-server -- --port 3000

# 运行 attacli
cargo run -p atta-cli -- status
cargo run -p atta-cli -- chat

# 全 workspace check（排除 Tauri）
cargo check --workspace --exclude atta-shell
# 运行测试
cargo test --workspace --exclude atta-shell
# 运行单个 crate 测试
cargo test -p atta-core

# 格式化
cargo fmt --all

# Lint
cargo clippy --workspace --exclude atta-shell --exclude atta-updater --all-targets -- -D warnings
```

## 编码规范

- 所有公开 API 必须有文档注释（`///`）
- 错误类型用 `thiserror` 派生，业务逻辑用 `anyhow` 传播
- 异步函数优先使用 `async fn`，避免手写 `Pin<Box<dyn Future>>`
- trait 对象用 `Arc<dyn Trait>` 传递，不用裸指针
- 版本切换通过 Cargo features（`desktop` / `enterprise`），**不**用条件编译散落业务逻辑
- 命名遵循 Rust 标准：`snake_case` 函数/变量，`PascalCase` 类型/trait，`SCREAMING_SNAKE_CASE` 常量
- 每个 crate 的 `lib.rs` 只做 re-export，逻辑放子模块
- 测试与源码同目录（`#[cfg(test)] mod tests`），集成测试放 `tests/`

## 核心架构决策

1. **Client-Server 架构**：attaos（服务）+ attacli（CLI 客户端）+ attash（桌面 Shell）
2. **分层模型**：Client → Core → Flow → Agent → Skill → Tool/MCP
3. **4 个核心 trait** 实现双版本切换：EventBus / StateStore / Authz / AuditSink
4. **事件驱动**：所有状态变更通过 EventEnvelope 在总线上流转
6. **Flow 状态机**：YAML 定义，支持审批门控（WAIT_APPROVAL）
7. **Agent 执行模型**：ReAct 循环（Observe → Think → Act → Observe）
8. **Client ↔ Server 通信**：HTTP REST（CRUD）+ SSE（流式聊天）+ WebSocket（实时事件推送）
9. **Web UI**：Vue 3 SPA，静态资源嵌入 axum 服务
10. **桌面 Shell**：Tauri v2 WebView + 原生系统托盘，自动启动 attaos 服务

## 文档索引

| 文档 | 说明 |
|------|------|
| [docs/architecture.md](docs/architecture.md) | 系统架构（分层、核心 Trait、系统对象、数据流） |
| [docs/client-server-architecture.md](docs/client-server-architecture.md) | Client-Server 架构（二进制、通信协议、自动启动） |
| [docs/tech-stack.md](docs/tech-stack.md) | 技术选型（全部依赖及选型理由） |
| [docs/usage.md](docs/usage.md) | 使用指南（CLI、API、配置、Skill/Flow 定义） |
| [docs/comparison.md](docs/comparison.md) | 竞品对比（vs OpenClaw / ZeroClaw） |
