# Competitive Comparison: OpenAtta vs OpenClaw vs ZeroClaw

Version: v0.1.0

---

## Project Identity

| | OpenAtta | OpenClaw | ZeroClaw |
|---|---|---|---|
| **Language** | Rust | TypeScript (Node.js >= 22) | Rust |
| **Version** | 0.1.0 | 2026.3.3 | 0.1.7 |
| **License** | Apache-2.0 | MIT | MIT / Apache-2.0 |
| **Positioning** | AI Operating System (schedule, isolate, audit agents) | Personal AI Assistant (multi-channel gateway) | Zero-overhead Agent Runtime (edge-friendly) |
| **Architecture** | Workspace monorepo (16 crates) | pnpm monorepo (~100 packages) | Single crate (~19,000 lines) |
| **Test count** | 1,023 | ~100 (vitest) | 1,050 |

---

## Architecture Philosophy

### OpenAtta — "Agent as Process"
- Agents are managed like OS processes: scheduled by FlowEngine, audited via AuditSink
- 4 core traits enable Desktop/Enterprise dual profile at compile time
- Flow state machine (YAML DSL) with approval gates — unique among the three
- Layered security: Core authz → Flow approval → SecurityGuard
- Optimistic locking on task state transitions

### OpenClaw — "Gateway + Nodes"
- Central Gateway (Express + WS) routes messages between channels and a pi-agent-core
- Companion apps (macOS/iOS/Android) act as "nodes" with device capabilities
- Plugin SDK via npm packages (30+ channel-specific SDKs)
- Consumer UX focus: Voice Wake, Talk Mode, Live Canvas

### ZeroClaw — "Single Binary, 8 Traits"
- Everything in one crate, 8 pluggable traits (Provider, Channel, Memory, Tunnel, Tool, Observability, Sandbox, Scheduler)
- Size-optimized for edge hardware (Raspberry Pi, STM32)
- Hardware peripherals: USB, serial, GPIO — unique among the three
- 7-step onboard wizard, migration tool from OpenClaw

---

## Feature Matrix

### AI Providers

| Provider | OpenAtta | OpenClaw | ZeroClaw |
|----------|--------|----------|----------|
| Anthropic Claude | native | native | native |
| OpenAI / compatible | native | native | native |
| DeepSeek | native | - | native |
| Google Gemini | - | native | native |
| AWS Bedrock | - | native | native |
| Ollama | - | - | native |
| Groq | - | native | native |
| Mistral | - | native | native |
| xAI / Grok | - | - | native |
| Local LLM (llama.cpp) | - | peer dep | native |
| 35+ total via catalog | - | - | native |
| Multi-provider failover | `ReliableProvider` | per-sender routing | per-model routing |
| Router / model switching | `RouterProvider` | per-session `/model` | custom:<url> |
| **Total providers** | **3 native + failover** | **~10** | **35+** |

### Messaging Channels

| Channel | OpenAtta | OpenClaw | ZeroClaw |
|---------|--------|----------|----------|
| CLI / Terminal | feature | - | built-in |
| Webhook | feature | - | built-in |
| Telegram | feature | native | native |
| Slack | feature | native | native |
| Discord | feature | native | native |
| WhatsApp (Cloud API) | feature | Baileys (Web) | Cloud API |
| WhatsApp Web | feature | - | wa-rs |
| Signal | feature | - | signal-cli bridge |
| iMessage | feature | BlueBubbles | AppleScript bridge |
| Matrix | feature | - | matrix-sdk E2EE |
| Lark / Feishu | feature | - | WebSocket + webhook |
| DingTalk | feature | - | stream |
| QQ | feature | - | webhook + WS |
| Email | feature | - | IMAP + SMTP |
| IRC | feature | - | built-in |
| Mattermost | feature | - | polling |
| Nextcloud Talk | feature | - | webhook |
| Nostr | feature | - | NIP-04 + NIP-17 |
| MQTT | feature | - | - |
| LINE | - | native | - |
| Google Chat | - | native | - |
| Microsoft Teams | - | native | - |
| Twitch | - | native | - |
| Linq (iMessage/RCS/SMS) | - | - | native |
| **Total** | **22 (stubs)** | **22 (production)** | **18 (production)** |

