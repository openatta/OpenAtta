# AttaOS 安装与升级架构设计

Status: v0.4

---

## 概述

AttaOS 分两个独立产品线打包和安装：

| 产品线 | 分发物 | 目标用户 | 安装方式 |
|--------|--------|----------|----------|
| **Desktop** | `attash` (.dmg/.msi) + `atta-desktop-components.tar.gz` | 普通用户 | GUI 安装向导 |
| **Enterprise** | `atta-enterprise-components.tar.gz` + `install.sh` | 运维专业人员 | 命令行脚本 |

核心原则：
- `attash` 保持轻量（~15 MB），不内嵌组件
- 组件包含全部运行时依赖（含 ONNX 模型），**支持完全离线安装**
- Desktop 和 Enterprise 组件包独立打包、独立版本
- Enterprise 不提供 GUI 安装，假设用户有能力使用脚本

---

## 一、实测组件体积

基于当前代码 `cargo build --release` + `strip` 的实际数据（darwin-aarch64）：

### 1.1 二进制体积

| 组件 | strip 后 | gzip 压缩 |
|------|----------|-----------|
| `attaos`（服务守护进程） | **19.2 MB** | 7.8 MB |
| `attacli`（CLI 客户端） | **4.1 MB** | 1.7 MB |
| `attash`（桌面 Shell） | **10.6 MB** | 3.9 MB |

### 1.2 数据资源体积

| 组件 | 大小 | 说明 |
|------|------|------|
| ONNX 模型 (model.onnx) | **86 MB** | AllMiniLML6V2 全精度，Qdrant/all-MiniLM-L6-v2-onnx |
| tokenizer.json | **695 KB** | 分词器 |
| config.json + special_tokens_map.json | **1.3 KB** | 模型配置 |
| WebUI dist | **216 KB** | Vue 3 SPA 静态文件 |
| 内置 Skills | **24 KB** | YAML 定义文件 |
| 内置 Flows | **20 KB** | YAML 定义文件 |

> 注：DB migrations 已通过 `sqlx::migrate!()` 编译时嵌入 attaos 二进制，无需外部文件。

### 1.3 分发包体积

| 分发物 | 内容 | 体积（估） |
|--------|------|-----------|
| `attash` 安装包 (DMG/MSI) | attash 二进制 + Tauri 运行时 | **~15 MB** |
| Desktop 组件包 (tar.gz) | attaos + attacli + ONNX 模型 + WebUI + Skills + Flows | **~40 MB** |
| Enterprise 组件包 (tar.gz) | attaos(enterprise) + attacli + WebUI + Skills + Flows + 安装脚本 | **~12 MB** |

---

## 二、Desktop 版

### 2.1 架构总览

```
┌──────────────────────────────────────────────────────────┐
│                   GitHub Releases                         │
│                                                          │
│  ├── attash-v0.2.0-darwin-aarch64.dmg        (~15 MB)    │
│  ├── atta-desktop-v0.2.0-darwin-aarch64.tar.gz (~40 MB)  │
│  ├── atta-desktop-v0.2.0-darwin-x86_64.tar.gz            │
│  ├── atta-desktop-v0.2.0-linux-x86_64.tar.gz             │
│  ├── atta-desktop-v0.2.0-windows-x86_64.zip              │
│  └── desktop-manifest.json              (版本索引)        │
└──────────────────────────────────────────────────────────┘
           │                         │
           │ 用户下载安装             │ 首次启动时下载
           ▼                         │ 或指定本地文件
┌──────────────────┐                 │
│  attash (~15 MB) │─────────────────┘
│                  │
│  • Runtime Shell │          ┌──────────────────────┐
│  • Installer UI  │────────▶ │ atta-desktop.tar.gz  │
│  • Manager UI    │  安装    │ (~40 MB)             │
│  • Updater UI    │          │                      │
│                  │          │ • attaos  (19.2 MB)  │
│                  │          │ • attacli (4.1 MB)   │
│                  │          │ • ONNX model (86 MB) │
│                  │          │ • WebUI   (216 KB)   │
│                  │          │ • Skills + Flows     │
│                  │          │ • manifest.json      │
└──────────────────┘          └──────────────────────┘
```

### 2.2 attash 启动逻辑

```
attash 启动
    │
    ├── 检查 ATTA_HOME/.manifest.json
    │   │
    │   ├── 存在 ──▶ Runtime Shell 模式
    │   │           启动 ATTA_HOME/bin/attaos
    │   │           main 窗口导航到 http://localhost:{port}
    │   │
    │   └── 不存在
    │       │
    │       ├── 检查 ATTA_HOME/etc/connections.json
    │       │   │
    │       │   ├── 存在 ──▶ 远程连接模式
    │       │   │           main 窗口导航到远程 URL
    │       │   │
    │       │   └── 不存在 ──▶ Installer 模式
    │       │                  显示安装向导窗口
    │       │
    ▼
```

