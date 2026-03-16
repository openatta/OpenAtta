//! Integration tests for PostgresStore
//!
//! These tests require a running PostgreSQL instance. Set the environment
//! variable `ATTA_POSTGRES_URL` to a valid connection URL before running:
//!
//! ```sh
//! ATTA_POSTGRES_URL=postgres://user:pass@localhost/atta_test \
//!   cargo test -p atta-store --features postgres --test postgres_tests
//! ```
//!
//! If `ATTA_POSTGRES_URL` is not set, every test prints a skip notice and
//! returns immediately — the test suite will still pass.
//!
//! Because Postgres is a shared, persistent server (unlike the per-test
//! SQLite temp files), each test uses randomly generated UUIDs / string IDs
//! so that concurrent runs do not collide.

#![cfg(feature = "postgres")]

use std::collections::HashMap;
use std::time::Duration;

use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use atta_store::{
    ApprovalStore, CronStore, FlowStore, McpStore, NodeStore, PackageStore, RbacStore,
    RegistryStore, PostgresStore, TaskStore,
};
use atta_types::{
    Actor, ApprovalContext, ApprovalFilter, ApprovalRequest, ApprovalStatus, CronJob, CronRun,
    CronRunStatus, FlowDef, FlowState, McpServerConfig, McpTransport, NodeCapacity, NodeInfo,
    NodeStatus, PackageRecord, PackageType, PluginManifest, RiskLevel, Role, SkillDef, StateDef,
    StateTransition, StateType, Task, TaskFilter, TaskStatus, ToolBinding, ToolDef, TransitionDef,
};

// ---------------------------------------------------------------------------
// Skip macro & store constructor
// ---------------------------------------------------------------------------

/// Read `ATTA_POSTGRES_URL` from the environment; skip the test if absent.
macro_rules! skip_unless_postgres {
    () => {
        match std::env::var("ATTA_POSTGRES_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("skipping Postgres test: ATTA_POSTGRES_URL not set");
                return;
            }
        }
    };
}

/// Connect to the Postgres instance and run migrations.
async fn new_store(url: &str) -> PostgresStore {
    PostgresStore::connect(url)
        .await
        .expect("failed to connect to Postgres")
}

// ---------------------------------------------------------------------------
// Helpers — mirror sqlite_tests.rs exactly
// ---------------------------------------------------------------------------

/// Build a minimal Task for testing.
fn make_task(flow_id: &str) -> Task {
    let now = Utc::now();
    Task {
        id: Uuid::new_v4(),
        flow_id: flow_id.to_string(),
        current_state: "start".to_string(),
        state_data: json!({"step": 0}),
        input: json!({"prompt": "hello"}),
        output: None,
        status: TaskStatus::Running,
        created_by: Actor::user("test-user"),
        created_at: now,
        updated_at: now,
        completed_at: None,
        version: 0,
    }
}

/// Build a minimal FlowDef for testing.
fn make_flow_def(id: &str) -> FlowDef {
    let mut states = HashMap::new();
    states.insert(
        "start".to_string(),
        StateDef {
            state_type: StateType::Start,
            agent: None,
            skill: None,
            gate: None,
            on_enter: None,
            branches: None,
            join_strategy: None,
            timeout_secs: None,
            transitions: vec![TransitionDef {
                to: "end".to_string(),
                when: None,
                auto: Some(true),
            }],
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
            branches: None,
            join_strategy: None,
            timeout_secs: None,
            transitions: vec![],
        },
    );
    FlowDef {
        id: id.to_string(),
        version: "1.0.0".to_string(),
        name: Some("Test Flow".to_string()),
        description: Some("A test flow".to_string()),
        initial_state: "start".to_string(),
        states,
        on_error: None,
        skills: vec![],
        source: "builtin".to_string(),
    }
}

/// Build a FlowState for a given task id.
fn make_flow_state(task_id: Uuid) -> FlowState {
    FlowState {
        task_id,
        current_state: "start".to_string(),
        history: vec![],
        pending_approval: None,
        retry_count: 0,
    }
}

/// Build a ToolDef with a Native plugin binding.
fn make_tool(name: &str) -> ToolDef {
    ToolDef {
        name: name.to_string(),
        description: format!("Tool {name}"),
        binding: ToolBinding::Native {
            handler_name: "test-plugin".to_string(),
        },
        risk_level: RiskLevel::Low,
        parameters: json!({"type": "object", "properties": {}}),
    }
}

/// Build a PluginManifest.
fn make_plugin(name: &str) -> PluginManifest {
    PluginManifest {
        name: name.to_string(),
        version: "0.1.0".to_string(),
        description: Some("A test plugin".to_string()),
        author: Some("tester".to_string()),
        organization: None,
        permissions: vec!["fs.read".to_string()],
        resource_limits: None,
    }
}

