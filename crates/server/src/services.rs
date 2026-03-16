//! Service initialization
//!
//! Constructs all DI components (store, bus, auth, audit, tool registry,
//! security guard, flow engine, skill registry, channels, MCP, etc.)
//! and bundles them into [`ServicesResult`].

use std::sync::Arc;

use anyhow::{Context, Result};

use atta_agent::LlmProvider;
use atta_audit::NoopAudit;
use atta_auth::AllowAll;
use atta_bus::InProcBus;
use atta_core::{
    builtin_tools, log_broadcast::LogBroadcast, remote_agent_hub::RemoteAgentHub, ws_hub::WsHub,
    AppState, DefaultToolRegistry, FlowEngine,
};
use atta_memory::MemoryStore;
use atta_security::{
    ApprovalManager, CliApprovalBackend, EstopManager, SecurityGuard, SecurityPolicy,
};
use atta_store::SqliteStore;

use crate::config::AttaConfig;
use crate::home::AttaHome;

/// Services result includes the raw tool registry for late-binding
pub(crate) struct ServicesResult {
    pub(crate) state: AppState,
    pub(crate) raw_tool_registry: Arc<DefaultToolRegistry>,
}

/// Default sub-agent runner for delegation tool.
pub(crate) struct DefaultSubAgentRunner {
    pub(crate) llm: Arc<dyn LlmProvider>,
    pub(crate) tool_registry: Arc<dyn atta_core::ToolRegistry>,
}

#[async_trait::async_trait]
impl atta_tools::delegation::SubAgentRunner for DefaultSubAgentRunner {
    async fn run(
        &self,
        task: &str,
        _model: Option<&str>,
        allowed_tools: Option<&[String]>,
        timeout: std::time::Duration,
        depth: u32,
    ) -> Result<serde_json::Value, atta_types::AttaError> {
        let all_tools = self.tool_registry.list_schemas();
        let tools = match allowed_tools {
            Some(names) => all_tools
                .into_iter()
                .filter(|t| names.iter().any(|n| n == &t.name))
                .collect(),
            None => all_tools,
        };

        let mut context = atta_agent::ConversationContext::new(4096);
        context.set_system(&format!(
            "You are a sub-agent (depth {depth}). Complete the following task concisely."
        ));
        context.add_user(task);

        let mut agent = atta_agent::ReactAgent::new(
            Arc::clone(&self.llm),
            Arc::clone(&self.tool_registry),
            context,
            10,
        )
        .with_tools(tools);

        let result = tokio::time::timeout(timeout, agent.run())
            .await
            .map_err(|_| {
                atta_types::AttaError::Agent(atta_types::AgentError::Timeout(timeout))
            })??;

        Ok(result)
    }
}

