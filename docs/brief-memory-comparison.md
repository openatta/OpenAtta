# 分析简报：OpenClaw vs ZeroClaw 记忆系统对比 & AttaOS 选型评估

## 一、全维度对比

### 1. Embedding 提供者

| 维度 | OpenClaw (TypeScript) | ZeroClaw (Rust) | AttaOS (Rust) |
|------|----------------------|-----------------|---------------|
| 提供者数量 | 7 种 | 3 种 + Noop | 1 种 (Noop) |
| 本地推理 | node-llama-cpp (GGUF) | ❌ 无 | ❌ 无（fastembed 待实现） |
| 云端 API | OpenAI, Gemini, Voyage, Mistral, Ollama | OpenAI, OpenRouter, Custom URL | ❌ 无 |
| 自动选择 | ✅ `auto` 模式（按优先级尝试） | ❌ 手动配置 | ❌ |
| 降级策略 | embedding 不可用 → FTS-only | Noop provider → keyword-only | Noop → 向量搜索跳过 |
| 批量嵌入 | ✅ embedBatch + 并发控制 | ✅ embed(&[&str]) | ❌ 单条 embed |
| 缓存 | ✅ DB 级缓存（provider+model+hash） | ✅ LRU 缓存（SHA-256 hash, 10K 上限） | ❌ 无缓存 |

**评价**：OpenClaw 在 embedding 层面远超 ZeroClaw — 7 个提供者 + 本地推理 + 自动降级 + API Key 轮换（Gemini）。ZeroClaw 务实但功能有限，仅支持 OpenAI 兼容 API。

### 2. 向量存储与搜索

| 维度 | OpenClaw | ZeroClaw | AttaOS |
|------|----------|----------|--------|
| 主存储 | SQLite + sqlite-vec 扩展 | SQLite 暴力扫描 | SQLite 暴力扫描 |
| 向量索引 | ✅ sqlite-vec (IVF/flat) | ❌ 全表 O(n×d) 扫描 | ❌ 全表 O(n×d) 扫描 |
| 外部向量 DB | LanceDB（可选） | Qdrant（可选） | ❌ 无 |
| 距离度量 | cosine（via sqlite-vec） | cosine（手工 f64 计算） | cosine（手工 f32 计算） |
| Embedding 存储 | TEXT (JSON 序列化) + BLOB (sqlite-vec) | BLOB (little-endian f32) | BLOB (little-endian f32) |
| 10K 条性能 | < 10ms（sqlite-vec 索引） | ~50-100ms（暴力扫描） | ~50-100ms（暴力扫描） |
| 100K 条性能 | < 50ms | ~500ms-1s | ~500ms-1s |

**评价**：OpenClaw 用 sqlite-vec 实现了亚线性查询，是三者中唯一有**真正向量索引**的。ZeroClaw 和 AttaOS 都是暴力扫描，仅适合 < 10K 条。

### 3. 混合搜索策略

| 维度 | OpenClaw | ZeroClaw | AttaOS |
|------|----------|----------|--------|
| FTS 引擎 | SQLite FTS5 | SQLite FTS5 | SQLite FTS5 |
| 评分算法 | BM25 + cosine 加权融合 | BM25 + cosine RRF 融合 | BM25 + cosine RRF 融合 |
| 默认权重 | vector 0.7 / text 0.3 | vector 0.7 / keyword 0.3 | vector 0.7 / fts 0.3 |
| 合并公式 | `w_v × vec_score + w_t × text_score` | `w_v / (k + rank_v + 1) + w_k / (k + rank_k + 1)` | 同 ZeroClaw（RRF） |
| MMR 多样性 | ✅ lambda 可调 | ❌ 无 | ❌ 无 |
| 时间衰减 | ✅ 指数衰减（halfLife 可调） | ❌ 无 | ❌ 无 |
| 多语言分词 | ✅ 8 种语言停词表 + CJK n-gram | ❌ 基础分词 | ❌ 基础分词 |
| 降级搜索 | FTS-only → 仍可用 | LIKE 模糊搜索兜底 | FTS-only → 仍可用 |

**评价**：

- **OpenClaw** 的搜索策略最成熟 — MMR 避免结果同质化，时间衰减处理知识老化，多语言分词覆盖中日韩
- **ZeroClaw** 的 RRF 公式更学术规范（基于排名而非分数），且有 LIKE 兜底
- **AttaOS** 直接采用了 RRF 方案，与 ZeroClaw 同源

**关键差异**：OpenClaw 用**分数加权融合**（score-based），ZeroClaw/AttaOS 用 **RRF（rank-based）**。学术研究表明 RRF 在不同评分尺度的融合上更鲁棒（不受 BM25 绝对值影响），但 score-based 在分数校准良好时信息保留更多。

