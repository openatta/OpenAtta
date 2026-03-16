# OpenAtta Architecture

Version: v0.1.0

---

## Overview

OpenAtta is a Rust-based AI operating system that schedules, isolates, and audits AI agents like processes. It ships in two profiles:

| Profile | Infrastructure | Use Case |
|---------|---------------|----------|
| **Desktop** | SQLite + in-process bus + AllowAll authz | Single user, zero external deps |
| **Enterprise** | Postgres + NATS JetStream + RBAC + audit | Multi-node, multi-user |

Both profiles share the same codebase — switching is done via Cargo feature flags, not conditional compilation in business logic.

---

## System Layers

```
Client Layer
  ├── WebUI (Vue 3 SPA, embedded in axum via rust-embed)
  ├── Shell (Tauri v2 WebView + native system tray)
  ├── CLI (attacli status | chat | task | skill | flow)
  └── System Tray (integrated in Shell)

                        ▼ HTTP + SSE + WebSocket

Control Plane (atta-core)
  ├── API Router (axum REST + /api/v1/ws)
  ├── CoreCoordinator (event-driven orchestrator)
  ├── FlowEngine (YAML state machine with approval gates)
  ├── SkillRegistry (load from disk + preloaded)
  └── ToolRegistry (builtin + native + MCP)

                        ▼ EventBus

Execution Layer
  ├── ReactAgent (Observe → Think → Act → Observe loop)
  ├── LlmProvider (Anthropic / OpenAI / DeepSeek / Router / Reliable)
  ├── SecurityGuard (risk classification + approval + rate limit)
  ├── Channel Runtime (22 messaging platforms)
  └── Memory System (FTS5 + vector hybrid search)

Infrastructure Layer
  ├── MCP Registry (SSE + Stdio transports)
  ├── SecretStore (AES-256-GCM encrypted SQLite, with key rotation)
  └── E-Stop Manager (4-level emergency stop)
```

---

## 4 Core Traits (Desktop/Enterprise Switching)

These traits are the architectural seams. Each has a Desktop and Enterprise implementation, swapped via feature flags at compile time.

| Trait | Crate | Desktop | Enterprise |
|-------|-------|---------|------------|
| `EventBus` | atta-bus | `InProcBus` (tokio broadcast) | `NatsBus` (NATS JetStream) |
| `StateStore` | atta-store | `SqliteStore` | `PostgresStore` |
| `Authz` | atta-auth | `AllowAll` | `RBACAuthz` (6-role) |
| `AuditSink` | atta-audit | `NoopAudit` | `AuditStore` |

---

## Secondary Traits

| Trait | Crate | Purpose |
|-------|-------|---------|
| `ToolRegistry` | atta-types | Tool registration, schema listing, invocation |
| `NativeTool` | atta-types | In-process Rust tool implementation contract |
| `LlmProvider` | atta-agent | LLM backend abstraction (chat, stream, model info) |
| `ToolDispatcher` | atta-agent | Parse LLM response into tool calls (native vs XML) |
| `ToolHook` | atta-agent | Pre/post tool execution interception pipeline |
| `PromptSection` | atta-agent | Composable system prompt building blocks |
| `MemoryStore` | atta-memory | Agent memory persistence (store, search, cleanup) |
| `EmbeddingProvider` | atta-memory | Vector embedding generation |
| `SecretStore` | atta-secret | Encrypted key-value secret storage with rotation |
| `McpClient` | atta-mcp | MCP server communication (list tools, call tool) |
| `Channel` | atta-channel | Bi-directional messaging platform integration |
| `ApprovalBackend` | atta-security | Route approval requests (CLI, WebSocket, webhook) |

---

## System Objects

### Event System

**`EventEnvelope`** — the universal event carrier on the bus. `new()` returns `Result<Self, AttaError>`, propagating serialization errors instead of silently replacing with Null.