/// Build a SkillDef.
fn make_skill(id: &str) -> SkillDef {
    SkillDef {
        id: id.to_string(),
        version: "1.0.0".to_string(),
        name: Some(format!("Skill {id}")),
        description: Some("A test skill".to_string()),
        system_prompt: "You are a helpful assistant.".to_string(),
        tools: vec!["tool-a".to_string()],
        steps: None,
        output_format: None,
        requires_approval: false,
        risk_level: RiskLevel::Low,
        tags: vec!["test".to_string()],
        variables: None,
        author: Some("tester".to_string()),
        source: "builtin".to_string(),
    }
}

/// Build a CronJob.
fn make_cron_job(id: &str, enabled: bool) -> CronJob {
    let now = Utc::now();
    CronJob {
        id: id.to_string(),
        name: format!("Job {id}"),
        schedule: "0 */5 * * * *".to_string(),
        command: "run-task".to_string(),
        config: json!({"flow": "daily-check"}),
        enabled,
        created_by: "admin".to_string(),
        created_at: now,
        updated_at: now,
        last_run_at: None,
        next_run_at: None,
    }
}

/// Build a CronRun.
fn make_cron_run(run_id: &str, job_id: &str, status: CronRunStatus) -> CronRun {
    CronRun {
        id: run_id.to_string(),
        job_id: job_id.to_string(),
        status,
        started_at: Utc::now(),
        completed_at: None,
        output: None,
        error: None,
        triggered_by: "scheduler".to_string(),
    }
}

/// Build a NodeInfo.
fn make_node(node_id: &str) -> NodeInfo {
    NodeInfo {
        id: node_id.to_string(),
        hostname: format!("{node_id}.local"),
        labels: vec!["gpu".to_string()],
        status: NodeStatus::Online,
        capacity: NodeCapacity {
            total_memory: 8192,
            available_memory: 4096,
            running_agents: 2,
            max_concurrent: 10,
        },
        last_heartbeat: Utc::now(),
    }
}

/// Build an ApprovalRequest.
fn make_approval(task_id: Uuid) -> ApprovalRequest {
    let now = Utc::now();
    let timeout = Duration::from_secs(3600);
    ApprovalRequest {
        id: Uuid::new_v4(),
        task_id,
        requested_by: Actor::user("dev-alice"),
        approver_role: Role::Approver,
        context: ApprovalContext {
            summary: "Deploy to production".to_string(),
            diff_summary: Some("3 files changed".to_string()),
            test_results: Some("all passed".to_string()),
            risk_assessment: "medium".to_string(),
            pending_tools: vec![],
        },
        status: ApprovalStatus::Pending,
        created_at: now,
        resolved_at: None,
        resolved_by: None,
        timeout,
        timeout_at: now + timeout,
    }
}

/// Build a McpServerConfig (stdio transport).
fn make_mcp_server(name: &str) -> McpServerConfig {
    McpServerConfig {
        name: name.to_string(),
        description: Some("Test MCP server".to_string()),
        transport: McpTransport::Stdio,
        url: None,
        command: Some("mcp-server".to_string()),
        args: vec!["--port".to_string(), "9000".to_string()],
        auth: None,
    }
}

/// Build a PackageRecord.
fn make_package(name: &str) -> PackageRecord {
    PackageRecord {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        package_type: PackageType::Plugin,
        installed_at: Utc::now(),
        installed_by: "admin".to_string(),
    }
}

// ===========================================================================
// TaskStore tests
// ===========================================================================

#[tokio::test]
async fn test_task_create_and_get() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let task = make_task(&format!("flow-{}", Uuid::new_v4()));
    let task_id = task.id;

    store.create_task(&task).await.expect("create_task");

    let fetched = store
        .get_task(&task_id)
        .await
        .expect("get_task")
        .expect("task should exist");

    assert_eq!(fetched.id, task_id);
    assert_eq!(fetched.current_state, "start");
    assert!(matches!(fetched.status, TaskStatus::Running));
    assert_eq!(fetched.state_data, json!({"step": 0}));
    assert_eq!(fetched.input, json!({"prompt": "hello"}));
    assert!(fetched.output.is_none());
    assert!(fetched.completed_at.is_none());
}

#[tokio::test]
async fn test_get_task_nonexistent() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let result = store
        .get_task(&Uuid::new_v4())
        .await
        .expect("get_task should not error");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_update_task_status_to_completed() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let task = make_task(&format!("flow-{}", Uuid::new_v4()));
    let task_id = task.id;

    store.create_task(&task).await.expect("create_task");
    store
        .update_task_status(&task_id, TaskStatus::Completed)
        .await
        .expect("update_task_status");

    let fetched = store
        .get_task(&task_id)
        .await
        .expect("get_task")
        .expect("task should exist");

    assert!(matches!(fetched.status, TaskStatus::Completed));
    assert!(fetched.completed_at.is_some());
}

