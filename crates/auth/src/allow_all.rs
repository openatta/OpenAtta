//! AllowAll 授权实现
//!
//! Desktop 版默认使用的授权策略：无条件放行所有操作。

use atta_types::{Action, Actor, AttaError, AuthzDecision, Resource};

use crate::traits::Authz;

/// 全部放行的授权实现
///
/// 适用于 Desktop 单机单用户场景，所有操作一律返回 `Allow`。
pub struct AllowAll;

impl AllowAll {
    /// 创建新的 AllowAll 实例
    pub fn new() -> Self {
        Self
    }
}

impl Default for AllowAll {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Authz for AllowAll {
    async fn check(
        &self,
        actor: &Actor,
        action: Action,
        resource: &Resource,
    ) -> Result<AuthzDecision, AttaError> {
        tracing::debug!(
            actor_id = %actor.id,
            actor_type = actor.actor_type.as_str(),
            action = ?action,
            resource_type = resource.resource_type.as_str(),
            resource_id = ?resource.id,
            "AllowAll: granting access"
        );
        Ok(AuthzDecision::Allow)
    }

    async fn check_batch(
        &self,
        actor: &Actor,
        checks: &[(Action, Resource)],
    ) -> Result<Vec<AuthzDecision>, AttaError> {
        tracing::debug!(
            actor_id = %actor.id,
            count = checks.len(),
            "AllowAll: granting batch access"
        );
        Ok(vec![AuthzDecision::Allow; checks.len()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::{ActorType, ResourceType};

    #[tokio::test]
    async fn test_allow_all_check() {
        let authz = AllowAll::new();
        let actor = Actor {
            actor_type: ActorType::User,
            id: "test-user".to_string(),
        };
        let resource = Resource::all(ResourceType::Task);

        let result = authz.check(&actor, Action::Read, &resource).await.unwrap();
        assert!(matches!(result, AuthzDecision::Allow));
    }

    #[tokio::test]
    async fn test_allow_all_check_batch() {
        let authz = AllowAll::new();
        let actor = Actor::system();
        let checks = vec![
            (Action::Create, Resource::all(ResourceType::Task)),
            (Action::Execute, Resource::all(ResourceType::Tool)),
            (Action::Delete, Resource::all(ResourceType::Secret)),
        ];

        let results = authz.check_batch(&actor, &checks).await.unwrap();
        assert_eq!(results.len(), 3);
        for result in results {
            assert!(matches!(result, AuthzDecision::Allow));
        }
    }
}
