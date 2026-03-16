export interface Task {
  id: string
  flow_id: string
  current_state: string
  state_data: Record<string, unknown>
  input: Record<string, unknown>
  output?: Record<string, unknown>
  status: TaskStatus
  created_by: Actor
  created_at: string
  updated_at: string
  completed_at?: string
}

export type TaskStatus =
  | 'running'
  | 'waiting_approval'
  | 'completed'
  | { failed: { error: string } }
  | 'cancelled'

export interface TaskFilter {
  status?: string
  flow_id?: string
  created_by?: string
  limit?: number
  offset?: number
}

export interface Actor {
  actor_type: 'user' | 'agent' | 'service' | 'system'
  id: string
}

export interface FlowDef {
  id: string
  version: string
  name?: string
  description?: string
  initial_state: string
  states: Record<string, StateDef>
  skills?: string[]
  source?: string
}

export interface StateDef {
  type: 'start' | 'agent' | 'gate' | 'parallel' | 'end'
  agent?: string
  skill?: string
  transitions: TransitionDef[]
}

export interface TransitionDef {
  to: string
  when?: string
  auto?: boolean
}

export interface ToolSchema {
  name: string
  description: string
  parameters: Record<string, unknown>
}

export interface SkillDef {
  id: string
  version: string
  name?: string
  description?: string
  system_prompt: string
  tools: string[]
  requires_approval: boolean
  risk_level: 'low' | 'medium' | 'high'
  tags: string[]
  author?: string
  source?: string
}

export interface McpServerConfig {
  name: string
  description?: string
  transport: 'stdio' | 'sse'
  url?: string
  command?: string
  args?: string[]
}

export interface McpServerInfo {
  name: string
  tools?: { name: string; description: string; input_schema: Record<string, unknown> }[]
}

export interface PluginManifest {
  name: string
  version: string
  description?: string
  author?: string
  permissions: string[]
  wasm_path: string
}

export interface SystemConfig {
  mode: string
  version: string
}

export interface SecurityPolicy {
  autonomy_level: string
  max_calls_per_minute: number
  max_high_risk_per_minute: number
  allow_network: boolean
  max_write_size: number
}

export interface NodeInfo {
  id: string
  hostname: string
  labels: string[]
  status: 'online' | 'draining' | 'offline'
  capacity: {
    total_memory: number
    available_memory: number
    running_agents: number
    running_plugins: number
    max_concurrent: number
  }
  last_heartbeat: string
}

export interface WsEvent {
  event_type: string
  entity: {
    entity_type: string
    id: string
  }
  payload: Record<string, unknown>
  occurred_at: string
}

// ── Cron ──
export interface CronJob {
  id: string
  name: string
  schedule: string
  command: string
  config: any
  enabled: boolean
  created_by: string
  created_at: string
  updated_at: string
  last_run_at?: string
  next_run_at?: string
}

export interface CronRun {
  id: string
  job_id: string
  status: 'running' | 'completed' | 'failed'
  started_at: string
  completed_at?: string
  output?: string
  error?: string
  triggered_by: string
}

// ── Channel ──
export interface ChannelInfo {
  name: string
  healthy: boolean
}

// ── Usage ──
export interface UsageSummary {
  total_tokens: number
  total_cost_usd: number
  input_tokens: number
  output_tokens: number
  request_count: number
  by_model: ModelUsage[]
  period: string
}

export interface ModelUsage {
  model: string
  tokens: number
  cost_usd: number
  request_count: number
}

export interface UsageDaily {
  date: string
  tokens: number
  cost_usd: number
  input_tokens: number
  output_tokens: number
}

// ── Logs ──
export interface LogEntry {
  timestamp: string
  level: 'trace' | 'debug' | 'info' | 'warn' | 'error'
  target: string
  message: string
  fields?: Record<string, any>
}

// ── Memory ──
export interface MemoryEntry {
  id: string
  content: string
  metadata: Record<string, any>
  score?: number
  created_at: string
}

// ── Audit ──
export interface AuditEntry {
  id: string
  actor: { actor_type: string; id: string }
  action: string
  resource_type: string
  resource_id: string
  result: string
  timestamp: string
  details?: Record<string, any>
}

// ── Approval ──
export interface Approval {
  id: string
  task_id: string
  tool_name: string
  risk_level: string
  status: 'pending' | 'approved' | 'denied' | 'changes_requested'
  comment?: string
  requested_at: string
  decided_at?: string
  decided_by?: string
}

// ── Diagnostics ──
export interface DiagResult {
  severity: 'ok' | 'warn' | 'error'
  category: string
  message: string
}
