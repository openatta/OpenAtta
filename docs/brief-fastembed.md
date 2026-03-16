# 分析简报：本地 fastembed 嵌入方案

## 现状

- `EmbeddingProvider` trait 已定义：`embed(&str) -> Vec<f32>` + `dimensions() -> usize`
- 运行时仅有 `NoopEmbeddingProvider`（dimensions=0，向量搜索完全跳过）
- Memory Store 未接入 AppState（tools 返回 not_implemented）

## fastembed-rs 方案

**crate**: `fastembed = "4"`（Rust 封装 ONNX Runtime，支持 all-MiniLM-L6-v2 / BGE-small 等模型）

### 优点

- 纯本地运行，无 API Key 需求，零网络延迟
- 384 维（MiniLM）→ 每条记忆仅 1.5KB embedding
- 首次运行自动下载模型（~90MB for MiniLM），后续离线可用
- Desktop 版无需外部服务

### 缺点

- 首次下载模型 ~90MB（需网络）
- CPU 推理：embed 一条约 5-15ms（够用），批量可走 `embed_batch`
- ONNX Runtime 引入 ~20MB 二进制体积
- 仅支持英文模型效果好；中文需选 `multilingual-e5-small`（同样 384 维，~100MB）

### 实现步骤

1. `memory/Cargo.toml`: `fastembed = { version = "4", optional = true }`, feature "fastembed"
2. `memory/src/fastembed.rs`: `FastEmbedProvider` 实现 `EmbeddingProvider` trait
   - 构造: `TextEmbedding::try_new(InitOptions::new(model))`
   - embed: `spawn_blocking` 调 `model.embed(vec![text], None)`
   - dimensions: 384 (MiniLM) 或根据模型返回
3. `server/services.rs`: 构造 `FastEmbedProvider`，注入 `SqliteMemoryStore`
4. `server AppState`: 新增 `memory_store` 字段
5. `tools/memory.rs`: 接入 `MemoryStore` 替换 `not_implemented` stub

### 模型选择

| 模型 | 维度 | 大小 | 语言 | 推荐场景 |
|------|------|------|------|----------|
| all-MiniLM-L6-v2 | 384 | 90MB | 英文 | 开发/测试 |
| multilingual-e5-small | 384 | 100MB | 多语言 | 生产（含中文） |
| BAAI/bge-small-en | 384 | 130MB | 英文 | 高质量英文 |

### 复杂度

**低** — 约 5 个文件改动，核心工作量在 `FastEmbedProvider` 实现（~50 行）和 `services.rs` 接线（~10 行）。

### 决策点

1. 默认模型选择：`multilingual-e5-small`（支持中文）还是 `all-MiniLM-L6-v2`（更小更快）？
2. 模型下载策略：首次运行自动下载 vs 手动安装？
3. 是否同时保留 `NoopEmbeddingProvider` 作为无向量搜索的降级模式？
