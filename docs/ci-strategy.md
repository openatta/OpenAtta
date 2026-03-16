# CI & 测试策略

## 测试分层

| 层级 | 位置 | 说明 | 外部依赖 |
|------|------|------|----------|
| 单元测试 | `src/**/*.rs` 内 `#[cfg(test)]` | 纯逻辑、类型转换、工具函数 | 无 |
| 集成测试 | `crates/*/tests/*.rs` | trait 实现、API 契约、组件交互 | SQLite (temp file) |
| wiremock 测试 | `crates/channel/tests/channel_wiremock.rs` | Channel HTTP API 契约 | wiremock (进程内) |
| E2E 烟雾测试 | `crates/core/tests/e2e_smoke.rs` | 全栈生命周期验证 | SQLite (temp file) |
| Enterprise 测试 | `crates/bus/tests/nats_tests.rs`, `crates/store/tests/postgres_tests.rs` | NatsBus + PostgresStore | NATS / PostgreSQL |

## 测试命令

```bash
# 日常开发（无外部依赖）
cargo test --workspace --exclude atta-shell --exclude atta-updater

# Channel wiremock 测试（需 telegram+slack+webhook features）
cargo test -p atta-channel --features "telegram,slack,webhook"

# Enterprise: NatsBus
ATTA_NATS_URL=nats://localhost:4222 cargo test -p atta-bus --features nats --test nats_tests

# Enterprise: PostgresStore
ATTA_POSTGRES_URL=postgres://user:pass@localhost/atta_test cargo test -p atta-store --features postgres --test postgres_tests

# 代码覆盖率
cargo llvm-cov --workspace --exclude atta-shell --exclude atta-updater --html

# Lint
cargo clippy --workspace --exclude atta-shell --exclude atta-updater --all-targets -- -D warnings

# 格式化检查
cargo fmt --all -- --check
```

## 覆盖率

使用 [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) 生成覆盖率报告。

### 安装

```bash
cargo install cargo-llvm-cov
```

### 运行

```bash
# HTML 报告（输出到 target/llvm-cov/html/）
cargo llvm-cov --workspace --exclude atta-shell --exclude atta-updater --html

# 终端文本摘要
cargo llvm-cov --workspace --exclude atta-shell --exclude atta-updater

# LCOV 格式（用于 CI 集成）
cargo llvm-cov --workspace --exclude atta-shell --exclude atta-updater --lcov --output-path lcov.info

# 包含 Channel wiremock 测试
cargo llvm-cov --workspace --exclude atta-shell --exclude atta-updater \
  --features "telegram,slack,webhook" --html
```

### 覆盖率目标

当前阶段不设硬性门槛，建立基线后逐步提升：

| 阶段 | Line | Branch | 说明 |
|------|------|--------|------|
| 当前 | 建立基线 | 建立基线 | 首次运行，记录基线数据 |
| 短期 | 60% | 45% | 核心 crate（types, bus, store, agent） |
| 中期 | 70% | 55% | 参考 OpenClaw 的 70/55 门槛 |

## Bug 模式分类（参考 ZeroClaw TG 模式）

测试文件使用 TG (Test Group) 前缀标识防御的 Bug 模式：

| TG | 文件 | 防御模式 |
|----|------|----------|
| TG-CH1 | `channel_wiremock.rs` | Telegram Bot API 契约 |
| TG-CH2 | `channel_wiremock.rs` | Slack Web API 契约 |
| TG-CH3 | `channel_wiremock.rs` | Webhook HTTP 契约 |
| TG-E2E1 | `e2e_smoke.rs` | 全任务生命周期 |
| TG-E2E2 | `e2e_smoke.rs` | Agent 状态暂停 |
| TG-E2E3 | `e2e_smoke.rs` | Flow 验证守卫 |
| TG-E2E4 | `e2e_smoke.rs` | 安全策略持久化 |
| TG-E2E5 | `e2e_smoke.rs` | 事件总线投递 |
| TG-E2E6 | `e2e_smoke.rs` | 配置持久化 |
| TG-E2E7 | `e2e_smoke.rs` | 并发任务创建 |

## GitHub Actions 建议

推荐的 CI pipeline 配置（`P0` 优先级，待实施）：

### 基础 Pipeline

```yaml
name: CI
on: [push, pull_request]
concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all -- --check
      - run: cargo clippy --workspace --exclude atta-shell --exclude atta-updater --all-targets -- -D warnings
      - run: cargo test --workspace --exclude atta-shell --exclude atta-updater

  channel-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test -p atta-channel --features "telegram,slack,webhook"

  coverage:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: taiki-e/install-action@cargo-llvm-cov
      - uses: Swatinem/rust-cache@v2
      - run: cargo llvm-cov --workspace --exclude atta-shell --exclude atta-updater --lcov --output-path lcov.info
      - uses: codecov/codecov-action@v4
        with:
          files: lcov.info
```

### Enterprise Pipeline（可选）

```yaml
  enterprise-tests:
    runs-on: ubuntu-latest
    services:
      postgres:
        image: postgres:16
        env:
          POSTGRES_PASSWORD: test
          POSTGRES_DB: atta_test
        ports: ['5432:5432']
        options: --health-cmd pg_isready --health-interval 10s --health-timeout 5s --health-retries 5
      nats:
        image: nats:latest
        ports: ['4222:4222']
        options: --entrypoint /nats-server -- -js
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: |
          ATTA_POSTGRES_URL=postgres://postgres:test@localhost/atta_test \
          cargo test -p atta-store --features postgres --test postgres_tests
      - run: |
          ATTA_NATS_URL=nats://localhost:4222 \
          cargo test -p atta-bus --features nats --test nats_tests
```
