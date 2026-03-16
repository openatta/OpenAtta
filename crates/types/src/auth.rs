//! 授权相关类型

use serde::{Deserialize, Serialize};

/// 操作者类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorType {
    User,
    Agent,
    Service,
    System,
}

impl ActorType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
            Self::Service => "service",
            Self::System => "system",
        }
    }
}

/// 操作者
///
/// 表示执行操作的主体，可以是用户、Agent、服务或系统自身。
///
/// # Examples
///
/// ```
/// use atta_types::Actor;
///
/// let sys = Actor::system();
/// assert_eq!(sys.id, "system");
///
/// let alice = Actor::user("alice");
/// assert_eq!(alice.id, "alice");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Actor {
    pub actor_type: ActorType,
    pub id: String,
}

impl Actor {
    pub fn system() -> Self {
        Self {
            actor_type: ActorType::System,
            id: "system".to_string(),
        }
    }

    pub fn user(id: impl Into<String>) -> Self {
        Self {
            actor_type: ActorType::User,
            id: id.into(),
        }
    }
}

/// 动作
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Create,
    Read,
    Update,
    Delete,
    Execute,
    Approve,
    Register,
    Manage,
}

/// 资源类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    Task,
    Flow,
    Skill,
    Tool,
    Node,
    Secret,
    AuditLog,
    Approval,
    Package,
    Mcp,
    RemoteAgent,
    Channel,
}

impl ResourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Flow => "flow",
            Self::Skill => "skill",
            Self::Tool => "tool",
            Self::Node => "node",
            Self::Secret => "secret",
            Self::AuditLog => "audit_log",
            Self::Approval => "approval",
            Self::Package => "package",
            Self::Mcp => "mcp",
            Self::RemoteAgent => "remote_agent",
            Self::Channel => "channel",
        }
    }
}

/// 资源
///
/// 表示授权检查的目标资源。可以指定具体实体 ID，
/// 或使用 `all()` 表示该类型的所有资源。
///
/// # Examples
///
/// ```
/// use atta_types::{Resource, ResourceType};
///
/// // 具体资源
/// let task = Resource::new(ResourceType::Task, "task-123");
/// assert_eq!(task.id.as_deref(), Some("task-123"));
///
/// // 所有 Flow 资源
/// let all_flows = Resource::all(ResourceType::Flow);
/// assert!(all_flows.id.is_none());
///
/// // Tool 快捷方式
/// let tool = Resource::tool("web_search");
/// assert_eq!(tool.resource_type, ResourceType::Tool);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub resource_type: ResourceType,
    pub id: Option<String>,
}

impl Resource {
    pub fn new(resource_type: ResourceType, id: impl Into<String>) -> Self {
        Self {
            resource_type,
            id: Some(id.into()),
        }
    }

    pub fn all(resource_type: ResourceType) -> Self {
        Self {
            resource_type,
            id: None,
        }
    }

    pub fn tool(name: &str) -> Self {
        Self::new(ResourceType::Tool, name)
    }
}

/// 授权决策结果
///
/// # Examples
///
/// ```
/// use atta_types::AuthzDecision;
///
/// let allow = AuthzDecision::Allow;
/// assert!(matches!(allow, AuthzDecision::Allow));
///
/// let deny = AuthzDecision::Deny { reason: "forbidden".into() };
/// assert!(matches!(deny, AuthzDecision::Deny { .. }));
/// ```
#[derive(Debug, Clone)]
pub enum AuthzDecision {
    Allow,
    Deny { reason: String },
    RequireApproval { approver_role: String },
}

