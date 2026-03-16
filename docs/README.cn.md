<div align="center">

# OpenAtta

**AI Agent 操作系统**

*调度 · 隔离 · 审计 —— 像管理进程一样管理 AI*

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](../LICENSE)
[![Rust](https://img.shields.io/badge/Rust-2021_Edition-orange.svg)](https://www.rust-lang.org/)
[![Crates](https://img.shields.io/badge/Workspace-16_crates-green.svg)](#架构)
[![Tests](https://img.shields.io/badge/Tests-1023-brightgreen.svg)](#测试)

[English](../README.md) | [中文](README.cn.md)

---

*如果 AI Agent 能像操作系统中的进程一样被调度、沙箱隔离、权限管控、全程审计——会怎样？*

</div>

## 为什么叫 "Atta"？

*Atta* 是切叶蚁的属名——这个物种的单个群落包含 **800 万个体**，已繁荣了 **5000 万年**。没有中央指挥调度行为，智能从简单规则、角色分工和共享通信中涌现。OpenAtta 将同样的原则带入软件：众多自主 Agent 在清晰规则下协调运作，产生单个 Agent 无法独立完成的成果。

---

## 什么是 OpenAtta？

OpenAtta 是一个 **Rust 原生的 AI Agent 操作系统**。它将每个 Agent 视为受管进程，提供调度、隔离、安全执行和全程审计——传统操作系统为程序提供的同等保障，专为自主 AI 设计。

系统分为四层：

```
                    ┌──────────────────────────────────────────┐
  客户端             │  WebUI    Shell    CLI    系统托盘        │
                    └──────────────┬───────────────────────────┘
                                   │ HTTP + SSE + WebSocket
                    ┌──────────────▼───────────────────────────┐
  控制面             │  API 路由  ·  FlowEngine  ·  Skills      │
  (atta-core)       │  CoreCoordinator  ·  ToolRegistry        │
                    └──────────────┬───────────────────────────┘
                                   │ EventBus
                    ┌──────────────▼───────────────────────────┐
  执行层             │  ReactAgent  ·  LLM 提供者                │
                    │  SecurityGuard  ·  Channels  ·  Memory   │
                    └──────────────┬───────────────────────────┘
                    ┌──────────────▼───────────────────────────┐
  基础设施           │  MCP 服务器  ·  Channels                   │
                    │  SecretStore  ·  紧急停止管理器             │
                    └──────────────────────────────────────────┘
```

- **客户端** —— WebUI (Vue 3)、Tauri Shell（原生桌面）、CLI、系统托盘提供多种入口。
- **控制面** —— `CoreCoordinator` 接收任务，驱动 Flow 状态机，分派 Agent 工作。
- **执行层** —— `ReactAgent` 执行 ReAct 循环（Observe → Think → Act → Observe），每次工具调用必经 `SecurityGuard`。
- **基础设施** —— MCP 服务器提供可扩展性；`SecretStore` 和 `EstopManager` 强制执行运行边界。

---

## 架构

### 4 个核心 Trait —— Desktop/Enterprise 的架构接缝

OpenAtta 从 **同一套代码** 编译出两个版本。四个抽象 trait 定义了业务逻辑与基础设施之间的边界：

| Trait | Desktop 实现 | Enterprise 实现 |
|-------|-------------|----------------|
| `EventBus` | tokio broadcast（进程内） | NATS JetStream（分布式） |
| `StateStore` | SQLite（单文件） | PostgreSQL（集群） |
| `Authz` | AllowAll（单用户） | RBAC 6 级角色 |
| `AuditSink` | NoopAudit | 完整审计链路 + 防篡改 |

编译时切换：

```bash
cargo build -p atta-server --features desktop      # 零外部依赖
cargo build -p atta-server --features enterprise   # 生产级基础设施
```

业务逻辑——Agent、Flow、Skill、Tool——只针对这些 trait 编写一次，永远不感知底层运行的基础设施。

### 16 Crate 工作区

每项职责独立成 crate，依赖边界明确：

```
atta-types ─────── 共享领域类型、错误枚举、trait 定义
    │
    ├── atta-bus ──────── EventBus trait + InProcBus / NatsBus
    ├── atta-store ────── StateStore trait + SqliteStore / PostgresStore
    ├── atta-auth ─────── Authz trait + AllowAll / RBACAuthz
    ├── atta-audit ────── AuditSink trait + NoopAudit / AuditStore
    ├── atta-memory ───── MemoryStore + FTS5/向量混合搜索
    ├── atta-secret ───── AES-256-GCM 加密键值存储
    ├── atta-mcp ──────── MCP 客户端（SSE + Stdio 传输）
    ├── atta-tools ────── 40+ 原生 Rust 工具实现
    └── atta-agent ────── LLM 提供者 + ReAct 引擎 + 提示系统
            │
            ├── atta-security ── SecurityGuard + EstopManager + 审批
            └── atta-channel ─── Channel trait + 22 平台实现
                    │
                    └── atta-core ──── 控制面（API + FlowEngine + Coordinator）
                            │
                            ├── atta-server ──── attaos 守护进程
                            ├── atta-cli ──────── attacli 客户端
                            └── atta-shell ────── attash 桌面 Shell（Tauri v2）
```

### 数据流 —— 任务执行

```
1.  客户端 POST /api/v1/tasks
2.  FlowEngine 创建 Task → 发布 "task.created"
3.  CoreCoordinator 接收事件 → 推进 Flow
4.  FlowEngine → "flow.advanced" 事件
5.  CoreCoordinator 用 Skill 的系统提示生成 ReactAgent
6.  ReactAgent ReAct 循环：
    a. SystemPromptBuilder 组装提示词
    b. LlmProvider.chat() → LLM 响应
    c. ToolDispatcher 解析工具调用
    d. SecurityGuard 验证 + 审批
    e. ToolRegistry.invoke() 执行工具
    f. 结果回注上下文 → 循环
7.  Agent → "agent.completed" + 输出
8.  CoreCoordinator 推进 Flow 到下一状态
9.  如果是 Gate → "approval.requested" → 等待人工审批
10. 重复直到 End → 任务完成
```

---

## 安全体系 —— 纵深防御

安全不是一个可选功能——它融入每一层。每次工具调用都经过多阶段执行管道：

```
请求 → 紧急停止检查 → 风险分类 → 自治级别判定
     → 审批门控 → 速率限制 → 路径安全 → SSRF 检查
     → 密钥脱敏 → 执行
```

### SecurityGuard

核心策略执行点。`SecurityGuard` 包装每次工具调用，在任何副作用发生之前应用完整安全管道。它将风险评估、审批路由和运行时检查组合在一个可组合的守卫中。

### 风险分类与审批

`CommandClassifier` 将每个命令分类为 **Low**、**Medium** 或 **High** 风险。高风险操作自动路由到 `ApprovalManager`，支持三种审批通道：

- **CLI 提示** —— 交互式终端确认
- **WebSocket** —— 通过 WebUI 实时审批
- **Webhook** —— 外部审批系统（Slack、PagerDuty、自定义）

高风险操作的审批不可跳过。Agent 将阻塞直到人工批准或拒绝。

### 紧急停止（E-Stop）

`EstopManager` 提供 4 个分级紧急干预级别：

| 级别 | 操作 | 范围 |
|------|------|------|
| **KillAll** | 立即终止所有运行中的 Agent | 全局 |
| **NetworkKill** | 阻断所有出站网络访问 | 全局 |
| **DomainBlock** | 封锁特定域名 | 按域名 |
| **ToolFreeze** | 禁用特定工具 | 按工具 |

E-Stop 在 **每次** 工具调用之前检查。一个 API 调用即可停止整个系统。

### RBAC（企业版）

六个分层角色控制系统级访问：

```
Owner → Admin → Operator → Developer → Approver → Viewer
```

每个角色继承其下层的权限。`Authz` trait 强制执行边界——Desktop 模式下 `AllowAll` 消除开销；Enterprise 模式下 `RBACAuthz` 执行每项检查。

### 密钥管理

`atta-secret` 提供 AES-256-GCM 加密键值存储，支持密钥轮换。API Key、Token、凭证永不明文存储。安全管道中的密钥脱敏确保敏感值从 Agent 输出和日志中被剥离。

---

## 企业特性

### Flow 编排与审批门控

`FlowEngine` 执行 YAML 定义的状态机，每个状态可以是 Agent 任务、人工审批门控或条件分支：

```yaml
id: code-review
initial_state: start
states:
  start:
    type: start
    transitions:
      - to: analyze
        auto: true
  analyze:
    type: agent
    skill: code-review
    transitions:
      - to: review_gate
        when: "has_high_risk_tools"
      - to: done
        auto: true
  review_gate:
    type: gate
    gate:
      approver_role: developer
      timeout: "24h"
      on_timeout: done
    transitions:
      - to: apply_fixes
        when: "approved"
      - to: done
        when: "denied"
  apply_fixes:
    type: agent
    skill: fix-bug
    transitions:
      - to: done
        auto: true
  done:
    type: end
```

Gate 是一等公民。Flow 可以在任意步骤要求人工签字确认，超时行为可配置（阻塞、自动批准、自动拒绝）。这使 OpenAtta 适用于需要监管 AI 操作的合规环境。

内置 6 个 Flow 模板：`bug-triage`、`code-review`、`daily-digest`、`prd-to-code`、`research-report`、`skill-onboard`。

### 完整审计链路

Enterprise 模式下，`AuditStore` 记录每个重要事件：任务创建、Agent 操作、工具调用、审批决策、E-Stop 激活。审计链路仅追加写入，通过白名单验证过滤字段防止 SQL 注入，为受监管行业提供合规记录。

### 分布式事件总线

企业部署使用 **NATS JetStream** 进行节点间通信。事件持久化、有序、可跨多个 OpenAtta 实例投递。Desktop 模式使用 tokio broadcast channel 实现零依赖运行——同样的事件语义，不同的传输层。

### 多模型 LLM 与故障转移

开箱支持三个提供者：

- **Anthropic Claude** —— 原生 API
- **OpenAI** —— GPT-4o 及兼容模型
- **DeepSeek** —— OpenAI 兼容 API

`ReliableProvider` 将多个提供者串联为故障转移链。主提供者失败时，下一个无缝接管。`RouterProvider` 根据任务类型将请求分发到不同模型（如编码任务发给 Claude，简单查询发给 DeepSeek）。

### 22 个消息通道

Agent 连接到用户已有的工作平台：

| | | |
|---|---|---|
| Terminal | Webhook | Telegram |
| Slack | Discord | 飞书 / Lark |
| 钉钉 | QQ | WATI |
| Mattermost | Nextcloud Talk | ClawdTalk |
| Signal | WhatsApp | WhatsApp Web |
| 邮件 (IMAP/SMTP) | IRC | iMessage |
| Matrix | MQTT | Nostr |

每个通道实现 `Channel` trait。添加新平台只需实现一个 trait——无需修改 Agent、Flow 或 Tool。

---

## Agent 执行引擎

**ReAct 循环**（Observe → Think → Act → Observe）驱动每个 Agent：

- **流式增量** —— 实时 `AgentDelta` 事件（Thinking → ToolStart → ToolComplete → TextChunk → Done），响应式 UI
- **提示工程** —— `SystemPromptBuilder` 组合 10 个有序段落；`PromptGuard` 检测注入攻击
- **研究阶段** —— 可选的预循环信息收集阶段
- **子 Agent 委派** —— `DelegationTool` 生成带作用域工具和可配置超时的子 Agent

### 40+ 原生工具

| 类别 | 示例 |
|------|------|
| **文件 I/O** | `file_read`、`file_write`、`file_edit`、`apply_patch` |
| **搜索** | `glob_search`、`content_search` |
| **Shell** | `shell`、`process` |
| **Git** | `git_ops` |
| **Web** | `web_fetch`、`web_search`、`http_request` |
| **记忆** | `memory_store`、`memory_recall`、`memory_forget` |
| **调度** | `cron`、`schedule`、`cron_list`、`cron_update`、`cron_run` |
| **多 Agent** | `delegation`、`subagent_spawn`、`subagent_list` |
| **媒体** | `image_info`、`screenshot`、`pdf_read` |
| **IPC** | `agents_list`、`agents_send`、`agents_inbox` |

另支持 **MCP 协议**（SSE + Stdio 传输）连接远程工具服务器。

### 12 个内置 Skill

| Skill | 说明 |
|-------|------|
| `atta-code-review` | 代码审查：Bug、安全、风格 |
| `atta-fix-bug` | 诊断与修复 Bug |
| `atta-research` | 主题研究与信息整理 |
| `atta-summarize` | 文本摘要 |
| `atta-prd-writer` | 从需求生成结构化 PRD |
| `atta-spec-writer` | 从 PRD 生成技术规格 |
| `atta-task-planner` | 将规格分解为实施任务 |
| `atta-code-generator` | 根据任务计划生成代码 |
| `atta-spec-verifier` | 验证实现是否符合规格 |
| `atta-code-fixer` | 修复验证中发现的问题 |
| `atta-find-skills` | 发现可用 Skill |
| `atta-skill-creator` | 从模板创建新 Skill |

### 混合记忆系统

Agent 跨会话记忆，双模搜索：

- **FTS5** —— BM25 评分的关键词搜索，精确召回
- **向量相似度** —— 余弦距离语义理解
- **混合融合** —— 两种结果集的加权组合
- **可插拔嵌入** —— 自带 `EmbeddingProvider`

### 桌面体验

| 组件 | 技术 |
|------|------|
| **Web UI** | Vue 3 + Vite + Pinia + vue-i18n（中英双语），通过 `rust-embed` 嵌入二进制 |
| **桌面 Shell** | Tauri v2 WebView + 原生系统托盘 + 自动更新（< 10 MB） |
| **CLI 客户端** | `attacli` —— 轻量 HTTP/SSE 客户端 |

---

## 快速开始

### 前置条件

- **Rust** 1.75+（2021 edition）
- 以下任一 API Key：`ANTHROPIC_API_KEY`、`OPENAI_API_KEY`、`DEEPSEEK_API_KEY`

### 构建与运行

```bash
# 克隆
git clone https://github.com/openatta/OpenAtta.git
cd OpenAtta

# 构建 Desktop 版服务
cargo build -p atta-server --features desktop

# 构建 CLI 客户端
cargo build -p atta-cli

# 运行服务
cargo run -p atta-server -- --port 3000

# 在另一个终端检查状态
cargo run -p atta-cli -- status
```

在浏览器中打开 `http://localhost:3000` 访问 WebUI。

### 环境变量

```bash
# LLM 提供者（设置一个或多个 —— 多 key 自动启用故障转移）
export ANTHROPIC_API_KEY="sk-ant-..."
export OPENAI_API_KEY="sk-..."
export DEEPSEEK_API_KEY="sk-..."

# 可选：覆盖默认值
export ANTHROPIC_MODEL="claude-sonnet-4-20250514"
export OPENAI_MODEL="gpt-4o"
export DEEPSEEK_MODEL="deepseek-chat"
```

### 二进制

| 二进制 | Crate | 说明 |
|--------|-------|------|
| `attaos` | `crates/server` | 核心服务守护进程：HTTP API + WebUI + Agent 执行 |
| `attacli` | `crates/cli` | 轻量 CLI 客户端：HTTP/SSE 通信 |
| `attash` | `apps/shell/src-tauri` | 桌面 Shell：Tauri WebView + 原生系统托盘 |

### CLI 命令

```bash
# attaos 服务
attaos [--mode desktop|enterprise] [--port 3000]    # 启动服务

# attacli 客户端
attacli status                                       # 检查服务状态
attacli chat                                         # 交互式聊天
attacli task list|create|get                         # 任务管理
attacli flow list|get                                # Flow 管理
attacli skill list|get|run                           # Skill 管理
attacli tool list|get                                # 工具管理
attacli approval list|approve|deny                   # 审批管理
```

---

## 技术栈

| 层次 | 技术 |
|------|------|
| 语言 | Rust 2021 edition |
| 异步运行时 | Tokio |
| HTTP 框架 | axum 0.7 |
| 序列化 | serde / serde_json / serde_yml |
| 数据库 | sqlx 0.8（SQLite + Postgres） |
| 日志 | tracing + tracing-subscriber |
| 事件总线 | tokio mpsc/broadcast · async-nats |
| 错误处理 | thiserror + anyhow |
| CLI | clap 4 |
| Web UI | Vue 3 + Vite + Pinia + vue-i18n |
| 桌面 Shell | Tauri v2 |
| 加密 | AES-256-GCM · HKDF · SHA-256 |

---

## 测试

```bash
# 运行所有测试（排除 Shell）
cargo test --workspace --exclude atta-shell

# 运行特定 crate 测试
cargo test -p atta-core

# 运行真实 LLM 集成测试（需要 API Key）
ATTA_LIVE_TEST=1 cargo test -p atta-agent --test provider_live_deepseek -- --nocapture

# Lint 检查
cargo clippy --workspace --exclude atta-shell --all-targets -- -D warnings

# 格式检查
cargo fmt --all -- --check
```

| 指标 | 数量 |
|------|------|
| 单元 + 集成测试 | 1,023 |
| 真实 LLM 集成测试 | 10 |
| 基准测试套件 | 1 |

---

## 项目指标

| 指标 | 数值 |
|------|------|
| Workspace crate 数 | 16 |
| Rust 源文件 | 291 |
| Rust 代码行数 | ~72,000 |
| 原生工具 | 40+ |
| 内置 Skill | 12 |
| 内置 Flow | 6 |
| 通道集成 | 22 |
| LLM 提供者 | 3（+ 故障转移 + 路由） |
| 生产二进制 | 3（attaos、attacli、attash） |

---

## 文档

| 文档 | 说明 |
|------|------|
| [系统架构](architecture.md) | 系统分层、核心 trait、数据流、Feature Flag |
| [Client-Server 架构](client-server-architecture.md) | 二进制职责、通信协议、自动启动 |
| [技术选型](tech-stack.md) | 完整依赖列表及选型理由 |
| [使用指南](usage.md) | CLI、API 端点、配置、Skill/Flow 定义 |
| [竞品对比](comparison.md) | 与 OpenClaw、ZeroClaw 的功能对比 |
| [第三方声明](../THIRD-PARTY-NOTICES.md) | 依赖的许可证归属 |

---

## 许可证

基于 [Apache License, Version 2.0](../LICENSE) 开源。

查看 [NOTICE](../NOTICE) 了解归属信息，查看 [THIRD-PARTY-NOTICES.md](../THIRD-PARTY-NOTICES.md) 了解依赖许可证。
