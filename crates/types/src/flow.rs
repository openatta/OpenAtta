//! Flow 状态机类型

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::approval::ApprovalRequest;

fn default_source() -> String {
    "builtin".to_string()
}

/// Flow 定义（模板）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowDef {
    pub id: String,
    pub version: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub initial_state: String,
    pub states: HashMap<String, StateDef>,
    pub on_error: Option<ErrorPolicy>,
    /// Skill IDs used by this flow (for dependency tracking and validation)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    /// "builtin" | "imported"
    #[serde(default = "default_source")]
    pub source: String,
}

/// 状态定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDef {
    #[serde(rename = "type")]
    pub state_type: StateType,
    pub agent: Option<String>,
    pub skill: Option<String>,
    pub gate: Option<GateDef>,
    pub on_enter: Option<Vec<OnEnterAction>>,
    #[serde(default)]
    pub transitions: Vec<TransitionDef>,
    /// Parallel branches (only used when `state_type` is `Parallel`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branches: Option<Vec<BranchDef>>,
    /// Join strategy for parallel branches
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join_strategy: Option<JoinStrategy>,
    /// Optional timeout in seconds for parallel branch completion
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
}

/// 状态类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StateType {
    #[serde(rename = "start")]
    Start,
    #[serde(rename = "agent")]
    Agent,
    #[serde(rename = "gate")]
    Gate,
    #[serde(rename = "parallel")]
    Parallel,
    #[serde(rename = "end")]
    End,
}

/// 审批门控定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateDef {
    pub approver_role: String,
    pub timeout: String,
    pub on_timeout: String,
    pub notify: Option<Vec<NotifyChannel>>,
    pub context: Option<Vec<String>>,
}

/// 转移定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionDef {
    pub to: String,
    pub when: Option<String>,
    pub auto: Option<bool>,
}

/// 进入状态时执行的动作
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum OnEnterAction {
    /// 发布事件到 EventBus
    PublishEvent { event_type: String },
    /// 设置 state_data 变量
    SetVariable {
        key: String,
        value: serde_json::Value,
    },
    /// 输出日志
    Log { message: String },
}

/// 审批门控通知渠道
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum NotifyChannel {
    Email { to: String, template: String },
    Webhook { url: String },
    EventBus { topic: String },
}

/// 错误策略
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPolicy {
    pub max_retries: u32,
    pub retry_states: Vec<String>,
    pub fallback: String,
}

/// Flow 运行状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowState {
    pub task_id: Uuid,
    pub current_state: String,
    pub history: Vec<StateTransition>,
    pub pending_approval: Option<ApprovalRequest>,
    pub retry_count: u32,
}

impl Default for FlowState {
    fn default() -> Self {
        Self {
            task_id: Uuid::nil(),
            current_state: String::new(),
            history: Vec::new(),
            pending_approval: None,
            retry_count: 0,
        }
    }
}

/// 状态转移记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransition {
    pub from: String,
    pub to: String,
    pub reason: String,
    pub timestamp: DateTime<Utc>,
}

/// 条件表达式 AST
#[derive(Debug, Clone, PartialEq)]
pub enum CondExpr {
    /// 不等比较: key != value
    Ne(String, String),
    Ident(String),
    Eq(String, String),
    And(Box<CondExpr>, Box<CondExpr>),
    Or(Box<CondExpr>, Box<CondExpr>),
    Not(Box<CondExpr>),
}

/// 并行分支定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchDef {
    pub agent: String,
    pub skill: String,
    pub input_mapping: Option<HashMap<String, String>>,
}

