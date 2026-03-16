//! attacli — AttaOS 轻量 CLI 客户端
//!
//! 通过 HTTP/SSE 与 attaos 服务通信。
//! 首次使用时自动启动 attaos 服务。

mod autostart;
mod client;

use anyhow::Result;
use clap::{Parser, Subcommand};
use futures::StreamExt;

use atta_types::ChatRequest;
use client::AttaClient;

/// attacli — AttaOS CLI Client
#[derive(Debug, Parser)]
#[command(
    name = "attacli",
    version,
    about = "AttaOS CLI — lightweight client for AttaOS server"
)]
struct Cli {
    /// 服务 URL（也可通过 ATTA_URL 环境变量设置）
    #[arg(long)]
    url: Option<String>,

    /// ATTA_HOME 目录（也可通过 ATTA_HOME 环境变量设置，默认 ~/.atta）
    #[arg(long)]
    home: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 交互式聊天
    Chat {
        /// 可选的 Skill ID
        #[arg(long)]
        skill: Option<String>,
    },

    /// 查看服务状态
    Status,

    /// 查看系统配置
    Config,

    /// 查看系统指标
    Metrics,

    /// 任务管理
    Task {
        #[command(subcommand)]
        action: TaskAction,
    },

    /// Flow 管理
    Flow {
        #[command(subcommand)]
        action: FlowAction,
    },

    /// Skill 管理
    Skill {
        #[command(subcommand)]
        action: SkillAction,
    },

    /// Tool 管理
    Tool {
        #[command(subcommand)]
        action: ToolAction,
    },

    /// MCP Server 管理
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },

    /// 审批管理
    Approval {
        #[command(subcommand)]
        action: ApprovalAction,
    },

    /// 节点管理
    Node {
        #[command(subcommand)]
        action: NodeAction,
    },

    /// Channel 管理
    Channel {
        #[command(subcommand)]
        action: ChannelAction,
    },

    /// 安全策略
    Security {
        #[command(subcommand)]
        action: SecurityAction,
    },

    /// 审计日志
    Audit {
        #[command(subcommand)]
        action: AuditAction,
    },
}

#[derive(Debug, Subcommand)]
enum TaskAction {
    /// 列出任务
    List {
        /// 按状态过滤
        #[arg(long)]
        status: Option<String>,
        /// 按 flow ID 过滤
        #[arg(long)]
        flow_id: Option<String>,
        /// 最大返回数量
        #[arg(long)]
        limit: Option<u32>,
        /// 偏移量
        #[arg(long)]
        offset: Option<u32>,
    },
    /// 查看任务详情
    Get {
        /// 任务 ID
        id: String,
    },
    /// 创建任务
    Create {
        /// Flow ID
        #[arg(long)]
        flow_id: String,
        /// 输入 JSON
        #[arg(long)]
        input: String,
    },
    /// 删除任务
    Delete {
        /// 任务 ID
        id: String,
    },
    /// 取消任务
    Cancel {
        /// 任务 ID
        id: String,
    },
}

#[derive(Debug, Subcommand)]
enum FlowAction {
    /// 列出 flows
    List,
    /// 查看 flow 详情
    Get {
        /// Flow ID
        id: String,
    },
    /// 创建 flow（从 YAML/JSON 文件）
    Create {
        /// 定义文件路径
        #[arg(long)]
        file: String,
    },
    /// 删除 flow
    Delete {
        /// Flow ID
        id: String,
    },
}

#[derive(Debug, Subcommand)]
enum SkillAction {
    /// 列出 skills
    List,
    /// 查看 skill 详情
    Get {
        /// Skill ID
        id: String,
    },
    /// 创建 skill（从 YAML/JSON 文件）
    Create {
        /// 定义文件路径
        #[arg(long)]
        file: String,
    },
}

#[derive(Debug, Subcommand)]
enum ToolAction {
    /// 列出 tools
    List,
    /// 查看 tool 详情
    Get {
        /// Tool 名称
        name: String,
    },
    /// 测试 tool
    Test {
        /// Tool 名称
        name: String,
        /// 参数 JSON
        #[arg(long)]
        args: String,
    },
}