**Installer 窗口不是独立窗口，而是 main 窗口的不同前端页面。** attash 只有一个 main 窗口，根据状态加载不同内容：

| 状态 | main 窗口内容 | 说明 |
|------|--------------|------|
| 已安装（`.manifest.json` 存在） | `http://localhost:{port}` | 导航到本地 attaos WebUI |
| 已连接（`connections.json` 存在） | `https://remote:port` | 导航到远程 attaos WebUI |
| 未安装 | `/installer.html` | 加载内嵌安装向导 |

### 2.3 本地安装流程

```
[1] Welcome        →  版本信息
[2] 安装模式        →  本地安装 / 连接服务器
[3] 安装目录        →  默认 ~/.atta，可自定义
[4] 组件来源        →  在线下载（推荐）/ 选择本地文件
[5] 基本配置        →  LLM Provider + API Key（可跳过）
[6] 安装进度        →  下载 → 解压 → 初始化 DB → 校验
[7] 完成            →  启动 attaos，main 窗口导航到 WebUI
```

**详细步骤：**

1. **安装目录**：默认 `~/.atta`，检查磁盘空间（需 ~150 MB）和写权限

2. **组件来源**：
   - **在线下载**：从 `desktop-manifest.json` 获取当前平台 URL，下载到 `cache/downloads/`
   - **本地文件**：用户通过文件对话框指定已下载的 `atta-desktop-*.tar.gz`

3. **下载与部署**：
   - 验证包 SHA-256
   - 创建 ATTA_HOME 目录结构（调用 `home.ensure_dirs()`）
   - 解压到对应目录
   - 设置二进制 `chmod +x`
   - 从 `templates/` 生成 `etc/attaos.yaml` 和 `etc/keys.env`

4. **初始化**：
   - 启动 attaos（自动创建 SQLite DB + 运行嵌入的 migrations）
   - 等待健康检查通过
   - 写入 `.manifest.json`

5. **完成**：
   - main 窗口从 installer 页面导航到 `http://localhost:{port}`（WebUI）

### 2.4 连接服务器流程

attash 作为纯客户端，连接已部署的 Enterprise attaos。**不安装本地组件，不启动本地 attaos。**

```
[1] 安装模式        →  选择"连接服务器"
[2] 服务地址        →  输入 attaos URL（如 https://attaos.company.com:3000）
[3] 认证            →  用户名 + 密码 / OIDC SSO
[4] 连接测试        →  GET /api/v1/health + 认证验证
[5] 完成            →  保存连接配置
                        main 窗口导航到远程 WebUI URL
```

**认证与权限：**
- 认证成功后，远程 attaos 根据用户角色返回相应权限的 WebUI
- 管理员：与本地安装场景完全一致（全功能）
- 普通用户：只能看到自己有权限的资源（由 Enterprise RBAC 控制）
- attash 本身不做权限过滤，完全由远程 attaos 的 API 层控制

**连接信息保存到 `~/.atta/etc/connections.json`：**

```json
{
  "connections": [
    {
      "name": "Production",
      "url": "https://attaos.company.com:3000",
      "auth_type": "oidc",
      "default": true
    }
  ]
}
```

**后续启动行为：**
- attash 检测到 `connections.json` → 直接导航 main 窗口到远程 URL
- Tray 菜单显示 "Open Console"（打开 main 窗口）和 "Disconnect"（清除连接配置，回到安装向导）

### 2.5 Desktop 组件包结构

```
atta-desktop-v0.2.0-darwin-aarch64.tar.gz
├── manifest.json
├── bin/
│   ├── attaos                   # Desktop build (stripped, 19.2 MB)
│   └── attacli                  # CLI (stripped, 4.1 MB)
├── models/
│   └── fastembed/               # 完整 fastembed HF cache 格式
│       └── models--Qdrant--all-MiniLM-L6-v2-onnx/
│           ├── blobs/
│           │   ├── bbd7b466...  # model.onnx (86 MB)
│           │   ├── c17ed520...  # tokenizer.json (695 KB)
│           │   ├── 56c8c186...  # config.json
│           │   ├── 9bbecc17...  # special_tokens_map.json
│           │   └── 61e23f16...  # tokenizer_config.json
│           ├── refs/
│           │   └── main
│           └── snapshots/
│               └── 5f1b8cd7.../
│                   ├── model.onnx → ../../blobs/bbd7b466...
│                   └── ...
├── webui/
│   └── dist/                    # Vue SPA
├── lib/
│   ├── skills/                  # 内置 Skills
│   └── flows/                   # 内置 Flows
└── templates/
    ├── attaos.yaml              # 默认配置
    └── keys.env.example
```

