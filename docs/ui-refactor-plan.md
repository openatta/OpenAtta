# WebUI 重构设计与实现计划

> 目标：补齐 AttaOS 与 OpenClaw / ZeroClaw 的 UI 功能差距，同时保持桌面原生体验优势。

## 0. 现状摘要

### 后端已有能力（无需从零开始）

| 能力 | 位置 | 状态 |
|------|------|------|
| Cron 引擎 | `crates/core/src/cron_engine.rs` | ✅ 完整（调度、执行、历史） |
| Cron 类型 | `crates/types/src/cron.rs` | ✅ CronJob / CronRun / CronRunStatus |
| Cron 存储 | `crates/store/src/traits.rs` → CronStore | ✅ 持久化 trait |
| 24 种 Channel | `crates/channel/src/` | ✅ Discord/Slack/Telegram/钉钉/飞书/微信等 |
| Channel API | `/api/v1/channels/*` | ✅ CRUD + health + webhook |
| 审计日志 | `crates/audit/` | ✅ 追踪所有操作 + 导出 |
| 事件总线 | `crates/bus/` | ✅ EventEnvelope 广播 |
| WebSocket 推送 | `/api/v1/ws` | ✅ 实时事件 |
| 记忆系统 | `/api/v1/memory/*` | ✅ 向量 + FTS 搜索 |

### 后端需要新增

| 能力 | 说明 |
|------|------|
| Cron REST API | 暴露 cron_engine 到 HTTP 路由 |
| Usage/Cost 追踪 | Token 计量 + 成本计算 + 聚合查询 |
| 日志流式 API | SSE 端点推送结构化日志 |
| 系统诊断 API | 健康检查聚合 |

### 前端需要新增（6 个新页面 + 3 项增强）

```
新页面：Cron / Channels / Usage / Logs / Memory / Diagnostics
增强项：暗色模式 / 配置编辑器 / i18n
```

---

## 1. 架构设计

### 1.1 新增导航结构

```
┌─────────────────────────────────────────────┐
│              Topbar (通知 │ 主题切换)          │
├────────┬────────────────────────────────────┤
│        │                                    │
│  侧栏   │         主内容区                    │
│ 220px  │                                    │
│        │                                    │
│ ── 核心 ──                                   │
│ Dashboard                                   │
│ Chat                                        │
│ Tasks                                       │
│ Agents                                      │
│ ── 编排 ──                                   │
│ Flows                                       │
│ Skills                                      │
│ Tools                                       │
│ MCP                                         │
│ Cron ← NEW                                  │
│ ── 运维 ──                                   │
│ Channels ← NEW                              │
│ Usage ← NEW                                 │
│ Logs ← NEW                                  │
│ Memory ← NEW                                │
│ ── 系统 ──                                   │
│ Settings (含配置编辑)                         │
│ Diagnostics ← NEW                           │
│                                             │
└────────┴────────────────────────────────────┘
```

导航从扁平 9 项改为 **分组 16 项**（核心 4 + 编排 5 + 运维 4 + 系统 2），侧栏分组可折叠。

### 1.2 状态管理新增

```
stores/
├── chat.ts          # 已有
├── task.ts          # 已有
├── flow.ts          # 已有
├── skill.ts         # 已有
├── mcp.ts           # 已有
├── notification.ts  # 已有
├── cron.ts          # NEW — CronJob CRUD + CronRun 历史
├── channel.ts       # NEW — Channel 列表 + 配置 + 健康
├── usage.ts         # NEW — 用量聚合 + 时间序列
├── log.ts           # NEW — SSE 日志流 + 缓冲
├── memory.ts        # NEW — 记忆 CRUD + 搜索
└── theme.ts         # NEW — 暗色/亮色模式
```

### 1.3 API 类型扩展（`types/api.ts`）