#[derive(Debug, Subcommand)]
enum McpAction {
    /// 列出 MCP servers
    List,
    /// 查看 MCP server 详情
    Get {
        /// Server 名称
        name: String,
    },
    /// 注册 MCP server
    Register {
        /// Server 名称
        #[arg(long)]
        name: String,
        /// 传输类型：stdio 或 sse
        #[arg(long)]
        transport: String,
        /// 命令（stdio 模式）
        #[arg(long)]
        command: Option<String>,
        /// URL（sse 模式）
        #[arg(long)]
        url: Option<String>,
    },
    /// 注销 MCP server
    Unregister {
        /// Server 名称
        name: String,
    },
    /// 连接 MCP server
    Connect {
        /// Server 名称
        name: String,
    },
    /// 断开 MCP server
    Disconnect {
        /// Server 名称
        name: String,
    },
}

#[derive(Debug, Subcommand)]
enum ApprovalAction {
    /// 列出审批
    List {
        /// 按状态过滤
        #[arg(long)]
        status: Option<String>,
    },
    /// 查看审批详情
    Get {
        /// 审批 ID
        id: String,
    },
    /// 批准
    Approve {
        /// 审批 ID
        id: String,
        /// 可选备注
        #[arg(long)]
        comment: Option<String>,
    },
    /// 拒绝
    Deny {
        /// 审批 ID
        id: String,
        /// 可选拒绝原因
        #[arg(long)]
        reason: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum NodeAction {
    /// 列出节点
    List,
    /// 查看节点详情
    Get {
        /// 节点 ID
        id: String,
    },
    /// 排空节点
    Drain {
        /// 节点 ID
        id: String,
    },
    /// 恢复节点
    Resume {
        /// 节点 ID
        id: String,
    },
}

#[derive(Debug, Subcommand)]
enum ChannelAction {
    /// 列出 channels
    List,
    /// Channel 健康检查
    Health {
        /// Channel 名称
        name: String,
    },
}

#[derive(Debug, Subcommand)]
enum SecurityAction {
    /// 查看当前安全策略
    Policy,
}

#[derive(Debug, Subcommand)]
enum AuditAction {
    /// 查询审计日志
    Query {
        /// 按操作者过滤
        #[arg(long)]
        actor: Option<String>,
        /// 按动作过滤
        #[arg(long)]
        action: Option<String>,
        /// 起始时间
        #[arg(long)]
        from: Option<String>,
        /// 截止时间
        #[arg(long)]
        to: Option<String>,
        /// 最大返回数量
        #[arg(long)]
        limit: Option<u32>,
    },
    /// 导出审计日志
    Export {
        /// 导出格式：json 或 csv
        #[arg(long, default_value = "json")]
        format: String,
        /// 按操作者过滤
        #[arg(long)]
        actor: Option<String>,
        /// 按动作过滤
        #[arg(long)]
        action: Option<String>,
        /// 起始时间
        #[arg(long)]
        from: Option<String>,
        /// 截止时间
        #[arg(long)]
        to: Option<String>,
        /// 最大返回数量
        #[arg(long)]
        limit: Option<u32>,
    },
}

/// Helper: print JSON value
fn print_json(val: &serde_json::Value) {
    println!("{}", serde_json::to_string_pretty(val).unwrap_or_default());
}

/// Helper: read a file and parse as JSON (supports YAML/JSON)
fn read_definition_file(path: &str) -> Result<serde_json::Value> {
    let content = std::fs::read_to_string(path)?;
    if path.ends_with(".yaml") || path.ends_with(".yml") {
        let val: serde_json::Value = serde_yml::from_str(&content)?;
        Ok(val)
    } else {
        let val: serde_json::Value = serde_json::from_str(&content)?;
        Ok(val)
    }
}

/// Resolve ATTA_HOME path from CLI arg or env var
fn resolve_home(cli_home: &Option<String>) -> std::path::PathBuf {
    if let Some(home) = cli_home {
        return std::path::PathBuf::from(home);
    }
    if let Ok(env_home) = std::env::var("ATTA_HOME") {
        return std::path::PathBuf::from(env_home);
    }
    let user_home = std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    user_home.join(".atta")
}

/// Try to read port from $ATTA_HOME/etc/attaos.yaml
fn read_config_port(home: &std::path::Path) -> Option<u16> {
    let config_path = home.join("etc/attaos.yaml");
    let content = std::fs::read_to_string(config_path).ok()?;
    let value: serde_yml::Value = serde_yml::from_str(&content).ok()?;
    value["server"]["port"].as_u64().map(|p| p as u16)
}

/// Resolve the server base URL
fn resolve_url(cli_url: &Option<String>, home: &std::path::Path) -> String {
    if let Some(url) = cli_url {
        return url.clone();
    }
    if let Ok(url) = std::env::var("ATTA_URL") {
        return url;
    }
    let port = std::env::var("ATTA_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .or_else(|| read_config_port(home))
        .unwrap_or(3000);
    format!("http://localhost:{port}")
}

/// Resolve the port from URL or env
fn resolve_port(url: &str) -> u16 {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.port())
        .unwrap_or(3000)
}

/// Ensure server is running before executing commands
async fn ensure_server(url: &str, home: &std::path::Path) -> Result<()> {
    let port = resolve_port(url);
    autostart::ensure_server(url, port, home).await
}

/// Interactive chat loop
async fn run_chat(client: &AttaClient, skill: Option<String>) -> Result<()> {
    use std::io::{self, BufRead, Write};

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        // Prompt
        print!("\x1b[1;36m>\x1b[0m ");
        stdout.flush()?;

        // Read line
        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            break; // EOF
        }

        let message = line.trim().to_string();
        if message.is_empty() {
            continue;
        }
        if message == "/quit" || message == "/exit" {
            break;
        }

        let request = ChatRequest {
            message,
            skill_id: skill.clone(),
            flow_id: None,
            task_id: None,
        };

        match client.chat_stream(&request).await {
            Ok(stream) => {
                let mut stream = Box::pin(stream);
                while let Some(event_result) = stream.next().await {
                    match event_result {
                        Ok(event) => match event {
                            atta_types::ChatEvent::TextDelta { delta } => {
                                print!("{delta}");
                                stdout.flush()?;
                            }
                            atta_types::ChatEvent::Thinking { iteration } => {
                                if iteration == 1 {
                                    eprint!("\x1b[2m[thinking...]\x1b[0m ");
                                }
                            }
                            atta_types::ChatEvent::ToolStart { tool_name, .. } => {
                                eprint!("\x1b[2m[{tool_name}]\x1b[0m ");
                            }
                            atta_types::ChatEvent::ToolError { error, .. } => {
                                eprintln!("\x1b[31m[error: {error}]\x1b[0m");
                            }
                            atta_types::ChatEvent::Done { .. } => {
                                println!();
                            }
                            atta_types::ChatEvent::Error { message } => {
                                eprintln!("\x1b[31mError: {message}\x1b[0m");
                            }
                            _ => {}
                        },
                        Err(e) => {
                            eprintln!("\x1b[31mStream error: {e}\x1b[0m");
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("\x1b[31mFailed to connect: {e}\x1b[0m");
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let home = resolve_home(&cli.home);
    let base_url = resolve_url(&cli.url, &home);

    // Ensure server is running for all commands
    ensure_server(&base_url, &home).await?;

    let client = AttaClient::new(&base_url);

    match cli.command {
        Command::Chat { skill } => {
            run_chat(&client, skill).await?;
        }

        Command::Status => match client.health().await {
            Ok(true) => println!("attaos: \x1b[32mrunning\x1b[0m ({base_url})"),
            _ => println!("attaos: \x1b[31mnot running\x1b[0m ({base_url})"),
        },

        Command::Config => {
            let val = client.system_config().await?;
            print_json(&val);
        }

        Command::Metrics => {
            let val = client.metrics().await?;
            print_json(&val);
        }

        Command::Task { action } => match action {
            TaskAction::List {
                status,
                flow_id,
                limit,
                offset,
            } => {
                let val = client
                    .list_tasks_filtered(status.as_deref(), flow_id.as_deref(), limit, offset)
                    .await?;
                print_json(&val);
            }
            TaskAction::Get { id } => {
                let val = client.get_task(&id).await?;
                print_json(&val);
            }
            TaskAction::Create { flow_id, input } => {
                let input_val: serde_json::Value = serde_json::from_str(&input)?;
                let body = serde_json::json!({
                    "flow_id": flow_id,
                    "input": input_val,
                });
                let val = client.create_task(&body).await?;
                print_json(&val);
            }
            TaskAction::Delete { id } => {
                let val = client.delete_task(&id).await?;
                print_json(&val);
            }
            TaskAction::Cancel { id } => {
                let val = client.cancel_task(&id).await?;
                print_json(&val);
            }
        },

        Command::Flow { action } => match action {
            FlowAction::List => {
                let val = client.list_flows().await?;
                print_json(&val);
            }
            FlowAction::Get { id } => {
                let val = client.get_flow(&id).await?;
                print_json(&val);
            }
            FlowAction::Create { file } => {
                let body = read_definition_file(&file)?;
                let val = client.create_flow(&body).await?;
                print_json(&val);
            }
            FlowAction::Delete { id } => {
                let val = client.delete_flow(&id).await?;
                print_json(&val);
            }
        },

        Command::Skill { action } => match action {
            SkillAction::List => {
                let val = client.list_skills().await?;
                print_json(&val);
            }
            SkillAction::Get { id } => {
                let val = client.get_skill(&id).await?;
                print_json(&val);
            }
            SkillAction::Create { file } => {
                let body = read_definition_file(&file)?;
                let val = client.create_skill(&body).await?;
                print_json(&val);
            }
        },

        Command::Tool { action } => match action {
            ToolAction::List => {
                let val = client.list_tools().await?;
                print_json(&val);
            }
            ToolAction::Get { name } => {
                let val = client.get_tool(&name).await?;
                print_json(&val);
            }
            ToolAction::Test { name, args } => {
                let args_val: serde_json::Value = serde_json::from_str(&args)?;
                let val = client.test_tool(&name, &args_val).await?;
                print_json(&val);
            }
        },

        Command::Mcp { action } => match action {
            McpAction::List => {
                let val = client.list_mcp_servers().await?;
                print_json(&val);
            }
            McpAction::Get { name } => {
                let val = client.get_mcp_server(&name).await?;
                print_json(&val);
            }
            McpAction::Register {
                name,
                transport,
                command,
                url,
            } => {
                let mut body = serde_json::json!({
                    "name": name,
                    "transport": transport,
                });
                if let Some(cmd) = command {
                    body["command"] = serde_json::Value::String(cmd);
                }
                if let Some(u) = url {
                    body["url"] = serde_json::Value::String(u);
                }
                let val = client.register_mcp_server(&body).await?;
                print_json(&val);
            }
            McpAction::Unregister { name } => {
                let val = client.unregister_mcp_server(&name).await?;
                print_json(&val);
            }
            McpAction::Connect { name } => {
                let val = client.connect_mcp_server(&name).await?;
                print_json(&val);
            }
            McpAction::Disconnect { name } => {
                let val = client.disconnect_mcp_server(&name).await?;
                print_json(&val);
            }
        },

        Command::Approval { action } => match action {
            ApprovalAction::List { status } => {
                let val = client.list_approvals(status.as_deref()).await?;
                print_json(&val);
            }
            ApprovalAction::Get { id } => {
                let val = client.get_approval(&id).await?;
                print_json(&val);
            }
            ApprovalAction::Approve { id, comment } => {
                let val = client.approve(&id, comment.as_deref()).await?;
                print_json(&val);
            }
            ApprovalAction::Deny { id, reason } => {
                let val = client.deny(&id, reason.as_deref()).await?;
                print_json(&val);
            }
        },

        Command::Node { action } => match action {
            NodeAction::List => {
                let val = client.list_nodes().await?;
                print_json(&val);
            }
            NodeAction::Get { id } => {
                let val = client.get_node(&id).await?;
                print_json(&val);
            }
            NodeAction::Drain { id } => {
                let val = client.drain_node(&id).await?;
                print_json(&val);
            }
            NodeAction::Resume { id } => {
                let val = client.resume_node(&id).await?;
                print_json(&val);
            }
        },

        Command::Channel { action } => match action {
            ChannelAction::List => {
                let val = client.list_channels().await?;
                print_json(&val);
            }
            ChannelAction::Health { name } => {
                let val = client.channel_health(&name).await?;
                print_json(&val);
            }
        },

        Command::Security { action } => match action {
            SecurityAction::Policy => {
                let val = client.get_security_policy().await?;
                print_json(&val);
            }
        },

        Command::Audit { action } => match action {
            AuditAction::Query {
                actor,
                action,
                from,
                to,
                limit,
            } => {
                let val = client
                    .query_audit(
                        actor.as_deref(),
                        action.as_deref(),
                        from.as_deref(),
                        to.as_deref(),
                        limit,
                    )
                    .await?;
                print_json(&val);
            }
            AuditAction::Export {
                format,
                actor,
                action,
                from,
                to,
                limit,
            } => {
                let text = client
                    .export_audit(
                        Some(&format),
                        actor.as_deref(),
                        action.as_deref(),
                        from.as_deref(),
                        to.as_deref(),
                        limit,
                    )
                    .await?;
                print!("{text}");
            }
        },
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // ── resolve_home tests ──

    #[test]
    fn resolve_home_explicit_path() {
        let home = resolve_home(&Some("/custom/atta".to_string()));
        assert_eq!(home, std::path::PathBuf::from("/custom/atta"));
    }

    #[test]
    fn resolve_home_from_env() {
        std::env::set_var("ATTA_HOME", "/env/atta-home");
        let home = resolve_home(&None);
        assert_eq!(home, std::path::PathBuf::from("/env/atta-home"));
        std::env::remove_var("ATTA_HOME");
    }

    #[test]
    fn resolve_home_default_uses_home_dir() {
        std::env::remove_var("ATTA_HOME");
        let home = resolve_home(&None);
        assert!(
            home.ends_with(".atta"),
            "expected path ending with .atta, got: {:?}",
            home
        );
    }

    #[test]
    fn resolve_home_explicit_overrides_env() {
        std::env::set_var("ATTA_HOME", "/env/should-not-be-used");
        let home = resolve_home(&Some("/explicit/path".to_string()));
        assert_eq!(home, std::path::PathBuf::from("/explicit/path"));
        std::env::remove_var("ATTA_HOME");
    }

    // ── resolve_port tests ──

    #[test]
    fn resolve_port_standard_url() {
        assert_eq!(resolve_port("http://localhost:3000"), 3000);
    }

    #[test]
    fn resolve_port_custom_port() {
        assert_eq!(resolve_port("http://localhost:8080"), 8080);
    }

    #[test]
    fn resolve_port_https() {
        assert_eq!(resolve_port("https://myserver:4443"), 4443);
    }

    #[test]
    fn resolve_port_no_port_returns_default() {
        assert_eq!(resolve_port("http://localhost"), 3000);
    }

    #[test]
    fn resolve_port_invalid_url_returns_default() {
        assert_eq!(resolve_port("not-a-url"), 3000);
    }

    // ── read_config_port tests ──

    #[test]
    fn read_config_port_valid_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let etc_dir = dir.path().join("etc");
        std::fs::create_dir_all(&etc_dir).unwrap();
        let config_path = etc_dir.join("attaos.yaml");
        let mut f = std::fs::File::create(&config_path).unwrap();
        writeln!(f, "server:\n  port: 4567").unwrap();

        let port = read_config_port(dir.path());
        assert_eq!(port, Some(4567));
    }

    #[test]
    fn read_config_port_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let port = read_config_port(dir.path());
        assert_eq!(port, None);
    }

    #[test]
    fn read_config_port_invalid_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let etc_dir = dir.path().join("etc");
        std::fs::create_dir_all(&etc_dir).unwrap();
        let config_path = etc_dir.join("attaos.yaml");
        std::fs::write(&config_path, ":::not valid yaml:::").unwrap();

        let port = read_config_port(dir.path());
        assert_eq!(port, None);
    }

    #[test]
    fn read_config_port_yaml_missing_server_key() {
        let dir = tempfile::tempdir().unwrap();
        let etc_dir = dir.path().join("etc");
        std::fs::create_dir_all(&etc_dir).unwrap();
        let config_path = etc_dir.join("attaos.yaml");
        std::fs::write(&config_path, "other:\n  key: value\n").unwrap();

        let port = read_config_port(dir.path());
        assert_eq!(port, None);
    }

    #[test]
    fn read_config_port_yaml_missing_port_key() {
        let dir = tempfile::tempdir().unwrap();
        let etc_dir = dir.path().join("etc");
        std::fs::create_dir_all(&etc_dir).unwrap();
        let config_path = etc_dir.join("attaos.yaml");
        std::fs::write(&config_path, "server:\n  host: localhost\n").unwrap();

        let port = read_config_port(dir.path());
        assert_eq!(port, None);
    }
}