/// Build all DI components from config and home directory.
pub(crate) async fn build_services(
    llm: Arc<dyn LlmProvider>,
    home: &AttaHome,
    config: &AttaConfig,
    log_broadcast: Arc<LogBroadcast>,
) -> Result<ServicesResult> {
    let db_path = home.database();
    let db_path_str = db_path.to_string_lossy().to_string();

    let bus: Arc<dyn atta_bus::EventBus> = Arc::new(InProcBus::new());
    let sqlite_store = SqliteStore::open(&db_path_str)
        .await
        .context("failed to open SQLite database")?;
    let audit: Arc<dyn atta_audit::AuditSink> = {
        #[cfg(feature = "enterprise")]
        {
            Arc::new(atta_audit::AuditStore::new(sqlite_store.pool().clone()))
        }
        #[cfg(not(feature = "enterprise"))]
        {
            Arc::new(NoopAudit::new())
        }
    };
    let store: Arc<dyn atta_store::StateStore> = Arc::new(sqlite_store);
    let authz: Arc<dyn atta_auth::Authz> = {
        #[cfg(feature = "enterprise")]
        {
            Arc::new(atta_auth::RBACAuthz::new(store.clone()))
        }
        #[cfg(not(feature = "enterprise"))]
        {
            Arc::new(AllowAll::new())
        }
    };

    let mcp_registry = Arc::new(atta_mcp::McpRegistry::new());

    // Auto-load MCP server configs from $ATTA_HOME/exts/mcp/
    load_mcp_configs(&home.exts_mcp(), &mcp_registry).await;

    // Build memory store
    let memory_store: Arc<dyn MemoryStore> = {
        #[cfg(feature = "enterprise")]
        {
            // Enterprise: Postgres + pgvector backend
            let pg_url = std::env::var("DATABASE_URL")
                .or_else(|_| std::env::var("ATTA_PG_URL"))
                .unwrap_or_else(|_| db_path_str.clone());
            let pg_pool = sqlx::PgPool::connect(&pg_url)
                .await
                .context("failed to connect to PostgreSQL for memory store")?;
            let embedding_provider = build_embedding_provider(home).await;
            Arc::new(
                atta_memory::PgMemoryStore::new(pg_pool, embedding_provider)
                    .await
                    .context("failed to initialize PostgreSQL memory store")?,
            )
        }
        #[cfg(not(feature = "enterprise"))]
        {
            let memory_db_path = home.root().join("memory.db");
            let memory_db_str = format!("sqlite:{}?mode=rwc", memory_db_path.display());
            let memory_pool = sqlx::SqlitePool::connect(&memory_db_str)
                .await
                .context("failed to open memory database")?;
            let embedding_provider = build_embedding_provider(home, memory_pool.clone()).await;
            Arc::new(
                atta_memory::SqliteMemoryStore::new(memory_pool, embedding_provider)
                    .await
                    .context("failed to initialize memory store")?,
            )
        }
    };

    let tool_registry =
        Arc::new(DefaultToolRegistry::new().with_mcp_registry(Arc::clone(&mcp_registry)));
    builtin_tools::register_builtins(&tool_registry);

    for tool in atta_tools::register_all_tools(Some(Arc::clone(&memory_store))) {
        tool_registry.register_native(tool);
    }

    let raw_tool_registry = Arc::clone(&tool_registry);

    let estop_manager = Arc::new(EstopManager::load(home.estop_file()));
    let approval_backend: Arc<dyn atta_security::ApprovalBackend> =
        Arc::new(CliApprovalBackend::new());
    let approval_manager = Arc::new(ApprovalManager::new(approval_backend));

    let security_policy = SecurityPolicy::default();
    let security_policy_shared = Arc::new(tokio::sync::RwLock::new(security_policy.clone()));
    let tool_registry: Arc<dyn atta_core::ToolRegistry> = Arc::new(
        SecurityGuard::new(tool_registry, security_policy)
            .with_approval_manager(approval_manager)
            .with_estop(estop_manager),
    );

    let flow_engine = Arc::new(FlowEngine::new(
        Arc::clone(&store),
        Arc::clone(&bus),
        Arc::clone(&tool_registry),
    ));
    flow_engine
        .load_flows()
        .await
        .context("failed to load flow definitions")?;

    // Load flow definitions from filesystem (lib/ first, then exts/ overrides)
    let lib_flows = flow_engine.load_flows_from_dir(&home.lib_flows()).await?;
    let exts_flows = flow_engine.load_flows_from_dir(&home.exts_flows()).await?;
    if lib_flows + exts_flows > 0 {
        tracing::info!(
            lib_flows,
            exts_flows,
            "loaded flow definitions from filesystem"
        );
    }

    let ws_hub = Arc::new(WsHub::new());
    let remote_agent_hub = Arc::new(RemoteAgentHub::new());

    // Initialize channel registry
    let channel_registry = Arc::new(atta_channel::ChannelRegistry::new());

    // Load skills from lib/ and exts/ directories
    let mut skill_registry = atta_core::skill_engine::SkillRegistry::new();
    skill_registry.add_skill_dir(home.lib_skills());
    skill_registry.add_skill_dir(home.exts_skills());

    // Sync community skills
    let sync_config = atta_core::skill_engine::SkillSyncConfig::new(
        home.cache().join("open-skills"),
        config.skill_sync.repo_url.clone(),
        config.skill_sync.interval_secs,
        config.skill_sync.enabled,
    );
    let skill_sync = atta_core::skill_engine::SkillSync::new(sync_config);

    if skill_sync.needs_sync() {
        match skill_sync.sync().await {
            Ok(sync_dir) => {
                tracing::info!(dir = %sync_dir.display(), "community skills synced");
                skill_registry.add_skill_dir(sync_dir);
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to sync community skills");
                skill_registry.add_skill_dir(skill_sync.local_dir().clone());
            }
        }
    } else if skill_sync.enabled() {
        skill_registry.add_skill_dir(skill_sync.local_dir().clone());
    }

    let skill_registry = Arc::new(skill_registry);
    if let Err(e) = skill_registry.load_all().await {
        tracing::warn!(error = %e, "failed to load skills from disk");
    }
    atta_core::skill_engine::register_preloaded(&skill_registry);

    // Sync in-memory registries → StateStore so HTTP handlers see the data
    sync_skills_to_store(&skill_registry, &store).await;
    sync_flows_to_store(&flow_engine, &store).await;

    // Resolve WebUI directory
    let webui_dir = {
        let dir = home.lib_webui();
        if dir.join("index.html").exists() {
            Some(dir)
        } else {
            None
        }
    };

    Ok(ServicesResult {
        state: AppState {
            store,
            bus,
            authz,
            audit,
            flow_engine,
            tool_registry,
            llm,
            ws_hub,
            skill_registry,
            mcp_registry,
            channel_registry,
            memory_store,
            security_policy: security_policy_shared,
            webui_dir,
            auth_mode: build_auth_mode(&config.auth),
            cron_engine: None,
            agent_registry: None,
            log_broadcast,
            remote_agent_hub,
            session_router: None,
            access_control: None,
        },
        raw_tool_registry,
    })
}

