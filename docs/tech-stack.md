# OpenAtta Technology Stack

Version: v0.1.0

---

## Language & Runtime

| Choice | Rationale |
|--------|-----------|
| **Rust 2021 edition** | Memory safety without GC, zero-cost abstractions, single binary deployment |
| **Tokio (full features)** | Industry-standard async runtime; needed for axum, sqlx, reqwest, all I/O |
| **async-trait** | Ergonomic async trait methods (until RPITIT stabilization replaces it) |
| **futures 0.3** | Stream combinators for EventBus subscriptions |

---

## Web & HTTP

| Crate | Version | Role |
|-------|---------|------|
| **axum 0.7** | HTTP server | REST API + WebSocket upgrade + static file serving |
| **tower-http** | HTTP middleware | CORS, tracing, compression |
| **reqwest 0.12** | HTTP client | LLM API calls, MCP SSE transport, GitHub API, webhook delivery |
| **rust-embed 8** | Asset embedding | Vue 3 SPA embedded in server binary — zero-dependency frontend serving |

**Why axum over actix-web or warp:**
- Tower middleware ecosystem (shared with tonic/hyper)
- First-class WebSocket support (`axum::extract::ws`)
- Type-safe routing with extractors
- Same async model as the rest of the stack (tokio-native, no actor system overhead)

---

## Serialization

| Crate | Role |
|-------|------|
| **serde 1** + **serde_json 1** | JSON for API, EventEnvelope payloads, tool arguments, LLM messages |
| **serde_yml** | Flow definitions, Skill manifests, config files |

---

## Database

| Crate | Version | Role |
|-------|---------|------|
| **sqlx 0.8** | Unified DB abstraction | Compile-time checked queries, async, connection pooling |

Features enabled: `runtime-tokio`, `sqlite`, `postgres`, `uuid`, `chrono`, `json`.

**Dual backend strategy:**
- `SqliteStore` (Desktop) — zero external deps, single file, embedded in process
- `PostgresStore` (Enterprise) — horizontal scaling, concurrent access, JSONB indexing

Both implement the same `StateStore` trait with optimistic locking (`version` field on Task).

---

## Event Bus

| Desktop | Enterprise |
|---------|------------|
| **tokio broadcast** (in-process) | **async-nats 0.38** (NATS JetStream) |

**Why NATS JetStream for Enterprise:**
- At-least-once delivery with durable consumers
- Topic wildcards matching OpenAtta event naming (`atta.task.*`)
- Lightweight — single binary, sub-millisecond latency
- Native Rust client with async/await

`InProcBus` features: configurable capacity via `with_capacity()`, topic count warnings (> 5000), lag detection.

---

## AI / LLM

| Component | Implementation |
|-----------|---------------|
| LLM abstraction | `LlmProvider` trait (chat, stream, model_info) |
| Anthropic backend | `AnthropicProvider` — Messages API with tool_use, streaming, thinking |
| OpenAI backend | `OpenAiProvider` — Chat Completions API, function calling, streaming |
| DeepSeek backend | `DeepSeekProvider` — OpenAI-compatible API |
| Failover | `ReliableProvider` — retry + fallback chain across providers |
| Routing | `RouterProvider` — per-request model selection |
| Tool dispatch | `ToolDispatcher` trait — native (function calling) vs XML (for models without tool support) |

---

## Security & Cryptography

| Crate | Version | Role |
|-------|---------|------|
| **aes-gcm 0.10** | AES-256-GCM | Secret store encryption at rest |
| **hkdf 0.12** | Key derivation | Derive encryption keys from master secret |
| **sha2 0.10** | SHA-256 | Hashing for integrity checks |
| **rand 0.8** | CSPRNG | Key generation, OTP generation |
| **ed25519-dalek 2** | Ed25519 | Package signing and verification |
| **secrecy 0.10** | Secret types | Prevent accidental logging of sensitive data |

---

## CLI

| Crate | Version | Role |
|-------|---------|------|
| **clap 4** (derive) | CLI framework | Subcommands, argument parsing, help generation |
| **semver 1** | Version parsing | Update check — compare current vs latest release |

---

## Tauri (Desktop Shell)

| Component | Version | Role |
|-----------|---------|------|
| **tauri 2** | App framework | Native WebView windows, IPC, lifecycle management |
| **tauri-plugin-shell 2** | Shell plugin | Open external URLs, spawn processes |
| **tauri-plugin-updater 2** | Update plugin | Check GitHub Releases, download + install updates |

**Why Tauri v2 over Electron:**
- Rust backend (matches OpenAtta stack)
- Uses system WebView (WebKit on macOS) — no bundled Chromium
- < 10 MB app size vs 100+ MB for Electron
- Native capabilities via plugins (updater, shell, fs)

---

## Observability

| Crate | Version | Role |
|-------|---------|------|
| **tracing 0.1** | Structured logging | Span-based instrumentation throughout codebase |
| **tracing-subscriber 0.3** | Log output | Console formatter + `env-filter` for `RUST_LOG` |

---

## Identity & Time

| Crate | Version | Role |
|-------|---------|------|
| **uuid 1** (v4, serde) | Identifiers | Task IDs, event IDs, approval IDs — all UUIDv4 |
| **chrono 0.4** (serde) | Timestamps | `DateTime<Utc>` throughout all domain types |

---

## System

| Crate | Version | Role |
|-------|---------|------|
| **sysinfo 0.33** | System info | Node capacity reporting (memory, CPU) |
| **tokio-util 0.7** | Utilities | `CancellationToken` for graceful shutdown |

---

## Package System

| Crate | Version | Role |
|-------|---------|------|
| **zip 2** | Archive | `.apkg` package format (zip container) |
| **semver 1** | Versioning | Dependency resolution, version constraints |

---

## Frontend (Web UI)

| Technology | Role |
|------------|------|
| **Vue 3** | SPA framework |
| **Vite 5** | Build tool |
| **Pinia** | State management |
| **vue-i18n** | Internationalization (en / zh-CN) |
| **vue-router 4** | Client-side routing |
| **ofetch** | HTTP client |

The built SPA is embedded into the server binary via `rust-embed`.

---

## Workspace Dependencies (full list)

```toml
# Async
tokio = { version = "1", features = ["full"] }
async-trait = "0.1"
futures = "0.3"

# Web
axum = { version = "0.7", features = ["ws"] }
tower-http = { version = "0.6", features = ["cors", "trace"] }
reqwest = { version = "0.12", features = ["json", "stream"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yml = "0.0.12"

# Database
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "postgres", "uuid", "chrono", "json"] }

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# Identity & Time
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }

# Error handling
anyhow = "1"
thiserror = "2"

# CLI
clap = { version = "4", features = ["derive"] }

# Embed static assets
rust-embed = "8"

# Async messaging
async-nats = "0.38"

# Crypto & Security
aes-gcm = "0.10"
hkdf = "0.12"
sha2 = "0.10"
rand = "0.8"
ed25519-dalek = "2"
secrecy = "0.10"

# Package
zip = "2"
semver = "1"

# System
sysinfo = "0.33"

# Utilities
regex = "1"
glob = "0.3"
tokio-util = "0.7"
```
