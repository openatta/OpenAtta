//! Integration tests: FlowEngine + StateStore + EventBus
//!
//! Wires up real InProcBus + SqliteStore(":memory:") to verify full Flow lifecycle.

use std::sync::Arc;

use serde_json::json;

use atta_bus::InProcBus;
use atta_core::{DefaultToolRegistry, FlowEngine};
use atta_store::SqliteStore;
use atta_types::{Actor, ErrorPolicy, FlowDef, StateDef, StateType, TaskFilter, TransitionDef};

fn temp_db_path() -> String {
    format!(
        "/tmp/atta_test_{}.db",
        uuid::Uuid::new_v4().to_string().replace('-', "")
    )
}

async fn make_engine() -> (Arc<FlowEngine>, Arc<dyn atta_store::StateStore>) {
    let bus: Arc<dyn atta_bus::EventBus> = Arc::new(InProcBus::new());
    let store = Arc::new(SqliteStore::open(&temp_db_path()).await.unwrap());
    let tool_reg: Arc<dyn atta_types::ToolRegistry> = Arc::new(DefaultToolRegistry::new());
    let engine = Arc::new(FlowEngine::new(
        store.clone() as Arc<dyn atta_store::StateStore>,
        bus,
        tool_reg,
    ));
    (engine, store as Arc<dyn atta_store::StateStore>)
}

fn simple_flow() -> FlowDef {
    FlowDef {
        id: "test_simple".into(),
        version: "1.0".into(),
        name: Some("Simple Test Flow".into()),
        description: None,
        initial_state: "init".into(),
        states: vec![
            (
                "init".into(),
                StateDef {
                    state_type: StateType::Start,
                    agent: None,
                    skill: None,
                    gate: None,
                    on_enter: None,
                    transitions: vec![TransitionDef {
                        to: "complete".into(),
                        when: None,
                        auto: Some(true),
                    }],
                    branches: None,
                    join_strategy: None,
                    timeout_secs: None,
                },
            ),
            (
                "complete".into(),
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
            ),
        ]
        .into_iter()
        .collect(),
        on_error: None,
        skills: vec![],
        source: "builtin".to_string(),
    }
}

fn multi_state_flow() -> FlowDef {
    FlowDef {
        id: "test_multi".into(),
        version: "1.0".into(),
        name: Some("Multi-State Flow".into()),
        description: None,
        initial_state: "start".into(),
        states: vec![
            (
                "start".into(),
                StateDef {
                    state_type: StateType::Start,
                    agent: None,
                    skill: None,
                    gate: None,
                    on_enter: None,
                    transitions: vec![TransitionDef {
                        to: "process".into(),
                        when: None,
                        auto: Some(true),
                    }],
                    branches: None,
                    join_strategy: None,
                    timeout_secs: None,
                },
            ),
            (
                "process".into(),
                StateDef {
                    state_type: StateType::Agent,
                    agent: Some("react".into()),
                    skill: Some("research".into()),
                    gate: None,
                    on_enter: None,
                    transitions: vec![TransitionDef {
                        to: "done".into(),
                        when: Some("all_done".into()),
                        auto: None,
                    }],
                    branches: None,
                    join_strategy: None,
                    timeout_secs: None,
                },
            ),
            (
                "done".into(),
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
            ),
        ]
        .into_iter()
        .collect(),
        on_error: None,
        skills: vec![],
        source: "builtin".to_string(),
    }
}

fn flow_with_error_policy() -> FlowDef {
    FlowDef {
        id: "test_error_policy".into(),
        version: "1.0".into(),
        name: Some("Error Policy Flow".into()),
        description: None,
        initial_state: "start".into(),
        states: vec![
            (
                "start".into(),
                StateDef {
                    state_type: StateType::Start,
                    agent: None,
                    skill: None,
                    gate: None,
                    on_enter: None,
                    transitions: vec![TransitionDef {
                        to: "done".into(),
                        when: None,
                        auto: Some(true),
                    }],
                    branches: None,
                    join_strategy: None,
                    timeout_secs: None,
                },
            ),
            (
                "done".into(),
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
            ),
        ]
        .into_iter()
        .collect(),
        on_error: Some(ErrorPolicy {
            max_retries: 3,
            retry_states: vec!["start".into()],
            fallback: "done".into(),
        }),
        skills: vec![],
        source: "builtin".to_string(),
    }
}

// ── Flow validation ──

#[tokio::test]
async fn test_register_and_retrieve_flow() {
    let (engine, _store) = make_engine().await;
    let flow = simple_flow();
    engine.register_flow_def(flow.clone()).unwrap();

    let retrieved = engine.get_flow_def("test_simple").unwrap();
    assert_eq!(retrieved.id, "test_simple");
    assert_eq!(retrieved.version, "1.0");
    assert_eq!(retrieved.states.len(), 2);
}