/// Build the embedding provider with graceful fallback.
///
/// Desktop: tries FastEmbed local model → wraps with SQLite embedding cache.
/// On failure (e.g. model download fails), falls back to NoopEmbeddingProvider
/// so the server still starts (FTS-only mode, no semantic search).
#[cfg(not(feature = "enterprise"))]
async fn build_embedding_provider(
    home: &AttaHome,
    cache_pool: sqlx::SqlitePool,
) -> Box<dyn atta_memory::EmbeddingProvider> {
    let cache_dir = home.models().join("fastembed");
    // FastEmbed init is synchronous and may block on model download.
    // Wrap in spawn_blocking with a timeout so a slow/unreachable network
    // doesn't hang the server startup.
    let cd = cache_dir.clone();
    let fastembed_result = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::task::spawn_blocking(move || {
            atta_memory::FastEmbedProvider::default_model_with_cache(cd)
        }),
    )
    .await;
    let fastembed = match fastembed_result {
        Ok(Ok(Ok(provider))) => {
            tracing::info!("FastEmbed embedding provider initialized (AllMiniLML6V2, 384d)");
            provider
        }
        Ok(Ok(Err(e))) => {
            tracing::warn!(
                error = %e,
                "FastEmbed initialization failed (model download may have failed), \
                 falling back to FTS-only mode. Semantic search will be unavailable."
            );
            return Box::new(atta_memory::NoopEmbeddingProvider);
        }
        Ok(Err(e)) => {
            tracing::warn!(
                error = %e,
                "FastEmbed task panicked, falling back to FTS-only mode."
            );
            return Box::new(atta_memory::NoopEmbeddingProvider);
        }
        Err(_) => {
            tracing::warn!(
                "FastEmbed initialization timed out (30s) — model download may be stuck. \
                 Falling back to FTS-only mode. Semantic search will be unavailable. \
                 To fix: download the model manually or check network connectivity."
            );
            return Box::new(atta_memory::NoopEmbeddingProvider);
        }
    };

    // Wrap with SQLite embedding cache for faster repeat lookups
    match atta_memory::CachedEmbeddingProvider::new(Box::new(fastembed), cache_pool, 10_000).await {
        Ok(cached) => Box::new(cached),
        Err(e) => {
            tracing::warn!(error = %e, "embedding cache init failed, using uncached FastEmbed");
            match atta_memory::FastEmbedProvider::default_model_with_cache(cache_dir) {
                Ok(p) => Box::new(p),
                Err(_) => Box::new(atta_memory::NoopEmbeddingProvider),
            }
        }
    }
}

/// Enterprise embedding provider (FTS-only for now).
///
/// Enterprise deployments typically use an external embedding API.
/// Local FastEmbed is not included in the enterprise feature set.
#[cfg(feature = "enterprise")]
async fn build_embedding_provider(_home: &AttaHome) -> Box<dyn atta_memory::EmbeddingProvider> {
    tracing::info!("Enterprise mode: using FTS-only memory search (no local embedding provider)");
    Box::new(atta_memory::NoopEmbeddingProvider)
}

/// Sync in-memory SkillRegistry → StateStore.
///
/// For each skill in the registry, if it doesn't exist in the DB (or has a
/// different version), call `store.register_skill()` to upsert it.
async fn sync_skills_to_store(
    skill_registry: &atta_core::skill_engine::SkillRegistry,
    store: &Arc<dyn atta_store::StateStore>,
) {
    let mem_skills = skill_registry.list();
    let db_skills = match store.list_skills().await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "failed to list skills from store, skipping sync");
            return;
        }
    };

    let db_map: std::collections::HashMap<String, String> = db_skills
        .into_iter()
        .map(|s| (s.id.clone(), s.version.clone()))
        .collect();

    let mut synced = 0usize;
    for skill in &mem_skills {
        let needs_sync = match db_map.get(&skill.id) {
            None => true,
            Some(v) => v != &skill.version,
        };
        if needs_sync {
            if let Err(e) = store.register_skill(skill).await {
                tracing::warn!(skill_id = %skill.id, error = %e, "failed to sync skill to store");
            } else {
                synced += 1;
            }
        }
    }

    if synced > 0 {
        tracing::info!(synced, total = mem_skills.len(), "synced skills to store");
    }
}