```typescript
// ── Cron ──
interface CronJob {
  id: string; name: string; schedule: string; command: string;
  config: any; enabled: boolean; created_by: string;
  created_at: string; updated_at: string;
  last_run_at?: string; next_run_at?: string;
}
interface CronRun {
  id: string; job_id: string; status: 'running'|'completed'|'failed';
  started_at: string; completed_at?: string;
  output?: string; error?: string; triggered_by: string;
}

// ── Channel ──
interface ChannelInfo {
  name: string; channel_type: string; enabled: boolean;
  connected: boolean; last_activity?: string; error?: string;
  config: Record<string, any>;
}
interface ChannelHealth {
  name: string; healthy: boolean; latency_ms?: number; error?: string;
}

// ── Usage ──
interface UsageSummary {
  total_tokens: number; total_cost_usd: number;
  input_tokens: number; output_tokens: number;
  request_count: number; by_model: ModelUsage[];
}
interface ModelUsage {
  model: string; tokens: number; cost_usd: number;
  request_count: number; share_pct: number;
}
interface UsageDaily {
  date: string; tokens: number; cost_usd: number;
  input_tokens: number; output_tokens: number;
}

// ── Logs ──
interface LogEntry {
  timestamp: string; level: 'trace'|'debug'|'info'|'warn'|'error';
  target: string; message: string; fields?: Record<string, any>;
}

// ── Memory ──
interface MemoryEntry {
  id: string; content: string; metadata: Record<string, any>;
  score?: number; created_at: string;
}

// ── Diagnostics ──
interface DiagResult {
  severity: 'ok'|'warn'|'error'; category: string; message: string;
}
```

---

## 2. 功能模块详细设计

### 2.1 ★ 成本/用量分析（高优先级）

**后端新增**

```
新文件：crates/core/src/server/handlers/usage.rs
新文件：crates/types/src/usage.rs
修改：  crates/core/src/server/mod.rs（注册路由）
修改：  crates/agent/src/react.rs（记录 token 用量）
```

API 端点：

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/v1/usage/summary` | 汇总（session/daily/monthly） |
| GET | `/api/v1/usage/daily?start=&end=` | 按日聚合 |
| GET | `/api/v1/usage/by-model` | 按模型分解 |
| GET | `/api/v1/usage/export?format=csv` | 导出 CSV |

数据来源：
1. 在 Agent ReAct 循环中记录每次 LLM 调用的 `input_tokens / output_tokens / model`
2. 写入 `usage_records` 表（新建 SQLite 表）
3. 聚合查询使用 SQL `GROUP BY date / model`

成本计算：
```rust
/// 内置模型价格表（可通过配置覆盖）
struct ModelPricing {
    input_per_million: f64,   // $/M tokens
    output_per_million: f64,
}
```

**前端页面：`UsageView.vue`**

```
┌─────────────────────────────────────────────┐
│  Usage & Cost                               │
├─────────────────────────────────────────────┤
│                                             │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌────┐│
│  │ Session  │ │  Daily  │ │ Monthly │ │Req ││
│  │ $0.42   │ │  $3.18  │ │ $47.20  │ │156 ││
│  └─────────┘ └─────────┘ └─────────┘ └────┘│
│                                             │
│  ┌──────────────────────────────────────┐   │
│  │  日用量趋势图（Tokens / Cost 切换）     │   │
│  │  ▁▂▃▅▇▆▅▃▂▁▂▃▅▇█▇▅▃             │   │
│  │  [日期范围选择器]                      │   │
│  └──────────────────────────────────────┘   │
│                                             │
│  模型分解                                    │
│  ┌────────────────────────────────────────┐ │
│  │ Model          │ Cost  │ Tokens │ Share│ │
│  │ claude-sonnet  │ $28.4 │  1.2M  │ 60% │ │
│  │ claude-haiku   │ $12.1 │  3.4M  │ 26% │ │
│  │ gpt-4o         │ $6.7  │  0.4M  │ 14% │ │
│  └────────────────────────────────────────┘ │
│                                             │
│  [导出 CSV]                                  │
└─────────────────────────────────────────────┘
```

技术要点：
- 趋势图使用 **SVG 手绘**（与 FlowGraph 一致，不引入图表库）
- 日期范围默认最近 7 天，支持 7d / 30d / 90d 快捷选择
- Summary 卡片使用 WebSocket 实时更新

---

### 2.2 ★ 实时日志查看（高优先级）

**后端新增**

```
新文件：crates/core/src/server/handlers/logs.rs
新文件：crates/core/src/log_stream.rs（tracing Layer → broadcast）
修改：  crates/core/src/server/mod.rs（注册路由）
修改：  crates/server/src/main.rs（挂载 log Layer）
```

API 端点：

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/v1/logs/stream` | SSE 实时日志流 |
| GET | `/api/v1/logs/recent?limit=&level=` | 最近 N 条日志（REST） |

