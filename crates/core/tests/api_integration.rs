//! Integration tests: HTTP API layer
//!
//! Builds an axum Router with real InProcBus + SqliteStore (temp file)
//! and tests REST endpoints via tower::ServiceExt.

use std::sync::Arc;

use axum::body::Body;
use http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

use atta_audit::NoopAudit;
use atta_auth::AllowAll;
use atta_bus::InProcBus;
use atta_core::skill_engine::SkillRegistry;
use atta_core::ws_hub::WsHub;
use atta_core::{api_router, AppState, DefaultToolRegistry, FlowEngine};
use atta_store::SqliteStore;

fn temp_db_path() -> String {
    format!(
        "/tmp/atta_test_{}.db",
        uuid::Uuid::new_v4().to_string().replace('-', "")
    )
}

/// Stub LLM provider for tests
struct StubLlm;

#[async_trait::async_trait]
impl atta_agent::LlmProvider for StubLlm {
    async fn chat(
        &self,
        _messages: &[atta_agent::Message],
        _tools: &[atta_types::ToolSchema],
    ) -> Result<atta_agent::LlmResponse, atta_types::AttaError> {
        Ok(atta_agent::LlmResponse::Message("stub".into()))
    }

    fn model_info(&self) -> atta_agent::ModelInfo {
        atta_agent::ModelInfo {
            model_id: "stub".into(),
            context_window: 128_000,
            supports_tools: false,
            provider: "stub".into(),
            supports_streaming: false,
        }
    }
}

async fn build_app() -> axum::Router {
    let store = Arc::new(SqliteStore::open(&temp_db_path()).await.unwrap());
    let bus: Arc<dyn atta_bus::EventBus> = Arc::new(InProcBus::new());
    let tool_reg: Arc<dyn atta_types::ToolRegistry> = Arc::new(DefaultToolRegistry::new());
    let flow_engine = Arc::new(FlowEngine::new(
        store.clone() as Arc<dyn atta_store::StateStore>,
        bus.clone(),
        tool_reg.clone(),
    ));

    let state = AppState {
        store: store as Arc<dyn atta_store::StateStore>,
        bus,
        authz: Arc::new(AllowAll::new()),
        audit: Arc::new(NoopAudit::new()),
        flow_engine,
        tool_registry: tool_reg,
        llm: Arc::new(StubLlm),
        ws_hub: Arc::new(WsHub::new()),
        skill_registry: Arc::new(SkillRegistry::new()),
        mcp_registry: Arc::new(atta_mcp::McpRegistry::new()),
        channel_registry: Arc::new(atta_channel::ChannelRegistry::new()),
        memory_store: Arc::new(atta_memory::NoopMemoryStore),
        security_policy: Arc::new(tokio::sync::RwLock::new(
            atta_security::SecurityPolicy::default(),
        )),
        webui_dir: None,
        auth_mode: atta_core::middleware::AuthMode::default(),
        agent_registry: None,
        cron_engine: None,
        log_broadcast: Arc::new(atta_core::log_broadcast::LogBroadcast::new()),
        remote_agent_hub: Arc::new(atta_core::remote_agent_hub::RemoteAgentHub::new()),
            session_router: None,
            access_control: None,
    };

    api_router(state)
}

async fn get_json(app: &axum::Router, uri: &str) -> (StatusCode, Value) {
    let resp = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, json)
}