```
EventEnvelope {
    event_id: Uuid,
    event_type: String,           // e.g. "task.created", "agent.completed"
    occurred_at: DateTime<Utc>,
    actor: Actor,                 // who triggered
    entity: EntityRef,            // what entity is affected
    correlation_id: Uuid,         // trace across related events
    payload: Value,               // arbitrary JSON
}
```

Factory methods: `task_created`, `flow_advanced`, `agent_assigned`, `agent_completed`, `agent_error`, `approval_requested`, `tool_completed`, `agent_stream_delta`, `system_started`, `system_shutdown`, etc. All return `Result<Self, AttaError>`.

### Flow System

**`FlowDef`** — a YAML-defined state machine template.

```
FlowDef {
    id, version, name, description,
    initial_state: String,
    states: HashMap<String, StateDef>,
    on_error: Option<ErrorPolicy>,   // max_retries + fallback state
    skills: Vec<String>,
    source: String,
}

StateDef {
    state_type: Start | Agent | Gate | Parallel | End,
    agent: Option<String>,
    skill: Option<String>,
    gate: Option<GateDef>,           // approval gate with timeout
    transitions: Vec<TransitionDef>, // conditional transitions
    timeout_secs: Option<u64>,       // per-state timeout (for parallel branches)
}

GateDef {
    approver_role: String,
    timeout: String,
    on_timeout: String,              // fallback state on timeout
    notify: Vec<NotifyChannel>,
}
```

**Validation**: `validate_flow_def()` checks for exactly one Start state, at least one End state, all transition targets exist, ErrorPolicy fallback/retry_states reference valid states, and Gate states do not have `auto: true` transitions.

**`Task`** — a running instance of a Flow.

```
Task {
    id: Uuid,
    flow_id: String,
    current_state, status, input, output,
    state_data: Value,               // accumulated state across steps
    created_by: Actor,
    version: u64,                    // optimistic locking
}
```

**Optimistic Locking**: `advance_task` uses `WHERE version = ?` and increments on success. On conflict, returns `AttaError::Conflict` with expected/actual versions.

**`FlowEngine`** — advances Tasks through their FlowDef state machine, evaluates conditions, fires events. Auto-transitions are bounded by `MAX_CASCADE_DEPTH = 100` to prevent infinite loops.

### Agent System

**`ReactAgent`** — the core execution engine (ReAct loop).

```
ReactAgent {
    llm: Arc<dyn LlmProvider>,
    tool_registry: Arc<dyn ToolRegistry>,
    context: ConversationContext,
    max_iterations: usize,
}
```

Methods: `run()` (blocking), `run_streaming()` (delta events). Supports thinking levels, research phases, custom dispatchers, tool execution configs.

**`ConversationContext`** — manages message history with token budget.

**`SystemPromptBuilder`** — composes system prompt from 10 ordered `PromptSection`s: PromptGuard, Identity, Safety, Tools, Skills, Workspace, Runtime, DateTime, ChannelMedia, ConversationControl.

**`PromptGuard`** — detects and neutralizes prompt injection attempts.

**LLM Providers:**
- `AnthropicProvider` — Claude API
- `OpenAiProvider` — OpenAI/compatible APIs
- `DeepSeekProvider` — DeepSeek API
- `ReliableProvider` — retry + fallback chain
- `RouterProvider` — multi-model routing by model ID

### Security System

**`SecurityGuard`** — wraps `ToolRegistry` with security enforcement.

```
SecurityGuard (implements ToolRegistry) {
    inner: Arc<dyn ToolRegistry>,
    policy: SecurityPolicy,
    approval_manager: Option<Arc<ApprovalManager>>,
    estop: Option<Arc<EstopManager>>,
}
```

On every `invoke()`:
1. Check E-Stop state (fail-closed)
2. Classify risk level (Low/Medium/High)
3. Check autonomy level (ReadOnly/Supervised/Full)
4. If Supervised + High risk → route to ApprovalBackend
5. Rate limit (sliding window)
6. Path safety + TOCTOU prevention
7. Shell command validation against allowlist
8. Network access control + SSRF protection
9. Secret scrubbing in outputs

