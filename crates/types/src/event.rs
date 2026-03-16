//! 事件信封与相关类型

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{Actor, ResourceType};
use crate::error::AttaError;
use crate::node::NodeCapacity;
use crate::task::Task;

/// 实体引用
///
/// 标识系统中的一个实体（Task、Flow、Tool 等），用于事件和审计。
///
/// # Examples
///
/// ```
/// use atta_types::{EntityRef, ResourceType};
/// use uuid::Uuid;
///
/// let task_id = Uuid::new_v4();
/// let entity = EntityRef::task(&task_id);
/// assert_eq!(entity.entity_type, ResourceType::Task);
///
/// let flow = EntityRef::flow("deploy");
/// assert_eq!(flow.id, "deploy");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRef {
    pub entity_type: ResourceType,
    pub id: String,
}

impl EntityRef {
    pub fn new(entity_type: ResourceType, id: impl Into<String>) -> Self {
        Self {
            entity_type,
            id: id.into(),
        }
    }

    pub fn task(id: &Uuid) -> Self {
        Self::new(ResourceType::Task, id.to_string())
    }

    pub fn flow(id: &str) -> Self {
        Self::new(ResourceType::Flow, id)
    }

    pub fn tool(name: &str) -> Self {
        Self::new(ResourceType::Tool, name)
    }

    pub fn node(id: &str) -> Self {
        Self::new(ResourceType::Node, id)
    }
}

/// 统一事件信封
///
/// 系统中所有事件都封装为 `EventEnvelope`，通过 `EventBus` 流转。
/// 包含事件类型、操作者、目标实体、关联 ID 和负载。
///
/// # Examples
///
/// ```
/// use atta_types::{Actor, EntityRef, EventEnvelope};
/// use uuid::Uuid;
///
/// let event = EventEnvelope::new(
///     "atta.task.created",
///     EntityRef::flow("deploy"),
///     Actor::user("alice"),
///     Uuid::new_v4(),
///     serde_json::json!({"key": "value"}),
/// ).unwrap();
/// assert_eq!(event.event_type, "atta.task.created");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub event_id: Uuid,
    pub event_type: String,
    pub occurred_at: DateTime<Utc>,
    pub actor: Actor,
    pub entity: EntityRef,
    pub correlation_id: Uuid,
    pub payload: serde_json::Value,
}

impl EventEnvelope {
    /// 通用构造方法
    pub fn new(
        event_type: impl Into<String>,
        entity: EntityRef,
        actor: Actor,
        correlation_id: Uuid,
        payload: impl Serialize,
    ) -> Result<Self, AttaError> {
        let payload = serde_json::to_value(payload)
            .map_err(|e| AttaError::Other(anyhow::anyhow!("failed to serialize event payload: {e}")))?;
        Ok(Self {
            event_id: Uuid::new_v4(),
            event_type: event_type.into(),
            occurred_at: Utc::now(),
            actor,
            entity,
            correlation_id,
            payload,
        })
    }

    // ── 快捷构造方法 ──

    /// 任务创建事件
    pub fn task_created(task: &Task) -> Result<Self, AttaError> {
        Self::new(
            "atta.task.created",
            EntityRef::task(&task.id),
            task.created_by.clone(),
            task.id,
            serde_json::json!({
                "flow_id": task.flow_id,
                "current_state": task.current_state,
            }),
        )
    }

    /// Flow 状态推进事件
    pub fn flow_advanced(task_id: &Uuid, from: &str, to: &str) -> Result<Self, AttaError> {
        Self::new(
            "atta.flow.advanced",
            EntityRef::task(task_id),
            Actor::system(),
            *task_id,
            serde_json::json!({ "from": from, "to": to }),
        )
    }

    /// Agent 分配事件
    pub fn agent_assigned(task_id: &Uuid, agent_type: &str) -> Result<Self, AttaError> {
        Self::new(
            "atta.agent.assigned",
            EntityRef::task(task_id),
            Actor::system(),
            *task_id,
            serde_json::json!({ "agent_type": agent_type }),
        )
    }

    /// Agent 完成事件
    pub fn agent_completed(task_id: &Uuid, output: &serde_json::Value, iterations: u32) -> Result<Self, AttaError> {
        Self::new(
            "atta.agent.completed",
            EntityRef::task(task_id),
            Actor::system(),
            *task_id,
            serde_json::json!({ "output": output, "iterations": iterations }),
        )
    }