/// Sync in-memory FlowEngine → StateStore.
///
/// For each flow def in the engine, if it doesn't exist in the DB (or has a
/// different version), call `store.save_flow_def()` to upsert it.
async fn sync_flows_to_store(
    flow_engine: &FlowEngine,
    store: &Arc<dyn atta_store::StateStore>,
) {
    let mem_flows = match flow_engine.list_flow_defs() {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!(error = %e, "failed to list flow defs from engine, skipping sync");
            return;
        }
    };
    let db_flows = match store.list_flow_defs().await {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!(error = %e, "failed to list flow defs from store, skipping sync");
            return;
        }
    };

    let db_map: std::collections::HashMap<String, String> = db_flows
        .into_iter()
        .map(|f| (f.id.clone(), f.version.clone()))
        .collect();

    let mut synced = 0usize;
    for flow in &mem_flows {
        let needs_sync = match db_map.get(&flow.id) {
            None => true,
            Some(v) => v != &flow.version,
        };
        if needs_sync {
            if let Err(e) = store.save_flow_def(flow).await {
                tracing::warn!(flow_id = %flow.id, error = %e, "failed to sync flow to store");
            } else {
                synced += 1;
            }
        }
    }

    if synced > 0 {
        tracing::info!(synced, total = mem_flows.len(), "synced flow defs to store");
    }
}

/// Build AuthMode from configuration.
fn build_auth_mode(auth: &crate::config::AuthConfig) -> atta_core::middleware::AuthMode {
    match auth.mode.as_str() {
        "oidc" => {
            let issuer = auth.issuer.clone().unwrap_or_default();
            let audience = auth.audience.clone().unwrap_or_default();
            let secret = auth.secret.clone().unwrap_or_default();
            if issuer.is_empty() || audience.is_empty() || secret.is_empty() {
                tracing::warn!("OIDC auth mode requires issuer, audience, and secret — falling back to NoAuth");
                atta_core::middleware::AuthMode::NoAuth
            } else {
                tracing::info!("auth mode: OIDC Bearer");
                atta_core::middleware::AuthMode::OidcBearer { issuer, audience, secret }
            }
        }
        "api_key" => {
            tracing::info!("auth mode: API Key");
            atta_core::middleware::AuthMode::ApiKey
        }
        _ => {
            tracing::info!("auth mode: NoAuth (desktop)");
            atta_core::middleware::AuthMode::NoAuth
        }
    }
}

/// Load MCP server configs from `$ATTA_HOME/exts/mcp/` and auto-connect them.
///
/// Reads `.json` / `.yaml` / `.yml` files, parses each as `McpServerConfig`,
/// and spawns/connects the MCP server.
async fn load_mcp_configs(mcp_dir: &std::path::Path, mcp_registry: &atta_mcp::McpRegistry) {
    if !mcp_dir.exists() {
        return;
    }

    let mut read_dir = match tokio::fs::read_dir(mcp_dir).await {
        Ok(rd) => rd,
        Err(e) => {
            tracing::warn!(error = %e, dir = %mcp_dir.display(), "failed to read MCP config directory");
            return;
        }
    };

    while let Ok(Some(entry)) = read_dir.next_entry().await {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        let content = match tokio::fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(_) => continue,
        };

        let config: atta_types::McpServerConfig = match ext {
            "json" => match serde_json::from_str(&content) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "failed to parse MCP config");
                    continue;
                }
            },
            "yaml" | "yml" => match serde_yml::from_str(&content) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "failed to parse MCP config");
                    continue;
                }
            },
            _ => continue,
        };

        match config.transport {
            atta_types::McpTransport::Stdio => {
                let command = match &config.command {
                    Some(cmd) => cmd.clone(),
                    None => {
                        tracing::warn!(
                            server = %config.name,
                            "stdio MCP server missing 'command' field, skipping"
                        );
                        continue;
                    }
                };
                match atta_mcp::StdioMcpClient::spawn(&config.name, &command, &config.args).await {
                    Ok(client) => {
                        tracing::info!(server = %config.name, "connected stdio MCP server from config");
                        mcp_registry.add(&config.name, Arc::new(client)).await;
                    }
                    Err(e) => {
                        tracing::warn!(
                            server = %config.name,
                            error = %e,
                            "failed to spawn MCP server, skipping"
                        );
                    }
                }
            }
            atta_types::McpTransport::Sse => {
                let url = match &config.url {
                    Some(u) => u.clone(),
                    None => {
                        tracing::warn!(
                            server = %config.name,
                            "SSE MCP server missing 'url' field, skipping"
                        );
                        continue;
                    }
                };
                let client = atta_mcp::SseMcpClient::new(&config.name, url);
                tracing::info!(server = %config.name, "registered SSE MCP server from config");
                mcp_registry.add(&config.name, Arc::new(client)).await;
            }
        }
    }
}
