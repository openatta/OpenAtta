//! IRC channel
//!
//! Connects to an IRC server over TCP, handles PING/PONG keepalive,
//! joins the specified channel, and relays PRIVMSG messages.

use std::sync::Arc;

use atta_types::AttaError;
use tracing::{debug, info, warn};

use crate::traits::{Channel, ChannelMessage, ChatType, SendMessage};

/// IRC channel configuration
pub struct IrcChannel {
    name: String,
    /// IRC server host
    server: String,
    /// IRC server port (6667 for plain, 6697 for TLS)
    port: u16,
    /// Nickname
    nickname: String,
    /// IRC channel to join (e.g., "#mychannel")
    irc_channel: String,
    /// Optional server password
    server_password: Option<String>,
    /// Use TLS
    use_tls: bool,
    /// Shared writer for sending messages from send()
    #[cfg(feature = "irc")]
    writer: Arc<tokio::sync::Mutex<Option<tokio::net::tcp::OwnedWriteHalf>>>,
}

impl IrcChannel {
    /// Create a new IRC channel
    pub fn new(server: String, port: u16, nickname: String, irc_channel: String) -> Self {
        Self {
            name: "irc".to_string(),
            server,
            port,
            nickname,
            irc_channel,
            server_password: None,
            use_tls: false,
            #[cfg(feature = "irc")]
            writer: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    /// Set server password
    pub fn with_password(mut self, password: String) -> Self {
        self.server_password = Some(password);
        self
    }

    /// Enable TLS
    pub fn with_tls(mut self, use_tls: bool) -> Self {
        self.use_tls = use_tls;
        self
    }

    /// Parse an IRC message line into its components
    fn parse_irc_line(line: &str) -> Option<IrcMessage> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }

        let (prefix, rest) = if line.starts_with(':') {
            let space = line.find(' ')?;
            (Some(&line[1..space]), &line[space + 1..])
        } else {
            (None, line)
        };

        let (command, params_str) = if let Some(space) = rest.find(' ') {
            (&rest[..space], Some(&rest[space + 1..]))
        } else {
            (rest, None)
        };

        let mut params = Vec::new();
        if let Some(params_str) = params_str {
            let mut remaining = params_str;
            loop {
                if remaining.starts_with(':') {
                    params.push(&remaining[1..]);
                    break;
                }
                if let Some(space) = remaining.find(' ') {
                    params.push(&remaining[..space]);
                    remaining = &remaining[space + 1..];
                } else {
                    params.push(remaining);
                    break;
                }
            }
        }

        Some(IrcMessage {
            prefix: prefix.map(|s| s.to_string()),
            command: command.to_string(),
            params: params.into_iter().map(|s| s.to_string()).collect(),
        })
    }

    /// Extract nickname from a prefix like "nick!user@host"
    fn nick_from_prefix(prefix: &str) -> &str {
        prefix.split('!').next().unwrap_or(prefix)
    }
}

/// Parsed IRC message
struct IrcMessage {
    prefix: Option<String>,
    command: String,
    params: Vec<String>,
}

#[async_trait::async_trait]
impl Channel for IrcChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: SendMessage) -> Result<(), AttaError> {
        #[cfg(feature = "irc")]
        {
            use tokio::io::AsyncWriteExt;

            let target = if message.recipient.is_empty() {
                self.irc_channel.clone()
            } else {
                message.recipient.clone()
            };

            let mut guard = self.writer.lock().await;
            let writer = guard
                .as_mut()
                .ok_or_else(|| AttaError::Channel("IRC: not connected, cannot send".to_string()))?;

            // Split message into lines to respect IRC line length limits
            let max_line_len = 400; // conservative limit (IRC max is 512 including prefix)
            let lines: Vec<&str> = message.content.lines().collect();

            for line in lines {
                for chunk in line.as_bytes().chunks(max_line_len) {
                    let chunk_str = String::from_utf8_lossy(chunk);
                    let irc_line = format!("PRIVMSG {} :{}\r\n", target, chunk_str);
                    writer
                        .write_all(irc_line.as_bytes())
                        .await
                        .map_err(|e| AttaError::Channel(format!("IRC send failed: {e}")))?;
                }
            }

            debug!(target = %target, "IRC message sent");
            Ok(())
        }