async fn post_json(app: &axum::Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

// ── Health & System ──

#[tokio::test]
async fn test_health_endpoint() {
    let app = build_app().await;
    let (status, body) = get_json(&app, "/api/v1/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn test_system_health_endpoint() {
    let app = build_app().await;
    let (status, body) = get_json(&app, "/api/v1/system/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn test_system_config_endpoint() {
    let app = build_app().await;
    let (status, body) = get_json(&app, "/api/v1/system/config").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["mode"].is_string());
    assert!(body["version"].is_string());
}

// ── Flows ──

#[tokio::test]
async fn test_list_flows_empty() {
    let app = build_app().await;
    let (status, body) = get_json(&app, "/api/v1/flows").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_array());
    assert_eq!(body.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_create_flow_returns_created() {
    let app = build_app().await;
    let flow = json!({
        "id": "api_test_flow",
        "version": "1.0",
        "initial_state": "start",
        "states": {
            "start": {
                "type": "start",
                "transitions": [{"to": "end", "auto": true}]
            },
            "end": {
                "type": "end",
                "transitions": []
            }
        }
    });
    let (status, body) = post_json(&app, "/api/v1/flows", flow).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["id"], "api_test_flow");
    assert_eq!(body["version"], "1.0");
}

#[tokio::test]
async fn test_create_flow_appears_in_list() {
    let app = build_app().await;
    let flow = json!({
        "id": "listed_flow",
        "version": "2.0",
        "initial_state": "s",
        "states": {
            "s": { "type": "start", "transitions": [{"to": "e", "auto": true}] },
            "e": { "type": "end", "transitions": [] }
        }
    });
    post_json(&app, "/api/v1/flows", flow).await;

    let (status, body) = get_json(&app, "/api/v1/flows").await;
    assert_eq!(status, StatusCode::OK);
    let flows = body.as_array().unwrap();
    assert!(flows.iter().any(|f| f["id"] == "listed_flow"));
}

#[tokio::test]
async fn test_create_invalid_flow_rejected() {
    let app = build_app().await;
    // Missing end state
    let flow = json!({
        "id": "bad_flow",
        "version": "1.0",
        "initial_state": "start",
        "states": {
            "start": { "type": "start", "transitions": [] }
        }
    });
    let (status, _) = post_json(&app, "/api/v1/flows", flow).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ── Tasks ──

#[tokio::test]
async fn test_list_tasks_empty() {
    let app = build_app().await;
    let (status, body) = get_json(&app, "/api/v1/tasks").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_array());
}

#[tokio::test]
async fn test_create_task_returns_task() {
    let app = build_app().await;

    // Register a flow first
    let flow = json!({
        "id": "task_flow",
        "version": "1.0",
        "initial_state": "start",
        "states": {
            "start": { "type": "start", "transitions": [{"to": "end", "auto": true}] },
            "end": { "type": "end", "transitions": [] }
        }
    });
    let (status, _) = post_json(&app, "/api/v1/flows", flow).await;
    assert_eq!(status, StatusCode::CREATED);

    // Create a task
    let (status, body) = post_json(
        &app,
        "/api/v1/tasks",
        json!({"flow_id": "task_flow", "input": {"msg": "hello"}}),
    )
    .await;
    assert!(
        status == StatusCode::CREATED || status == StatusCode::OK,
        "create task: {status} {body}"
    );
    assert!(body["id"].is_string(), "task should have id: {body}");
}

#[tokio::test]
async fn test_created_task_appears_in_list() {
    let app = build_app().await;

    let flow = json!({
        "id": "list_task_flow",
        "version": "1.0",
        "initial_state": "s",
        "states": {
            "s": { "type": "start", "transitions": [{"to": "e", "auto": true}] },
            "e": { "type": "end", "transitions": [] }
        }
    });
    post_json(&app, "/api/v1/flows", flow).await;
    post_json(
        &app,
        "/api/v1/tasks",
        json!({"flow_id": "list_task_flow", "input": {}}),
    )
    .await;

    let (status, body) = get_json(&app, "/api/v1/tasks").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.as_array().unwrap().is_empty());
}

// ── Tools ──

#[tokio::test]
async fn test_list_tools() {
    let app = build_app().await;
    let (status, body) = get_json(&app, "/api/v1/tools").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_array());
}

// ── Skills ──

#[tokio::test]
async fn test_list_skills_empty() {
    let app = build_app().await;
    let (status, body) = get_json(&app, "/api/v1/skills").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_array());
}

// ── Channels ──

#[tokio::test]
async fn test_list_channels() {
    let app = build_app().await;
    let (status, body) = get_json(&app, "/api/v1/channels").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["data"].is_array());
    // Dynamic list — empty when no channels are configured
    assert!(body["data"].as_array().unwrap().is_empty());
}

// ── Security Policy ──

#[tokio::test]
async fn test_security_policy() {
    let app = build_app().await;
    let (status, body) = get_json(&app, "/api/v1/security/policy").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["data"].is_object());
    assert!(body["data"]["autonomy_level"].is_string());
}