> 模型目录保持 fastembed 原生的 HF hub cache 格式（blobs + refs + snapshots），安装时整体复制到 `ATTA_HOME/models/fastembed/`，attaos 通过 `FASTEMBED_CACHE_DIR` 环境变量或 `InitOptions::with_cache_dir()` 指向此目录。

### 2.6 部署后目录结构

```
$ATTA_HOME (~/.atta)
├── etc/                          # 用户配置（升级保留）
│   ├── attaos.yaml
│   ├── keys.env
│   └── connections.json          # 远程连接配置（连接服务器模式）
├── data/                         # 持久数据（升级保留）
│   ├── data.db
│   └── estop.json
├── log/
├── cache/
│   ├── downloads/                # 组件包下载缓存
│   └── open-skills/              # 社区 Skill 同步
├── run/
│   └── attaos.pid
├── bin/                          # 二进制（升级替换）
│   ├── attaos
│   └── attacli
├── models/                       # 模型文件（升级按需替换）
│   └── fastembed/                # fastembed HF cache 目录
│       └── models--Qdrant--all-MiniLM-L6-v2-onnx/
│           └── ...
├── lib/                          # 内置资源（升级整体替换）
│   ├── webui/
│   ├── skills/
│   └── flows/
├── exts/                         # 用户扩展（升级永不覆盖）
│   ├── skills/
│   ├── flows/
│   ├── tools/
│   └── mcp/
├── .manifest.json                # 已安装组件清单
└── .backup/                      # 升级备份
```

**分区策略：**

| 目录 | 管理方 | 升级行为 |
|------|--------|----------|
| `etc/` | 用户 | **保留** |
| `data/` | 系统 | **迁移**（嵌入式 migration 自动执行） |
| `bin/` | 安装器 | **替换** |
| `models/` | 安装器 | **按需替换** |
| `lib/` | 安装器 | **整体替换** |
| `exts/` | 用户 | **保留** |

---

## 三、Enterprise 版

### 3.1 环境要求

| 依赖 | 版本要求 | 说明 |
|------|----------|------|
| **Linux** | x86_64 或 aarch64 | 推荐 Ubuntu 22.04+ / RHEL 9+ |
| **PostgreSQL** | >= 14 | 主存储 |
| **NATS Server** | >= 2.10（含 JetStream） | 事件总线 |
| **git** | >= 2.25 | 社区 Skill 同步 |
| **systemd** | - | 服务管理（可选） |
| 磁盘空间 | >= 500 MB | 二进制 + 数据 |
| 内存 | >= 2 GB | attaos 运行时 |

**可选依赖：**

| 依赖 | 用途 | 说明 |
|------|------|------|
| Embedding API（OpenAI / 自建） | 向量搜索 | 企业版不含本地 ONNX，使用外部 API |
| Nginx / Caddy | 反向代理 + TLS | 生产部署推荐 |
| OIDC Provider | 企业 SSO | Keycloak / Azure AD / Okta |

### 3.2 Enterprise 组件包结构

```
atta-enterprise-v0.2.0-linux-x86_64.tar.gz
├── manifest.json
├── bin/
│   ├── attaos                   # Enterprise build (stripped)
│   └── attacli                  # CLI (stripped)
├── webui/
│   └── dist/                    # Vue SPA
├── lib/
│   ├── skills/
│   └── flows/
├── templates/
│   ├── attaos.yaml              # 默认配置（含 Postgres/NATS 配置段）
│   ├── keys.env.example
│   └── attaos.service           # systemd unit file 模板
├── install.sh                   # 安装脚本
└── README.md                    # 安装说明
```

> Enterprise 包 **不含** ONNX 模型（企业版使用外部 Embedding API）。
> DB migrations 已嵌入 attaos 二进制，首次启动时自动执行，无需外部 SQL 文件。

### 3.3 安装脚本 (install.sh)

```bash
#!/bin/bash
# AttaOS Enterprise Installer
# Usage: ./install.sh [--home /path/to/atta] [--check-only] [--upgrade]

# 1. 环境检查
#    - OS / 架构
#    - PostgreSQL 连通性 (psql --version, 连接测试)
#    - NATS Server 连通性 (nats-server --version, 连接测试)
#    - git 版本
#    - 磁盘空间
#    - 端口可用性 (默认 3000)

# 2. 创建目录结构
#    - $ATTA_HOME/{etc,data,log,cache,run,bin,lib,exts}

# 3. 部署文件
#    - 复制 bin/ → $ATTA_HOME/bin/
#    - 复制 webui/ → $ATTA_HOME/lib/webui/
#    - 复制 lib/ → $ATTA_HOME/lib/
#    - chmod +x bin/*

# 4. 生成配置
#    - 交互式询问或从环境变量读取:
#      ATTA_PG_URL, ATTA_NATS_URL, ATTA_PORT, ATTA_ADMIN_USER
#    - 生成 attaos.yaml

# 5. 启动 attaos（自动运行嵌入的 Postgres migrations）

# 6. 安装 systemd service（可选）
#    - 复制 attaos.service → /etc/systemd/system/
#    - systemctl daemon-reload && systemctl enable attaos

# 7. 完整性校验
#    - 校验 manifest.json 中每个组件的 SHA-256
#    - 写入 .manifest.json

# 8. 健康检查
#    - GET http://localhost:{port}/api/v1/health
```

