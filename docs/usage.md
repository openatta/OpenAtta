# AttaOS Usage Guide

Status: v0.4

---

## Prerequisites

- Rust toolchain (rustup, cargo)
- Node.js >= 18 (for Tauri apps)
- At least one LLM API key: `ANTHROPIC_API_KEY` or `OPENAI_API_KEY`

---

## Build

```bash
# Build all (excluding Tauri apps)
cargo build --workspace --exclude atta-console --exclude atta-updater

# Build Desktop profile (default)
cargo build --features desktop

# Build Enterprise profile
cargo build --features enterprise

# Build with extra channel support
cargo build --features "desktop,terminal-channel,webhook-channel"

# Build Tauri Console
cd apps/console && npm install && npx tauri build

# Build Tauri Updater
cd apps/updater && npm install && npm run build && npx tauri build
```

---

## Binaries

| Binary | Description |
|--------|-------------|
| `attaos` | Main CLI entry point |
| `atta-tray` | System tray standalone (Enterprise) |
| `atta-console` | Tauri management console |
| `atta-updater` | Tauri update checker/installer |

---

## CLI Commands

### `attaos run` — Start Server

```bash
# Desktop mode (default), port 3000
attaos run

# Custom port
attaos run --port 8080

# Enterprise mode
attaos run --mode enterprise --port 8080
```

The server starts:
- HTTP API at `http://127.0.0.1:{port}/api/v1/`
- WebSocket at `http://127.0.0.1:{port}/api/v1/ws`
- WebUI at `http://127.0.0.1:{port}/`
- CoreCoordinator event loop (subscribes to EventBus)

### `attaos launch` — Start with Update Check

```bash
# Check for updates, then start server
attaos launch

# Custom port
attaos launch --port 8080

# Skip update check
attaos launch --skip-update-check
```

Behavior:
1. Queries GitHub Releases API for latest version
2. If newer version found → spawns `atta-updater` and exits
3. If up-to-date or check fails → falls through to `run` in desktop mode

### `attaos chat` — Interactive Chat

```bash
# Terminal chat (default)
attaos chat

# Specify channel type
attaos chat --channel terminal
```

Starts a direct agent conversation loop. The agent has access to all registered tools.

### `attaos channels` — List Available Channels

```bash
attaos channels
```

### `attaos task` — Task Management

```bash
# List all tasks
attaos task list

# Create a task from a Flow definition
attaos task create --flow-id dev_pr_flow --input '{"repo":"myorg/myrepo"}'

# Get task details
attaos task get <task-uuid>
```

### `attaos skill` — Skill Management

```bash
# List registered skills
attaos skill list

# View skill details (JSON)
attaos skill get <skill-id>

# Run a skill directly
attaos skill run <skill-id> --input "Write a blog post about Rust"

# Run with JSON variables
attaos skill run <skill-id> --input '{"topic":"Rust","length":"short"}'
```

---

## System Tray

### Desktop Mode (in-process)
The tray starts automatically when `attaos run` is called (planned integration).

### Enterprise Mode (standalone)
```bash
# Set port via environment variable
ATTA_PORT=3000 atta-tray
```

Menu items:
- **管理控制台** — launches `atta-console`
- **检查更新** — launches `atta-updater`
- **退出** — stops tray and kills child processes

---

## Console (Tauri)

The management console is a native WebView window that connects to the running AttaOS server.

```bash
# Launch directly
ATTA_PORT=3000 atta-console

# Or via tray menu "管理控制台"
```

Behavior:
- Navigates to `http://localhost:{ATTA_PORT}` on startup
- Closing the window hides it (does not exit the process)
- Can be re-shown via tray menu

---

## Updater (Tauri)

```bash
# Launch directly
atta-updater

# Or via tray menu "检查更新"
```

UI flow: idle → checking → available → downloading → done (or error at any step).

---

## API Endpoints

Base URL: `http://127.0.0.1:{port}/api/v1`

### System
| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | Health check |
| GET | `/system/info` | System information |

