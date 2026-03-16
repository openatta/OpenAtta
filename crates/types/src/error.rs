//! AttaOS 统一错误类型

use thiserror::Error;

/// AttaOS 统一错误类型
///
/// 各 crate 定义自己的领域错误（BusError, StoreError 等），
/// 通过 `#[from]` 自动转换为 AttaError。trait 方法签名统一返回 AttaError，
/// 调用方无需关心底层错误来源。
///
/// # Examples
///
/// ```
/// use atta_types::AttaError;
/// use atta_types::error::StoreError;
///
/// // 子错误自动转换为 AttaError
/// let store_err = StoreError::NotFound {
///     entity_type: "Task".into(),
///     id: "abc".into(),
/// };
/// let atta: AttaError = store_err.into();
/// assert!(atta.to_string().contains("Task"));
///
/// // 业务层变体直接构造
/// let err = AttaError::FlowNotFound("deploy".into());
/// assert_eq!(err.to_string(), "flow not found: deploy");
/// ```
#[derive(Debug, Error)]
pub enum AttaError {
    // ── 基础设施层 ──
    #[error(transparent)]
    Bus(#[from] BusError),

    #[error(transparent)]
    Store(#[from] StoreError),

    #[error(transparent)]
    Auth(#[from] AuthzError),

    #[error(transparent)]
    Audit(#[from] AuditError),

    #[error(transparent)]
    Runtime(#[from] RuntimeError),

    // ── 执行层 ──
    #[error(transparent)]
    Agent(#[from] AgentError),

    #[error(transparent)]
    Llm(#[from] LlmError),

    // ── 业务层 ──
    #[error("flow not found: {0}")]
    FlowNotFound(String),

    #[error("skill not found: {0}")]
    SkillNotFound(String),

    #[error("tool not found: {0}")]
    ToolNotFound(String),

    #[error("permission denied: {permission}")]
    PermissionDenied { permission: String },

    #[error("sandbox violation: {path}")]
    SandboxViolation { path: String },

    #[error("integrity check failed")]
    IntegrityCheckFailed,

    #[error("untrusted publisher")]
    UntrustedPublisher,

    #[error("validation error: {0}")]
    Validation(String),

    #[error("not found: {entity_type} {id}")]
    NotFound { entity_type: String, id: String },

    #[error("already exists: {entity_type} {id}")]
    AlreadyExists { entity_type: String, id: String },

    // ── MCP ──
    #[error("mcp server not found: {0}")]
    McpServerNotFound(String),

    #[error("mcp server not connected: {0}")]
    McpServerNotConnected(String),

    // ── Secret ──
    #[error("secret not found: {0}")]
    SecretNotFound(String),

    // ── Auth ──
    #[error("no id token in OIDC response")]
    NoIdToken,

    // ── Multi-Agent ──
    #[error("no agent defined for state: {0}")]
    NoAgentDefined(String),

    #[error("channel error: {0}")]
    Channel(String),

    #[error("channel capacity exhausted: {0}")]
    ChannelCapacityExhausted(String),

    #[error("parallel execution partial failure: {succeeded}/{total} succeeded")]
    ParallelExecutionPartialFailure { succeeded: usize, total: usize },

    #[error("no available nodes")]
    NoAvailableNodes,

    #[error("no suitable nodes for labels: {0:?}")]
    NoSuitableNodes(Vec<String>),

    // ── Package ──
    #[error("dependency missing: {name} {version}")]
    DependencyMissing { name: String, version: String },

    // ── Security ──
    #[error("rate limited: {0}")]
    RateLimited(String),

    #[error("security violation: {0}")]
    SecurityViolation(String),

    #[error("emergency stopped: {0}")]
    EmergencyStopped(String),

    #[error("approval denied for tool: {tool}")]
    ApprovalDenied { tool: String },

    #[error("approval timeout for tool '{tool}' after {timeout_secs}s")]
    ApprovalTimeout { tool: String, timeout_secs: u64 },

    // ── Skill ──
    #[error("missing variable: {0}")]
    MissingVariable(String),

    // ── Concurrency ──
    #[error("conflict: {entity_type} {id} version mismatch (expected {expected}, actual {actual})")]
    Conflict {
        entity_type: String,
        id: String,
        expected: u64,
        actual: u64,
    },

    // ── 兜底 ──
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// 便捷转换：serde_json::Error → AttaError
impl From<serde_json::Error> for AttaError {
    fn from(e: serde_json::Error) -> Self {
        AttaError::Other(e.into())
    }
}

/// 便捷转换：std::io::Error → AttaError
impl From<std::io::Error> for AttaError {
    fn from(e: std::io::Error) -> Self {
        AttaError::Other(e.into())
    }
}

// ── 子错误定义 ──

/// 事件总线错误
#[derive(Debug, Error)]
pub enum BusError {
    #[error("failed to publish event to topic '{topic}': {source}")]
    PublishFailed {
        topic: String,
        source: anyhow::Error,
    },

    #[error("failed to subscribe to topic '{topic}': {source}")]
    SubscribeFailed {
        topic: String,
        source: anyhow::Error,
    },

    #[error("connection lost: {0}")]
    ConnectionLost(String),
}

/// 状态存储错误
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("entity not found: {entity_type} {id}")]
    NotFound { entity_type: String, id: String },

    #[error("duplicate entity: {entity_type} {id}")]
    Duplicate { entity_type: String, id: String },

    #[error("database error: {0}")]
    Database(String),

    #[error("serialization error: {0}")]
    Serialization(String),
}

/// 授权错误
#[derive(Debug, Error)]
pub enum AuthzError {
    #[error("permission denied: {actor} cannot {action} on {resource}")]
    PermissionDenied {
        actor: String,
        action: String,
        resource: String,
    },

    #[error("actor not found: {0}")]
    ActorNotFound(String),

    #[error("policy evaluation error: {0}")]
    PolicyError(String),
}

/// 审计错误
#[derive(Debug, Error)]
pub enum AuditError {
    #[error("failed to record audit entry: {0}")]
    RecordFailed(anyhow::Error),

    #[error("audit query failed: {0}")]
    QueryFailed(anyhow::Error),
}

/// 运行时错误
#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("execution timeout after {0:?}")]
    Timeout(std::time::Duration),

    #[error("resource limit exceeded: {0}")]
    ResourceExceeded(String),

    #[error("node capacity exhausted")]
    CapacityExhausted,
}

/// Agent 执行错误
#[derive(Debug, Error)]
pub enum AgentError {
    #[error("max iterations reached: {0}")]
    MaxIterations(u32),

    #[error("tool call failed: {tool}: {error}")]
    ToolCallFailed { tool: String, error: String },

    #[error("llm error: {0}")]
    LlmFailed(String),

    #[error("agent timeout after {0:?}")]
    Timeout(std::time::Duration),
}

/// LLM 交互错误
#[derive(Debug, Error)]
pub enum LlmError {
    #[error("request failed: {0}")]
    RequestFailed(String),

    #[error("rate limited: retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },

    #[error("context window exceeded: used {used} of {limit} tokens")]
    ContextWindowExceeded { used: usize, limit: usize },

    #[error("invalid response: {0}")]
    InvalidResponse(String),

    #[error("auth error: {0}")]
    AuthError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // ── BusError → AttaError ──

    #[test]
    fn bus_error_publish_failed_converts_to_atta_error() {
        let bus_err = BusError::PublishFailed {
            topic: "events".to_string(),
            source: anyhow::anyhow!("timeout"),
        };
        let atta: AttaError = bus_err.into();
        assert!(matches!(
            atta,
            AttaError::Bus(BusError::PublishFailed { .. })
        ));
        let msg = atta.to_string();
        assert!(msg.contains("events"), "expected topic in message: {msg}");
    }

    #[test]
    fn bus_error_subscribe_failed_converts_to_atta_error() {
        let bus_err = BusError::SubscribeFailed {
            topic: "tasks".to_string(),
            source: anyhow::anyhow!("refused"),
        };
        let atta: AttaError = bus_err.into();
        assert!(matches!(
            atta,
            AttaError::Bus(BusError::SubscribeFailed { .. })
        ));
    }

    #[test]
    fn bus_error_connection_lost_converts_to_atta_error() {
        let bus_err = BusError::ConnectionLost("nats down".to_string());
        let atta: AttaError = bus_err.into();
        let msg = atta.to_string();
        assert!(msg.contains("nats down"));
    }

    // ── StoreError → AttaError ──

    #[test]
    fn store_error_not_found_converts_to_atta_error() {
        let store_err = StoreError::NotFound {
            entity_type: "Task".to_string(),
            id: "abc-123".to_string(),
        };
        let atta: AttaError = store_err.into();
        assert!(matches!(
            atta,
            AttaError::Store(StoreError::NotFound { .. })
        ));
        let msg = atta.to_string();
        assert!(msg.contains("Task"));
        assert!(msg.contains("abc-123"));
    }

    #[test]
    fn store_error_duplicate_converts_to_atta_error() {
        let store_err = StoreError::Duplicate {
            entity_type: "Flow".to_string(),
            id: "flow-1".to_string(),
        };
        let atta: AttaError = store_err.into();
        assert!(matches!(
            atta,
            AttaError::Store(StoreError::Duplicate { .. })
        ));
    }

    #[test]
    fn store_error_database_display() {
        let store_err = StoreError::Database("connection pool exhausted".to_string());
        let msg = store_err.to_string();
        assert!(msg.contains("connection pool exhausted"));
    }

    #[test]
    fn store_error_serialization_display() {
        let store_err = StoreError::Serialization("invalid utf-8".to_string());
        let msg = store_err.to_string();
        assert!(msg.contains("invalid utf-8"));
    }

    // ── AuthzError → AttaError ──

    #[test]
    fn authz_error_converts_to_atta_error() {
        let authz_err = AuthzError::PermissionDenied {
            actor: "user:bob".to_string(),
            action: "delete".to_string(),
            resource: "task:123".to_string(),
        };
        let atta: AttaError = authz_err.into();
        assert!(matches!(
            atta,
            AttaError::Auth(AuthzError::PermissionDenied { .. })
        ));
        let msg = atta.to_string();
        assert!(msg.contains("user:bob"));
        assert!(msg.contains("delete"));
        assert!(msg.contains("task:123"));
    }

    #[test]
    fn authz_error_actor_not_found_display() {
        let err = AuthzError::ActorNotFound("ghost".to_string());
        assert!(err.to_string().contains("ghost"));
    }

    #[test]
    fn authz_error_policy_error_display() {
        let err = AuthzError::PolicyError("invalid rule".to_string());
        assert!(err.to_string().contains("invalid rule"));
    }

    // ── AuditError → AttaError ──

    #[test]
    fn audit_error_converts_to_atta_error() {
        let audit_err = AuditError::RecordFailed(anyhow::anyhow!("disk full"));
        let atta: AttaError = audit_err.into();
        assert!(matches!(
            atta,
            AttaError::Audit(AuditError::RecordFailed(_))
        ));
        let msg = atta.to_string();
        assert!(msg.contains("disk full"));
    }

    #[test]
    fn audit_error_query_failed_display() {
        let err = AuditError::QueryFailed(anyhow::anyhow!("table missing"));
        assert!(err.to_string().contains("table missing"));
    }

    // ── RuntimeError → AttaError ──

    #[test]
    fn runtime_error_converts_to_atta_error() {
        let rt_err = RuntimeError::Timeout(Duration::from_secs(60));
        let atta: AttaError = rt_err.into();
        assert!(matches!(atta, AttaError::Runtime(RuntimeError::Timeout(_))));
        let msg = atta.to_string();
        assert!(msg.contains("60"));
    }

    #[test]
    fn runtime_error_resource_exceeded_display() {
        let err = RuntimeError::ResourceExceeded("memory".to_string());
        assert!(err.to_string().contains("memory"));
    }

    #[test]
    fn runtime_error_capacity_exhausted_display() {
        let err = RuntimeError::CapacityExhausted;
        assert!(err.to_string().contains("capacity exhausted"));
    }

    // ── AgentError → AttaError ──

    #[test]
    fn agent_error_converts_to_atta_error() {
        let agent_err = AgentError::MaxIterations(10);
        let atta: AttaError = agent_err.into();
        assert!(matches!(
            atta,
            AttaError::Agent(AgentError::MaxIterations(10))
        ));
        let msg = atta.to_string();
        assert!(msg.contains("10"));
    }

    #[test]
    fn agent_error_tool_call_failed_display() {
        let err = AgentError::ToolCallFailed {
            tool: "web_search".to_string(),
            error: "network error".to_string(),
        };
        assert!(err.to_string().contains("web_search"));
        assert!(err.to_string().contains("network error"));
    }

    #[test]
    fn agent_error_timeout_display() {
        let err = AgentError::Timeout(Duration::from_secs(30));
        assert!(err.to_string().contains("30"));
    }

    // ── LlmError → AttaError ──

    #[test]
    fn llm_error_converts_to_atta_error() {
        let llm_err = LlmError::RequestFailed("500 internal".to_string());
        let atta: AttaError = llm_err.into();
        assert!(matches!(atta, AttaError::Llm(LlmError::RequestFailed(_))));
    }

    #[test]
    fn llm_error_rate_limited_display() {
        let err = LlmError::RateLimited {
            retry_after_secs: 30,
        };
        let msg = err.to_string();
        assert!(msg.contains("30"));
    }

    #[test]
    fn llm_error_context_window_exceeded_display() {
        let err = LlmError::ContextWindowExceeded {
            used: 200_000,
            limit: 128_000,
        };
        let msg = err.to_string();
        assert!(msg.contains("200000"));
        assert!(msg.contains("128000"));
    }

    // ── AttaError 业务层变体 display ──

    #[test]
    fn atta_error_flow_not_found_display() {
        let err = AttaError::FlowNotFound("deploy-flow".to_string());
        assert_eq!(err.to_string(), "flow not found: deploy-flow");
    }

    #[test]
    fn atta_error_skill_not_found_display() {
        let err = AttaError::SkillNotFound("summarize".to_string());
        assert_eq!(err.to_string(), "skill not found: summarize");
    }

    #[test]
    fn atta_error_tool_not_found_display() {
        let err = AttaError::ToolNotFound("magic_wand".to_string());
        assert_eq!(err.to_string(), "tool not found: magic_wand");
    }

    #[test]
    fn atta_error_permission_denied_display() {
        let err = AttaError::PermissionDenied {
            permission: "admin.delete".to_string(),
        };
        assert_eq!(err.to_string(), "permission denied: admin.delete");
    }

    #[test]
    fn atta_error_sandbox_violation_display() {
        let err = AttaError::SandboxViolation {
            path: "/root/secrets".to_string(),
        };
        assert_eq!(err.to_string(), "sandbox violation: /root/secrets");
    }

    #[test]
    fn atta_error_not_found_display() {
        let err = AttaError::NotFound {
            entity_type: "Agent".to_string(),
            id: "agent-1".to_string(),
        };
        assert_eq!(err.to_string(), "not found: Agent agent-1");
    }

    #[test]
    fn atta_error_already_exists_display() {
        let err = AttaError::AlreadyExists {
            entity_type: "Flow".to_string(),
            id: "my-flow".to_string(),
        };
        assert_eq!(err.to_string(), "already exists: Flow my-flow");
    }

    #[test]
    fn atta_error_validation_display() {
        let err = AttaError::Validation("missing field 'name'".to_string());
        assert_eq!(err.to_string(), "validation error: missing field 'name'");
    }

    #[test]
    fn atta_error_approval_timeout_display() {
        let err = AttaError::ApprovalTimeout {
            tool: "deploy".to_string(),
            timeout_secs: 3600,
        };
        assert!(err.to_string().contains("deploy"));
        assert!(err.to_string().contains("3600"));
    }

    #[test]
    fn atta_error_parallel_execution_partial_failure_display() {
        let err = AttaError::ParallelExecutionPartialFailure {
            succeeded: 3,
            total: 5,
        };
        assert!(err.to_string().contains("3/5"));
    }

    // ── 便捷 From 实现 ──

    #[test]
    fn serde_json_error_converts_to_atta_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let atta: AttaError = json_err.into();
        assert!(matches!(atta, AttaError::Other(_)));
    }

    #[test]
    fn io_error_converts_to_atta_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let atta: AttaError = io_err.into();
        assert!(matches!(atta, AttaError::Other(_)));
        let msg = atta.to_string();
        assert!(msg.contains("file missing"));
    }
}
