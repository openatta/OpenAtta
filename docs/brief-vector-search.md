# 分析简报：向量搜索优化方案

## 现状

- 暴力扫描：每次查询 `SELECT id, embedding FROM memories WHERE embedding IS NOT NULL` 加载全部行到内存
- 纯 Rust cosine_similarity 逐行计算，O(N×D) + O(N log N) 排序
- 10,000 条 × 384 维 ≈ 15MB 内存/查询，毫秒级延迟（可接受）
- 100,000 条 → ~150MB/查询，开始成为问题

## 方案对比

### 方案 A — sqlite-vec 扩展（推荐 Desktop）

SQLite 原生向量搜索扩展 (Alex Garcia)，支持 KNN 查询。

**优点：**
- 无需额外进程，嵌入 SQLite 进程
- SQL 原生语法 `WHERE embedding MATCH ? AND k = ?`
- 支持 IVF 索引，亚线性查询
- 维护活跃，与 sqlx 兼容（通过 `load_extension`）

**缺点：**
- 需要在编译时或运行时加载 `.dylib/.so` 扩展
- 维度在建表时固定
- 跨平台分发需打包扩展二进制

**复杂度：中**

```
1. 编译 sqlite-vec 扩展 → 打包进发布包
2. SqliteMemoryStore::new() 中 load_extension("vec0")
3. CREATE VIRTUAL TABLE memories_vec USING vec0(...)
4. store() 时同步写入 memories_vec
5. vector_search() 改为 KNN SQL 查询替代暴力扫描
```

### 方案 B — usearch 内存索引（高性能）

HNSW 算法的 Rust 绑定，内存中构建近似最近邻索引。

**优点：**
- 亚毫秒级查询（百万级数据）
- 纯 Rust（通过 C FFI），无 SQLite 扩展依赖
- 支持量化（f16/i8）减少内存占用

**缺点：**
- 索引在内存中，启动时需从 DB 重建（10K 条 <1 秒）
- 需要 UUID↔u64 映射层
- 额外内存占用（索引本身约等于向量数据大小）

### 方案 C — 暴力扫描优化（最小改动）

**适用场景：** 预期记忆数 < 10,000 条（Desktop 单用户）

**改动：**
- `vector_search` SQL 添加 `LIMIT 50000` 硬上限防止 OOM
- 在 Rust 层用 `partial_sort` 替代 full sort（只排前 K 个）
- 添加 `task_id`/`skill_id` 索引到 memories 表

**复杂度：极低** — 几行代码改动

## 建议

| 场景 | 推荐方案 |
|------|----------|
| Desktop（< 10K 条记忆） | 方案 C（暴力扫描优化） |
| Desktop（10K-100K 条） | 方案 A（sqlite-vec） |
| Enterprise（> 100K 条） | 方案 B（usearch HNSW）或直接用 pgvector |

## 决策点

1. Desktop 版预期记忆量：大部分用户 < 10K 条，方案 C 是否足够？
2. 是否愿意承担 sqlite-vec 扩展的跨平台打包复杂度？
3. Enterprise 版是否直接依赖 pgvector（Postgres 原生支持）？
