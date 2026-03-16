# OpenAtta Usage Guide

Version: v0.1.0

---

## Prerequisites

- Rust toolchain (rustup, cargo)
- Node.js >= 18 (for WebUI and Tauri Shell)
- At least one LLM API key: `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, or `DEEPSEEK_API_KEY`

---

## Build

```bash
# Build all (excluding Tauri Shell)
cargo build --workspace --exclude atta-shell

# Build server — Desktop profile (default)
cargo build -p atta-server --features desktop

# Build server — Enterprise profile
cargo build -p atta-server --features enterprise

# Build CLI client
cargo build -p atta-cli

# Build WebUI
cd webui && npm install && npx vite build

# Build Tauri Shell
cd apps/shell && npm install && npx tauri build
```

---

## Binaries

| Binary | Crate | Description |
|--------|-------|-------------|
| `attaos` | `crates/server` | Core server daemon: HTTP API + WebUI + Agent execution |
| `attacli` | `crates/cli` | Lightweight CLI client: HTTP/SSE communication |
| `attash` | `apps/shell/src-tauri` | Desktop Shell: Tauri WebView + native system tray + auto-updater |

---

## CLI Commands

### `attaos` — Server

```bash
# Desktop mode (default), port 3000
attaos --port 3000

# Enterprise mode
attaos --mode enterprise --port 8080

# Custom home directory
attaos --home /path/to/atta-data

# Skip update check on startup
attaos --skip-update-check
```

The server starts:
- HTTP API at `http://127.0.0.1:{port}/api/v1/`
- WebSocket at `http://127.0.0.1:{port}/api/v1/ws`
- WebUI at `http://127.0.0.1:{port}/`
- CoreCoordinator event loop (subscribes to EventBus)

### `attacli` — Client

```bash
# Check server status
attacli status

# Interactive chat
attacli chat

# Task management
attacli task list
attacli task create --flow-id code_review --input '{"repo":"myorg/myrepo"}'
attacli task get <task-uuid>

# Flow management
attacli flow list
attacli flow get <flow-id>

# Skill management
attacli skill list
attacli skill get <skill-id>
attacli skill run <skill-id> --input "Write a blog post about Rust"

# Tool management
attacli tool list
attacli tool get <tool-name>

# MCP server management
attacli mcp list
attacli mcp get <server-name>

# Approval management
attacli approval list
attacli approval approve <approval-id>
attacli approval deny <approval-id>

# System metrics
attacli metrics
```

---

## API Endpoints

Base URL: `http://127.0.0.1:{port}/api/v1`

### System
| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | Health check |
| GET | `/system/health` | System health (alias) |
| GET | `/system/info` | System information |
| GET | `/system/metrics` | System metrics |

### Tasks
| Method | Path | Description |
|--------|------|-------------|
| GET | `/tasks` | List tasks (with filter/pagination) |
| POST | `/tasks` | Create task (max 1MB input) |
| GET | `/tasks/:id` | Get task |
| DELETE | `/tasks/:id` | Delete task |
| POST | `/tasks/:id/advance` | Advance task state |

### Flows
| Method | Path | Description |
|--------|------|-------------|
| GET | `/flows` | List flow definitions |
| POST | `/flows` | Register flow |
| GET | `/flows/:id` | Get flow definition |
| PUT | `/flows/:id` | Update flow |
| DELETE | `/flows/:id` | Delete flow |

### Skills
| Method | Path | Description |
|--------|------|-------------|
| GET | `/skills` | List skills |
| POST | `/skills` | Create skill |
| GET | `/skills/:id` | Get skill |
| PUT | `/skills/:id` | Update skill |
| DELETE | `/skills/:id` | Delete skill |

### Tools
| Method | Path | Description |
|--------|------|-------------|
| GET | `/tools` | List tools |
| GET | `/tools/:name` | Get tool schema |
| POST | `/tools/:name/test` | Test tool execution |

### MCP
| Method | Path | Description |
|--------|------|-------------|
| GET | `/mcp/servers` | List MCP servers |
| POST | `/mcp/servers` | Register MCP server |
| GET | `/mcp/servers/:id` | Get MCP server |
| DELETE | `/mcp/servers/:id` | Unregister MCP server |
| POST | `/mcp/servers/:id/tools` | Refresh server tools |
| GET | `/mcp/tools` | List all MCP tools |

### Channels
| Method | Path | Description |
|--------|------|-------------|
| GET | `/channels` | List channels |

### Security
| Method | Path | Description |
|--------|------|-------------|
| GET | `/security/policy` | Get security policy |

### Approvals
| Method | Path | Description |
|--------|------|-------------|
| GET | `/approvals` | List pending approvals |
| POST | `/approvals/:id/approve` | Approve |
| POST | `/approvals/:id/deny` | Deny |
| POST | `/approvals/:id/request-changes` | Request changes |

