# Flow System

AttaOS Flow is a state-machine-based workflow engine that orchestrates multi-step agent pipelines with human-in-the-loop approval gates.

## Core Concepts

| Concept | Description |
|---------|-------------|
| **FlowDef** | YAML template defining states and transitions |
| **Task** | Runtime instance of a Flow |
| **State** | Node in the state machine (start/agent/gate/parallel/end) |
| **Skill** | Behavior template assigned to an agent state |
| **Gate** | Human approval checkpoint |
| **Transition** | Edge between states, optionally conditional |

## Available Flows

| Flow | ID | Description |
|------|----|-------------|
| [PRD to Code](./prd-to-code.md) | `prd_to_code` | Requirements → Code pipeline |
| Bug Triage | `bug_triage` | Diagnose → Approve → Fix → Verify |
| Code Review | `code_review` | Analyze → Gate → Apply fixes |
| Daily Digest | `daily_digest` | Gather → Compile → Send |
| Skill Onboard | `skill_onboard` | Discover → Review → Create |

## Flow Definition Format

```yaml
id: my_flow            # Unique identifier
version: "1.0"
name: "My Flow"        # Display name
description: "..."
skills:                # Skills used (for dependency tracking)
  - skill-a
  - skill-b

initial_state: start
states:
  start:
    type: start
    transitions:
      - to: do_work
        auto: true

  do_work:
    type: agent
    skill: skill-a
    transitions:
      - to: review
        auto: true

  review:
    type: gate
    gate:
      approver_role: admin
      timeout: "24h"
      on_timeout: done
    transitions:
      - to: done
        when: "approved"
      - to: do_work
        when: "revision_requested"

  done:
    type: end

on_error:
  max_retries: 2
  retry_states: [do_work]
  fallback: done
```

## State Types

- **start**: Entry point. Exactly one required.
- **agent**: Executes a Skill via an agent (ReAct loop).
- **gate**: Pauses for human approval.
- **parallel**: Runs multiple branches concurrently.
- **end**: Terminal state. At least one required.

## API

```bash
# List flows
GET /api/v1/flows

# Create a task
POST /api/v1/tasks
{ "flow_id": "prd_to_code", "input": {...} }

# Check task status
GET /api/v1/tasks/{id}

# Start flow via chat
POST /api/v1/chat
{ "message": "...", "flow_id": "prd_to_code" }
```

## Tool Integration

The `atta-start-flow` tool allows agents to start flows during conversations:

```json
{
  "action": "list_flows"
}
```

```json
{
  "action": "start",
  "flow_id": "prd_to_code",
  "input": { "requirements": "..." }
}
```