#[tokio::test]
async fn test_update_task_status_to_failed() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let task = make_task(&format!("flow-{}", Uuid::new_v4()));
    let task_id = task.id;

    store.create_task(&task).await.expect("create_task");
    store
        .update_task_status(
            &task_id,
            TaskStatus::Failed {
                error: "timeout".to_string(),
            },
        )
        .await
        .expect("update_task_status");

    let fetched = store
        .get_task(&task_id)
        .await
        .expect("get_task")
        .expect("task should exist");

    match &fetched.status {
        TaskStatus::Failed { error } => assert_eq!(error, "timeout"),
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[tokio::test]
async fn test_list_tasks_filter_by_status() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    // Use a unique flow_id so we don't pick up rows from other test runs.
    let flow_id = format!("flow-status-{}", Uuid::new_v4());

    let t1 = make_task(&flow_id);
    let mut t2 = make_task(&flow_id);
    t2.status = TaskStatus::Completed;
    let t2_id = t2.id;

    store.create_task(&t1).await.unwrap();
    store.create_task(&t2).await.unwrap();

    let filter = TaskFilter {
        status: Some(TaskStatus::Completed),
        flow_id: Some(flow_id.clone()),
        created_by: None,
        limit: 100,
        offset: 0,
    };
    let tasks = store.list_tasks(&filter).await.expect("list_tasks");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, t2_id);
}

#[tokio::test]
async fn test_list_tasks_filter_by_flow_id() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let flow_a = format!("flow-a-{}", Uuid::new_v4());
    let flow_b = format!("flow-b-{}", Uuid::new_v4());

    let t1 = make_task(&flow_a);
    let t2 = make_task(&flow_b);

    store.create_task(&t1).await.unwrap();
    store.create_task(&t2).await.unwrap();

    let filter = TaskFilter {
        status: None,
        flow_id: Some(flow_b.clone()),
        created_by: None,
        limit: 100,
        offset: 0,
    };
    let tasks = store.list_tasks(&filter).await.expect("list_tasks");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].flow_id, flow_b);
}

#[tokio::test]
async fn test_list_tasks_limit_offset() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let flow_id = format!("flow-limit-{}", Uuid::new_v4());

    for _ in 0..5 {
        let t = make_task(&flow_id);
        store.create_task(&t).await.unwrap();
    }

    let filter = TaskFilter {
        status: None,
        flow_id: Some(flow_id.clone()),
        created_by: None,
        limit: 2,
        offset: 0,
    };
    let page1 = store.list_tasks(&filter).await.expect("list_tasks page1");
    assert_eq!(page1.len(), 2);

    let filter2 = TaskFilter {
        limit: 2,
        offset: 2,
        ..filter.clone()
    };
    let page2 = store.list_tasks(&filter2).await.expect("list_tasks page2");
    assert_eq!(page2.len(), 2);

    // IDs should not overlap
    assert_ne!(page1[0].id, page2[0].id);
}

#[tokio::test]
async fn test_merge_task_state_data() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let task = make_task(&format!("flow-{}", Uuid::new_v4()));
    let task_id = task.id;

    store.create_task(&task).await.unwrap();

    // Merge new keys
    store
        .merge_task_state_data(&task_id, json!({"result": "ok", "count": 42}))
        .await
        .expect("merge_task_state_data");

    let fetched = store.get_task(&task_id).await.unwrap().unwrap();
    assert_eq!(fetched.state_data["step"], 0);   // original key preserved
    assert_eq!(fetched.state_data["result"], "ok"); // new key added
    assert_eq!(fetched.state_data["count"], 42);
}

#[tokio::test]
async fn test_merge_task_state_data_overwrites_existing_key() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let task = make_task(&format!("flow-{}", Uuid::new_v4()));
    let task_id = task.id;

    store.create_task(&task).await.unwrap();

    // Overwrite "step"
    store
        .merge_task_state_data(&task_id, json!({"step": 99}))
        .await
        .expect("merge overwrite");

    let fetched = store.get_task(&task_id).await.unwrap().unwrap();
    assert_eq!(fetched.state_data["step"], 99);
}

// ===========================================================================
// FlowStore tests
// ===========================================================================

#[tokio::test]
async fn test_flow_def_save_and_get() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let flow_id = format!("flow-{}", Uuid::new_v4());
    let flow = make_flow_def(&flow_id);

    store.save_flow_def(&flow).await.expect("save_flow_def");

    let fetched = store
        .get_flow_def(&flow_id)
        .await
        .expect("get_flow_def")
        .expect("flow should exist");

    assert_eq!(fetched.id, flow_id);
    assert_eq!(fetched.version, "1.0.0");
    assert_eq!(fetched.initial_state, "start");
    assert!(fetched.states.contains_key("start"));
    assert!(fetched.states.contains_key("end"));
}

#[tokio::test]
async fn test_get_flow_def_nonexistent() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let result = store
        .get_flow_def(&format!("nope-{}", Uuid::new_v4()))
        .await
        .unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_flow_def_upsert() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let flow_id = format!("flow-{}", Uuid::new_v4());
    let mut flow = make_flow_def(&flow_id);
    store.save_flow_def(&flow).await.unwrap();

    // Update version
    flow.version = "2.0.0".to_string();
    store
        .save_flow_def(&flow)
        .await
        .expect("upsert should work");

    let fetched = store.get_flow_def(&flow_id).await.unwrap().unwrap();
    assert_eq!(fetched.version, "2.0.0");
}

