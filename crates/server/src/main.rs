//! AttaOS Server — 核心服务守护进程
//!
//! 承载 HTTP API + WebUI (filesystem) + Agent 执行引擎。
//! 所有重量级依赖集中在此 crate。

mod config;
mod home;
mod llm;
mod services;

use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

use atta_core::{
    api_router,
    coordinator::CoreCoordinator,
    log_broadcast::{BroadcastLayer, LogBroadcast},
};

use config::AttaConfig;
use home::AttaHome;
use llm::build_llm_provider;
use services::{DefaultSubAgentRunner, ServicesResult};

/// AttaOS Server — AI Agent 操作系统核心服务
#[derive(Debug, Parser)]
#[command(
    name = "attaos",
    version,
    about = "AttaOS Server — AI Agent Operating System"
)]
struct Cli {
    /// 运行模式
    #[arg(long, default_value = "desktop")]
    mode: String,

    /// HTTP 监听端口（也可通过 ATTA_PORT 环境变量设置）
    #[arg(long)]
    port: Option<u16>,

    /// ATTA_HOME 目录（也可通过 ATTA_HOME 环境变量设置，默认 ~/.atta）
    #[arg(long)]
    home: Option<String>,

    /// 跳过启动时的更新检查
    #[arg(long)]
    skip_update_check: bool,
}

/// Write PID file
fn write_pid_file(home: &AttaHome) -> Result<()> {
    let pid_file = home.pid_file();
    let pid = std::process::id();
    std::fs::write(&pid_file, pid.to_string())
        .with_context(|| format!("failed to write PID file: {}", pid_file.display()))?;
    tracing::info!(pid = pid, path = %pid_file.display(), "wrote PID file");
    Ok(())
}

/// Remove PID file on shutdown
fn remove_pid_file(home: &AttaHome) {
    let pid_file = home.pid_file();
    if pid_file.exists() {
        let _ = std::fs::remove_file(&pid_file);
        tracing::info!(path = %pid_file.display(), "removed PID file");
    }
}

/// Load API keys from `$ATTA_HOME/etc/keys.env` into the process environment.
///
/// Parses lines as `KEY=VALUE` pairs. Skips empty lines and `#` comments.
/// Does NOT override already-set environment variables.
///
/// # Security
///
/// Keys are stored in process environment variables for the lifetime of the
/// server. This is intentional — the server needs them for LLM API calls.
/// Child process spawning (e.g. `spawn_updater`) MUST filter env vars to
/// prevent secret leakage. See `autostart::sanitize_env()` for the shell's
/// child-process env filtering.
fn load_keys_env(home: &AttaHome) {
    let path = home.keys_env();
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return, // File doesn't exist — no-op
    };

    let mut count = 0;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim();
            // Don't override existing env vars
            if std::env::var(key).is_err() {
                std::env::set_var(key, value);
                count += 1;
            }
        }
    }

    if count > 0 {
        tracing::info!(path = %path.display(), count, "loaded keys from env file");
    }
}

/// 等待 Ctrl+C 信号
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install Ctrl+C handler");
    tracing::info!("received Ctrl+C, initiating graceful shutdown...");
}