### 4. 记忆生命周期

| 维度 | OpenClaw | ZeroClaw | AttaOS |
|------|----------|----------|--------|
| 记忆来源 | Markdown 文件 + 会话摘录 | 用户输入 + 自动保存 | Tool 调用（store/recall） |
| 去重机制 | SHA-256 内容哈希 | key UNIQUE 约束 | UUID 主键（无内容去重） |
| 文件监听 | ✅ chokidar + debounce | ❌ 无 | ❌ 无 |
| 自动清理 | ✅ 基于时间（可配置） | ✅ archive + purge（天数） | ✅ cleanup(before: DateTime) |
| 快照恢复 | ❌ 无 | ✅ MEMORY_SNAPSHOT.md 冷启动恢复 | ❌ 无 |
| Session 隔离 | ✅ source 过滤 | ✅ session_id 过滤 | ✅ task_id/skill_id 过滤 |
| 访问计数 | ❌ 无 | ❌ 无 | ✅ access_count 字段 |

**评价**：三者各有侧重。OpenClaw 面向"文件即记忆"（Markdown 驱动），ZeroClaw 面向"对话即记忆"（自动保存），AttaOS 面向"任务即记忆"（与 task/skill 关联）。

### 5. 性能与缓存

| 维度 | OpenClaw | ZeroClaw | AttaOS |
|------|----------|----------|--------|
| Embedding 缓存 | ✅ DB 表（provider+model+hash） | ✅ LRU 表（SHA-256, 10K 上限） | ❌ 无 |
| 响应缓存 | ❌ 无 | ✅ 可选（TTL + 5K 上限） | ❌ 无 |
| SQLite PRAGMA 调优 | 默认 | ✅ WAL + mmap 8MB + cache 2MB | 默认 |
| 批量操作 | ✅ embedBatch + 并发 4 路 | ✅ spawn_blocking | ❌ 单条操作 |
| DB 索引 | ✅ 基础索引 | ✅ category, key, cache 索引 | ❌ 无索引（除 FTS5） |

**评价**：ZeroClaw 在 SQLite 调优上最专业（WAL + PRAGMA），OpenClaw 在 embedding 批量操作上最高效。AttaOS 在性能优化方面几乎为零。

### 6. 代码质量与抽象

| 维度 | OpenClaw | ZeroClaw | AttaOS |
|------|----------|----------|--------|
| 语言 | TypeScript | Rust | Rust |
| 代码量 | ~3000 行 | ~2000 行 | ~1000 行 |
| 抽象层次 | 4 层类继承 | trait 多态 | trait 多态 |
| 后端切换 | builtin / qmd | 6 种后端 (Memory trait) | 1 种 (MemoryStore trait) |
| 类型安全 | 中等（TS 运行时检查） | 高（编译时保证） | 高（编译时保证） |
| 测试覆盖 | 良好（mock provider） | 良好（单元 + 集成） | 基础（核心路径） |
| 错误处理 | try-catch 降级 | anyhow + thiserror | anyhow（粗粒度） |

**评价**：AttaOS 的 trait 设计最干净（MemoryStore 5 个方法 vs ZeroClaw 的 8 个），但实现最不完整。ZeroClaw 的 Rust 实现是最佳参考 — 同语言、同 trait 模式、同 SQLite 后端。

### 7. 配置灵活性

| 维度 | OpenClaw | ZeroClaw | AttaOS |
|------|----------|----------|--------|
| 配置格式 | YAML (agent config) | TOML (全局 config) | ❌ 硬编码 |
| 搜索权重 | ✅ 可调 | ✅ 可调 | ❌ 硬编码 0.7/0.3 |
| 模型选择 | ✅ 每 agent 可配 | ✅ 全局 + route hint | ❌ 无 |
| 后端切换 | ✅ builtin/qmd | ✅ 6 种后端 | ❌ 仅 SQLite |

---

## 二、哪个方案更优？

### 综合评分

| 维度 | OpenClaw | ZeroClaw | 权重 |
|------|----------|----------|------|
| Embedding 生态 | ★★★★★ | ★★☆☆☆ | 高 |
| 向量搜索性能 | ★★★★★ | ★★☆☆☆ | 中 |
| 混合搜索质量 | ★★★★★ | ★★★★☆ | 高 |
| 代码质量 | ★★★★☆ | ★★★★★ | 中 |
| 配置灵活性 | ★★★★☆ | ★★★★★ | 中 |
| 性能优化 | ★★★★☆ | ★★★★★ | 中 |
| 可维护性 | ★★★☆☆ | ★★★★★ | 高 |
| 生产就绪度 | ★★★★★ | ★★★★☆ | 高 |

### 结论

