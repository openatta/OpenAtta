# Sandbox Runtime 方案 —— 可行性简报

> 日期：2026-03-06
> 状态：提案

---

## 背景

Agent 在工作中会生成并执行代码（Python 脚本、Node.js 程序等），但终端用户的机器环境千差万别。需要提供两种方案让用户在安装时选择：

- **方案 A：直接安装** — 在宿主机上安装/复用 Python、Node.js 等运行时
- **方案 B：沙箱运行** — Windows 用 WSL2，macOS 用 Lima，Linux 用 Docker

安装时由用户选择方案，并检测是否已有可复用环境。

---

## 结论：可行，且与现有架构高度契合

### 为什么契合

1. **可新增 `SandboxRuntime` trait 作为这个接缝**。此方案提供 2 种执行模式：`LocalRuntime`（方案 A，直接在宿主机执行）、`SandboxRuntime`（方案 B，在隔离环境执行）。

2. **`shell` 工具是关键执行路径**。Agent 生成代码后通过 `shell` 工具执行。目前 `shell` 直接在宿主机执行命令。方案 B 只需要把 `shell` 的执行目标从宿主机重定向到沙箱，上层 Agent 完全无感。

3. **安全层天然受益**。`SecurityGuard` 已经包装每次工具调用。沙箱方案等于在 SecurityGuard 之外再加一层 OS 级隔离，纵深防御更完整。

---

## 需要解决的问题

| 问题 | 难度 | 说明 |
|------|------|------|
| **环境检测** | 低 | 检测 python/node/wsl/lima/docker 是否存在，`which` + 版本检查即可 |
| **沙箱生命周期管理** | 中 | Lima VM / WSL distro / Docker container 的启动、停止、健康检查 |
| **文件系统映射** | 中 | Agent 读写的工作目录需要在宿主机和沙箱之间双向同步（mount/bind） |
| **网络透传** | 低 | Lima/WSL2/Docker 都支持端口转发，Agent 的 web_fetch 等工具可正常工作 |
| **跨平台沙箱差异** | 高 | WSL2、Lima、Docker 三套 API 完全不同，需要统一抽象层 |
| **首次安装体验** | 中 | Lima 需要下载 VM 镜像（~500MB），WSL2 需要启用 Windows 功能，Docker 需要 daemon |
| **GPU 透传** | 高 | 如果未来 Agent 需要本地推理，GPU passthrough 在各平台差异极大 |

---

## 建议的实现路径

### 阶段 1：安装向导 + 环境检测

- 检测宿主机已有的运行时和沙箱工具
- 交互式选择方案 A 或 B
- 方案 A：检测/安装 Python、Node.js
- 方案 B：检测/安装 Lima (macOS)、WSL2 (Windows)、Docker (Linux)

### 阶段 2：SandboxRuntime 抽象

- 新建 `SandboxRuntime` trait，统一 Lima/WSL2/Docker 的：
  - 启动/停止
  - 命令执行（替代直接 shell）
  - 文件挂载
  - 状态检查

### 阶段 3：shell 工具适配

- shell 工具根据配置决定执行目标（宿主机 vs 沙箱）
- 对 Agent 透明，不改变工具接口

---

## 最大的风险

**跨平台沙箱统一抽象**是核心难点。WSL2 是 Windows 子系统（`wsl.exe -d`），Lima 是 QEMU VM（`limactl shell`），Docker 是容器（`docker exec`）。三者在文件系统语义、网络模型、进程模型上都有差异。抽象层的设计质量决定了整体方案的可维护性。

---

## 总结

思路完全可行。核心工作量在跨平台沙箱抽象层——建议先做 macOS (Lima) 一个平台验证路径，再扩展到 Windows/Linux。