    /// Agent 错误事件
    pub fn agent_error(task_id: &Uuid, error: &str) -> Result<Self, AttaError> {
        Self::new(
            "atta.agent.error",
            EntityRef::task(task_id),
            Actor::system(),
            *task_id,
            serde_json::json!({ "error": error }),
        )
    }

    /// 审批请求事件
    pub fn approval_requested(task_id: &Uuid, approval_id: &Uuid) -> Result<Self, AttaError> {
        Self::new(
            "atta.approval.requested",
            EntityRef::task(task_id),
            Actor::system(),
            *task_id,
            serde_json::json!({ "approval_id": approval_id }),
        )
    }

    /// 审批超时事件
    pub fn approval_expired(task_id: &Uuid, approval_id: &Uuid) -> Result<Self, AttaError> {
        Self::new(
            "atta.approval.expired",
            EntityRef::task(task_id),
            Actor::system(),
            *task_id,
            serde_json::json!({ "approval_id": approval_id }),
        )
    }

    /// Flow 重试事件
    pub fn flow_retry(task_id: &Uuid, state: &str, attempt: u32) -> Result<Self, AttaError> {
        Self::new(
            "atta.flow.retry",
            EntityRef::task(task_id),
            Actor::system(),
            *task_id,
            serde_json::json!({ "state": state, "attempt": attempt }),
        )
    }

    /// 节点心跳事件
    pub fn node_heartbeat(node_id: &str, capacity: &NodeCapacity) -> Result<Self, AttaError> {
        Self::new(
            "atta.node.heartbeat",
            EntityRef::node(node_id),
            Actor::system(),
            Uuid::new_v4(),
            capacity,
        )
    }

    /// 包安装事件
    pub fn package_installed(name: &str, version: &str) -> Result<Self, AttaError> {
        Self::new(
            "atta.package.installed",
            EntityRef::new(ResourceType::Package, name),
            Actor::system(),
            Uuid::new_v4(),
            serde_json::json!({ "name": name, "version": version }),
        )
    }

    /// 系统启动事件
    pub fn system_started(mode: &str) -> Result<Self, AttaError> {
        Self::new(
            "atta.system.started",
            EntityRef::new(ResourceType::Node, "local"),
            Actor::system(),
            Uuid::new_v4(),
            serde_json::json!({ "mode": mode }),
        )
    }

    /// 系统关闭事件
    pub fn system_shutdown(reason: &str) -> Result<Self, AttaError> {
        Self::new(
            "atta.system.shutdown",
            EntityRef::new(ResourceType::Node, "local"),
            Actor::system(),
            Uuid::new_v4(),
            serde_json::json!({ "reason": reason }),
        )
    }

    /// Tool 调用完成事件
    pub fn tool_completed(task_id: &Uuid, tool_name: &str, duration_ms: u64) -> Result<Self, AttaError> {
        Self::new(
            "atta.tool.invocation_completed",
            EntityRef::tool(tool_name),
            Actor::system(),
            *task_id,
            serde_json::json!({ "tool": tool_name, "duration_ms": duration_ms }),
        )
    }

    /// Agent 流式增量事件
    pub fn agent_stream_delta(task_id: &Uuid, chunk: &serde_json::Value) -> Result<Self, AttaError> {
        Self::new(
            "atta.agent.stream_delta",
            EntityRef::task(task_id),
            Actor::system(),
            *task_id,
            chunk.clone(),
        )
    }