#[tokio::test]
async fn test_list_flow_defs() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    // Use a unique prefix so listing can be verified for exactly these two
    let prefix = Uuid::new_v4().to_string().replace('-', "");
    let id_a = format!("{prefix}-alpha");
    let id_b = format!("{prefix}-beta");

    store.save_flow_def(&make_flow_def(&id_a)).await.unwrap();
    store.save_flow_def(&make_flow_def(&id_b)).await.unwrap();

    let flows = store.list_flow_defs().await.expect("list_flow_defs");
    // At minimum both flows must be present (there may be others from parallel tests)
    let ids: Vec<&str> = flows.iter().map(|f| f.id.as_str()).collect();
    assert!(ids.contains(&id_a.as_str()), "id_a missing: {ids:?}");
    assert!(ids.contains(&id_b.as_str()), "id_b missing: {ids:?}");
}

#[tokio::test]
async fn test_flow_state_save_and_get() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let task_id = Uuid::new_v4();
    let state = make_flow_state(task_id);

    store
        .save_flow_state(&task_id, &state)
        .await
        .expect("save_flow_state");

    let fetched = store
        .get_flow_state(&task_id)
        .await
        .expect("get_flow_state")
        .expect("flow state should exist");

    assert_eq!(fetched.task_id, task_id);
    assert_eq!(fetched.current_state, "start");
    assert!(fetched.history.is_empty());
    assert_eq!(fetched.retry_count, 0);
}

#[tokio::test]
async fn test_flow_state_upsert_with_history() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let task_id = Uuid::new_v4();
    let mut state = make_flow_state(task_id);

    store.save_flow_state(&task_id, &state).await.unwrap();

    // Advance state
    state.current_state = "processing".to_string();
    state.history.push(StateTransition {
        from: "start".to_string(),
        to: "processing".to_string(),
        reason: "auto".to_string(),
        timestamp: Utc::now(),
    });
    state.retry_count = 1;

    store
        .save_flow_state(&task_id, &state)
        .await
        .expect("upsert flow state");

    let fetched = store.get_flow_state(&task_id).await.unwrap().unwrap();
    assert_eq!(fetched.current_state, "processing");
    assert_eq!(fetched.history.len(), 1);
    assert_eq!(fetched.history[0].from, "start");
    assert_eq!(fetched.history[0].to, "processing");
    assert_eq!(fetched.retry_count, 1);
}

#[tokio::test]
async fn test_create_task_with_flow() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let task = make_task(&format!("composite-flow-{}", Uuid::new_v4()));
    let task_id = task.id;
    let flow_state = make_flow_state(task_id);

    store
        .create_task_with_flow(&task, &flow_state)
        .await
        .expect("create_task_with_flow");

    // Both task and flow state should exist
    let fetched_task = store.get_task(&task_id).await.unwrap().unwrap();
    assert!(matches!(fetched_task.status, TaskStatus::Running));

    let fetched_state = store.get_flow_state(&task_id).await.unwrap().unwrap();
    assert_eq!(fetched_state.task_id, task_id);
    assert_eq!(fetched_state.current_state, "start");
}

#[tokio::test]
async fn test_advance_task() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let task = make_task(&format!("advance-flow-{}", Uuid::new_v4()));
    let task_id = task.id;
    let flow_state = make_flow_state(task_id);

    store
        .create_task_with_flow(&task, &flow_state)
        .await
        .unwrap();

    let transition = StateTransition {
        from: "start".to_string(),
        to: "processing".to_string(),
        reason: "auto transition".to_string(),
        timestamp: Utc::now(),
    };

    store
        .advance_task(&task_id, TaskStatus::Running, "processing", &transition, 0)
        .await
        .expect("advance_task");

    let fetched_task = store.get_task(&task_id).await.unwrap().unwrap();
    assert_eq!(fetched_task.current_state, "processing");
    assert!(matches!(fetched_task.status, TaskStatus::Running));

    let fetched_state = store.get_flow_state(&task_id).await.unwrap().unwrap();
    assert_eq!(fetched_state.current_state, "processing");
    assert_eq!(fetched_state.history.len(), 1);
    assert_eq!(fetched_state.history[0].from, "start");
    assert_eq!(fetched_state.history[0].to, "processing");
    assert_eq!(fetched_state.history[0].reason, "auto transition");
}

// ===========================================================================
// RegistryStore tests — plugins
// ===========================================================================

#[tokio::test]
async fn test_plugin_register_and_list() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let suffix = Uuid::new_v4().to_string().replace('-', "");
    let p1 = make_plugin(&format!("plugin-a-{suffix}"));
    let p2 = make_plugin(&format!("plugin-b-{suffix}"));

    store.register_plugin(&p1).await.expect("register plugin a");
    store.register_plugin(&p2).await.expect("register plugin b");

    let plugins = store.list_plugins().await.expect("list_plugins");
    let names: Vec<&str> = plugins.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&p1.name.as_str()), "p1 missing");
    assert!(names.contains(&p2.name.as_str()), "p2 missing");
}