实现方案：
```
tracing-subscriber
    ├── fmt Layer → 文件/stdout（已有）
    └── BroadcastLayer → tokio::broadcast::Sender<LogEntry>（新增）
            │
            ▼
        /api/v1/logs/stream (SSE)
            │
            ▼
        EventSource (浏览器)
```

核心组件：
```rust
/// 自定义 tracing Layer，将日志事件广播到 channel
pub struct BroadcastLayer {
    sender: broadcast::Sender<LogEntry>,
}

impl<S: Subscriber> Layer<S> for BroadcastLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        // 提取 level, target, message, fields → LogEntry
        // sender.send(entry) — 忽略无接收者的错误
    }
}
```

**前端页面：`LogsView.vue`**

```
┌─────────────────────────────────────────────┐
│  Logs              ● Connected   [⏸ Pause]  │
├─────────────────────────────────────────────┤
│  Level: [✓trace ✓debug ✓info ✓warn ✓error] │
│  Filter: [________________] target 搜索      │
├─────────────────────────────────────────────┤
│  10:23:45 INFO  core::server  request ok    │
│  10:23:46 DEBUG agent::react  thinking...   │
│  10:23:47 WARN  mcp::client   timeout 3s    │
│  10:23:48 ERROR channel::slack auth failed  │
│  ...                                        │
│                                             │
│  [↓ 跳到底部]                    500 / 500   │
└─────────────────────────────────────────────┘
```

技术要点：
- SSE 连接 `/api/v1/logs/stream`，与 chat SSE 类似
- 前端缓冲上限 500 条（FIFO），防止内存膨胀
- 暂停/恢复：暂停时停止追加但 SSE 保持连接
- Level 筛选 + target 文本过滤（前端过滤，减少后端压力）
- 每条日志 level 着色：trace=灰, debug=蓝, info=绿, warn=黄, error=红
- 自动滚动到底部，用户手动滚动时暂停自动滚动

---

### 2.3 ★ 多渠道集成 UI（用户特别关注）

**后端现状**：API 已有 `/api/v1/channels/*`（list / add / update / delete / health / webhook），无需新增端点。

需要增强的后端内容：
```
修改：crates/core/src/server/handlers/channel.rs
      - list_channels 返回更丰富的状态（connected, last_activity, error）
      - 新增 GET /api/v1/channels/{name}/config-schema → 返回该类型 channel 的配置 schema
```

**前端页面：`ChannelsView.vue`**

```
┌─────────────────────────────────────────────┐
│  Channels                     [+ 添加渠道]   │
├─────────────────────────────────────────────┤
│                                             │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐    │
│  │ 🟢 Slack │ │ 🟢 钉钉  │ │ 🔴 飞书  │    │
│  │ Connected│ │ Connected│ │ Error    │    │
│  │ 3m ago   │ │ 1h ago   │ │ Auth fail│    │
│  │ [配置]    │ │ [配置]    │ │ [配置]    │    │
│  └──────────┘ └──────────┘ └──────────┘    │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐    │
│  │ ⚪ Discord│ │ ⚪ Telegram│ │ ⚪ Email │    │
│  │ Disabled │ │ Not Config│ │ Disabled │    │
│  │          │ │          │ │          │    │
│  │ [启用]    │ │ [配置]    │ │ [启用]    │    │
│  └──────────┘ └──────────┘ └──────────┘    │
│                                             │
├─────────────────────────────────────────────┤
│  渠道详情：Slack                              │
│  ┌──────────────────────────────────────┐   │
│  │ 状态: Connected ● 延迟: 42ms          │   │
│  │ 最近入站: 3 minutes ago               │   │
│  │ 最近出站: 1 minute ago                │   │
│  │                                      │   │
│  │ 配置                                  │   │
│  │ ┌──────────────────────────────────┐ │   │
│  │ │ Bot Token:  [sk-****...****]     │ │   │
│  │ │ App Token:  [xapp-****...****]   │ │   │
│  │ │ Channel:    [#general       ]    │ │   │
│  │ │ DM Policy:  [allow-all ▼]       │ │   │
│  │ └──────────────────────────────────┘ │   │
│  │                                      │   │
│  │ [保存] [测试连接] [禁用] [删除]         │   │
│  └──────────────────────────────────────┘   │
└─────────────────────────────────────────────┘
```