使用示例：

```bash
# 检查环境（不安装）
./install.sh --check-only

# 非交互式安装
ATTA_HOME=/opt/attaos \
ATTA_PG_URL="postgres://user:pass@db:5432/attaos" \
ATTA_NATS_URL="nats://nats:4222" \
ATTA_PORT=3000 \
ATTA_ADMIN_USER=admin \
./install.sh

# 交互式安装
./install.sh

# 升级
./install.sh --upgrade --home /opt/attaos
```

### 3.4 Enterprise 升级

```bash
# 1. 下载新版本组件包
wget https://github.com/.../atta-enterprise-v0.3.0-linux-x86_64.tar.gz

# 2. 解压
tar xzf atta-enterprise-v0.3.0-linux-x86_64.tar.gz
cd atta-enterprise-v0.3.0

# 3. 执行升级
./install.sh --upgrade --home /opt/attaos

# 升级脚本自动:
#   - 校验当前安装
#   - 备份到 .backup/v{old}/
#   - 停止 attaos (systemctl stop)
#   - 替换 bin/ lib/ webui/
#   - 启动 attaos (自动运行新 migrations)
#   - 完整性校验
#   - 健康检查
```

---

## 四、ONNX 模型管理

### 4.1 构建时预下载

项目根目录的 `.fastembed_cache/` 包含 fastembed 运行时下载的模型缓存（87 MB，HF hub 格式）。

**构建时确保模型存在：**

```bash
# 方法 1: 运行一次 attaos 让 fastembed 自动下载
FASTEMBED_CACHE_DIR=.fastembed_cache cargo run -p atta-server --features desktop -- --port 0 &
sleep 10 && kill $!

# 方法 2: 直接用 HF CLI 下载
pip install huggingface_hub
huggingface-cli download Qdrant/all-MiniLM-L6-v2-onnx \
  --cache-dir .fastembed_cache
```

**`.fastembed_cache/` 目录结构（必须保持 HF hub 格式）：**

```
.fastembed_cache/
└── models--Qdrant--all-MiniLM-L6-v2-onnx/
    ├── blobs/                   # 内容寻址存储
    │   ├── bbd7b466...          # model.onnx (86 MB)
    │   ├── c17ed520...          # tokenizer.json (695 KB)
    │   ├── 56c8c186...          # config.json (650 B)
    │   ├── 9bbecc17...          # special_tokens_map.json (695 B)
    │   └── 61e23f16...          # tokenizer_config.json (1.4 KB)
    ├── refs/
    │   └── main                 # 版本引用
    └── snapshots/
        └── 5f1b8cd7.../         # 符号链接 → blobs/
            ├── model.onnx → ../../blobs/bbd7b466...
            ├── tokenizer.json → ../../blobs/c17ed520...
            └── ...
```

### 4.2 打包时纳入组件包

CI 打包脚本将 `.fastembed_cache/` 整体复制到 Desktop 组件包：

```bash
cp -rP .fastembed_cache staging/models/fastembed
```

> `-rP` 保留符号链接（snapshots 目录中的 symlinks），避免膨胀。

### 4.3 安装时部署

安装器将模型解压到 `ATTA_HOME/models/fastembed/`。

### 4.4 attaos 读取模型（需改代码）

**当前代码**（`crates/memory/src/fastembed.rs`）：
```rust
// 无 cache_dir 参数，使用 fastembed 默认位置
fastembed::InitOptions::new(model_type)
    .with_show_download_progress(true)
```

**需改为**：
```rust
// 接受 cache_dir 参数，指向 ATTA_HOME/models/fastembed/
fastembed::InitOptions::new(model_type)
    .with_cache_dir(cache_dir)
    .with_show_download_progress(true)
```

改动文件：
- `crates/memory/src/fastembed.rs` — `FastEmbedProvider::new()` 增加 `cache_dir: PathBuf` 参数
- `crates/server/src/services.rs` — 传入 `home.models().join("fastembed")`
- `crates/server/src/home.rs` — 添加 `pub fn models(&self) -> PathBuf` 和 `pub fn bin(&self) -> PathBuf`