Note: OpenAtta channel implementations are largely stubs with TODO markers. OpenClaw and ZeroClaw channels are production-ready.

### Agent & Orchestration

| Capability | OpenAtta | OpenClaw | ZeroClaw |
|------------|--------|----------|----------|
| Agent loop | ReAct (Observe→Think→Act) | pi-agent-core ReAct | Agent loop (Message→Memory→LLM→Tools→Memory→Response) |
| Flow orchestration | YAML state machine (FlowEngine) | - | - |
| Approval gates (WAIT_APPROVAL) | native (GateDef) | - | - |
| Multi-agent delegation | `DelegationTool` + sub-agents | sessions_send (agent-to-agent) | - |
| Skill system | Markdown SkillDef + SkillRegistry | skill manifests + ClawHub | TOML manifests + SkillForge |
| Streaming (tool call deltas) | `AgentDelta` events | block streaming | - |
| Thinking level control | `ThinkingLevel` enum | `/think` command | - |
| Prompt guard (injection detection) | `PromptGuard` module | mitigations | - |
| System prompt composition | `SystemPromptBuilder` (10 sections) | - | workspace MD files (8) |
| Conversation context management | `ConversationContext` (token budget) | session model | rolling context |

### Security

| Capability | OpenAtta | OpenClaw | ZeroClaw |
|------------|--------|----------|----------|
| Tool risk classification | `CommandClassifier` (Low/Medium/High) | - | - |
| Autonomy levels | ReadOnly / Supervised / Full | - | - |
| Tool approval flow | `ApprovalManager` (CLI/WS/Webhook) | - | `/approve` commands (3 modes) |
| E-Stop | 4 levels (KillAll/NetworkKill/DomainBlock/ToolFreeze) | - | 4 levels (kill-all/network-kill/domain-block/tool-freeze) |
| Rate limiting | Sliding window (per-minute) | - | Sliding window + cost/day cap |
| RBAC | 6 roles (Owner→Viewer) | - | - |
| Filesystem sandbox | `FileSandbox` | - | Path jail + symlink detection + levels |
| Secret encryption | AES-256-GCM + key rotation | - | ChaCha20Poly1305 (XSalsa20Poly1305) |
| Authz on all API endpoints | `check_authz` on every handler | - | - |
| Shell command validation | Allowlist-based | `/elevated` toggle | - |
| SSRF protection | IP validation in SecurityGuard | - | - |
| Audit trail | `AuditSink` trait (Enterprise) | - | Roadmap (HMAC JSONL) |
| SQL injection prevention | Whitelist-validated filter fields | - | - |
| Optimistic locking | Task version field | - | - |

### Plugin & Extension System

| Capability | OpenAtta | OpenClaw | ZeroClaw |
|------------|--------|----------|----------|
| Plugin runtime | MCP + Native Tool | npm packages | wasmi (optional, in-process) |
| MCP protocol | SSE + Stdio transports | - | - |
| Package format | `.apkg` (zip + manifest + Ed25519 signing) | npm | TOML manifests |
| Native tools | 40+ Rust implementations | ~20 (JS) | ~30 (Rust) |
| Composio integration | - | - | 1000+ OAuth tools (optional) |

### Memory System

| Capability | OpenAtta | OpenClaw | ZeroClaw |
|------------|--------|----------|----------|
| Vector search | SQLite BLOB + cosine similarity | sqlite-vec or LanceDB | SQLite BLOB + cosine similarity |
| Keyword search | FTS5 + BM25 | - | FTS5 + BM25 |
| Hybrid fusion | Weighted (vector + keyword) | - | Weighted (vector + keyword) |
| Embeddings | Pluggable `EmbeddingProvider` + fastembed | - | OpenAI / custom URL / noop |
| Chunking | - | - | Markdown-aware with heading context |
| Postgres backend | PostgresMemoryStore | - | Optional feature |

### Infrastructure & Operations