#[tokio::test]
async fn test_register_invalid_flow_no_start() {
    let (engine, _store) = make_engine().await;
    let mut flow = simple_flow();
    // Remove the start state and make both states End
    flow.states.get_mut("init").unwrap().state_type = StateType::End;
    let result = engine.register_flow_def(flow);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_register_invalid_flow_no_end() {
    let (engine, _store) = make_engine().await;
    let mut flow = simple_flow();
    flow.states.get_mut("complete").unwrap().state_type = StateType::Start;
    let result = engine.register_flow_def(flow);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_register_invalid_flow_bad_transition_target() {
    let (engine, _store) = make_engine().await;
    let mut flow = simple_flow();
    flow.states.get_mut("init").unwrap().transitions[0].to = "nonexistent".into();
    let result = engine.register_flow_def(flow);
    assert!(result.is_err());
}

// ── Task lifecycle ──

#[tokio::test]
async fn test_create_task_auto_advances_to_end() {
    let (engine, store) = make_engine().await;
    let flow = simple_flow();
    store.save_flow_def(&flow).await.unwrap();
    engine.register_flow_def(flow).unwrap();

    let task = engine
        .create_task("test_simple", json!({"key": "value"}), Actor::system())
        .await
        .unwrap();

    // Auto-transition should have moved task to "complete" (End state)
    let reloaded = store.get_task(&task.id).await.unwrap().unwrap();
    assert_eq!(reloaded.current_state, "complete");
    assert_eq!(reloaded.input["key"], "value");
}

#[tokio::test]
async fn test_create_task_stops_at_agent_state() {
    let (engine, store) = make_engine().await;
    let flow = multi_state_flow();
    store.save_flow_def(&flow).await.unwrap();
    engine.register_flow_def(flow).unwrap();

    let task = engine
        .create_task("test_multi", json!({}), Actor::system())
        .await
        .unwrap();

    // Should stop at "process" (Agent state — needs external agent to advance)
    let reloaded = store.get_task(&task.id).await.unwrap().unwrap();
    assert_eq!(reloaded.current_state, "process");
}

#[tokio::test]
async fn test_create_task_for_nonexistent_flow_fails() {
    let (engine, _store) = make_engine().await;
    let result = engine
        .create_task("no_such_flow", json!({}), Actor::system())
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_flow_with_error_policy_registers() {
    let (engine, _store) = make_engine().await;
    engine.register_flow_def(flow_with_error_policy()).unwrap();
    let flow = engine.get_flow_def("test_error_policy").unwrap();
    assert!(flow.on_error.is_some());
    let policy = flow.on_error.unwrap();
    assert_eq!(policy.max_retries, 3);
    assert_eq!(policy.fallback, "done");
}

// ── Task store integration ──

#[tokio::test]
async fn test_task_persists_in_store() {
    let (engine, store) = make_engine().await;
    let flow = simple_flow();
    store.save_flow_def(&flow).await.unwrap();
    engine.register_flow_def(flow).unwrap();

    let task = engine
        .create_task("test_simple", json!({"x": 42}), Actor::system())
        .await
        .unwrap();

    // Verify task is queryable from the store
    let found = store.get_task(&task.id).await.unwrap();
    assert!(found.is_some());
    let t = found.unwrap();
    assert_eq!(t.flow_id, "test_simple");
    assert_eq!(t.input["x"], 42);
}

#[tokio::test]
async fn test_list_tasks_returns_created_tasks() {
    let (engine, store) = make_engine().await;
    let flow = simple_flow();
    store.save_flow_def(&flow).await.unwrap();
    engine.register_flow_def(flow).unwrap();

    engine
        .create_task("test_simple", json!({"a": 1}), Actor::system())
        .await
        .unwrap();
    engine
        .create_task("test_simple", json!({"a": 2}), Actor::system())
        .await
        .unwrap();

    let filter = TaskFilter {
        status: None,
        flow_id: None,
        created_by: None,
        limit: 100,
        offset: 0,
    };
    let tasks = store.list_tasks(&filter).await.unwrap();
    assert!(tasks.len() >= 2);
}

// ── Flow YAML parsing ──

#[tokio::test]
async fn test_parse_flow_yaml() {
    let yaml = r#"
id: yaml_test
version: "1.0"
name: "YAML Test"
initial_state: start
states:
  start:
    type: start
    transitions:
      - to: end
        auto: true
  end:
    type: end
    transitions: []
"#;
    let flow: FlowDef = serde_yml::from_str(yaml).unwrap();
    assert_eq!(flow.id, "yaml_test");
    assert_eq!(flow.states.len(), 2);

    let (engine, _store) = make_engine().await;
    engine.register_flow_def(flow).unwrap();
}

#[tokio::test]
async fn test_parse_flow_yaml_with_gate() {
    let yaml = r#"
id: gate_test
version: "1.0"
initial_state: start
states:
  start:
    type: start
    transitions:
      - to: approval
        auto: true
  approval:
    type: gate
    gate:
      approver_role: admin
      timeout: "24h"
      on_timeout: done
    transitions:
      - to: done
        when: "approved"
  done:
    type: end
    transitions: []
"#;
    let flow: FlowDef = serde_yml::from_str(yaml).unwrap();
    assert_eq!(flow.states.len(), 3);

    let (engine, _store) = make_engine().await;
    engine.register_flow_def(flow).unwrap();
}