设计要点：
- **卡片网格 + 详情面板**：顶部网格概览，点击展开下方详情
- **状态三色**：🟢 Connected / 🟡 Configured but offline / 🔴 Error / ⚪ Disabled
- **敏感字段遮蔽**：Token 类字段显示 `****`，点击复制完整值
- **健康检查**：详情面板中「测试连接」按钮调用 `/channels/{name}/health`
- **添加渠道**：弹窗选择渠道类型 → 动态表单（根据 config-schema 生成）
- **活动追踪**：展示最近入站/出站时间戳

支持的 24 种渠道分类展示：
```
即时通讯:  Slack / Discord / Telegram / WhatsApp / Signal / QQ / iMessage
企业协作:  钉钉 / 飞书 / Mattermost / NextCloud Talk
开放协议:  IRC / Matrix / Nostr / MQTT
通知渠道:  Email / Webhook / ClawdTalk
本地:     Terminal
```

---

### 2.4 ★ 定时任务 Cron（用户特别关注）

**后端新增**

```
新文件：crates/core/src/server/handlers/cron.rs
修改：  crates/core/src/server/mod.rs（注册路由）
```

API 端点（暴露已有 cron_engine）：

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/v1/cron/jobs` | 列表（?enabled=true/false） |
| POST | `/api/v1/cron/jobs` | 创建任务 |
| GET | `/api/v1/cron/jobs/{id}` | 获取详情 |
| PUT | `/api/v1/cron/jobs/{id}` | 更新（schedule, enabled 等） |
| DELETE | `/api/v1/cron/jobs/{id}` | 删除 |
| POST | `/api/v1/cron/jobs/{id}/trigger` | 手动触发 |
| GET | `/api/v1/cron/jobs/{id}/runs` | 运行历史 |
| GET | `/api/v1/cron/status` | 引擎状态 |

**前端页面：`CronView.vue`**

```
┌─────────────────────────────────────────────┐
│  Cron Jobs           引擎: ● Running  [+ 新建]│
├─────────────────────────────────────────────┤
│  Filter: [全部 ▼]  [搜索...]                  │
├─────────────────────────────────────────────┤
│                                             │
│  ┌────────────────────────────────────────┐ │
│  │ Name     │ Schedule     │ Next Run     │ │
│  │          │              │ Last Status  │ │
│  ├──────────┼──────────────┼──────────────┤ │
│  │ cleanup  │ 0 0 * * *   │ 2h 15m       │ │
│  │          │ (每天 00:00) │ ✅ completed  │ │
│  ├──────────┼──────────────┼──────────────┤ │
│  │ report   │ 0 9 * * 1   │ 4d 8h        │ │
│  │          │ (每周一 9:00)│ ✅ completed  │ │
│  ├──────────┼──────────────┼──────────────┤ │
│  │ sync     │ */5 * * * * │ 3m 22s       │ │
│  │          │ (每 5 分钟)  │ ❌ failed     │ │
│  └────────────────────────────────────────┘ │
│                                             │
├─────────────────────────────────────────────┤
│  任务详情：sync                               │
│  ┌──────────────────────────────────────┐   │
│  │ 基本信息                               │   │
│  │ Schedule: */5 * * * *  Enabled: [✓]   │   │
│  │ Command:  sync-external-data          │   │
│  │ Created:  2026-03-10 by admin         │   │
│  │                                      │   │
│  │ [▶ 手动触发]  [编辑]  [删除]            │   │
│  │                                      │   │
│  │ 运行历史                               │   │
│  │ ┌──────────────────────────────────┐ │   │
│  │ │ Time        │ Status │ Duration │ │   │
│  │ │ 10:25:00    │ ❌ fail │ 2.3s    │ │   │
│  │ │ 10:20:00    │ ✅ ok   │ 1.8s    │ │   │
│  │ │ 10:15:00    │ ✅ ok   │ 1.5s    │ │   │
│  │ │ 10:10:00    │ ✅ ok   │ 2.1s    │ │   │
│  │ └──────────────────────────────────┘ │   │
│  └──────────────────────────────────────┘   │
└─────────────────────────────────────────────┘
```

设计要点：
- **Cron 表达式人类可读**：`0 0 * * *` → `每天 00:00`（前端解析）
- **倒计时显示**：Next Run 显示距下次运行的相对时间
- **运行历史**：点击任务展开，显示最近 20 次运行记录
- **手动触发**：一键触发按钮，结果实时通过 WebSocket 推送
- **状态着色**：completed=绿, failed=红, running=蓝动画
- **新建表单**：名称 + Cron 表达式（带预览） + 命令 + 配置 JSON

---

### 2.5 暗色模式

**实现方案**：CSS Variables 主题切换（不引入新依赖）

```
新文件：webui/src/stores/theme.ts
修改：  webui/src/assets/main.css（增加 dark 变量集）
修改：  webui/src/components/layout/Topbar.vue（主题切换按钮）
修改：  webui/src/App.vue（挂载 theme class）
```

```css
/* main.css */
:root {
  --bg-page: #f5f5f5;    --bg-card: #ffffff;
  --bg-sidebar: #1a1a2e; --text-primary: #1f2937;
  --text-secondary: #6b7280; --border-color: #e5e7eb;
}