/// 启动 AttaOS 服务
async fn run_server(
    mode: &str,
    port: u16,
    home: &AttaHome,
    config: &AttaConfig,
    log_broadcast: Arc<LogBroadcast>,
) -> Result<()> {
    tracing::info!(
        mode = %mode,
        port = port,
        home = %home.root().display(),
        "starting AttaOS"
    );

    let llm = build_llm_provider()?;
    let ServicesResult {
        mut state,
        raw_tool_registry,
    } = services::build_services(Arc::clone(&llm), home, config, log_broadcast).await?;

    // Late-bind DelegationTool with real SubAgentRunner
    let runner = Arc::new(DefaultSubAgentRunner {
        llm: Arc::clone(&llm),
        tool_registry: Arc::clone(&state.tool_registry),
    });
    let delegation_tool = atta_tools::delegation::DelegationTool::default().with_runner(runner);
    raw_tool_registry.replace_native(Arc::new(delegation_tool));

    // Initialize CronEngine and late-bind cron tools
    let cron_engine = Arc::new(atta_core::cron_engine::CronEngine::new(
        Arc::clone(&state.store),
        Arc::clone(&state.bus),
    ));
    cron_engine.start();
    state.cron_engine = Some(Arc::clone(&cron_engine));
    let cron_scheduler: Arc<dyn atta_types::CronScheduler> = Arc::clone(&cron_engine) as _;

    // Replace cron tools with scheduler-backed versions
    raw_tool_registry.replace_native(Arc::new(
        atta_tools::cron::CronTool::new().with_scheduler(Arc::clone(&cron_scheduler)),
    ));
    raw_tool_registry.replace_native(Arc::new(
        atta_tools::cron_list::CronListTool::new().with_scheduler(Arc::clone(&cron_scheduler)),
    ));
    raw_tool_registry.replace_native(Arc::new(
        atta_tools::cron_remove::CronRemoveTool::new().with_scheduler(Arc::clone(&cron_scheduler)),
    ));
    raw_tool_registry.replace_native(Arc::new(
        atta_tools::cron_update::CronUpdateTool::new().with_scheduler(Arc::clone(&cron_scheduler)),
    ));
    raw_tool_registry.replace_native(Arc::new(
        atta_tools::cron_run::CronRunTool::new().with_scheduler(Arc::clone(&cron_scheduler)),
    ));
    raw_tool_registry.replace_native(Arc::new(
        atta_tools::cron_runs::CronRunsTool::new().with_scheduler(Arc::clone(&cron_scheduler)),
    ));

    // Initialize AgentRegistry and late-bind subagent tools
    let agent_registry = Arc::new(atta_core::agent_registry::AgentRegistry::new());
    let agent_reg: Arc<dyn atta_types::SubAgentRegistry> = Arc::clone(&agent_registry) as _;

    raw_tool_registry.replace_native(Arc::new(
        atta_tools::subagent_spawn::SubagentSpawnTool::new(Some(Arc::clone(&agent_reg))),
    ));
    raw_tool_registry.replace_native(Arc::new(atta_tools::subagent_list::SubagentListTool::new(
        Some(Arc::clone(&agent_reg)),
    )));
    raw_tool_registry.replace_native(Arc::new(
        atta_tools::subagent_manage::SubagentManageTool::new(Some(Arc::clone(&agent_reg))),
    ));

    // Inject AgentRegistry into AppState for API handlers
    state.agent_registry = Some(Arc::clone(&agent_registry));

    // Late-bind StartFlowTool with FlowEngine as FlowRunner
    let flow_runner: Arc<dyn atta_types::FlowRunner> = Arc::clone(&state.flow_engine) as _;
    raw_tool_registry.replace_native(Arc::new(
        atta_tools::start_flow::StartFlowTool::new().with_runner(flow_runner),
    ));

    // Initialize channels from config
    let mut channel_instances: Vec<Arc<dyn atta_channel::Channel>> = Vec::new();
    for ch_config in config.channels.iter().filter(|c| c.enabled) {
        match atta_channel::create_channel(ch_config) {
            Ok(channel) => {
                tracing::info!(
                    channel_type = %ch_config.channel_type,
                    name = %channel.name(),
                    "channel created"
                );
                state
                    .channel_registry
                    .insert(channel.name().to_string(), Arc::clone(&channel))
                    .await;
                channel_instances.push(channel);
            }
            Err(e) => {
                tracing::warn!(
                    channel_type = %ch_config.channel_type,
                    error = %e,
                    "failed to create channel, skipping"
                );
            }
        }
    }

    // Start CoreCoordinator event loop
    let coordinator = Arc::new(CoreCoordinator::new(
        Arc::clone(&state.bus),
        Arc::clone(&state.store),
        Arc::clone(&state.flow_engine),
        Arc::clone(&state.tool_registry),
        Arc::clone(&state.llm),
        Arc::clone(&state.ws_hub),
    ));
    coordinator
        .start()
        .await
        .context("failed to start CoreCoordinator")?;

    // Build channel message processing pipeline
    let session_router = Arc::new(atta_channel::SessionRouter::default());
    let access_control = Arc::new(atta_channel::AccessControlPolicy::new());

    // Inject into AppState so HTTP handlers can access them
    state.session_router = Some(Arc::clone(&session_router));
    state.access_control = Some(Arc::clone(&access_control));

    // Spawn channel listeners if any channels are configured
    let channel_cancel = tokio_util::sync::CancellationToken::new();
    if !channel_instances.is_empty() {
        let channel_handler = Arc::new(atta_core::channel_handler::AgentChannelHandler::new(
            Arc::clone(&state.llm),
            Arc::clone(&state.tool_registry),
            Arc::clone(&state.store),
        ));

        // Build policy chain: dedup → mention filter → access control → send policy
        let policy_chain = atta_channel::PolicyChain::default_chain()
            .add(Arc::clone(&access_control) as Arc<dyn atta_channel::MessagePolicy>);
        let message_store = Arc::new(atta_channel::InMemoryMessageStore::default());

        let ctx = atta_channel::ChannelRuntimeContext {
            channels: channel_instances,
            handler: channel_handler,
            cancel: channel_cancel.clone(),
            policy: Some(Arc::new(policy_chain)),
            session_router: Some(session_router),
            debounce_config: Some((std::time::Duration::from_millis(1500), 4000)),
            message_store: Some(message_store),
            heartbeat_interval: Some(std::time::Duration::from_secs(300)),
        };
        tokio::spawn(async move {
            if let Err(e) = atta_channel::start_channels(ctx).await {
                tracing::error!(error = %e, "channel runtime exited with error");
            }
        });
        tracing::info!("channel runtime started with policy pipeline");
    }

    // Write PID file
    write_pid_file(home)?;

    let bind = &config.server.bind;
    let addr = format!("{bind}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .context(format!("failed to bind to {addr}"))?;

    let app = api_router(state);

    tracing::info!(%addr, mode = %mode, "AttaOS is running — http://{addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;

    // Cleanup on shutdown — cancel channel listeners
    channel_cancel.cancel();
    // Allow channel tasks a brief window to drain before exit
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    remove_pid_file(home);

    tracing::info!("AttaOS server shut down gracefully");
    Ok(())
}

/// 检查 GitHub Releases 是否有新版本
async fn check_for_update(current: &str) -> Result<Option<String>> {
    let current_ver =
        semver::Version::parse(current).context("failed to parse current version as semver")?;

    let client = reqwest::Client::builder()
        .user_agent("attaos-updater")
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let resp = client
        .get("https://api.github.com/repos/anthropics/attaos/releases/latest")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("GitHub API returned {}", resp.status());
    }

    let body: serde_json::Value = resp.json().await?;
    let tag = body["tag_name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing tag_name in release"))?;

    let version_str = tag.strip_prefix('v').unwrap_or(tag);
    let latest_ver =
        semver::Version::parse(version_str).context("failed to parse latest version")?;

    if latest_ver > current_ver {
        Ok(Some(latest_ver.to_string()))
    } else {
        Ok(None)
    }
}