#[tokio::test]
async fn test_plugin_get() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let name = format!("plugin-{}", Uuid::new_v4());
    let p = make_plugin(&name);
    store.register_plugin(&p).await.unwrap();

    let fetched = store
        .get_plugin(&name)
        .await
        .expect("get_plugin")
        .expect("plugin should exist");
    assert_eq!(fetched.name, name);
    assert_eq!(fetched.version, "0.1.0");
    assert_eq!(fetched.permissions, vec!["fs.read"]);
}

#[tokio::test]
async fn test_plugin_get_nonexistent() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let result = store
        .get_plugin(&format!("nope-{}", Uuid::new_v4()))
        .await
        .unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_plugin_upsert() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let name = format!("plugin-{}", Uuid::new_v4());
    let mut p = make_plugin(&name);
    store.register_plugin(&p).await.unwrap();

    p.version = "0.2.0".to_string();
    store.register_plugin(&p).await.expect("upsert plugin");

    let fetched = store.get_plugin(&name).await.unwrap().unwrap();
    assert_eq!(fetched.version, "0.2.0");
}

#[tokio::test]
async fn test_plugin_unregister() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let name = format!("doomed-plugin-{}", Uuid::new_v4());
    let p = make_plugin(&name);
    store.register_plugin(&p).await.unwrap();

    store
        .unregister_plugin(&name)
        .await
        .expect("unregister");

    let result = store.get_plugin(&name).await.unwrap();
    assert!(result.is_none());
}

// ===========================================================================
// RegistryStore tests — tools
// ===========================================================================

#[tokio::test]
async fn test_tool_register_and_get() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let name = format!("tool-{}", Uuid::new_v4());
    let tool = make_tool(&name);

    store.register_tool(&tool).await.expect("register_tool");

    let fetched = store
        .get_tool(&name)
        .await
        .expect("get_tool")
        .expect("tool should exist");

    assert_eq!(fetched.name, name);
    assert!(matches!(fetched.risk_level, RiskLevel::Low));
    match &fetched.binding {
        ToolBinding::Native { handler_name } => assert_eq!(handler_name, "test-plugin"),
        other => panic!("expected Native binding, got {other:?}"),
    }
}

#[tokio::test]
async fn test_tool_builtin_binding() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let name = format!("builtin-{}", Uuid::new_v4());
    let tool = ToolDef {
        name: name.clone(),
        description: "A builtin tool".to_string(),
        binding: ToolBinding::Builtin {
            handler_name: "handle_status".to_string(),
        },
        risk_level: RiskLevel::Low,
        parameters: json!({}),
    };

    store.register_tool(&tool).await.unwrap();
    let fetched = store.get_tool(&name).await.unwrap().unwrap();

    match &fetched.binding {
        ToolBinding::Builtin { handler_name } => assert_eq!(handler_name, "handle_status"),
        other => panic!("expected Builtin binding, got {other:?}"),
    }
}

#[tokio::test]
async fn test_tool_mcp_binding() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let name = format!("mcp-tool-{}", Uuid::new_v4());
    let tool = ToolDef {
        name: name.clone(),
        description: "An MCP tool".to_string(),
        binding: ToolBinding::Mcp {
            server_name: "my-mcp".to_string(),
        },
        risk_level: RiskLevel::Medium,
        parameters: json!({}),
    };

    store.register_tool(&tool).await.unwrap();
    let fetched = store.get_tool(&name).await.unwrap().unwrap();

    assert_eq!(fetched.name, name);
    assert_eq!(fetched.description, "An MCP tool");
    assert!(matches!(fetched.risk_level, RiskLevel::Medium));
}

#[tokio::test]
async fn test_tool_native_high_risk_binding() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let name = format!("native-{}", Uuid::new_v4());
    let tool = ToolDef {
        name: name.clone(),
        description: "A native tool".to_string(),
        binding: ToolBinding::Native {
            handler_name: "exec_shell".to_string(),
        },
        risk_level: RiskLevel::High,
        parameters: json!({}),
    };

    store.register_tool(&tool).await.unwrap();
    let fetched = store.get_tool(&name).await.unwrap().unwrap();

    match &fetched.binding {
        ToolBinding::Native { handler_name } => assert_eq!(handler_name, "exec_shell"),
        other => panic!("expected Native binding, got {other:?}"),
    }
    assert!(matches!(fetched.risk_level, RiskLevel::High));
}