:root.dark {
  --bg-page: #0f172a;    --bg-card: #1e293b;
  --bg-sidebar: #0c0f1a; --text-primary: #f1f5f9;
  --text-secondary: #94a3b8; --border-color: #334155;
}
```

```typescript
// stores/theme.ts
export const useThemeStore = defineStore('theme', {
  state: () => ({
    mode: (localStorage.getItem('theme') || 'light') as 'light' | 'dark'
  }),
  actions: {
    toggle() {
      this.mode = this.mode === 'light' ? 'dark' : 'light'
      localStorage.setItem('theme', this.mode)
      document.documentElement.classList.toggle('dark', this.mode === 'dark')
    }
  }
})
```

---

### 2.6 记忆管理 UI

**后端现状**：API 已有 `/api/v1/memory/search` 和 `/api/v1/memory/{id}`（GET / DELETE），无需新增端点。

**前端页面：`MemoryView.vue`**

```
┌─────────────────────────────────────────────┐
│  Memory                                     │
├─────────────────────────────────────────────┤
│  搜索: [________________________] [🔍]      │
│                                             │
│  ┌──────────────────────────────────────┐   │
│  │ "用户偏好使用中文回复"                    │   │
│  │ score: 0.92 │ 2026-03-10 │ [删除]     │   │
│  ├──────────────────────────────────────┤   │
│  │ "项目使用 Rust + Tauri 技术栈"         │   │
│  │ score: 0.88 │ 2026-03-08 │ [删除]     │   │
│  └──────────────────────────────────────┘   │
│                                             │
│  共 24 条记忆                                │
└─────────────────────────────────────────────┘
```

---

### 2.7 配置编辑器（增强 Settings 页）

**当前**：Settings 页只读展示系统配置和安全策略。

**增强**：
- Security Policy 改为可编辑（已有 `PUT /api/v1/security/policy`）
- 新增配置文件编辑（表单模式 + JSON 原始模式切换）
- 敏感字段（如 API Key）显示遮蔽值

---

### 2.8 系统诊断

**后端新增**

```
新文件：crates/core/src/server/handlers/diagnostics.rs
修改：  crates/core/src/server/mod.rs（注册路由）
```

| 方法 | 路径 | 说明 |
|------|------|------|
| POST | `/api/v1/diagnostics/run` | 运行诊断 |

诊断项目：
1. 数据库连接 + 表完整性
2. 事件总线连通性
3. Channel 健康（批量检查所有已启用 channel）
4. Cron 引擎状态
5. 磁盘空间
6. 内存使用
7. MCP Server 连接状态

**前端页面：`DiagnosticsView.vue`**

```
┌─────────────────────────────────────────────┐
│  Diagnostics           [▶ 运行诊断]          │
├─────────────────────────────────────────────┤
│  ✅ 8 ok  ⚠️ 1 warn  ❌ 0 error             │
├─────────────────────────────────────────────┤
│  数据库                                      │
│  ✅ SQLite connection OK (0.2ms)             │
│  ✅ All 12 tables present                    │
│                                             │
│  Channel                                    │
│  ✅ Slack: connected (42ms)                  │
│  ⚠️ 飞书: high latency (1200ms)              │
│  ✅ 钉钉: connected (89ms)                   │
│                                             │
│  Cron                                       │
│  ✅ Engine running, 3 jobs scheduled         │
│                                             │
│  System                                     │
│  ✅ Disk: 42GB free (78%)                    │
│  ✅ Memory: 256MB used                       │
└─────────────────────────────────────────────┘
```

---

### 2.9 i18n 国际化

**实现方案**：使用 `vue-i18n` 库

```
新增依赖：vue-i18n
新文件：webui/src/i18n/index.ts
新文件：webui/src/i18n/locales/zh-CN.json
新文件：webui/src/i18n/locales/en.json
```

初期支持中文 + 英文，结构按页面组织：
```json
{
  "nav": { "dashboard": "仪表盘", "chat": "对话", ... },
  "cron": { "title": "定时任务", "new_job": "新建任务", ... },
  "channel": { "title": "渠道管理", "add": "添加渠道", ... }
}
```

---

## 3. 实施计划

### Phase 1：高优先级（预计 2 周）

| # | 任务 | 后端 | 前端 | 依赖 |
|---|------|------|------|------|
| 1.1 | 暗色模式 | — | theme store + CSS vars | 无 |
| 1.2 | 侧栏分组重构 | — | Sidebar.vue 改造 | 无 |
| 1.3 | 成本/用量追踪 | usage handler + usage 表 + agent 记录 token | UsageView + usage store + SVG 图表 | agent 改造 |
| 1.4 | 实时日志查看 | BroadcastLayer + SSE endpoint | LogsView + log store + SSE client | tracing 改造 |

```
Week 1: 1.1 暗色模式 + 1.2 侧栏分组（前端独立，可并行）
         1.3 后端 usage 表 + agent token 记录
         1.4 后端 BroadcastLayer + SSE endpoint