### 4.5 .gitignore

```gitignore
# ONNX model cache (downloaded at build time, packaged into components)
.fastembed_cache/
```

---

## 五、升级机制

### 5.1 Desktop 升级 — 两条独立通道

```
┌─────────────────────────────────────────────────┐
│               GitHub Releases                    │
│                                                  │
│  ┌────────────────┐    ┌─────────────────────┐   │
│  │ attash releases│    │ desktop-components  │   │
│  │ (Tauri updater)│    │ releases            │   │
│  └───────┬────────┘    └──────────┬──────────┘   │
└──────────┼─────────────────────────┼─────────────┘
           │                         │
           ▼                         ▼
  ┌────────────────┐      ┌──────────────────┐
  │ 通道 A:         │      │ 通道 B:           │
  │ attash 自身     │      │ 组件包            │
  │                │      │                   │
  │ Tauri updater  │      │ 版本对比           │
  │ ~15 MB         │      │ ~40 MB            │
  └────────────────┘      └──────────────────┘
```

两个通道完全独立，各自维护版本号，通过 `min_shell_version` 约束兼容性。

### 5.2 通道 A：attash 自身升级

已实现的 Tauri updater 机制：

1. 检查 `latest.json`
2. semver 比较
3. 下载 (~15 MB) + Ed25519 签名验证
4. 替换 + 重启

### 5.3 通道 B：组件升级

```
attash Manager（Tray → Check Updates）
    │
    ▼
下载 desktop-manifest.json
    │
    ▼
对比本地 .manifest.json ──── 无更新 ──▶ 结束
    │
    │ 有更新
    ▼
检查 min_shell_version ──── 不兼容 ──▶ 提示先升级 attash
    │
    │ 兼容
    ▼
显示 changelog + 版本对比
    │ 用户确认
    ▼
下载组件包 (~40 MB) → SHA-256 验证
    │
    ▼
备份 → 停 attaos → 解压覆盖 → 校验 → 启动 attaos
    │
    │ 校验失败 → 回滚 .backup/
    │ 校验通过 → 更新 .manifest.json
    ▼
完成
```

### 5.4 升级策略

| 组件 | 升级方式 | 服务影响 |
|------|----------|----------|
| `attaos` | 停服 → 替换 → 重启（migration 自动执行） | 短暂中断 |
| `attacli` | 直接替换 | 无 |
| ONNX 模型 | 替换文件 | 下次搜索生效 |
| WebUI | 替换 dist/ | 刷新浏览器 |
| 内置 Skills/Flows | 替换 lib/ | 重新加载 |

### 5.5 用户数据保护

升级 **永不触碰**：

- `etc/attaos.yaml` — 用户配置
- `etc/keys.env` — API Key
- `etc/connections.json` — 远程连接
- `data/data.db` — 数据库（仅 migration 增量更新）
- `exts/` — 用户扩展

### 5.6 Enterprise 升级

由运维人员通过脚本执行，参见 3.4 节。

---

## 六、完整性校验

### 6.1 校验时机

| 时机 | 触发方 | 范围 |
|------|--------|------|
| 安装完成 | Installer / install.sh | 全量 |
| 每次启动 | attash | 快速（仅二进制 hash） |
| 升级完成 | Manager / install.sh | 全量，失败则回滚 |
| 用户手动 | Manager UI / CLI | 全量 + 报告 |

### 6.2 manifest.json 格式

```json
{
  "schema_version": 1,
  "version": "0.2.0",
  "edition": "desktop",
  "min_shell_version": "0.2.0",
  "build_time": "2026-03-10T12:00:00Z",
  "platform": "darwin-aarch64",
  "components": [
    {
      "name": "attaos",
      "type": "binary",
      "path": "bin/attaos",
      "sha256": "a1b2c3d4...",
      "size": 20147280,
      "version": "0.2.0"
    },
    {
      "name": "attacli",
      "type": "binary",
      "path": "bin/attacli",
      "sha256": "e5f6a7b8...",
      "size": 4248672,
      "version": "0.2.0"
    },
    {
      "name": "embedding-model",
      "type": "model",
      "path": "models/fastembed/models--Qdrant--all-MiniLM-L6-v2-onnx",
      "sha256": "c9d0e1f2...",
      "size": 90177536,
      "version": "0.3.0"
    },
    {
      "name": "webui",
      "type": "frontend",
      "path": "lib/webui",
      "sha256": "34567890...",
      "size": 221184,
      "version": "0.2.0"
    },
    {
      "name": "skills",
      "type": "data",
      "path": "lib/skills",
      "sha256": "abcdef01...",
      "size": 24576,
      "version": "0.2.0"
    },
    {
      "name": "flows",
      "type": "data",
      "path": "lib/flows",
      "sha256": "23456789...",
      "size": 20480,
      "version": "0.2.0"
    }
  ]
}
```