#[tokio::test]
async fn test_tool_get_nonexistent() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let result = store
        .get_tool(&format!("nope-{}", Uuid::new_v4()))
        .await
        .unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_tool_upsert() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let name = format!("tool-{}", Uuid::new_v4());
    let mut tool = make_tool(&name);
    store.register_tool(&tool).await.unwrap();

    tool.description = "Updated description".to_string();
    tool.risk_level = RiskLevel::High;
    store.register_tool(&tool).await.expect("upsert tool");

    let fetched = store.get_tool(&name).await.unwrap().unwrap();
    assert_eq!(fetched.description, "Updated description");
    assert!(matches!(fetched.risk_level, RiskLevel::High));
}

// ===========================================================================
// RegistryStore tests — skills
// ===========================================================================

#[tokio::test]
async fn test_skill_register_and_get() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let id = format!("skill-{}", Uuid::new_v4());
    let skill = make_skill(&id);

    store.register_skill(&skill).await.expect("register_skill");

    let fetched = store
        .get_skill(&id)
        .await
        .expect("get_skill")
        .expect("skill should exist");

    assert_eq!(fetched.id, id);
    assert_eq!(fetched.version, "1.0.0");
    assert_eq!(fetched.system_prompt, "You are a helpful assistant.");
    assert_eq!(fetched.tools, vec!["tool-a"]);
    assert!(!fetched.requires_approval);
    assert!(matches!(fetched.risk_level, RiskLevel::Low));
    assert_eq!(fetched.tags, vec!["test"]);
}

#[tokio::test]
async fn test_skill_get_nonexistent() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let result = store
        .get_skill(&format!("nope-{}", Uuid::new_v4()))
        .await
        .unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_skill_upsert() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let id = format!("skill-{}", Uuid::new_v4());
    let mut skill = make_skill(&id);
    store.register_skill(&skill).await.unwrap();

    skill.version = "2.0.0".to_string();
    skill.risk_level = RiskLevel::High;
    store.register_skill(&skill).await.expect("upsert skill");

    let fetched = store.get_skill(&id).await.unwrap().unwrap();
    assert_eq!(fetched.version, "2.0.0");
    assert!(matches!(fetched.risk_level, RiskLevel::High));
}

// ===========================================================================
// CronStore tests
// ===========================================================================

#[tokio::test]
async fn test_cron_job_save_and_get() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let id = format!("cron-{}", Uuid::new_v4());
    let job = make_cron_job(&id, true);

    store.save_cron_job(&job).await.expect("save_cron_job");

    let fetched = store
        .get_cron_job(&id)
        .await
        .expect("get_cron_job")
        .expect("job should exist");

    assert_eq!(fetched.id, id);
    assert_eq!(fetched.schedule, "0 */5 * * * *");
    assert_eq!(fetched.command, "run-task");
    assert!(fetched.enabled);
    assert_eq!(fetched.created_by, "admin");
}

#[tokio::test]
async fn test_get_cron_job_nonexistent() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let result = store
        .get_cron_job(&format!("nope-{}", Uuid::new_v4()))
        .await
        .unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_list_cron_jobs() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let id1 = format!("cron-{}", Uuid::new_v4());
    let id2 = format!("cron-{}", Uuid::new_v4());

    store
        .save_cron_job(&make_cron_job(&id1, true))
        .await
        .unwrap();
    store
        .save_cron_job(&make_cron_job(&id2, false))
        .await
        .unwrap();

    let all = store.list_cron_jobs(None).await.expect("list all");
    let ids: Vec<&str> = all.iter().map(|j| j.id.as_str()).collect();
    assert!(ids.contains(&id1.as_str()), "id1 missing");
    assert!(ids.contains(&id2.as_str()), "id2 missing");
}

#[tokio::test]
async fn test_delete_cron_job() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let id = format!("cron-del-{}", Uuid::new_v4());
    store
        .save_cron_job(&make_cron_job(&id, true))
        .await
        .unwrap();

    store.delete_cron_job(&id).await.expect("delete");

    let result = store.get_cron_job(&id).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_cron_job_upsert() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let id = format!("cron-u-{}", Uuid::new_v4());
    let mut job = make_cron_job(&id, true);
    store.save_cron_job(&job).await.unwrap();

    job.schedule = "0 0 * * * *".to_string();
    job.enabled = false;
    store.save_cron_job(&job).await.expect("upsert cron job");

    let fetched = store.get_cron_job(&id).await.unwrap().unwrap();
    assert_eq!(fetched.schedule, "0 0 * * * *");
    assert!(!fetched.enabled);
}

#[tokio::test]
async fn test_cron_runs() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let job_id = format!("job-{}", Uuid::new_v4());
    store
        .save_cron_job(&make_cron_job(&job_id, true))
        .await
        .unwrap();

    let run_prefix = Uuid::new_v4().to_string().replace('-', "");
    let run1 = make_cron_run(&format!("run-{run_prefix}-1"), &job_id, CronRunStatus::Completed);
    let run2 = make_cron_run(&format!("run-{run_prefix}-2"), &job_id, CronRunStatus::Running);
    let run3 = make_cron_run(&format!("run-{run_prefix}-3"), &job_id, CronRunStatus::Failed);

    store.save_cron_run(&run1).await.expect("save run 1");
    store.save_cron_run(&run2).await.expect("save run 2");
    store.save_cron_run(&run3).await.expect("save run 3");

    let runs = store.list_cron_runs(&job_id, 10).await.expect("list runs");
    assert_eq!(runs.len(), 3);
}