Week 2: 1.3 前端 UsageView（SVG 图表 + 模型分解表）
         1.4 前端 LogsView（SSE 消费 + 过滤 + 自动滚动）
         集成测试
```

### Phase 2：用户重点关注（预计 2 周）

| # | 任务 | 后端 | 前端 | 依赖 |
|---|------|------|------|------|
| 2.1 | Cron 管理 | cron handler（暴露已有引擎） | CronView + cron store | cron_engine |
| 2.2 | Channel 管理 | 增强 channel list 返回 + config-schema | ChannelsView + channel store | channel API |

```
Week 3: 2.1 后端 cron REST 路由（6 个端点）
         2.2 后端 channel 返回增强 + config-schema
         2.1 前端 CronView（列表 + 新建 + 详情 + 运行历史）

Week 4: 2.2 前端 ChannelsView（卡片网格 + 详情 + 配置表单）
         集成测试
```

### Phase 3：补齐功能（预计 2 周）

| # | 任务 | 后端 | 前端 | 依赖 |
|---|------|------|------|------|
| 3.1 | 记忆管理 | — （API 已有） | MemoryView + memory store | 无 |
| 3.2 | Settings 增强 | — （API 已有） | 配置编辑器组件 | 无 |
| 3.3 | 系统诊断 | diagnostics handler | DiagnosticsView | 多组件 |
| 3.4 | i18n | — | vue-i18n + zh/en | 所有页面 |

```
Week 5: 3.1 MemoryView
         3.2 Settings 编辑模式
         3.3 后端诊断 + 前端 DiagnosticsView

Week 6: 3.4 i18n（抽取所有页面文本）
         全流程测试 + UI 打磨