### Tasks
| Method | Path | Description |
|--------|------|-------------|
| GET | `/tasks` | List tasks |
| POST | `/tasks` | Create task |
| GET | `/tasks/:id` | Get task |

### Flows
| Method | Path | Description |
|--------|------|-------------|
| GET | `/flows` | List flow definitions |
| POST | `/flows` | Register flow |
| GET | `/flows/:id` | Get flow definition |

### Skills
| Method | Path | Description |
|--------|------|-------------|
| GET | `/skills` | List skills |
| GET | `/skills/:id` | Get skill |

### Tools
| Method | Path | Description |
|--------|------|-------------|
| GET | `/tools` | List tools |
| GET | `/tools/:name` | Get tool schema |

### Plugins
| Method | Path | Description |
|--------|------|-------------|
| GET | `/plugins` | List plugins |
| POST | `/plugins` | Register plugin |

### MCP
| Method | Path | Description |
|--------|------|-------------|
| GET | `/mcp/servers` | List MCP servers |
| POST | `/mcp/servers` | Register MCP server |

### Channels
| Method | Path | Description |
|--------|------|-------------|
| GET | `/channels` | List channels |

### Security
| Method | Path | Description |
|--------|------|-------------|
| GET | `/security/policy` | Get security policy |

### Approvals (Enterprise)
| Method | Path | Description |
|--------|------|-------------|
| GET | `/approvals` | List approvals |
| POST | `/approvals/:id/approve` | Approve |
| POST | `/approvals/:id/deny` | Deny |

### Audit (Enterprise)
| Method | Path | Description |
|--------|------|-------------|
| GET | `/audit` | Query audit log |

### WebSocket
| Path | Description |
|------|-------------|
| `/ws` | Real-time event stream (EventEnvelope JSON) |

---

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `ANTHROPIC_API_KEY` | Anthropic Claude API key | - |
| `OPENAI_API_KEY` | OpenAI API key | - |
| `OPENAI_BASE_URL` | OpenAI-compatible base URL | `https://api.openai.com` |
| `OPENAI_MODEL` | OpenAI model ID | `gpt-4o` |
| `ATTA_PORT` | Server port (for tray/console/updater) | `3000` |
| `RUST_LOG` | Log level filter | `info` |

When both `ANTHROPIC_API_KEY` and `OPENAI_API_KEY` are set, a `ReliableProvider` is created with Anthropic as primary and OpenAI as fallback.

---

## Data Directory

AttaOS stores data in `~/.atta/`:

```
~/.atta/
├── data.db          # SQLite database (tasks, flows, plugins, etc.)
├── estop.json       # E-Stop state (persisted across restarts)
├── skills/          # User-installed skills
└── secrets.db       # Encrypted secret store (if used)
```

---

## Skill Files

Skills are defined in YAML (SkillDef) and loaded from:
1. `./skills/` (project-local)
2. `~/.atta/skills/` (user-global)
3. Community sync directory

Example skill:
```yaml
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
```

---

## Flow Definitions

Flows are YAML state machines registered via API or disk:

```yaml
id: code_review_flow
version: "1.0"
name: "Code Review"
initial_state: analyze
states:
  analyze:
    state_type: agent
    skill: code_analysis
    transitions:
      - to: review_gate
        when: "output.needs_review == true"
      - to: done
        auto: true

  review_gate:
    state_type: gate
    gate:
      approver_role: developer
      timeout: "24h"
      on_timeout: done
    transitions:
      - to: apply_fixes
        when: "approval.status == approved"
      - to: done
        when: "approval.status == denied"

  apply_fixes:
    state_type: agent
    skill: apply_review_fixes
    transitions:
      - to: done

  done:
    state_type: end

on_error:
  max_retries: 2
  fallback: done
```

---

## Development

```bash
# Format
cargo fmt --all

# Lint
cargo clippy --workspace --all-targets -- -D warnings

# Test all
cargo test --workspace

# Test single crate
cargo test -p atta-core

# Run with debug logging
RUST_LOG=debug attaos run
```