        #[cfg(not(feature = "irc"))]
        {
            let _ = message;
            Err(AttaError::Channel(
                "IRC channel requires the 'irc' feature".to_string(),
            ))
        }
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        #[cfg(feature = "irc")]
        {
            use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
            use tokio::net::TcpStream;

            loop {
                info!(
                    server = %self.server,
                    port = self.port,
                    channel = %self.irc_channel,
                    nick = %self.nickname,
                    "IRC connecting"
                );

                let connect_result = TcpStream::connect((&*self.server, self.port)).await;

                let stream = match connect_result {
                    Ok(s) => s,
                    Err(e) => {
                        warn!(error = %e, "IRC TCP connect failed, retrying in 5s");
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                        continue;
                    }
                };

                info!("IRC TCP connected to {}:{}", self.server, self.port);

                let (reader, writer) = stream.into_split();

                // Store writer for send() to use
                {
                    let mut guard = self.writer.lock().await;
                    *guard = Some(writer);
                }

                // We need a reference to the writer for registration commands.
                // Re-acquire the lock for each write operation.
                let writer_ref = Arc::clone(&self.writer);

                let mut lines = BufReader::new(reader).lines();

                // Send PASS if configured
                if let Some(ref pass) = self.server_password {
                    let cmd = format!("PASS {}\r\n", pass);
                    let mut guard = writer_ref.lock().await;
                    if let Some(w) = guard.as_mut() {
                        if let Err(e) = w.write_all(cmd.as_bytes()).await {
                            warn!(error = %e, "IRC failed to send PASS, reconnecting");
                            *guard = None;
                            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                            continue;
                        }
                    }
                }

                // Send NICK
                {
                    let nick_cmd = format!("NICK {}\r\n", self.nickname);
                    let mut guard = writer_ref.lock().await;
                    if let Some(w) = guard.as_mut() {
                        if let Err(e) = w.write_all(nick_cmd.as_bytes()).await {
                            warn!(error = %e, "IRC failed to send NICK, reconnecting");
                            *guard = None;
                            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                            continue;
                        }
                    }
                }

                // Send USER
                {
                    let user_cmd = format!("USER {} 0 * :{}\r\n", self.nickname, self.nickname);
                    let mut guard = writer_ref.lock().await;
                    if let Some(w) = guard.as_mut() {
                        if let Err(e) = w.write_all(user_cmd.as_bytes()).await {
                            warn!(error = %e, "IRC failed to send USER, reconnecting");
                            *guard = None;
                            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                            continue;
                        }
                    }
                }

                // Wait for RPL_WELCOME (001), then JOIN
                let mut registered = false;
                let mut disconnected = false;

                while let Ok(Some(line)) = lines.next_line().await {
                    if let Some(msg) = Self::parse_irc_line(&line) {
                        match msg.command.as_str() {
                            "001" => {
                                // RPL_WELCOME — registration complete
                                info!("IRC registered, joining {}", self.irc_channel);
                                let join_cmd = format!("JOIN {}\r\n", self.irc_channel);
                                let mut guard = writer_ref.lock().await;
                                if let Some(w) = guard.as_mut() {
                                    if let Err(e) = w.write_all(join_cmd.as_bytes()).await {
                                        warn!(error = %e, "IRC failed to send JOIN");
                                        disconnected = true;
                                        break;
                                    }
                                }
                                registered = true;
                            }
                            "PING" => {
                                let pong_param =
                                    msg.params.first().map(|s| s.as_str()).unwrap_or("");
                                let pong_cmd = format!("PONG :{}\r\n", pong_param);
                                let mut guard = writer_ref.lock().await;
                                if let Some(w) = guard.as_mut() {
                                    if let Err(e) = w.write_all(pong_cmd.as_bytes()).await {
                                        warn!(error = %e, "IRC failed to send PONG");
                                        disconnected = true;
                                        break;
                                    }
                                }
                            }
                            "PRIVMSG" if registered => {
                                if msg.params.len() >= 2 {
                                    let target = &msg.params[0];
                                    let text = &msg.params[1];
                                    let sender = msg
                                        .prefix
                                        .as_deref()
                                        .map(Self::nick_from_prefix)
                                        .unwrap_or("unknown");

                                    let channel_msg = ChannelMessage {
                                        id: uuid::Uuid::new_v4().to_string(),
                                        sender: sender.to_string(),
                                        content: text.clone(),
                                        channel: "irc".to_string(),
                                        reply_target: None,
                                        timestamp: chrono::Utc::now(),
                                        thread_ts: None,
                                        metadata: serde_json::json!({
                                            "irc_target": target,
                                            "prefix": msg.prefix,
                                        }),
                                        chat_type: ChatType::default(),
                                        bot_mentioned: false,
                                        group_id: None,
                                    };

                                    if tx.send(channel_msg).await.is_err() {
                                        info!("IRC listener: receiver dropped, stopping");
                                        return Ok(());
                                    }
                                }
                            }
                            "ERROR" => {
                                let reason =
                                    msg.params.first().map(|s| s.as_str()).unwrap_or("unknown");
                                warn!(reason, "IRC server ERROR, reconnecting");
                                disconnected = true;
                                break;
                            }
                            _ => {
                                debug!(command = %msg.command, "IRC message");
                            }
                        }
                    }
                }

                // Clear writer on disconnect
                {
                    let mut guard = self.writer.lock().await;
                    *guard = None;
                }

                if !disconnected {
                    warn!("IRC connection stream ended");
                }

                warn!("IRC disconnected, reconnecting in 5s");
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }

        #[cfg(not(feature = "irc"))]
        {
            let _ = tx;
            warn!("IRC channel requires the 'irc' feature");
            std::future::pending::<()>().await;
            Ok(())
        }
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        if self.server.is_empty() {
            return Err(AttaError::Validation(
                "IRC channel: server not configured".to_string(),
            ));
        }
        if self.nickname.is_empty() {
            return Err(AttaError::Validation(
                "IRC channel: nickname not configured".to_string(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_irc_channel_name() {
        let ch = IrcChannel::new(
            "irc.libera.chat".to_string(),
            6667,
            "atta-bot".to_string(),
            "#atta".to_string(),
        );
        assert_eq!(ch.name(), "irc");
    }

    #[test]
    fn test_parse_privmsg() {
        let line = ":nick!user@host PRIVMSG #channel :Hello, world!";
        let msg = IrcChannel::parse_irc_line(line).unwrap();
        assert_eq!(msg.command, "PRIVMSG");
        assert_eq!(msg.params[0], "#channel");
        assert_eq!(msg.params[1], "Hello, world!");
        assert_eq!(
            IrcChannel::nick_from_prefix(msg.prefix.as_deref().unwrap()),
            "nick"
        );
    }

    #[test]
    fn test_parse_ping() {
        let line = "PING :server.example.com";
        let msg = IrcChannel::parse_irc_line(line).unwrap();
        assert_eq!(msg.command, "PING");
        assert_eq!(msg.params[0], "server.example.com");
    }

    #[test]
    fn test_parse_numeric() {
        let line = ":server 001 nick :Welcome to IRC";
        let msg = IrcChannel::parse_irc_line(line).unwrap();
        assert_eq!(msg.prefix, Some("server".to_string()));
        assert_eq!(msg.command, "001");
        assert_eq!(msg.params[0], "nick");
        assert_eq!(msg.params[1], "Welcome to IRC");
    }

    #[tokio::test]
    async fn test_health_check_valid() {
        let ch = IrcChannel::new(
            "irc.libera.chat".to_string(),
            6667,
            "bot".to_string(),
            "#test".to_string(),
        );
        assert!(ch.health_check().await.is_ok());
    }

    #[tokio::test]
    async fn test_health_check_missing_server() {
        let ch = IrcChannel::new("".to_string(), 6667, "bot".to_string(), "#test".to_string());
        assert!(ch.health_check().await.is_err());
    }
}