```

---

## 4. 技术决策

| 决策 | 选择 | 理由 |
|------|------|------|
| 图表库 | 手绘 SVG | 与 FlowGraph 一致，不引入 Chart.js/ECharts 重依赖 |
| 日志推送 | SSE（非 WS 复用） | 独立流，不干扰事件 WS；与 chat SSE 模式一致 |
| 暗色模式 | CSS Variables | 零依赖，与现有 CSS 架构兼容 |
| Cron 表达式解析 | cronstrue（前端库） | 轻量，将 `0 0 * * *` 翻译为人类可读文本 |
| i18n | vue-i18n | Vue 生态标准方案 |
| 配置编辑 | 表单 + JSON 双模式 | 参考 OpenClaw，兼顾易用和灵活 |
| Channel 配置表单 | JSON Schema → 动态表单 | 后端返回 schema，前端自动生成，支持 24 种渠道 |

---

## 5. 文件变更清单

### 后端（Rust）

```
新增文件：
  crates/types/src/usage.rs          — UsageRecord / UsageSummary / ModelUsage 类型
  crates/core/src/server/handlers/usage.rs       — 用量 API handler
  crates/core/src/server/handlers/cron.rs        — Cron API handler
  crates/core/src/server/handlers/diagnostics.rs — 诊断 API handler
  crates/core/src/log_stream.rs      — BroadcastLayer (tracing → broadcast)
  crates/core/src/server/handlers/logs.rs        — 日志 SSE handler

修改文件：
  crates/core/src/server/mod.rs      — 注册新路由（cron/usage/logs/diagnostics）
  crates/types/src/lib.rs            — pub mod usage
  crates/agent/src/react.rs          — 记录 token 用量
  crates/store/src/traits.rs         — UsageStore trait
  crates/store/src/sqlite.rs         — SQLite usage_records 表
  crates/server/src/main.rs          — 挂载 BroadcastLayer
  crates/core/src/server/handlers/channel.rs — 增强 list 返回、config-schema
```

### 前端（Vue）

```
新增文件：
  webui/src/views/CronView.vue
  webui/src/views/ChannelsView.vue
  webui/src/views/UsageView.vue
  webui/src/views/LogsView.vue
  webui/src/views/MemoryView.vue
  webui/src/views/DiagnosticsView.vue
  webui/src/stores/cron.ts
  webui/src/stores/channel.ts
  webui/src/stores/usage.ts
  webui/src/stores/log.ts
  webui/src/stores/memory.ts
  webui/src/stores/theme.ts
  webui/src/components/charts/BarChart.vue    — SVG 柱状图
  webui/src/components/charts/LineChart.vue   — SVG 折线图
  webui/src/components/form/DynamicForm.vue   — JSON Schema 动态表单
  webui/src/composables/useSSE.ts             — SSE 连接复用
  webui/src/i18n/index.ts
  webui/src/i18n/locales/zh-CN.json
  webui/src/i18n/locales/en.json

修改文件：
  webui/src/router/index.ts          — 6 条新路由
  webui/src/types/api.ts             — 新增类型定义
  webui/src/components/layout/Sidebar.vue — 分组导航
  webui/src/components/layout/Topbar.vue  — 主题切换 + 语言切换
  webui/src/assets/main.css          — dark 主题变量
  webui/src/App.vue                  — theme class 绑定
  webui/src/views/SettingsView.vue   — 编辑模式
  webui/package.json                 — +vue-i18n +cronstrue
```

---

## 6. 风险与缓解

| 风险 | 缓解措施 |
|------|----------|
| SVG 手绘图表在复杂数据下性能不足 | 限制数据点为 90 天；若不够再引入 lightweight chart 库 |
| 24 种 Channel 配置表单工作量大 | JSON Schema 动态表单生成，只需后端返回 schema |
| token 记录增加 Agent 调用延迟 | 异步写入，不阻塞 ReAct 主循环 |
| BroadcastLayer 在高日志量下丢消息 | broadcast channel 容量设 1024，lagged 时丢弃旧消息 |
| i18n 文本量大 | Phase 3 最后做，只翻译 UI 静态文本，不翻译动态数据 |