**`EstopManager`** — 4-level emergency stop: KillAll, NetworkKill, DomainBlock, ToolFreeze. Persists to disk, optional OTP to resume. OTP generation uses cryptographic RNG.

**`ApprovalManager`** — session-scoped tool approval with backends:
- `CliApprovalBackend` — interactive stdin prompt
- `WsApprovalBackend` — WebSocket push to UI
- `WebhookApprovalBackend` — POST to external URL

### Tool System

**40+ native tools** organized by category:

| Category | Tools |
|----------|-------|
| File I/O | `file_read`, `file_write`, `file_edit`, `apply_patch` |
| Search | `glob_search`, `content_search` |
| Process | `shell`, `process` |
| Git | `git_ops` |
| Scheduling | `cron`, `cron_list`, `cron_remove`, `cron_update`, `cron_run`, `cron_runs`, `schedule` |
| Multi-agent | `delegation`, `subagent_spawn`, `subagent_list`, `subagent_manage`, `delegate_status` |
| IPC | `agents_list`, `agents_send`, `agents_inbox`, `state_get`, `state_set` |
| Planning | `task_plan` |
| Media | `image_info`, `screenshot`, `pdf_read` |
| Web | `web_fetch`, `web_search`, `http_request`, `url_validation` |
| Memory | `memory_store`, `memory_recall`, `memory_forget` |
| Network | `proxy_config` |
| LLM | `model_routing` |
| System | `cli_discovery` |
| Notifications | `pushover` |

**3 tool binding types:** `Builtin`, `Native` (Rust), `Mcp` (remote).

### Channel System

**22 messaging platforms** (feature-gated), all implementing the `Channel` trait:

Terminal, Webhook, Telegram, Slack, Discord, Lark, DingTalk, QQ, WATI, Mattermost, Nextcloud Talk, ClawdTalk, Signal, WhatsApp, WhatsApp Web, Email, IRC, iMessage, Matrix, MQTT, Nostr, WhatsApp Storage.

**`ChannelRuntimeContext`** — wires channels to the agent pipeline. `start_channels()` spawns supervised listeners with automatic reconnect.

### Memory System

**`SqliteMemoryStore`** — hybrid search:
- FTS5 virtual tables + BM25 scoring (keyword)
- BLOB vector storage + cosine similarity (semantic)
- Weighted hybrid fusion

**`EmbeddingProvider`** trait for pluggable embedding backends. Optional fastembed integration for local embeddings.

### Client Applications

| Binary | Crate | Role |
|--------|-------|------|
| `attaos` | atta-server | Core server daemon (HTTP API + WebUI + Agent execution) |
| `attacli` | atta-cli | Lightweight CLI client (HTTP/SSE) |
| `attash` | atta-shell | Desktop Shell (Tauri v2 WebView + system tray + auto-updater) |

---

## Crate Dependency Graph

```
atta-types (shared domain types, error enums, traits: ToolRegistry, NativeTool)
    │
    ├── atta-bus       (EventBus trait + InProcBus / NatsBus)
    ├── atta-store     (StateStore trait + SqliteStore / PostgresStore)
    ├── atta-auth      (Authz trait + AllowAll / RBACAuthz)
    ├── atta-audit     (AuditSink trait + NoopAudit / AuditStore)
    ├── atta-memory    (MemoryStore + EmbeddingProvider)
    ├── atta-secret    (SecretStore + SqliteSecretStore + key rotation)
    ├── atta-mcp       (McpClient + McpRegistry + SSE/Stdio transports)
    ├── atta-tools     (40+ NativeTool implementations)
    └── atta-agent     (LlmProvider + ReactAgent + ConversationContext)
            │
            ├── atta-security  (SecurityGuard + EstopManager + ApprovalManager)
            └── atta-channel   (Channel trait + 22 platform impls)
                    │
                    └── atta-core  (AppState + CoreCoordinator + FlowEngine + API)
                            │
                            ├── atta-server  (attaos binary)
                            ├── atta-cli     (attacli binary)
                            └── atta-shell   (attash binary, Tauri v2)
```