### 6.3 校验流程

```
对每个组件:
  1. 文件/目录是否存在       → MISSING
  2. 大小是否匹配            → SIZE_MISMATCH
  3. SHA-256 是否匹配        → HASH_MISMATCH
  4. 可执行权限 (二进制)     → PERMISSION_ERROR
  5. 全部通过               → OK
```

异常修复：重新从组件包解压，或提示用户重新安装。

---

## 七、云端 ↔ 桌面端交互

### 7.1 交互拓扑

```
┌──────────────────────────────────────────────────────┐
│                 桌面端 (attash + attaos)               │
│                                                       │
│  ┌──────────┐   ┌──────────┐   ┌──────────────────┐  │
│  │  WebView  │──▶│  attaos  │──▶│ FastEmbed (ONNX) │  │
│  │  (Shell)  │   │ (Server) │   │ 本地推理         │  │
│  └──────────┘   └────┬─────┘   └──────────────────┘  │
│                      │                                 │
└──────────────────────┼─────────────────────────────────┘
                       │
          ┌────────────┼──────────────────┐
          │            │                  │
          ▼            ▼                  ▼
   ┌────────────┐ ┌──────────┐   ┌──────────────┐
   │  LLM API   │ │  GitHub  │   │ Community    │
   │ Anthropic  │ │ Releases │   │ Skills Repo  │
   │ OpenAI     │ │ (升级)   │   │ (git sync)   │
   │ DeepSeek   │ │          │   │              │
   └────────────┘ └──────────┘   └──────────────┘
```

### 7.2 交互清单

| # | 交互 | 协议 | 频率 | 必需 | 可离线 |
|---|------|------|------|------|--------|
| 1 | LLM 推理 | HTTPS+SSE | 高 | 是 | 否* |
| 2 | 组件包下载（首次安装） | HTTPS | 一次 | 否** | 是 |
| 3 | attash 升级 | HTTPS | 低 | 否 | 是 |
| 4 | 组件升级 | HTTPS | 低 | 否 | 是 |
| 5 | 社区 Skill 同步 | HTTPS(git) | 每 7 天 | 否 | 是 |

> \* 未来可集成 Ollama 实现离线推理
> \*\* 可用本地文件离线安装，完全不需要网络

### 7.3 离线能力

安装完成后（含 ONNX 模型），除 LLM API 外完全离线可用：

| 功能 | 离线可用 |
|------|----------|
| 记忆搜索（向量+FTS） | 是 |
| Task/Flow 管理 | 是 |
| Skill/Flow 编辑 | 是 |
| WebUI | 是 |
| Agent 对话 | 否（依赖 LLM API） |
| 升级检查 | 否（无网络时跳过） |

---

## 八、Tauri Commands 清单

attash 需要实现的 Tauri IPC commands（Rust 侧），供 installer/manager 前端调用：

### 8.1 安装相关

| Command | 参数 | 返回 | 说明 |
|---------|------|------|------|
| `check_installation` | - | `{ installed: bool, connected: bool }` | 检查 `.manifest.json` 和 `connections.json` |
| `get_default_home` | - | `String` | 返回默认 ATTA_HOME 路径 |
| `check_disk_space` | `path: String` | `{ available_mb: u64, required_mb: u64 }` | 检查目标路径磁盘空间 |
| `select_file` | `filters: Vec<String>` | `Option<String>` | 打开文件对话框选择 tar.gz |
| `download_components` | `url: String, dest: String` | - | 下载组件包，emit `download-progress` 事件 |
| `verify_package` | `path: String, sha256: String` | `bool` | SHA-256 校验 |
| `extract_package` | `tar_gz: String, dest: String` | - | 解压 tar.gz，emit `extract-progress` 事件 |
| `install_components` | `package_dir: String, home: String` | - | 部署文件到 ATTA_HOME |
| `start_server` | `home: String, port: u16` | `u16` | 启动 attaos 并等待健康检查 |
| `write_manifest` | `home: String, manifest: Value` | - | 写入 .manifest.json |
| `fetch_manifest` | `url: String` | `Value` | 下载云端 desktop-manifest.json |
| `get_platform` | - | `String` | 返回当前平台标识（如 `darwin-aarch64`） |

### 8.2 连接服务器相关

| Command | 参数 | 返回 | 说明 |
|---------|------|------|------|
| `test_connection` | `url: String` | `{ ok: bool, version: String }` | GET /api/v1/health |
| `save_connection` | `name: String, url: String, auth_type: String` | - | 保存到 connections.json |
| `load_connections` | - | `Vec<Connection>` | 读取 connections.json |
| `remove_connection` | `name: String` | - | 删除连接配置 |