/// 启动 updater 应用
fn spawn_updater(port: u16) -> Result<()> {
    let exe_dir = std::env::current_exe()
        .context("failed to get current exe")?
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();

    let bin_name = if cfg!(windows) {
        "atta-updater.exe"
    } else {
        "atta-updater"
    };

    let bin_path = exe_dir.join(bin_name);
    // Security: clear inherited env and only pass safe variables to prevent
    // API keys loaded from keys.env from leaking to child processes.
    let safe_env: Vec<(String, String)> = std::env::vars()
        .filter(|(k, _)| {
            let upper = k.to_uppercase();
            // Only pass through system essentials — no secrets
            matches!(
                upper.as_str(),
                "HOME" | "USER" | "USERNAME" | "PATH" | "LANG" | "TERM"
                    | "TMPDIR" | "TMP" | "TEMP" | "SHELL"
            ) || upper.starts_with("LC_")
              || upper.starts_with("XDG_")
              || upper == "ATTA_HOME"
              || upper == "ATTA_LOG"
              || upper == "ATTA_LOG_LEVEL"
              || upper == "ATTA_DATA_DIR"
        })
        .collect();
    std::process::Command::new(&bin_path)
        .env_clear()
        .envs(safe_env)
        .env("ATTA_PORT", port.to_string())
        .spawn()
        .context(format!("failed to spawn {}", bin_path.display()))?;

    tracing::info!(path = %bin_path.display(), "updater spawned");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Resolve ATTA_HOME
    let home = AttaHome::resolve(cli.home.as_deref());

    // Ensure directory structure exists
    home.ensure_dirs()
        .context("failed to create ATTA_HOME directory structure")?;

    // Seed built-in skills/flows from project source tree (dev mode)
    home.seed_builtins()
        .context("failed to seed built-in definitions")?;

    // Load API keys from $ATTA_HOME/etc/keys.env (before config, before logging)
    load_keys_env(&home);

    // Load config from $ATTA_HOME/etc/attaos.yaml
    let config = AttaConfig::load(&home.config_file()).context("failed to load configuration")?;

    // Initialize logging with config level + broadcast layer for log streaming
    let log_level = &config.log.level;
    let log_broadcast = Arc::new(LogBroadcast::new());
    let broadcast_layer = BroadcastLayer::new((*log_broadcast).clone());

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer().with_filter(
                EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| EnvFilter::new(log_level)),
            ),
        )
        .with(broadcast_layer)
        .init();

    tracing::info!(home = %home.root().display(), "ATTA_HOME resolved");

    // Resolve port: CLI arg > env var > config > default
    let port = cli
        .port
        .or_else(|| std::env::var("ATTA_PORT").ok().and_then(|p| p.parse().ok()))
        .unwrap_or(config.server.port);

    // Optional update check
    if !cli.skip_update_check {
        let current = env!("CARGO_PKG_VERSION");
        match check_for_update(current).await {
            Ok(Some(ver)) => {
                tracing::info!(current = current, latest = %ver, "update available — launching updater");
                spawn_updater(port)?;
                return Ok(());
            }
            Ok(None) => {
                tracing::info!("already on latest version");
            }
            Err(e) => {
                tracing::warn!(error = %e, "update check failed, continuing with server");
            }
        }
    }

    run_server(&cli.mode, port, &home, &config, log_broadcast).await
}
