//! RBAC Authorization implementation
//!
//! Role-based access control for Enterprise edition.
//! 6 roles: Owner, Admin, Operator, Developer, Approver, Viewer.

use std::sync::Arc;

use atta_store::StateStore;
use atta_types::{Action, Actor, AttaError, AuthzDecision, Resource, ResourceType, Role};
use tracing::{debug, warn};

use crate::Authz;

/// RBAC authorization implementation
///
/// Loads role bindings from StateStore for each authorization check.
pub struct RBACAuthz {
    store: Arc<dyn StateStore>,
}

impl RBACAuthz {
    /// Create a new RBAC authorizer with a StateStore for role lookups
    pub fn new(store: Arc<dyn StateStore>) -> Self {
        Self { store }
    }

    /// Check if a role has permission for an action on a resource type
    fn role_has_permission(role: &Role, action: &Action, resource_type: &ResourceType) -> bool {
        match role {
            Role::Owner | Role::Admin => true, // Full access
            Role::Operator => matches!(
                (action, resource_type),
                (Action::Read, _)
                    | (Action::Create, ResourceType::Task)
                    | (Action::Update, ResourceType::Task)
                    | (Action::Delete, ResourceType::Task)
                    | (Action::Execute, ResourceType::Task)
                    | (Action::Create, ResourceType::Flow)
            ),
            Role::Developer => matches!(
                (action, resource_type),
                (Action::Read, _)
                    | (Action::Create, ResourceType::Task)
                    | (Action::Create, ResourceType::Flow)
                    | (Action::Update, ResourceType::Flow)
                    | (Action::Delete, ResourceType::Flow)
                    | (Action::Create, ResourceType::Skill)
                    | (Action::Update, ResourceType::Skill)
                    | (Action::Register, ResourceType::Mcp)
            ),
            Role::Approver => matches!(
                (action, resource_type),
                (Action::Read, _) | (Action::Update, ResourceType::Approval)
            ),
            Role::Viewer => matches!(action, Action::Read),
        }
    }
}

#[async_trait::async_trait]
impl Authz for RBACAuthz {
    async fn check(
        &self,
        actor: &Actor,
        action: Action,
        resource: &Resource,
    ) -> Result<AuthzDecision, AttaError> {
        debug!(
            actor = %actor.id,
            action = ?action,
            resource_type = ?resource.resource_type,
            "RBAC check"
        );

        // System actor always allowed
        if actor.id == "system" {
            return Ok(AuthzDecision::Allow);
        }

        // Load role bindings from StateStore
        let roles = match self.store.get_roles_for_actor(&actor.id).await {
            Ok(roles) => {
                if roles.is_empty() {
                    debug!(actor = %actor.id, "no roles found, denying access");
                    return Ok(AuthzDecision::Deny {
                        reason: format!("actor '{}' has no assigned roles", actor.id),
                    });
                }
                roles
            }
            Err(e) => {
                warn!(actor = %actor.id, error = %e, "failed to load roles, denying access");
                return Ok(AuthzDecision::Deny {
                    reason: format!("failed to load roles for actor '{}': {}", actor.id, e),
                });
            }
        };

        for role in &roles {
            if Self::role_has_permission(role, &action, &resource.resource_type) {
                return Ok(AuthzDecision::Allow);
            }
        }

        Ok(AuthzDecision::Deny {
            reason: format!(
                "actor '{}' does not have permission to {:?} on {:?}",
                actor.id, action, resource.resource_type
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_owner_has_full_access() {
        assert!(RBACAuthz::role_has_permission(
            &Role::Owner,
            &Action::Delete,
            &ResourceType::Task
        ));
    }

    #[test]
    fn test_viewer_can_only_read() {
        assert!(RBACAuthz::role_has_permission(
            &Role::Viewer,
            &Action::Read,
            &ResourceType::Task
        ));
        assert!(!RBACAuthz::role_has_permission(
            &Role::Viewer,
            &Action::Create,
            &ResourceType::Task
        ));
    }

    #[test]
    fn test_approver_can_update_approval() {
        assert!(RBACAuthz::role_has_permission(
            &Role::Approver,
            &Action::Update,
            &ResourceType::Approval
        ));
        assert!(!RBACAuthz::role_has_permission(
            &Role::Approver,
            &Action::Create,
            &ResourceType::Task
        ));
    }
}