/// 角色定义
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Admin,
    Operator,
    Developer,
    Approver,
    Viewer,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Actor::system ──

    #[test]
    fn actor_system_has_correct_type_and_id() {
        let actor = Actor::system();
        assert_eq!(actor.actor_type, ActorType::System);
        assert_eq!(actor.id, "system");
    }

    // ── Actor::user ──

    #[test]
    fn actor_user_has_correct_type_and_id() {
        let actor = Actor::user("alice");
        assert_eq!(actor.actor_type, ActorType::User);
        assert_eq!(actor.id, "alice");
    }

    #[test]
    fn actor_user_accepts_string() {
        let actor = Actor::user(String::from("bob"));
        assert_eq!(actor.id, "bob");
    }

    // ── Actor serde round-trip ──

    #[test]
    fn actor_serde_round_trip() {
        let actor = Actor::user("charlie");
        let json = serde_json::to_string(&actor).unwrap();
        let back: Actor = serde_json::from_str(&json).unwrap();
        assert_eq!(back.actor_type, ActorType::User);
        assert_eq!(back.id, "charlie");
    }

    #[test]
    fn actor_system_serde_round_trip() {
        let actor = Actor::system();
        let json = serde_json::to_string(&actor).unwrap();
        let back: Actor = serde_json::from_str(&json).unwrap();
        assert_eq!(back.actor_type, ActorType::System);
        assert_eq!(back.id, "system");
    }

    // ── ActorType::as_str ──

    #[test]
    fn actor_type_as_str() {
        assert_eq!(ActorType::User.as_str(), "user");
        assert_eq!(ActorType::Agent.as_str(), "agent");
        assert_eq!(ActorType::Service.as_str(), "service");
        assert_eq!(ActorType::System.as_str(), "system");
    }

    // ── ActorType serde ──

    #[test]
    fn actor_type_serde_uses_snake_case() {
        let cases = vec![
            (ActorType::User, r#""user""#),
            (ActorType::Agent, r#""agent""#),
            (ActorType::Service, r#""service""#),
            (ActorType::System, r#""system""#),
        ];
        for (variant, expected) in cases {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected);
            let back: ActorType = serde_json::from_str(&json).unwrap();
            assert_eq!(back, variant);
        }
    }

    // ── Role serde ──

    #[test]
    fn role_serde_round_trip() {
        let roles = vec![
            (Role::Owner, r#""owner""#),
            (Role::Admin, r#""admin""#),
            (Role::Operator, r#""operator""#),
            (Role::Developer, r#""developer""#),
            (Role::Approver, r#""approver""#),
            (Role::Viewer, r#""viewer""#),
        ];
        for (role, expected_json) in roles {
            let json = serde_json::to_string(&role).unwrap();
            assert_eq!(json, expected_json, "serialization of {:?}", role);
            let back: Role = serde_json::from_str(&json).unwrap();
            assert_eq!(back, role);
        }
    }

    #[test]
    fn role_deserialize_rejects_unknown() {
        let result = serde_json::from_str::<Role>(r#""superadmin""#);
        assert!(result.is_err());
    }

    // ── Action serde ──

    #[test]
    fn action_serde_round_trip() {
        let actions = vec![
            (Action::Create, r#""create""#),
            (Action::Read, r#""read""#),
            (Action::Update, r#""update""#),
            (Action::Delete, r#""delete""#),
            (Action::Execute, r#""execute""#),
            (Action::Approve, r#""approve""#),
            (Action::Register, r#""register""#),
            (Action::Manage, r#""manage""#),
        ];
        for (action, expected_json) in actions {
            let json = serde_json::to_string(&action).unwrap();
            assert_eq!(json, expected_json, "serialization of {:?}", action);
            let back: Action = serde_json::from_str(&json).unwrap();
            assert_eq!(back, action);
        }
    }

    // ── ResourceType serde and as_str ──

    #[test]
    fn resource_type_as_str() {
        assert_eq!(ResourceType::Task.as_str(), "task");
        assert_eq!(ResourceType::Flow.as_str(), "flow");
        assert_eq!(ResourceType::Skill.as_str(), "skill");
        assert_eq!(ResourceType::Tool.as_str(), "tool");
        assert_eq!(ResourceType::Node.as_str(), "node");
        assert_eq!(ResourceType::Secret.as_str(), "secret");
        assert_eq!(ResourceType::AuditLog.as_str(), "audit_log");
        assert_eq!(ResourceType::Approval.as_str(), "approval");
        assert_eq!(ResourceType::Package.as_str(), "package");
        assert_eq!(ResourceType::Mcp.as_str(), "mcp");
        assert_eq!(ResourceType::RemoteAgent.as_str(), "remote_agent");
    }

    #[test]
    fn resource_type_serde_round_trip() {
        let types = vec![
            ResourceType::Task,
            ResourceType::Flow,
            ResourceType::Skill,
            ResourceType::Tool,
            ResourceType::Node,
            ResourceType::Secret,
            ResourceType::AuditLog,
            ResourceType::Approval,
            ResourceType::Package,
            ResourceType::Mcp,
            ResourceType::RemoteAgent,
        ];
        for rt in types {
            let json = serde_json::to_string(&rt).unwrap();
            let back: ResourceType = serde_json::from_str(&json).unwrap();
            assert_eq!(back, rt);
        }
    }

    // ── Resource constructors ──

    #[test]
    fn resource_new_sets_fields() {
        let r = Resource::new(ResourceType::Task, "task-1");
        assert_eq!(r.resource_type, ResourceType::Task);
        assert_eq!(r.id.as_deref(), Some("task-1"));
    }

    #[test]
    fn resource_all_has_no_id() {
        let r = Resource::all(ResourceType::Flow);
        assert_eq!(r.resource_type, ResourceType::Flow);
        assert!(r.id.is_none());
    }

    #[test]
    fn resource_tool_shortcut() {
        let r = Resource::tool("web_search");
        assert_eq!(r.resource_type, ResourceType::Tool);
        assert_eq!(r.id.as_deref(), Some("web_search"));
    }

    #[test]
    fn resource_serde_round_trip() {
        let r = Resource::new(ResourceType::Tool, "calc");
        let json = serde_json::to_string(&r).unwrap();
        let back: Resource = serde_json::from_str(&json).unwrap();
        assert_eq!(back.resource_type, ResourceType::Tool);
        assert_eq!(back.id.as_deref(), Some("calc"));
    }

    // ── AuthzDecision ──

    #[test]
    fn authz_decision_variants() {
        let allow = AuthzDecision::Allow;
        assert!(matches!(allow, AuthzDecision::Allow));

        let deny = AuthzDecision::Deny {
            reason: "forbidden".to_string(),
        };
        match deny {
            AuthzDecision::Deny { reason } => assert_eq!(reason, "forbidden"),
            _ => panic!("expected Deny"),
        }

        let require = AuthzDecision::RequireApproval {
            approver_role: "admin".to_string(),
        };
        match require {
            AuthzDecision::RequireApproval { approver_role } => {
                assert_eq!(approver_role, "admin");
            }
            _ => panic!("expected RequireApproval"),
        }
    }
}
