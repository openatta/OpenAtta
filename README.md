<div align="center">

# 🐜 AttaOS

**The AI Agent Operating System**

*Schedule. Isolate. Audit. Like processes, but for AI.*

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-2021_Edition-orange.svg)](https://www.rust-lang.org/)
[![Crates](https://img.shields.io/badge/Workspace-20_crates-green.svg)](#architecture)
[![Tests](https://img.shields.io/badge/Tests-388-brightgreen.svg)](#testing)

[English](README.md) | [中文](docs/README.cn.md)

---

*What if AI agents were managed like OS processes — scheduled, sandboxed, supervised, and auditable?*

</div>

## Why "Atta"?

*Atta* is the leafcutter ant — a species whose colonies contain **8 million individuals** and have thrived for **50 million years**. No central command dictates behavior; intelligence emerges from simple rules, role specialization, and shared communication. AttaOS takes the same principle into software: many autonomous agents, coordinated under clear rules, producing results no single agent could achieve alone.

---

## What is AttaOS?

AttaOS is a **Rust-native operating system for AI agents**. It treats every agent as a managed process with scheduling, isolation, security enforcement, and full auditability — the same guarantees a traditional OS provides to programs, purpose-built for autonomous AI.

The system is structured in four layers:

```
                    ┌──────────────────────────────────────────┐
  Clients           │  WebUI    Console    CLI    System Tray  │
                    └──────────────┬───────────────────────────┘
                                   │ HTTP + WebSocket
                    ┌──────────────▼───────────────────────────┐
  Control Plane     │  API Router  ·  FlowEngine  ·  Skills    │
  (atta-core)       │  CoreCoordinator  ·  ToolRegistry        │
                    └──────────────┬───────────────────────────┘
                                   │ EventBus
                    ┌──────────────▼───────────────────────────┐
  Execution Layer   │  ReactAgent  ·  LLM Providers            │
                    │  SecurityGuard  ·  Channels  ·  Memory   │
                    └──────────────┬───────────────────────────┘
                    ┌──────────────▼───────────────────────────┐
  Infrastructure    │  MCP Servers  ·  Channels                 │
                    │  SecretStore  ·  E-Stop Manager           │
                    └──────────────────────────────────────────┘
```

- **Clients** — WebUI (Vue 3), Tauri console, CLI, and system tray provide multiple entry points.
- **Control Plane** — The `CoreCoordinator` receives tasks, advances Flows through their state machines, and dispatches agent work.
- **Execution Layer** — `ReactAgent` runs the ReAct loop (Observe → Think → Act → Observe), with every tool call passing through `SecurityGuard`.
- **Infrastructure** — MCP servers provide extensibility; `SecretStore` and `EstopManager` enforce operational boundaries.

---

## Architecture

### 4 Core Traits — The Desktop/Enterprise Seam

AttaOS compiles into two profiles from **one codebase**. Four abstract traits define the boundary between business logic and infrastructure:

| Trait | Desktop Implementation | Enterprise Implementation |
|-------|----------------------|--------------------------|
| `EventBus` | tokio broadcast (in-process) | NATS JetStream (distributed) |
| `StateStore` | SQLite (single file) | PostgreSQL (clustered) |
| `Authz` | AllowAll (single user) | RBAC with 6 roles |
| `AuditSink` | NoopAudit | Full audit trail with tamper detection |

Switch at compile time:

```bash
cargo build --features desktop      # Zero external dependencies
cargo build --features enterprise   # Production-grade infrastructure
```

Business logic — agents, flows, skills, tools — is written once against these traits. It never knows which infrastructure it runs on.

### 20-Crate Workspace

Each responsibility lives in its own crate with explicit dependency edges:

```
atta-types ─────── Shared domain types, error enums, trait definitions
    │
    ├── atta-bus ──────── EventBus trait + InProcBus / NatsBus
    ├── atta-store ────── StateStore trait + SqliteStore / PostgresStore
    ├── atta-auth ─────── Authz trait + AllowAll / RBACAuthz
    ├── atta-audit ────── AuditSink trait + NoopAudit / AuditStore
    ├── atta-memory ───── MemoryStore + FTS5/vector hybrid search
    ├── atta-secret ───── AES-256-GCM encrypted key-value storage
    ├── atta-mcp ──────── MCP client (SSE + Stdio transports)
    ├── atta-tools ────── 50+ native Rust tool implementations
    └── atta-agent ────── LLM providers + ReAct engine + prompt system
            │
            ├── atta-security ── SecurityGuard + EstopManager + Approval
            └── atta-channel ─── Channel trait + 22 platform implementations
                    │
                    └── atta-core ──── Control plane (API + FlowEngine + Coordinator)
                            │
                            ├── atta-cli ──────────── attaos binary
                            ├── atta-tray ─────────── System tray (shared logic)
                            ├── atta-tray-standalone ─ Enterprise tray binary
                            ├── atta-console ──────── Tauri management console
                            └── atta-updater ──────── Tauri auto-updater
```

### Data Flow — Task Execution

```
1.  Client POST /api/v1/tasks
2.  FlowEngine creates Task → publishes "task.created"
3.  CoreCoordinator receives event → advances Flow
4.  FlowEngine → "flow.advanced" event
5.  CoreCoordinator spawns ReactAgent with skill's system prompt
6.  ReactAgent ReAct loop:
    a. SystemPromptBuilder composes prompt
    b. LlmProvider.chat() → LLM response
    c. ToolDispatcher parses tool calls
    d. SecurityGuard validates + approves
    e. ToolRegistry.invoke() executes tools
    f. Results feed back to context → loop
7.  Agent → "agent.completed" with output
8.  CoreCoordinator advances Flow to next state
9.  If Gate → "approval.requested" → wait for human
10. Repeat until End → Task completed
```

---

## Security — Defense in Depth

Security is not a feature flag — it is woven into every layer. Every tool invocation passes through a multi-stage enforcement pipeline:

```
Request → E-Stop Check → Risk Classification → Autonomy Level
       → Approval Gate → Rate Limit → Path Safety → SSRF Check
       → Secret Scrubbing → Execute
```

### SecurityGuard

The central policy enforcement point. `SecurityGuard` wraps every tool call, applying the full security pipeline before any side effect can occur. It combines risk assessment, approval routing, and runtime checks in a single composable guard.

### Risk Classification & Approval

`CommandClassifier` categorizes every command as **Low**, **Medium**, or **High** risk. High-risk actions are automatically routed to `ApprovalManager`, which supports three approval channels:

- **CLI prompt** — interactive terminal confirmation
- **WebSocket** — real-time approval via WebUI
- **Webhook** — external approval systems (Slack, PagerDuty, custom)

Approval is not optional for high-risk actions. The agent blocks until a human approves or rejects.

### Emergency Stop (E-Stop)

`EstopManager` provides 4 graduated levels of emergency intervention:

| Level | Action | Scope |
|-------|--------|-------|
| **KillAll** | Terminate all running agents immediately | Global |
| **NetworkKill** | Block all outbound network access | Global |
| **DomainBlock** | Block specific domains | Per-domain |
| **ToolFreeze** | Disable specific tools | Per-tool |

E-Stop is checked **before** every tool invocation. A single API call can halt the entire system.

### RBAC (Enterprise)

Six hierarchical roles control access across the system:

```
Owner → Admin → Operator → Developer → Approver → Viewer
```

Each role inherits permissions from those below it. The `Authz` trait enforces these boundaries — in Desktop mode, `AllowAll` removes the overhead; in Enterprise mode, `RBACAuthz` enforces every check.

### Secret Management

`atta-secret` provides AES-256-GCM encrypted key-value storage. API keys, tokens, and credentials are never stored in plaintext. Secret scrubbing in the security pipeline ensures sensitive values are stripped from agent output and logs.

---

## Enterprise Features

### Flow Orchestration with Approval Gates

The `FlowEngine` executes YAML-defined state machines where each state can be an agent task, a human approval gate, or a conditional branch:

```yaml
id: code-review
initial_state: analyze
states:
  analyze:
    type: Agent
    skill: code-analysis
    transitions:
      - target: review_gate
  review_gate:
    type: Gate
    gate:
      approver_role: tech-lead
      timeout: 24h
      on_timeout: auto_approve
    transitions:
      - condition: approved
        target: deploy
      - condition: rejected
        target: revise
  deploy:
    type: Agent
    skill: deployment
    transitions:
      - target: done
  done:
    type: End
```

Gates are first-class citizens. A flow can require human sign-off at any step, with configurable timeout behavior (block, auto-approve, or auto-reject). This makes AttaOS suitable for regulated environments where AI actions must be supervised.

### Full Audit Trail

In Enterprise mode, `AuditStore` records every significant event: task creation, agent actions, tool invocations, approval decisions, and E-Stop activations. The audit trail is append-only and provides the compliance record required in regulated industries.

### Distributed Event Bus

Enterprise deployments use **NATS JetStream** for inter-node communication. Events are durable, ordered, and deliverable across multiple AttaOS instances. Desktop mode uses tokio broadcast channels for zero-dependency operation — same event semantics, different transport.

### Multi-Model LLM with Failover

Three providers are supported out of the box:

- **Anthropic Claude** — via native API
- **OpenAI** — GPT-4o and compatible models
- **DeepSeek** — OpenAI-compatible API

`ReliableProvider` chains multiple providers into a failover sequence. If the primary provider fails, the next one takes over transparently. `RouterProvider` dispatches tasks to different models based on task type (e.g., coding tasks to Claude, simple queries to DeepSeek).

### 22 Messaging Channels

Agents connect to the platforms where users already work:

| | | |
|---|---|---|
| Terminal | Webhook | Telegram |
| Slack | Discord | Lark / Feishu |
| DingTalk | QQ | WATI |
| Mattermost | Nextcloud Talk | ClawdTalk |
| Signal | WhatsApp | WhatsApp Web |
| Email (IMAP/SMTP) | IRC | iMessage |
| Matrix | MQTT | Nostr |

Each channel implements the `Channel` trait. Adding a new platform means implementing one trait — no changes to agents, flows, or tools.

---

## Agent Execution Engine

The **ReAct loop** (Observe → Think → Act → Observe) drives every agent:

- **Streaming deltas** — real-time `AgentDelta` events (Thinking → ToolStart → ToolComplete → TextChunk → Done) for responsive UIs
- **Prompt engineering** — `SystemPromptBuilder` composes 10 ordered sections; `PromptGuard` detects injection attempts
- **Research phase** — optional pre-loop information gathering before the main ReAct cycle
- **Sub-agent delegation** — `DelegationTool` spawns child agents with scoped tools and configurable timeout

### 50+ Native Tools

| Category | Examples |
|----------|---------|
| **File I/O** | `file_read`, `file_write`, `file_edit`, `apply_patch` |
| **Search** | `glob_search`, `content_search` |
| **Shell** | `shell`, `process` |
| **Git** | `git_ops` |
| **Web** | `web_fetch`, `web_search`, `http_request` |
| **Memory** | `memory_store`, `memory_recall`, `memory_forget` |
| **Scheduling** | `cron`, `schedule`, `cron_list` |
| **Multi-Agent** | `delegation`, `subagent_spawn`, `subagent_list` |
| **Media** | `image_info`, `screenshot`, `pdf_read` |
| **IPC** | `agents_list`, `agents_send`, `agents_inbox` |

Plus **MCP protocol** support (SSE + Stdio transports) for connecting to remote tool servers.

### Hybrid Memory

Agents remember across conversations with dual-mode search:

- **FTS5** — BM25-scored keyword search for precise recall
- **Vector similarity** — cosine distance for semantic understanding
- **Hybrid fusion** — weighted combination of both result sets
- **Pluggable embeddings** — bring your own `EmbeddingProvider`

### Desktop Experience

| Component | Technology |
|-----------|-----------|
| **Web UI** | Vue 3 + Vite + Pinia, embedded in the binary via `rust-embed` |
| **System Tray** | `tray-icon` + `muda` with menu integration |
| **Console App** | Tauri v2 WebView (< 10 MB vs 100+ MB Electron) |
| **Auto-Updater** | Tauri v2 updater with GitHub Releases integration |

---

## Quick Start

### Prerequisites

- **Rust** 1.75+ (2021 edition)
- One of: `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, or `DEEPSEEK_API_KEY`

### Build & Run

```bash
# Clone
git clone https://github.com/anthropics/attaos.git
cd attaos

# Build Desktop version
cargo build --features desktop

# Run the server
cargo run --bin attaos -- run --port 3000

# Or start an interactive chat
cargo run --bin attaos -- chat
```

### Environment Variables

```bash
# LLM Providers (set one or more — multiple keys enable automatic failover)
export ANTHROPIC_API_KEY="sk-ant-..."
export OPENAI_API_KEY="sk-..."
export DEEPSEEK_API_KEY="sk-..."

# Optional: override defaults
export ANTHROPIC_MODEL="claude-sonnet-4-20250514"
export OPENAI_MODEL="gpt-4o"
export DEEPSEEK_MODEL="deepseek-chat"
```

### CLI Commands

```bash
attaos run [--mode desktop|enterprise] [--port 3000]  # Start server
attaos launch [--port 3000]                            # Start with update check
attaos chat [--channel terminal]                       # Interactive chat
attaos channels                                        # List available channels
attaos task list|create|get                             # Task management
attaos skill list|get|run                               # Skill management
```

---

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Language | Rust 2021 edition |
| Async Runtime | Tokio |
| HTTP | axum |
| Serialization | serde / serde_json / serde_yaml |
| Database | sqlx (SQLite + Postgres) |
| Logging | tracing + tracing-subscriber |
| Event Bus | tokio mpsc/broadcast · async-nats |
| Error Handling | thiserror + anyhow |
| CLI | clap |
| Web UI | Vue 3 + Vite + Pinia |
| System Tray | tray-icon + muda |
| Desktop Apps | Tauri v2 |
| Crypto | AES-256-GCM · Ed25519 · SHA-256 |

---

## Testing

```bash
# Run all tests
cargo test --workspace

# Run tests for a specific crate
cargo test -p atta-agent

# Run live LLM integration tests (requires API key)
ATTA_LIVE_TEST=1 cargo test -p atta-agent --test provider_live_deepseek -- --nocapture

# Lint
cargo clippy --workspace --all-targets -- -D warnings

# Format check
cargo fmt --all -- --check
```

| Metric | Count |
|--------|-------|
| Unit + integration tests | 388 |
| Live LLM integration tests | 10 |
| Benchmark suites | 1 |

---

## Project Metrics

| Metric | Value |
|--------|-------|
| Workspace crates | 20 |
| Rust source files | 253 |
| Lines of Rust code | ~41,000 |
| Native tools | 50+ |
| Channel integrations | 22 |
| LLM providers | 3 (+ failover + routing) |
| Production binaries | 4 |
| Third-party dependencies | 784 (all Apache-2.0 compatible) |

---

## Documentation

| Document | Description |
|----------|-------------|
| [Architecture](docs/architecture.md) | System layers, core traits, data flow, feature flags |
| [Tech Stack](docs/tech-stack.md) | Full dependency list with selection rationale |
| [Usage Guide](docs/usage.md) | CLI, API endpoints, configuration, skill/flow definitions |
| [Comparison](docs/comparison.md) | Feature comparison vs OpenClaw and ZeroClaw |
| [Third-Party Notices](THIRD-PARTY-NOTICES.md) | License attribution for all 784 dependencies |

---

## License

Licensed under the [Apache License, Version 2.0](LICENSE).

See [NOTICE](NOTICE) for attribution and [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) for dependency licenses.
