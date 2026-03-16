# 用户与权限管理实施计划

## 一、可行性结论：完全可行

现有代码已具备 80% 的基础设施，核心工作是「接线」而非「重建」。

## 二、现状盘点

| 模块 | 状态 | 说明 |
|------|------|------|
| RBAC 引擎 | ✅ 完整 | 6 角色体系（Owner/Admin/Operator/Developer/Approver/Viewer），`crates/auth/src/rbac.rs` |
| CurrentUser 中间件 | ⚠️ 提取但丢弃 | `crates/core/src/middleware.rs` 解析了用户，但所有 handler 忽略它 |
| AuthMode 枚举 | ⚠️ 硬编码 NoAuth | `crates/server/src/services.rs` 写死，无配置文件支持 |
| Store 过滤器 | ⚠️ 有能力未使用 | `TaskFilter.created_by`、`AuditFilter.actor_id` 存在但 handler 传 None |
| authz.check() | ❌ 从未调用 | handler 中零处调用 |
| 用户表 | ❌ 不存在 | 只有 `service_accounts` 和 `role_bindings` |
| WebSocket/SSE 认证 | ❌ 无 | 裸连接，无身份校验 |
| SecretStore | ⚠️ 无用户隔离 | 全局 KV，无 `owner` 字段 |

## 三、双模式设计

```
┌─────────────────────────────────────────────┐
│              AuthMode Config                │
├──────────────────┬──────────────────────────┤
│   Desktop        │   Enterprise             │
│   auth_mode:     │   auth_mode: oidc        │
│     local        │   oidc_issuer: ...       │
│                  │   oidc_client_id: ...    │
├──────────────────┼──────────────────────────┤
│ 单用户 = admin   │ 多用户，OIDC 登录        │
│ 启动即登录       │ 按角色分权               │
│ 看到所有数据     │ 普通用户只看自己的数据    │
│                  │ Admin 看所有人            │
└──────────────────┴──────────────────────────┘
```

## 四、实施步骤（6 步）

### Phase 1：用户与权限

| 步骤 | 内容 | 改动范围 | 工作量 |
|------|------|----------|--------|
| ① 用户表 + 本地认证 | 新建 `users` 表，支持密码哈希（argon2）；Desktop 自动创建 admin 用户并 auto-login | `crates/store` migration, `crates/auth` | 中 |
| ② 配置文件支持 AuthMode | `config.toml` 增加 `[auth]` 段，支持 `local` / `oidc` / `api_key` | `crates/server/src/services.rs` | 小 |
| ③ Handler 接入 CurrentUser | 所有 handler 从 Extension 取 CurrentUser，替换 `Actor::user("anonymous")` | `crates/core/src/server/handlers/*.rs` | 中 |
| ④ Handler 接入 authz.check() | 在每个 handler 入口调用 `authz.check(actor, resource, action)` | 同上 | 中 |
| ⑤ 数据隔离 | Handler 根据角色决定是否传 `created_by` 过滤：Admin→None（看全部），普通用户→Some(self) | 同上 + Store 层 | 小 |
| ⑥ WebUI 权限感知 | 登录页、菜单按角色显示/隐藏、管理页面过滤 | `webui/` | 中 |

### Phase 2：LLM API 网关（后做）

- 基于用户系统，为每用户生成 API Key
- 代理转发 LLM 请求，替换真实 provider key
- 用量追踪与配额

## 五、关键设计决策

1. **Desktop 零配置体验**：首次启动自动创建 admin 用户，无需登录页面（或一键登录）
2. **Enterprise 标准 OIDC**：接入任意 IdP（Keycloak/Auth0/Azure AD），token 中携带角色 claim
3. **统一 Actor 模型**：`CurrentUser → Actor`，所有下游（Store/Bus/Audit）已接受 Actor，无需改动
4. **渐进式实施**：每步完成后系统可正常运行，不存在「全做完才能用」的风险

## 六、风险与注意事项

- **WebSocket 认证**需在握手阶段校验 token，不能事后补
- **SecretStore** 如需用户隔离，需加 `owner` 列（migration）
- **SSE 流**需在连接建立时绑定用户身份，中途断开需重新认证