#[tokio::test]
async fn test_cron_run_list_respects_limit() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let job_id = format!("job-lim-{}", Uuid::new_v4());
    store
        .save_cron_job(&make_cron_job(&job_id, true))
        .await
        .unwrap();

    let prefix = Uuid::new_v4().to_string().replace('-', "");
    for i in 0..5_u32 {
        let run = make_cron_run(&format!("r-{prefix}-{i}"), &job_id, CronRunStatus::Completed);
        store.save_cron_run(&run).await.unwrap();
    }

    let runs = store.list_cron_runs(&job_id, 3).await.expect("list runs");
    assert_eq!(runs.len(), 3);
}

// ===========================================================================
// ApprovalStore tests
// ===========================================================================

#[tokio::test]
async fn test_approval_save_and_get() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let task_id = Uuid::new_v4();
    let approval = make_approval(task_id);
    let approval_id = approval.id;

    store.save_approval(&approval).await.expect("save_approval");

    let fetched = store
        .get_approval(&approval_id)
        .await
        .expect("get_approval")
        .expect("approval should exist");

    assert_eq!(fetched.id, approval_id);
    assert_eq!(fetched.task_id, task_id);
    assert!(matches!(fetched.status, ApprovalStatus::Pending));
    assert_eq!(fetched.context.summary, "Deploy to production");
    assert!(fetched.resolved_by.is_none());
    assert!(fetched.resolved_at.is_none());
}

#[tokio::test]
async fn test_update_approval_status() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let approval = make_approval(Uuid::new_v4());
    let approval_id = approval.id;
    store.save_approval(&approval).await.unwrap();

    let resolver = Actor::user("admin-bob");
    store
        .update_approval_status(&approval_id, ApprovalStatus::Approved, &resolver, None)
        .await
        .expect("update_approval_status");

    let fetched = store.get_approval(&approval_id).await.unwrap().unwrap();
    assert!(matches!(fetched.status, ApprovalStatus::Approved));
    assert!(fetched.resolved_by.is_some());
    assert!(fetched.resolved_at.is_some());
}

#[tokio::test]
async fn test_list_approvals_filter() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let task_id = Uuid::new_v4();
    let a1 = make_approval(task_id);
    let a1_id = a1.id;
    let a2 = make_approval(Uuid::new_v4());

    store.save_approval(&a1).await.unwrap();
    store.save_approval(&a2).await.unwrap();

    // Approve a1
    store
        .update_approval_status(&a1_id, ApprovalStatus::Approved, &Actor::system(), None)
        .await
        .unwrap();

    // Filter by task_id — should return only a2 (still Pending)
    let filter = ApprovalFilter {
        status: Some(ApprovalStatus::Pending),
        approver_role: None,
        task_id: Some(a2.task_id),
        limit: 100,
        offset: 0,
    };
    let results = store.list_approvals(&filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].task_id, a2.task_id);
}

// ===========================================================================
// NodeStore tests
// ===========================================================================

#[tokio::test]
async fn test_node_upsert_and_get() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let node_id = format!("node-{}", Uuid::new_v4());
    let node = make_node(&node_id);

    store.upsert_node(&node).await.expect("upsert_node");

    let fetched = store
        .get_node(&node_id)
        .await
        .expect("get_node")
        .expect("node should exist");

    assert_eq!(fetched.id, node_id);
    assert_eq!(fetched.hostname, format!("{node_id}.local"));
    assert!(matches!(fetched.status, NodeStatus::Online));
    assert_eq!(fetched.capacity.total_memory, 8192);
    assert_eq!(fetched.capacity.available_memory, 4096);
    assert_eq!(fetched.capacity.running_agents, 2);
    assert_eq!(fetched.capacity.max_concurrent, 10);
    assert_eq!(fetched.labels, vec!["gpu"]);
}

#[tokio::test]
async fn test_list_nodes() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let suffix = Uuid::new_v4().to_string().replace('-', "");
    let id_a = format!("node-a-{suffix}");
    let id_b = format!("node-b-{suffix}");

    store.upsert_node(&make_node(&id_a)).await.unwrap();
    store.upsert_node(&make_node(&id_b)).await.unwrap();

    let nodes = store.list_nodes().await.expect("list_nodes");
    let ids: Vec<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    assert!(ids.contains(&id_a.as_str()), "id_a missing");
    assert!(ids.contains(&id_b.as_str()), "id_b missing");
}

#[tokio::test]
async fn test_update_node_status() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let node_id = format!("node-{}", Uuid::new_v4());
    store.upsert_node(&make_node(&node_id)).await.unwrap();

    store
        .update_node_status(&node_id, NodeStatus::Draining)
        .await
        .expect("update_node_status");

    let fetched = store.get_node(&node_id).await.unwrap().unwrap();
    assert!(matches!(fetched.status, NodeStatus::Draining));
}