### 8.3 升级/管理相关

| Command | 参数 | 返回 | 说明 |
|---------|------|------|------|
| `read_manifest` | `home: String` | `Value` | 读取本地 .manifest.json |
| `verify_installation` | `home: String` | `Vec<ComponentStatus>` | 全量校验 |
| `backup_current` | `home: String` | `String` | 备份到 .backup/，返回备份路径 |
| `rollback` | `home: String, backup: String` | - | 从备份恢复 |
| `stop_server` | `home: String` | - | 停止 attaos |

### 8.4 已有 Commands（保留）

| Command | 说明 |
|---------|------|
| `show_window` | 显示 main 窗口 |
| `check_update` | Tauri updater 检查 attash 自身更新 |
| `install_update` | Tauri updater 安装 attash 更新 |

### 8.5 需要新增的 Tauri 依赖

| Crate | 用途 |
|-------|------|
| `tauri-plugin-dialog` | 文件选择对话框 |
| `flate2` | gzip 解压 |
| `tar` | tar 解包 |
| `sha2`（workspace 已有） | SHA-256 校验 |
| `reqwest`（workspace 已有） | HTTP 下载 |

---

## 九、代码改动清单

实施安装功能需要改动的现有代码：

### 9.1 `crates/memory/src/fastembed.rs`

```rust
// 改前：
pub fn new(model_type: fastembed::EmbeddingModel) -> Result<Self, AttaError> {
    let model = fastembed::TextEmbedding::try_new(
        fastembed::InitOptions::new(model_type)
            .with_show_download_progress(true),
    )?;
}

// 改后：
pub fn new(
    model_type: fastembed::EmbeddingModel,
    cache_dir: Option<PathBuf>,
) -> Result<Self, AttaError> {
    let mut opts = fastembed::InitOptions::new(model_type)
        .with_show_download_progress(true);
    if let Some(dir) = cache_dir {
        opts = opts.with_cache_dir(dir);
    }
    let model = fastembed::TextEmbedding::try_new(opts)?;
}
```

### 9.2 `crates/server/src/home.rs`

添加两个新方法和更新 `ensure_dirs()`：

```rust
pub fn bin(&self) -> PathBuf {
    self.root.join("bin")
}

pub fn models(&self) -> PathBuf {
    self.root.join("models")
}

// ensure_dirs() 中添加 self.bin() 和 self.models()
```

### 9.3 `crates/server/src/services.rs`

传入 models 路径：

```rust
// 改前：
FastEmbedProvider::default_model()

// 改后：
let cache_dir = home.models().join("fastembed");
FastEmbedProvider::new(
    fastembed::EmbeddingModel::AllMiniLML6V2,
    Some(cache_dir),
)
```

### 9.4 `apps/shell/src-tauri/src/autostart.rs`

优先从 ATTA_HOME/bin/ 查找 attaos：

```rust
// 改前：找同目录或 PATH
// 改后：
fn find_attaos(home: &Path) -> PathBuf {
    let in_home = home.join("bin/attaos");
    if in_home.exists() {
        return in_home;
    }
    // fallback: 同目录 → PATH
}
```

### 9.5 `apps/shell/src-tauri/src/lib.rs`

启动流程改为先检测安装状态：

```rust
// setup() 中：
tauri::async_runtime::spawn(async move {
    let home = resolve_home();

    if home.join(".manifest.json").exists() {
        // 已安装：启动 attaos → 导航到 WebUI
        let port = ensure_server(&home, port).await?;
        navigate_main_to(&app_handle, &format!("http://localhost:{port}"));
    } else if home.join("etc/connections.json").exists() {
        // 连接模式：导航到远程 URL
        let url = load_default_connection(&home)?;
        navigate_main_to(&app_handle, &url);
    } else {
        // 未安装：显示 installer 页面
        navigate_main_to(&app_handle, "installer.html");
    }
});
```

### 9.6 `apps/shell/src-tauri/tauri.conf.json`

添加 `dialog` plugin 和 installer 入口点。

### 9.7 新增前端文件

| 文件 | 说明 |
|------|------|
| `apps/shell/installer.html` | Installer 入口 HTML |
| `apps/shell/src/installer/main.ts` | Vue app 入口 |
| `apps/shell/src/installer/App.vue` | 安装向导主组件 |
| `apps/shell/src/installer/steps/*.vue` | 各步骤组件 |

Vite 多入口配置中添加 `installer` 入口。

---

## 十、安全考虑