    /// Agent 应用层 delta 事件（thinking / tool progress / text chunks）
    pub fn agent_delta(task_id: &Uuid, delta: &serde_json::Value) -> Result<Self, AttaError> {
        Self::new(
            "atta.agent.delta",
            EntityRef::task(task_id),
            Actor::system(),
            *task_id,
            delta.clone(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::ResourceType;
    use crate::task::{Task, TaskStatus};
    use chrono::Utc;
    use uuid::Uuid;

    // ── EntityRef constructors ──

    #[test]
    fn entity_ref_new_sets_fields() {
        let er = EntityRef::new(ResourceType::Flow, "my-flow");
        assert_eq!(er.entity_type, ResourceType::Flow);
        assert_eq!(er.id, "my-flow");
    }

    #[test]
    fn entity_ref_task_uses_uuid_string() {
        let id = Uuid::new_v4();
        let er = EntityRef::task(&id);
        assert_eq!(er.entity_type, ResourceType::Task);
        assert_eq!(er.id, id.to_string());
    }

    #[test]
    fn entity_ref_flow() {
        let er = EntityRef::flow("deploy");
        assert_eq!(er.entity_type, ResourceType::Flow);
        assert_eq!(er.id, "deploy");
    }

    #[test]
    fn entity_ref_tool() {
        let er = EntityRef::tool("web_search");
        assert_eq!(er.entity_type, ResourceType::Tool);
        assert_eq!(er.id, "web_search");
    }

    #[test]
    fn entity_ref_node() {
        let er = EntityRef::node("node-1");
        assert_eq!(er.entity_type, ResourceType::Node);
        assert_eq!(er.id, "node-1");
    }

    #[test]
    fn entity_ref_serde_round_trip() {
        let er = EntityRef::new(ResourceType::Skill, "summarize");
        let json = serde_json::to_string(&er).unwrap();
        let back: EntityRef = serde_json::from_str(&json).unwrap();
        assert_eq!(back.entity_type, ResourceType::Skill);
        assert_eq!(back.id, "summarize");
    }

    // ── EventEnvelope::new ──

    #[test]
    fn event_envelope_new_sets_event_type_and_payload() {
        let entity = EntityRef::flow("test-flow");
        let actor = Actor::system();
        let corr = Uuid::new_v4();
        let payload = serde_json::json!({"key": "value"});

        let env = EventEnvelope::new("test.event", entity, actor, corr, &payload).unwrap();

        assert_eq!(env.event_type, "test.event");
        assert_eq!(env.correlation_id, corr);
        assert_eq!(env.payload["key"], "value");
        assert_eq!(env.entity.id, "test-flow");
    }

    #[test]
    fn event_envelope_new_generates_unique_event_id() {
        let entity = EntityRef::flow("f");
        let actor = Actor::system();
        let corr = Uuid::new_v4();

        let e1 = EventEnvelope::new("t", entity.clone(), actor.clone(), corr, "a").unwrap();
        let e2 = EventEnvelope::new("t", entity, actor, corr, "b").unwrap();

        assert_ne!(e1.event_id, e2.event_id);
    }

    // ── task_created ──

    #[test]
    fn event_envelope_task_created() {
        let now = Utc::now();
        let task = Task {
            id: Uuid::new_v4(),
            flow_id: "deploy-flow".to_string(),
            current_state: "start".to_string(),
            state_data: serde_json::json!({}),
            input: serde_json::json!({"prompt": "hello"}),
            output: None,
            status: TaskStatus::Running,
            created_by: Actor::user("alice"),
            created_at: now,
            updated_at: now,
            completed_at: None,
            version: 0,
        };

        let env = EventEnvelope::task_created(&task).unwrap();
        assert_eq!(env.event_type, "atta.task.created");
        assert_eq!(env.correlation_id, task.id);
        assert_eq!(env.entity.id, task.id.to_string());
        assert_eq!(env.payload["flow_id"], "deploy-flow");
        assert_eq!(env.payload["current_state"], "start");
    }

    // ── flow_advanced ──

    #[test]
    fn event_envelope_flow_advanced() {
        let tid = Uuid::new_v4();
        let env = EventEnvelope::flow_advanced(&tid, "start", "review").unwrap();
        assert_eq!(env.event_type, "atta.flow.advanced");
        assert_eq!(env.payload["from"], "start");
        assert_eq!(env.payload["to"], "review");
        assert_eq!(env.correlation_id, tid);
    }

    // ── agent_assigned ──

    #[test]
    fn event_envelope_agent_assigned() {
        let tid = Uuid::new_v4();
        let env = EventEnvelope::agent_assigned(&tid, "react").unwrap();
        assert_eq!(env.event_type, "atta.agent.assigned");
        assert_eq!(env.payload["agent_type"], "react");
    }

    // ── agent_completed ──

    #[test]
    fn event_envelope_agent_completed() {
        let tid = Uuid::new_v4();
        let output = serde_json::json!({"result": "done"});
        let env = EventEnvelope::agent_completed(&tid, &output, 5).unwrap();
        assert_eq!(env.event_type, "atta.agent.completed");
        assert_eq!(env.payload["iterations"], 5);
        assert_eq!(env.payload["output"]["result"], "done");
    }

    // ── agent_error ──

    #[test]
    fn event_envelope_agent_error() {
        let tid = Uuid::new_v4();
        let env = EventEnvelope::agent_error(&tid, "something broke").unwrap();
        assert_eq!(env.event_type, "atta.agent.error");
        assert_eq!(env.payload["error"], "something broke");
    }

    // ── approval_requested / approval_expired ──

    #[test]
    fn event_envelope_approval_requested() {
        let tid = Uuid::new_v4();
        let aid = Uuid::new_v4();
        let env = EventEnvelope::approval_requested(&tid, &aid).unwrap();
        assert_eq!(env.event_type, "atta.approval.requested");
        assert_eq!(env.payload["approval_id"], aid.to_string());
    }

    #[test]
    fn event_envelope_approval_expired() {
        let tid = Uuid::new_v4();
        let aid = Uuid::new_v4();
        let env = EventEnvelope::approval_expired(&tid, &aid).unwrap();
        assert_eq!(env.event_type, "atta.approval.expired");
        assert_eq!(env.payload["approval_id"], aid.to_string());
    }

    // ── flow_retry ──

    #[test]
    fn event_envelope_flow_retry() {
        let tid = Uuid::new_v4();
        let env = EventEnvelope::flow_retry(&tid, "review", 3).unwrap();
        assert_eq!(env.event_type, "atta.flow.retry");
        assert_eq!(env.payload["state"], "review");
        assert_eq!(env.payload["attempt"], 3);
    }

    // ── node_heartbeat ──

    #[test]
    fn event_envelope_node_heartbeat() {
        let cap = NodeCapacity {
            total_memory: 1024,
            available_memory: 512,
            running_agents: 2,
            max_concurrent: 10,
        };
        let env = EventEnvelope::node_heartbeat("node-1", &cap).unwrap();
        assert_eq!(env.event_type, "atta.node.heartbeat");
        assert_eq!(env.entity.id, "node-1");
    }

    // ── system_started / system_shutdown ──

    #[test]
    fn event_envelope_system_started() {
        let env = EventEnvelope::system_started("desktop").unwrap();
        assert_eq!(env.event_type, "atta.system.started");
        assert_eq!(env.payload["mode"], "desktop");
    }

    #[test]
    fn event_envelope_system_shutdown() {
        let env = EventEnvelope::system_shutdown("user requested").unwrap();
        assert_eq!(env.event_type, "atta.system.shutdown");
        assert_eq!(env.payload["reason"], "user requested");
    }

    // ── tool_completed ──

    #[test]
    fn event_envelope_tool_completed() {
        let tid = Uuid::new_v4();
        let env = EventEnvelope::tool_completed(&tid, "web_search", 150).unwrap();
        assert_eq!(env.event_type, "atta.tool.invocation_completed");
        assert_eq!(env.payload["tool"], "web_search");
        assert_eq!(env.payload["duration_ms"], 150);
    }

    // ── package_installed ──

    #[test]
    fn event_envelope_package_installed() {
        let env = EventEnvelope::package_installed("calc", "1.0.0").unwrap();
        assert_eq!(env.event_type, "atta.package.installed");
        assert_eq!(env.payload["name"], "calc");
        assert_eq!(env.payload["version"], "1.0.0");
    }

    // ── serde round-trip ──

    #[test]
    fn event_envelope_serde_round_trip() {
        let entity = EntityRef::flow("f");
        let env = EventEnvelope::new(
            "test",
            entity,
            Actor::user("bob"),
            Uuid::new_v4(),
            serde_json::json!({"x": 1}),
        )
        .unwrap();

        let json = serde_json::to_string(&env).unwrap();
        let back: EventEnvelope = serde_json::from_str(&json).unwrap();

        assert_eq!(back.event_id, env.event_id);
        assert_eq!(back.event_type, env.event_type);
        assert_eq!(back.correlation_id, env.correlation_id);
        assert_eq!(back.payload, env.payload);
    }
}