#[tokio::test]
async fn test_update_node_status_to_offline() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let node_id = format!("node-{}", Uuid::new_v4());
    store.upsert_node(&make_node(&node_id)).await.unwrap();

    store
        .update_node_status(&node_id, NodeStatus::Offline)
        .await
        .expect("update to offline");

    let fetched = store.get_node(&node_id).await.unwrap().unwrap();
    assert!(matches!(fetched.status, NodeStatus::Offline));
}

// ===========================================================================
// McpStore tests
// ===========================================================================

#[tokio::test]
async fn test_mcp_register_and_list() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let name = format!("mcp-{}", Uuid::new_v4());
    let mcp = make_mcp_server(&name);

    store.register_mcp(&mcp).await.expect("register_mcp");

    let servers = store.list_mcp_servers().await.expect("list_mcp_servers");
    let found = servers.iter().find(|s| s.name == name).expect("mcp not found");

    assert_eq!(found.transport, McpTransport::Stdio);
    assert_eq!(found.command.as_deref(), Some("mcp-server"));
    assert_eq!(found.args, vec!["--port", "9000"]);
    assert_eq!(found.description.as_deref(), Some("Test MCP server"));
}

#[tokio::test]
async fn test_mcp_sse_transport() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let name = format!("sse-{}", Uuid::new_v4());
    let mcp = McpServerConfig {
        name: name.clone(),
        description: None,
        transport: McpTransport::Sse,
        url: Some("http://localhost:8080/sse".to_string()),
        command: None,
        args: vec![],
        auth: None,
    };

    store.register_mcp(&mcp).await.unwrap();

    let servers = store.list_mcp_servers().await.unwrap();
    let found = servers.iter().find(|s| s.name == name).expect("sse server not found");
    assert_eq!(found.transport, McpTransport::Sse);
    assert_eq!(found.url.as_deref(), Some("http://localhost:8080/sse"));
}

// ===========================================================================
// RbacStore tests
// ===========================================================================

#[tokio::test]
async fn test_rbac_get_roles_empty() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let actor = format!("nobody-{}", Uuid::new_v4());
    let roles = store.get_roles_for_actor(&actor).await.unwrap();
    assert!(roles.is_empty());
}

#[tokio::test]
async fn test_rbac_bind_and_get() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let actor = format!("alice-{}", Uuid::new_v4());
    store
        .bind_role(&actor, &Role::Admin)
        .await
        .expect("bind admin");
    store
        .bind_role(&actor, &Role::Developer)
        .await
        .expect("bind dev");

    let roles = store
        .get_roles_for_actor(&actor)
        .await
        .expect("get roles");
    assert_eq!(roles.len(), 2);
    assert!(roles.contains(&Role::Admin));
    assert!(roles.contains(&Role::Developer));
}

#[tokio::test]
async fn test_rbac_bind_idempotent() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let actor = format!("alice-{}", Uuid::new_v4());
    store.bind_role(&actor, &Role::Admin).await.unwrap();
    store
        .bind_role(&actor, &Role::Admin)
        .await
        .expect("idempotent bind");

    let roles = store.get_roles_for_actor(&actor).await.unwrap();
    assert_eq!(roles.len(), 1);
}

#[tokio::test]
async fn test_rbac_unbind() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let actor = format!("alice-{}", Uuid::new_v4());
    store.bind_role(&actor, &Role::Admin).await.unwrap();
    store.bind_role(&actor, &Role::Developer).await.unwrap();

    store
        .unbind_role(&actor, &Role::Admin)
        .await
        .expect("unbind");

    let roles = store.get_roles_for_actor(&actor).await.unwrap();
    assert_eq!(roles.len(), 1);
    assert_eq!(roles[0], Role::Developer);
}

#[tokio::test]
async fn test_rbac_unbind_nonexistent_is_noop() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let actor = format!("ghost-{}", Uuid::new_v4());
    store
        .unbind_role(&actor, &Role::Owner)
        .await
        .expect("unbind noop");
}

// ===========================================================================
// PackageStore tests
// ===========================================================================

#[tokio::test]
async fn test_package_register_and_get() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let name = format!("pkg-{}", Uuid::new_v4());
    let pkg = make_package(&name);

    store
        .register_package(&pkg)
        .await
        .expect("register_package");

    let fetched = store
        .get_package(&name)
        .await
        .expect("get_package")
        .expect("package should exist");

    assert_eq!(fetched.name, name);
    assert_eq!(fetched.version, "1.0.0");
    assert_eq!(fetched.installed_by, "admin");
}

#[tokio::test]
async fn test_get_package_nonexistent() {
    let url = skip_unless_postgres!();
    let store = new_store(&url).await;

    let result = store
        .get_package(&format!("nope-{}", Uuid::new_v4()))
        .await
        .unwrap();
    assert!(result.is_none());
}