| 威胁 | 防护 |
|------|------|
| 组件包篡改 | SHA-256 校验（manifest 中记录） |
| attash 安装包篡改 | Ed25519 签名验证（Tauri updater） |
| 已安装组件被篡改 | 启动时 + 手动校验 SHA-256 |
| API Key 泄露 | AES-256-GCM 加密存储（SecretStore） |
| 降级攻击 | 版本单调递增检查 |
| 远程连接安全 | HTTPS（attash → 远程 attaos） |
| Enterprise 认证绕过 | 由 attaos 服务端 RBAC 控制，attash 不做权限过滤 |

---

## 十一、构建与发布流程（CI）

```
git tag v0.2.0
    │
    ▼
CI Pipeline
    │
    ├─── Desktop Pipeline ─────────────────────────────────
    │    │
    │    ├── 准备 ONNX 模型
    │    │   └── 下载/缓存 Qdrant/all-MiniLM-L6-v2-onnx → .fastembed_cache/
    │    │
    │    ├── Build attash (Tauri, per platform)
    │    │   ├── darwin-aarch64 → attash-v0.2.0-darwin-aarch64.dmg
    │    │   ├── darwin-x86_64  → attash-v0.2.0-darwin-x86_64.dmg
    │    │   ├── linux-x86_64   → attash-v0.2.0-linux-x86_64.AppImage
    │    │   └── windows-x86_64 → attash-v0.2.0-windows-x86_64.msi
    │    │
    │    ├── Build Desktop components (per platform)
    │    │   ├── cargo build --release -p atta-server --features desktop
    │    │   ├── cargo build --release -p atta-cli
    │    │   ├── strip binaries
    │    │   ├── npm run build (webui)
    │    │   ├── collect: bin/ + models/fastembed/ + webui/dist/ + lib/ + templates/
    │    │   ├── generate manifest.json (SHA-256 per component)
    │    │   └── tar czf atta-desktop-v0.2.0-{platform}.tar.gz
    │    │
    │    └── Generate desktop-manifest.json (version index)
    │
    ├─── Enterprise Pipeline ──────────────────────────────
    │    │
    │    ├── Build Enterprise components (Linux only)
    │    │   ├── cargo build --release -p atta-server --features enterprise
    │    │   ├── cargo build --release -p atta-cli
    │    │   ├── strip binaries
    │    │   ├── npm run build (webui)
    │    │   ├── collect: bin/ + webui/dist/ + lib/ + templates/ + install.sh
    │    │   ├── generate manifest.json (SHA-256 per component)
    │    │   └── tar czf atta-enterprise-v0.2.0-{platform}.tar.gz
    │    │
    │    └── Generate enterprise-manifest.json (version index)
    │
    └── Publish to GitHub Releases
        ├── attash-*.dmg/msi/AppImage
        ├── atta-desktop-*.tar.gz
        ├── atta-enterprise-*.tar.gz
        ├── desktop-manifest.json
        ├── enterprise-manifest.json
        └── latest.json (Tauri updater)
```

---

## 十二、实现路径

### Phase 1: 代码适配

- [ ] `home.rs` 添加 `bin()`, `models()` 方法，`ensure_dirs()` 包含新目录
- [ ] `fastembed.rs` 添加 `cache_dir` 参数
- [ ] `services.rs` 传入 `home.models().join("fastembed")`
- [ ] `autostart.rs` 优先查找 `ATTA_HOME/bin/attaos`
- [ ] `.gitignore` 添加 `.fastembed_cache/`

### Phase 2: Desktop 安装框架

- [ ] `lib.rs` 启动逻辑：检测 `.manifest.json` / `connections.json` → 决定显示内容
- [ ] 实现 Tauri commands（安装、校验、下载、解压）
- [ ] `tauri.conf.json` 添加 dialog plugin
- [ ] `Cargo.toml` 添加 `flate2`, `tar`, `tauri-plugin-dialog`
- [ ] Installer 前端（installer.html + Vue 组件）
- [ ] 连接服务器前端（复用 installer 页面的模式选择）
- [ ] Tray 菜单适配（已安装/已连接/未安装 三种状态）

### Phase 3: Desktop 升级

- [ ] 组件版本检查（desktop-manifest.json）
- [ ] 升级前备份 + 回滚
- [ ] Manager UI（updater 窗口扩展或独立页面）

### Phase 4: Enterprise 安装包

- [ ] CI 打包脚本：Enterprise 组件包
- [ ] install.sh 完整实现（环境检查 + 部署 + systemd）
- [ ] install.sh --upgrade 升级模式
- [ ] README.md 安装文档

### Phase 5: CI/CD

- [ ] GitHub Actions：ONNX 模型缓存 + 多平台构建
- [ ] Tauri 签名密钥（`tauri signer generate`）
- [ ] 自动生成 manifest.json + desktop-manifest.json
- [ ] 发布到 GitHub Releases