---

## Data Flow

### Task Execution

```
1. Client creates task via POST /api/v1/tasks
2. FlowEngine creates Task record (with FK validation), publishes "task.created" event
3. CoreCoordinator receives event, advances Flow to first Agent state
4. FlowEngine publishes "flow.advanced" event
5. CoreCoordinator spawns ReactAgent for current state's skill
6. ReactAgent runs ReAct loop:
   a. Build system prompt (SystemPromptBuilder)
   b. Call LlmProvider.chat()
   c. ToolDispatcher parses response
   d. SecurityGuard validates + approves tool calls
   e. Execute tools via ToolRegistry
   f. Loop until LLM returns final answer or max_iterations
7. Agent publishes "agent.completed" event with output
8. CoreCoordinator advances Flow to next state (with optimistic locking)
9. If Gate state → publish "approval.requested", wait
10. Repeat until End state → Task status = Completed
```

### Channel Message Processing

```
1. Channel.listen() receives inbound ChannelMessage
2. process_channel_message() routes to agent pipeline
3. ReactAgent processes with channel-specific system prompt
4. Response sent back via Channel.send()
5. Streaming: AgentDelta events pushed for typing indicators
```

---

## Feature Flags

| Crate | Feature | What it enables |
|-------|---------|----------------|
| atta-bus | `inproc` (default) | tokio broadcast bus |
| atta-bus | `nats` | NATS JetStream bus |
| atta-store | `sqlite` (default) | SQLite state store |
| atta-store | `postgres` | PostgreSQL state store |
| atta-auth | `allow_all` (default) | Permit-all authorization |
| atta-auth | `rbac` | 6-role RBAC |
| atta-tools | `web` | web_fetch, web_search, http_request |
| atta-tools | `memory` | memory_store, memory_recall, memory_forget |
| atta-tools | `browser` | Browser automation tools |
| atta-channel | `terminal` (default) | stdin/stdout channel |
| atta-channel | `telegram` / `slack` / `discord` / ... | Per-platform channels |
| atta-server | `desktop` (default) | Desktop profile bundle |
| atta-server | `enterprise` | Enterprise profile bundle |

---

## Hardening

Key hardening measures implemented in v0.1.0:

- **Optimistic locking** — `version` field on Task, `WHERE version = ?` in SQL UPDATE
- **Authorization** — `check_authz` on all HTTP handlers
- **EventEnvelope Result** — serialization errors propagated, not silently swallowed
- **RwLock poisoning recovery** — graceful recovery with logging across all registries
- **FK validation** — `create_task_with_flow` validates flow_def exists in store
- **Query limits** — `MAX_QUERY_LIMIT = 1000` with `.clamp(1, MAX)` on list endpoints
- **Cascade depth limit** — `MAX_CASCADE_DEPTH = 100` prevents infinite auto-transitions
- **MCP request serialization** — `Mutex` prevents concurrent request interleaving on stdio
- **SQL injection prevention** — `ALLOWED_FILTER_FIELDS` whitelist in audit queries
- **Env sanitization** — whitelist of allowed `ATTA_*` environment variables for child processes
- **CORS middleware** — configurable `CorsLayer` on API routes
- **WebSocket auth** — validation before upgrade

---

## Metrics

| Metric | Value |
|--------|-------|
| Workspace crates | 16 |
| Rust source files | 291 |
| Lines of Rust | ~72,000 |
| Test functions | 1,023 |
| Native tools | 40+ |
| Built-in skills | 12 |
| Built-in flows | 6 |
| Channel integrations | 22 |
| Binary targets | 3 production + 3 fuzz |