### Audit (Enterprise)
| Method | Path | Description |
|--------|------|-------------|
| GET | `/audit` | Query audit log |

### Chat
| Method | Path | Description |
|--------|------|-------------|
| POST | `/chat` | SSE streaming chat |

### Agents
| Method | Path | Description |
|--------|------|-------------|
| GET | `/agents` | List running agents |
| GET | `/remote-agents` | List remote agents |
| POST | `/remote-agents` | Register remote agent |

### Cron
| Method | Path | Description |
|--------|------|-------------|
| GET | `/cron/jobs` | List cron jobs |
| GET | `/cron/jobs/:id` | Get cron job |
| GET | `/cron/status` | Cron engine status |

### Memory
| Method | Path | Description |
|--------|------|-------------|
| POST | `/memory/store` | Store memory entry |
| POST | `/memory/search` | Search memory |

### Usage
| Method | Path | Description |
|--------|------|-------------|
| GET | `/usage` | Usage statistics |

### Diagnostics
| Method | Path | Description |
|--------|------|-------------|
| GET | `/diagnostics` | System diagnostics |

### Logs
| Method | Path | Description |
|--------|------|-------------|
| GET | `/logs` | SSE log stream |

### WebSocket
| Path | Description |
|------|-------------|
| `/ws` | Real-time event stream (EventEnvelope JSON, with auth) |

---

## Chat SSE Protocol

Request:
```json
POST /api/v1/chat
{
  "message": "User message",
  "skill_id": "optional-skill-id"
}
```

SSE events (`ChatEvent`):
```
data: {"type":"thinking","data":{"iteration":1}}
data: {"type":"text_delta","data":{"delta":"Hello"}}
data: {"type":"tool_start","data":{"tool_name":"web_search","call_id":"abc"}}
data: {"type":"tool_complete","data":{"tool_name":"web_search","call_id":"abc","duration_ms":1200}}
data: {"type":"done","data":{"iterations":2}}
```

---

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `ANTHROPIC_API_KEY` | Anthropic Claude API key | - |
| `OPENAI_API_KEY` | OpenAI API key | - |
| `DEEPSEEK_API_KEY` | DeepSeek API key | - |
| `OPENAI_BASE_URL` | OpenAI-compatible base URL | `https://api.openai.com` |
| `ANTHROPIC_MODEL` | Anthropic model ID | `claude-sonnet-4-20250514` |
| `OPENAI_MODEL` | OpenAI model ID | `gpt-4o` |
| `DEEPSEEK_MODEL` | DeepSeek model ID | `deepseek-chat` |
| `ATTA_PORT` | Server port | `3000` |
| `ATTA_HOME` | Data directory | `~/.atta` |
| `ATTA_LOG` / `ATTA_LOG_LEVEL` | Log level filter | `info` |
| `ATTA_DATA_DIR` | Override data directory | - |
| `RUST_LOG` | Detailed log filter | `info` |

When multiple API keys are set, a `ReliableProvider` is created with automatic failover.

---

## Data Directory

OpenAtta stores data in `~/.atta/`:

```
~/.atta/
├── data.db          # SQLite database (tasks, flows, skills, etc.)
├── estop.json       # E-Stop state (persisted across restarts)
├── keys.env         # API keys file (loaded on startup)
├── skills/          # User-installed skills
├── secrets.db       # Encrypted secret store
└── models/          # Local embedding models (fastembed)
```

---

## Skill Files

Skills are defined in Markdown (SKILL.md) and loaded from:
1. `./skills/` (project-local, 12 built-in)
2. `~/.atta/skills/` (user-global)

Example skill (`skills/atta-summarize/SKILL.md`):
```yaml
---
id: summarize
version: "1.0"
name: "Summarize Text"
description: "Summarize input text concisely"
system_prompt: |
  You are a summarization assistant.
  Summarize the following text in {{length}} style.
tools:
  - web_fetch
variables:
  - name: length
    description: "Summary length (brief/detailed)"
    required: false
    default: "brief"
risk_level: low
tags:
  - text
  - productivity
---
```

---

## Flow Definitions

Flows are YAML state machines loaded from `./flows/` or registered via API:

```yaml
id: code_review
version: "1.0"
name: "Code Review"
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
on_error:
  max_retries: 2
  retry_states: [analyze, apply_fixes]
  fallback: done
```

Built-in flows (6): `bug-triage`, `code-review`, `daily-digest`, `prd-to-code`, `research-report`, `skill-onboard`.

---

## Development

```bash
# Format
cargo fmt --all

# Lint
cargo clippy --workspace --exclude atta-shell --all-targets -- -D warnings

# Test all
cargo test --workspace --exclude atta-shell

# Test single crate
cargo test -p atta-core

# Run with debug logging
RUST_LOG=debug cargo run -p atta-server -- --port 3000

# WebUI dev server (hot reload)
cd webui && npm run dev
```