**功能完整度：OpenClaw 胜出**
- 7 种 embedding 提供者、sqlite-vec 向量索引、MMR 多样性、时间衰减、8 语言分词
- 是三者中唯一真正"生产级"的记忆系统

**工程质量：ZeroClaw 胜出**
- Rust trait 抽象更干净、SQLite PRAGMA 调优专业、LRU 缓存设计合理
- 6 种后端通过统一 `Memory` trait 切换，配置灵活
- 对 AttaOS 而言是**更好的参考** — 同语言、同技术栈、同设计模式

**实际推荐**：

> OpenClaw 的**设计思路**更优（sqlite-vec + MMR + 时间衰减 + 多语言），
> ZeroClaw 的**工程实现**更优（Rust trait + PRAGMA 调优 + 缓存 + 配置化）。
>
> AttaOS 应**取 OpenClaw 之长补 ZeroClaw 之短** —— 即：以 ZeroClaw 的 Rust 工程模式为底座，吸收 OpenClaw 的搜索策略创新。

---

## 三、AttaOS 选型对比

### 现有优势（已领先的地方）

| AttaOS 已有 | 对应竞品 | 评价 |
|------------|---------|------|
| `MemoryStore` trait（5 方法，极简） | ZeroClaw 8 方法，OpenClaw 类继承 | ✅ **更干净** |
| RRF 融合算法（独立模块 rrf.rs） | ZeroClaw 同源，OpenClaw 用 score 加权 | ✅ **学术规范** |
| task_id / skill_id 关联 | ZeroClaw session_id, OpenClaw source | ✅ **更适合任务调度场景** |
| access_count 访问计数 | 两者均无 | ✅ **独有** |
| FTS5 trigger 同步 | 两者均有 | ✅ 持平 |

### 现有差距（需补齐的地方）

| 缺失项 | 优先级 | 参考来源 | 实现复杂度 |
|--------|--------|---------|-----------|
| 真实 EmbeddingProvider | **P0** | ZeroClaw（OpenAI 兼容） + fastembed（本地） | 低（~50 行） |
| Embedding 缓存 | **P0** | ZeroClaw（LRU + SHA-256 hash） | 低（~80 行 + 1 表） |
| DB 索引（task_id, skill_id, created_at） | **P0** | ZeroClaw | 极低（3 行 SQL） |
| SQLite PRAGMA 调优（WAL, mmap, cache） | **P1** | ZeroClaw | 极低（5 行） |
| 搜索权重可配置 | **P1** | 两者均支持 | 低 |
| 批量 embed | **P1** | OpenClaw embedBatch | 低 |
| MMR 多样性排序 | **P2** | OpenClaw | 中（~100 行） |
| 时间衰减评分 | **P2** | OpenClaw（指数衰减 + halfLife） | 低（~30 行） |
| sqlite-vec 向量索引 | **P2** | OpenClaw | 中（扩展打包） |
| 多语言分词 / 停词表 | **P3** | OpenClaw（8 语言） | 中 |

### 建议实施路径

```
Phase 1（MVP 激活）
├── fastembed EmbeddingProvider      ← brief-fastembed.md 已分析
├── Embedding 缓存（参考 ZeroClaw LRU）
├── DB 索引补齐
├── PRAGMA 调优
└── 接入 AppState + Tool 实现

Phase 2（搜索质量）
├── 搜索权重可配置（SearchOptions 或全局 config）
├── MMR 多样性（参考 OpenClaw lambda 算法）
├── 时间衰减（指数衰减 + halfLife 参数）
└── 批量 embed

Phase 3（规模化）
├── sqlite-vec 向量索引（参考 brief-vector-search.md 方案 A）
├── Enterprise: pgvector 后端
└── 多语言分词优化
```

### AttaOS 的独特优势

AttaOS 有一个两个竞品都没有的架构优势：**双版本 trait 切换**。

```
Desktop:  SqliteMemoryStore + fastembed（零外部依赖）
Enterprise: PgVectorMemoryStore + OpenAI API（可扩展）
```

通过 `MemoryStore` trait + Cargo features，一套代码自动适配两种部署模式。这是 OpenClaw（仅 SQLite）和 ZeroClaw（需手动配置后端）都不具备的。

---

## 四、总结

| 问题 | 答案 |
|------|------|
| OpenClaw vs ZeroClaw 哪个更优？ | **功能 OpenClaw 胜，工程 ZeroClaw 胜**。对 AttaOS 而言 ZeroClaw 是更好的参考（同语言同模式） |
| AttaOS 现有设计能否对标？ | ✅ trait 设计已领先，RRF 算法已正确，task/skill 关联是独有优势 |
| 最大差距在哪？ | P0：无真实 EmbeddingProvider、无缓存、无 DB 索引 |
| 建议策略 | Phase 1 激活 MVP → Phase 2 搜索质量 → Phase 3 规模化 |
