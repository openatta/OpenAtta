# 分析简报：记忆系统架构 — 内置 vs 独立服务

## 竞品实现现状

### OpenClaw（TypeScript）— 深度内置

记忆系统是 OpenClaw 的**核心模块**，非独立服务：

- **向量后端**：sqlite-vec（默认）+ LanceDB（可选），内置于主进程
- **Embedding 提供者**：6 种（OpenAI、Gemini、Voyage、Mistral、Ollama、Node-LLaMA）
- **搜索策略**：BM25 + cosine 混合搜索，MMR 多样性排序，时间衰减
- **代码量**：~3000 行，是该项目最重的模块之一
- **架构**：`MemoryManager` 作为 `AgentRuntime` 的成员字段，Agent 直接调用

**结论**：OpenClaw 将记忆视为 Agent 的核心能力，完全内置。

### ZeroClaw（Rust）— 内置 + 可选外部后端

- **默认后端**：SQLite + FTS5 + cosine 暴力扫描（内置，零外部依赖）
- **可选后端**：Qdrant（外部向量数据库），通过 backend 配置切换
- **Embedding**：OpenAI 兼容 API（需外部服务），含 embedding 缓存层
- **搜索策略**：向量 + FTS 混合，RRF（Reciprocal Rank Fusion）合并
- **多后端抽象**：`MemoryBackend` trait，支持 sqlite / qdrant / postgres / markdown / none

**结论**：ZeroClaw 也将记忆内置，但通过 trait 抽象支持外部向量数据库作为可选后端。

### 共同点

两个竞品都**没有**将记忆/向量搜索做成独立服务。原因：
1. Agent 调用记忆的延迟敏感（每次 ReAct 循环可能查询记忆）
2. 记忆与 Agent 上下文紧密耦合（需要 task_id、skill_id 等元数据过滤）
3. 单进程架构避免了序列化/网络开销

---

## AttaOS 现状

`crates/memory` 已有完整的抽象设计：

| 组件 | 状态 |
|------|------|
| `MemoryStore` trait | ✅ 已定义（store/recall/search/forget） |
| `SqliteMemoryStore` | ✅ 已实现（FTS5 + cosine + RRF 混合搜索） |
| `EmbeddingProvider` trait | ✅ 已定义（embed + dimensions） |
| `NoopEmbeddingProvider` | ✅ 已实现（dimensions=0，跳过向量搜索） |
| `cosine_similarity` 工具函数 | ✅ 已实现 |
| 接入 AppState | ❌ 未接入 |
| 真实 EmbeddingProvider | ❌ 无（fastembed 待实现） |
| Tool 实现 | ❌ stub（返回 not_implemented） |

**核心观察**：抽象层设计完善，但未激活。

---

## 方案对比：内置 vs MCP Server

### 方案 A — 保持内置（推荐）

```
attaos 进程
├── Core（调度、路由）
├── Agent（ReAct 循环）
│   └── 直接调用 MemoryStore::recall()  ← 进程内调用，零延迟
├── Memory（SqliteMemoryStore + EmbeddingProvider）
└── Tools（memory_store/memory_recall 作为 NativeTool）
```

**优点：**
- 零网络延迟：Agent 每轮 ReAct 可能查询记忆，进程内调用 < 1ms
- 零部署复杂度：Desktop 版无需启动额外进程
- 事务一致性：记忆写入与任务状态在同一 SQLite 数据库
- 已有完整抽象：`MemoryStore` trait 已支持未来换后端
- 竞品验证：OpenClaw 和 ZeroClaw 均采用此模式

**缺点：**
- Embedding 模型加载增加 attaos 内存占用（~100MB）
- 向量索引规模受限于单机内存

### 方案 B — 独立 MCP Server

```
attaos 进程                    memory-mcp-server 进程
├── Core                       ├── MCP Protocol Handler
├── Agent                      ├── EmbeddingProvider
│   └── MCP 调用 ──stdio──→   ├── VectorStore（sqlite-vec / Qdrant）
└── MCP Registry               └── FTS Index
```

**优点：**
- 解耦：记忆服务可独立升级、独立扩展
- 可复用：其他 Agent 框架也能通过 MCP 调用
- 隔离：Embedding 模型的内存/CPU 不影响主进程
- 可替换：用户可接入第三方记忆 MCP Server

**缺点：**
- 延迟增加：每次调用增加 stdio/SSE 序列化开销（~5-20ms）
- 部署复杂：Desktop 用户需管理两个进程
- 事务一致性：跨进程写入无法原子化
- MCP 协议限制：当前 MCP tool 调用是 request-response，不支持流式向量搜索
- 开发成本：需要独立的 MCP Server 项目 + 打包 + 更新

### 方案 C — 混合：内置默认 + Enterprise 可选外部（推荐 long-term）

```
Desktop:
  attaos → SqliteMemoryStore（内置，fastembed 本地 embedding）

Enterprise:
  attaos → MemoryStore trait
              ├── SqliteMemoryStore（内置）
              ├── PgVectorMemoryStore（Postgres + pgvector）
              └── QdrantMemoryStore（外部 Qdrant 服务）
```

**策略：**
1. Desktop 保持内置（fastembed + SQLite），零外部依赖
2. Enterprise 通过 `MemoryStore` trait 支持外部向量数据库
3. 不走 MCP — 记忆是 Agent 核心能力，不是可选工具

---

## 建议

| 场景 | 推荐 |
|------|------|
| 当前阶段（MVP） | **方案 A（内置）**：激活现有 memory crate，接入 fastembed |
| Desktop 长期 | **方案 A**：fastembed + sqlite-vec（数据量增长后） |
| Enterprise | **方案 C 混合**：trait 抽象已就绪，增加 pgvector/Qdrant 后端 |
| MCP Server | **不推荐**：记忆不适合做外部服务，延迟和一致性代价过高 |

### 核心理由

记忆系统**不适合**做独立服务，因为：

1. **调用频率高**：Agent 每轮 ReAct 可能查 1-3 次记忆，延迟敏感
2. **上下文耦合**：记忆查询需要 task_id、skill_id 等内部元数据，跨进程传递增加复杂度
3. **竞品共识**：OpenClaw（3000+ 行）和 ZeroClaw 都选择内置，经过实战验证
4. **AttaOS 已有抽象**：`MemoryStore` trait 已支持后端切换，无需通过 MCP 实现解耦
5. **Desktop 体验**：单进程 = 零配置，符合 "零外部依赖" 的 Desktop 定位

**MCP 适合的场景**是外部工具集成（浏览器、文件系统、第三方 API），而非核心 Agent 能力。记忆之于 Agent，就像内存之于进程 —— 不应该是远程调用。

---

## 决策点

1. 是否同意保持内置方案？→ 激活 memory crate（接入 AppState + fastembed）
2. Enterprise 版是否需要 Qdrant/pgvector 后端？→ 当前 trait 抽象已支持，按需添加
3. 是否需要将记忆暴露为 MCP tool（让外部 Agent 也能调用 attaos 的记忆）？→ 这与"记忆服务独立化"不同，可以作为 attaos MCP 对外暴露的 tool 之一