| Capability | OpenAtta | OpenClaw | ZeroClaw |
|------------|--------|----------|----------|
| Database | SQLite / Postgres (dual profile) | SQLite | SQLite / Postgres (feature flag) |
| Event bus | tokio broadcast / NATS JetStream | - | - |
| HTTP server | axum 0.7 | Express 5 | axum 0.8 |
| WebSocket | axum WS (WsHub broadcast, with auth) | ws (WS server) | tokio-tungstenite |
| CORS | tower-http CorsLayer | - | - |
| Daemon install | - | launchd/systemd | launchd/systemd |
| Docker support | - | Dockerfile | - |
| Tunnel support | SSH tunnel (attash) | Tailscale | Cloudflare / Tailscale / ngrok / custom |
| Auto-update | Tauri updater (attash) | - | `zeroclaw update` |
| Health diagnostics | `/api/v1/health` + `/api/v1/diagnostics` | `openclaw doctor` | `zeroclaw doctor` |
| i18n | vue-i18n (en / zh-CN) | - | - |

### Desktop & UI

| Capability | OpenAtta | OpenClaw | ZeroClaw |
|------------|--------|----------|----------|
| Web UI | Vue 3 SPA (embedded in binary, i18n) | Control panel + WebChat | rust-embed dashboard |
| System Tray | Integrated in Tauri Shell | macOS menu bar app | - |
| Native app | Tauri v2 Shell (WebView + tray + updater) | macOS/iOS/Android | - |
| Voice | - | Wake word, PTT, TTS (ElevenLabs) | - |
| Live Canvas | - | A2UI agent-driven workspace | - |
| Browser control | Feature-gated (headless_chrome) | CDP + Playwright | fantoccini (WebDriver, optional) |

### Hardware & IoT (ZeroClaw unique)

| Capability | OpenAtta | OpenClaw | ZeroClaw |
|------------|--------|----------|----------|
| USB device enumeration | - | - | nusb |
| Serial port communication | - | - | tokio-serial |
| Raspberry Pi GPIO | - | - | rppal |
| STM32 / probe-rs | - | - | probe-rs |
| Robot kit | - | - | robot-kit crate |

---

## Unique Strengths

### OpenAtta
1. **Flow state machine with approval gates** — the only project with YAML-defined orchestration and `WAIT_APPROVAL` gates
2. **Dual Desktop/Enterprise profile** — same codebase, compile-time switching via 4 core traits
3. **6-role RBAC** (Enterprise) — Owner, Admin, Operator, Developer, Approver, Viewer
4. **Full audit trail** (Enterprise) — structured `AuditEntry` with correlation tracking
5. **MCP protocol** — SSE + Stdio transports for remote tool servers
6. **Comprehensive security hardening** — optimistic locking, authz on all endpoints, SQL injection prevention, env sanitization
7. **22 messaging channels** — broadest channel coverage with supervisor-managed lifecycle
8. **12 built-in skills + 6 flow templates** — end-to-end workflows (PRD→Code, Bug Triage, etc.)

### OpenClaw
1. **Consumer UX** — Voice Wake, Talk Mode, Live Canvas, companion apps on 3 platforms
2. **Mature channel implementations** — 22 production-ready platforms
3. **Plugin SDK ecosystem** — 30+ npm packages with per-channel SDKs
4. **A2UI** — agent-driven visual workspace (unique)
5. **Device nodes** — macOS/iOS/Android with camera, screen recording, GPS, contacts

### ZeroClaw
1. **Edge efficiency** — < 5 MB RAM, < 10 ms cold start, 8.8 MB binary
2. **35+ AI providers** — broadest provider support
3. **Hardware peripherals** — USB, serial, GPIO, STM32 (unique)
4. **Comprehensive onboarding** — 7-step wizard with live connection testing
5. **Observability** — built-in Prometheus + optional OpenTelemetry
6. **Single binary simplicity** — everything in one crate, zero external deps at runtime
7. **1,050 tests** — highest test coverage

---

## Summary

```
                    OpenAtta            OpenClaw            ZeroClaw
                    ────────            ────────            ────────
Focus:              Enterprise          Consumer            Edge/Developer
                    orchestration       experience          efficiency

Strength:           Flow + RBAC +       Voice + Canvas +    Hardware + Providers +
                    Security            Mobile apps         Observability

Maturity:           v0.1.0              Production          Production
                    (channels are       (daily use)         (daily use)
                    stubs, core solid)

Best for:           Regulated           Personal            IoT / Edge /
                    enterprise          assistant           Self-hosted
                    AI workflows        with rich UX        minimalist setup
```
