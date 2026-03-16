//! Authz trait 定义
//!
//! 授权检查的核心抽象。Desktop 版使用 AllowAll（全部放行），
//! Enterprise 版使用 RBACAuthz（基于角色的访问控制）。

use atta_types::{Action, Actor, AttaError, AuthzDecision, Resource};

/// 授权检查 trait
///
/// 所有授权实现必须实现此 trait。`check` 是核心方法，
/// `check_batch` 提供默认的逐条检查实现，实现方可覆盖以优化批量检查。
///
/// # Examples
///
/// ```rust,no_run
/// use atta_auth::Authz;
/// use atta_types::{Actor, Action, Resource, ResourceType, AuthzDecision};
///
/// # async fn example(authz: impl Authz) -> Result<(), atta_types::AttaError> {
/// let actor = Actor::user("alice");
/// let resource = Resource::new(ResourceType::Task, "task-1");
/// let decision = authz.check(&actor, Action::Read, &resource).await?;
/// assert!(matches!(decision, AuthzDecision::Allow));
/// # Ok(())
/// # }
/// ```
#[async_trait::async_trait]
pub trait Authz: Send + Sync + 'static {
    /// 检查单个操作是否被允许
    ///
    /// # Arguments
    /// * `actor` - 执行操作的主体
    /// * `action` - 要执行的动作
    /// * `resource` - 目标资源
    ///
    /// # Returns
    /// * `AuthzDecision::Allow` - 允许
    /// * `AuthzDecision::Deny` - 拒绝（附带原因）
    /// * `AuthzDecision::RequireApproval` - 需要审批
    async fn check(
        &self,
        actor: &Actor,
        action: Action,
        resource: &Resource,
    ) -> Result<AuthzDecision, AttaError>;

    /// 批量检查多个操作
    ///
    /// 默认实现逐条调用 `check`，实现方可覆盖以提升性能。
    async fn check_batch(
        &self,
        actor: &Actor,
        checks: &[(Action, Resource)],
    ) -> Result<Vec<AuthzDecision>, AttaError> {
        let mut results = Vec::with_capacity(checks.len());
        for (action, resource) in checks {
            let decision = self.check(actor, action.clone(), resource).await?;
            results.push(decision);
        }
        Ok(results)
    }
}