/// 并行汇合策略
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinStrategy {
    All,
    FailFast,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // ── FlowDef serde round-trip ──

    #[test]
    fn flow_def_minimal_serde_round_trip() {
        let mut states = HashMap::new();
        states.insert(
            "start".to_string(),
            StateDef {
                state_type: StateType::Start,
                agent: None,
                skill: None,
                gate: None,
                on_enter: None,
                transitions: vec![TransitionDef {
                    to: "end".to_string(),
                    when: None,
                    auto: Some(true),
                }],
                branches: None,
                join_strategy: None,
                timeout_secs: None,
            },
        );
        states.insert(
            "end".to_string(),
            StateDef {
                state_type: StateType::End,
                agent: None,
                skill: None,
                gate: None,
                on_enter: None,
                transitions: vec![],
                branches: None,
                join_strategy: None,
                timeout_secs: None,
            },
        );

        let flow = FlowDef {
            id: "test-flow".to_string(),
            version: "1.0".to_string(),
            name: Some("Test Flow".to_string()),
            description: Some("A test flow".to_string()),
            initial_state: "start".to_string(),
            states,
            on_error: None,
            skills: vec![],
            source: "builtin".to_string(),
        };

        let json = serde_json::to_string(&flow).unwrap();
        let back: FlowDef = serde_json::from_str(&json).unwrap();

        assert_eq!(back.id, "test-flow");
        assert_eq!(back.version, "1.0");
        assert_eq!(back.name.as_deref(), Some("Test Flow"));
        assert_eq!(back.initial_state, "start");
        assert_eq!(back.states.len(), 2);
        assert!(back.states.contains_key("start"));
        assert!(back.states.contains_key("end"));
    }

    #[test]
    fn flow_def_with_error_policy_serde_round_trip() {
        let flow = FlowDef {
            id: "retry-flow".to_string(),
            version: "2.0".to_string(),
            name: None,
            description: None,
            initial_state: "start".to_string(),
            states: HashMap::new(),
            on_error: Some(ErrorPolicy {
                max_retries: 3,
                retry_states: vec!["fetch".to_string(), "process".to_string()],
                fallback: "error_handler".to_string(),
            }),
            skills: vec![],
            source: "builtin".to_string(),
        };

        let json = serde_json::to_string(&flow).unwrap();
        let back: FlowDef = serde_json::from_str(&json).unwrap();

        let policy = back.on_error.unwrap();
        assert_eq!(policy.max_retries, 3);
        assert_eq!(policy.retry_states.len(), 2);
        assert_eq!(policy.fallback, "error_handler");
    }

    // ── StateDef serde round-trip ──

    #[test]
    fn state_def_agent_type_serde_round_trip() {
        let sd = StateDef {
            state_type: StateType::Agent,
            agent: Some("react".to_string()),
            skill: Some("summarize".to_string()),
            gate: None,
            on_enter: Some(vec![OnEnterAction::Log {
                message: "entering agent state".to_string(),
            }]),
            transitions: vec![
                TransitionDef {
                    to: "review".to_string(),
                    when: Some("success".to_string()),
                    auto: None,
                },
                TransitionDef {
                    to: "error".to_string(),
                    when: Some("failure".to_string()),
                    auto: Some(false),
                },
            ],
            branches: None,
            join_strategy: None,
            timeout_secs: None,
        };

        let json = serde_json::to_string(&sd).unwrap();
        let back: StateDef = serde_json::from_str(&json).unwrap();

        assert_eq!(back.state_type, StateType::Agent);
        assert_eq!(back.agent.as_deref(), Some("react"));
        assert_eq!(back.skill.as_deref(), Some("summarize"));
        assert_eq!(back.transitions.len(), 2);
        assert!(back.on_enter.is_some());
    }

    #[test]
    fn state_def_gate_type_with_gate_def() {
        let sd = StateDef {
            state_type: StateType::Gate,
            agent: None,
            skill: None,
            gate: Some(GateDef {
                approver_role: "admin".to_string(),
                timeout: "1h".to_string(),
                on_timeout: "reject".to_string(),
                notify: Some(vec![NotifyChannel::Webhook {
                    url: "https://hooks.example.com".to_string(),
                }]),
                context: Some(vec!["summary".to_string()]),
            }),
            on_enter: None,
            transitions: vec![],
            branches: None,
            join_strategy: None,
            timeout_secs: None,
        };

        let json = serde_json::to_string(&sd).unwrap();
        let back: StateDef = serde_json::from_str(&json).unwrap();

        let gate = back.gate.unwrap();
        assert_eq!(gate.approver_role, "admin");
        assert_eq!(gate.timeout, "1h");
        assert_eq!(gate.on_timeout, "reject");
    }

    // ── StateType serde ──

    #[test]
    fn state_type_serde_values() {
        let cases = vec![
            (StateType::Start, r#""start""#),
            (StateType::Agent, r#""agent""#),
            (StateType::Gate, r#""gate""#),
            (StateType::Parallel, r#""parallel""#),
            (StateType::End, r#""end""#),
        ];
        for (variant, expected_json) in cases {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected_json, "serialization of {:?}", variant);
            let back: StateType = serde_json::from_str(&json).unwrap();
            assert_eq!(back, variant);
        }
    }

    // ── FlowState default ──

    #[test]
    fn flow_state_default() {
        let fs = FlowState::default();
        assert_eq!(fs.task_id, Uuid::nil());
        assert!(fs.current_state.is_empty());
        assert!(fs.history.is_empty());
        assert!(fs.pending_approval.is_none());
        assert_eq!(fs.retry_count, 0);
    }

    #[test]
    fn flow_state_serde_round_trip() {
        let fs = FlowState {
            task_id: Uuid::new_v4(),
            current_state: "review".to_string(),
            history: vec![StateTransition {
                from: "start".to_string(),
                to: "review".to_string(),
                reason: "auto".to_string(),
                timestamp: Utc::now(),
            }],
            pending_approval: None,
            retry_count: 1,
        };

        let json = serde_json::to_string(&fs).unwrap();
        let back: FlowState = serde_json::from_str(&json).unwrap();

        assert_eq!(back.task_id, fs.task_id);
        assert_eq!(back.current_state, "review");
        assert_eq!(back.history.len(), 1);
        assert_eq!(back.history[0].from, "start");
        assert_eq!(back.history[0].to, "review");
        assert_eq!(back.retry_count, 1);
    }

    // ── OnEnterAction serde ──

    #[test]
    fn on_enter_action_publish_event_serde() {
        let action = OnEnterAction::PublishEvent {
            event_type: "atta.custom".to_string(),
        };
        let json = serde_json::to_string(&action).unwrap();
        let back: OnEnterAction = serde_json::from_str(&json).unwrap();
        match back {
            OnEnterAction::PublishEvent { event_type } => {
                assert_eq!(event_type, "atta.custom");
            }
            _ => panic!("expected PublishEvent"),
        }
    }

    #[test]
    fn on_enter_action_set_variable_serde() {
        let action = OnEnterAction::SetVariable {
            key: "counter".to_string(),
            value: serde_json::json!(42),
        };
        let json = serde_json::to_string(&action).unwrap();
        let back: OnEnterAction = serde_json::from_str(&json).unwrap();
        match back {
            OnEnterAction::SetVariable { key, value } => {
                assert_eq!(key, "counter");
                assert_eq!(value, serde_json::json!(42));
            }
            _ => panic!("expected SetVariable"),
        }
    }

    // ── NotifyChannel serde ──

    #[test]
    fn notify_channel_email_serde() {
        let nc = NotifyChannel::Email {
            to: "admin@example.com".to_string(),
            template: "approval-needed".to_string(),
        };
        let json = serde_json::to_string(&nc).unwrap();
        let back: NotifyChannel = serde_json::from_str(&json).unwrap();
        match back {
            NotifyChannel::Email { to, template } => {
                assert_eq!(to, "admin@example.com");
                assert_eq!(template, "approval-needed");
            }
            _ => panic!("expected Email"),
        }
    }

    #[test]
    fn notify_channel_event_bus_serde() {
        let nc = NotifyChannel::EventBus {
            topic: "approvals".to_string(),
        };
        let json = serde_json::to_string(&nc).unwrap();
        let back: NotifyChannel = serde_json::from_str(&json).unwrap();
        match back {
            NotifyChannel::EventBus { topic } => assert_eq!(topic, "approvals"),
            _ => panic!("expected EventBus"),
        }
    }

    // ── JoinStrategy serde ──

    #[test]
    fn join_strategy_serde() {
        let json = serde_json::to_string(&JoinStrategy::All).unwrap();
        assert_eq!(json, r#""all""#);
        let back: JoinStrategy = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, JoinStrategy::All));

        let json = serde_json::to_string(&JoinStrategy::FailFast).unwrap();
        assert_eq!(json, r#""fail_fast""#);
        let back: JoinStrategy = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, JoinStrategy::FailFast));
    }

    // ── BranchDef serde ──

    #[test]
    fn branch_def_serde_round_trip() {
        let mut mapping = HashMap::new();
        mapping.insert("query".to_string(), "$.input.question".to_string());

        let bd = BranchDef {
            agent: "react".to_string(),
            skill: "search".to_string(),
            input_mapping: Some(mapping),
        };

        let json = serde_json::to_string(&bd).unwrap();
        let back: BranchDef = serde_json::from_str(&json).unwrap();

        assert_eq!(back.agent, "react");
        assert_eq!(back.skill, "search");
        assert!(back.input_mapping.is_some());
        assert_eq!(
            back.input_mapping.unwrap().get("query").unwrap(),
            "$.input.question"
        );
    }

    // ── CondExpr equality ──

    #[test]
    fn cond_expr_equality() {
        let a = CondExpr::Eq("status".into(), "ok".into());
        let b = CondExpr::Eq("status".into(), "ok".into());
        assert_eq!(a, b);

        let c = CondExpr::Ne("status".into(), "error".into());
        assert_ne!(a, c);
    }

    #[test]
    fn cond_expr_compound() {
        let left = CondExpr::Eq("x".into(), "1".into());
        let right = CondExpr::Ident("ready".into());
        let and = CondExpr::And(Box::new(left.clone()), Box::new(right.clone()));
        let or = CondExpr::Or(Box::new(left), Box::new(right));
        // Just verify they can be constructed and are not equal
        assert_ne!(and, or);
    }
}
