//! Discord channel
//!
//! Connects to the Discord Gateway (WebSocket) for real-time events and uses
//! the REST API for sending messages, reactions, and typing indicators.

use std::sync::Arc;

use atta_types::AttaError;
use reqwest::Client;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use crate::traits::{Channel, ChannelMessage, ChatType, SendMessage};

const DISCORD_API_BASE: &str = "https://discord.com/api/v10";
const DISCORD_GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=10&encoding=json";

/// Discord channel using Gateway WebSocket + REST API
pub struct DiscordChannel {
    name: String,
    /// Bot token
    bot_token: String,
    /// REST API base URL (overridable for testing)
    api_base: String,
    /// HTTP client for REST API
    client: Client,
    /// Heartbeat interval in milliseconds (set after HELLO)
    heartbeat_interval_ms: Arc<Mutex<Option<u64>>>,
    /// Last sequence number received from the Gateway
    last_sequence: Arc<Mutex<Option<u64>>>,
}

impl DiscordChannel {
    /// Create a new Discord channel with the given bot token
    pub fn new(bot_token: String) -> Self {
        Self {
            name: "discord".to_string(),
            bot_token,
            api_base: DISCORD_API_BASE.to_string(),
            client: Client::new(),
            heartbeat_interval_ms: Arc::new(Mutex::new(None)),
            last_sequence: Arc::new(Mutex::new(None)),
        }
    }

    /// Override the API base URL (for testing with wiremock)
    pub fn with_api_base(mut self, api_base: String) -> Self {
        self.api_base = api_base;
        self
    }

    /// Build an authorization header value
    fn auth_header(&self) -> String {
        format!("Bot {}", self.bot_token)
    }

    /// Send a REST API request to Discord
    async fn api_post(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, AttaError> {
        let url = format!("{}{}", self.api_base, path);
        let response = self
            .client
            .post(&url)
            .header("Authorization", self.auth_header())
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AttaError::Other(anyhow::anyhow!(
                "Discord API {} failed HTTP {}: {}",
                path,
                status,
                text
            )));
        }

        response
            .json()
            .await
            .map_err(|e| AttaError::Other(e.into()))
    }
}

#[async_trait::async_trait]
impl Channel for DiscordChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: SendMessage) -> Result<(), AttaError> {
        let channel_id = &message.recipient;
        let path = format!("/channels/{}/messages", channel_id);

        let mut body = serde_json::json!({
            "content": message.content,
        });

        // If replying in a thread, use the thread's message reference
        if let Some(ref thread_ts) = message.thread_ts {
            body["message_reference"] = serde_json::json!({
                "message_id": thread_ts,
            });
        }

        self.api_post(&path, &body).await?;
        debug!("Discord message sent to channel {}", channel_id);
        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        #[cfg(feature = "discord")]
        {
            use futures::stream::StreamExt;
            use futures::SinkExt;
            use tokio_tungstenite::{connect_async, tungstenite::Message};

            let heartbeat_interval_ms = self.heartbeat_interval_ms.clone();
            let last_sequence = self.last_sequence.clone();

            loop {
                info!("Discord Gateway connecting");

                let (ws_stream, _) = connect_async(DISCORD_GATEWAY_URL).await.map_err(|e| {
                    AttaError::Other(anyhow::anyhow!("Discord WS connect failed: {e}"))
                })?;

                let (write, mut read) = ws_stream.split();
                let write = Arc::new(Mutex::new(write));

                // Step 1: Receive HELLO (opcode 10) with heartbeat_interval
                let hello = read.next().await;
                if let Some(Ok(Message::Text(text))) = hello {
                    if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&text) {
                        if msg.get("op").and_then(|v| v.as_u64()) == Some(10) {
                            let interval = msg
                                .pointer("/d/heartbeat_interval")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(41250);
                            *heartbeat_interval_ms.lock().await = Some(interval);
                            debug!(interval_ms = interval, "Discord HELLO received");
                        }
                    }
                }

                // Step 2: Send IDENTIFY (opcode 2)
                let identify = serde_json::json!({
                    "op": 2,
                    "d": {
                        "token": self.bot_token,
                        "intents": 33281,
                        "properties": {
                            "os": std::env::consts::OS,
                            "browser": "atta",
                            "device": "atta"
                        }
                    }
                });
                {
                    let mut w = write.lock().await;
                    let _ = w.send(Message::Text(identify.to_string())).await;
                }

                info!("Discord Gateway identified, starting event loop");

                // Step 3: Start heartbeat task
                let hb_write = write.clone();
                let hb_interval = heartbeat_interval_ms.clone();
                let hb_seq = last_sequence.clone();
                let heartbeat_task = tokio::spawn(async move {
                    let interval_ms = hb_interval.lock().await.unwrap_or(41250);
                    let mut ticker =
                        tokio::time::interval(tokio::time::Duration::from_millis(interval_ms));
                    loop {
                        ticker.tick().await;
                        let seq = *hb_seq.lock().await;
                        let heartbeat = serde_json::json!({"op": 1, "d": seq});
                        let mut w = hb_write.lock().await;
                        if w.send(Message::Text(heartbeat.to_string())).await.is_err() {
                            break;
                        }
                    }
                });

                // Step 4: Read events
                while let Some(msg_result) = read.next().await {
                    let msg = match msg_result {
                        Ok(m) => m,
                        Err(e) => {
                            warn!(error = %e, "Discord WS read error, reconnecting");
                            break;
                        }
                    };

                    let text = match msg {
                        Message::Text(t) => t,
                        Message::Close(_) => {
                            info!("Discord WS closed, reconnecting");
                            break;
                        }
                        _ => continue,
                    };

                    let payload: serde_json::Value = match serde_json::from_str(&text) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    let op = payload.get("op").and_then(|v| v.as_u64()).unwrap_or(0);

                    // Update sequence number
                    if let Some(s) = payload.get("s").and_then(|v| v.as_u64()) {
                        *last_sequence.lock().await = Some(s);
                    }

                    match op {
                        0 => {
                            // Dispatch event
                            let event_type =
                                payload.get("t").and_then(|v| v.as_str()).unwrap_or("");
                            if event_type == "MESSAGE_CREATE" {
                                if let Some(d) = payload.get("d") {
                                    // Skip bot messages
                                    if d.pointer("/author/bot")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(false)
                                    {
                                        continue;
                                    }

                                    let channel_msg = ChannelMessage {
                                        id: d
                                            .get("id")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("")
                                            .to_string(),
                                        sender: d
                                            .pointer("/author/id")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("")
                                            .to_string(),
                                        content: d
                                            .get("content")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("")
                                            .to_string(),
                                        channel: "discord".to_string(),
                                        reply_target: d
                                            .pointer("/referenced_message/id")
                                            .and_then(|v| v.as_str())
                                            .map(|s| s.to_string()),
                                        timestamp: chrono::Utc::now(),
                                        thread_ts: None,
                                        metadata: serde_json::json!({
                                            "channel_id": d.get("channel_id")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or(""),
                                        }),
                                        chat_type: ChatType::default(),
                                        bot_mentioned: false,
                                        group_id: None,
                                    };

                                    if tx.send(channel_msg).await.is_err() {
                                        heartbeat_task.abort();
                                        return Ok(());
                                    }
                                }
                            }
                        }
                        7 => {
                            // Reconnect requested
                            info!("Discord Gateway reconnect requested");
                            break;
                        }
                        9 => {
                            // Invalid session
                            warn!("Discord Gateway invalid session");
                            break;
                        }
                        11 => {
                            // Heartbeat ACK — ok
                            debug!("Discord heartbeat ACK");
                        }
                        _ => {}
                    }
                }

                heartbeat_task.abort();
                warn!("Discord Gateway disconnected, reconnecting in 5s");
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }

        #[cfg(not(feature = "discord"))]
        {
            let _ = tx;
            warn!("Discord Gateway requires the 'discord' feature");
            std::future::pending::<()>().await;
            Ok(())
        }
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        let url = format!("{}/users/@me", self.api_base);
        let response = self
            .client
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            return Err(AttaError::Other(anyhow::anyhow!(
                "Discord health check failed: HTTP {}",
                response.status()
            )));
        }

        Ok(())
    }

    async fn start_typing(&self, recipient: &str) -> Result<(), AttaError> {
        let path = format!("/channels/{}/typing", recipient);
        let url = format!("{}{}", self.api_base, path);

        self.client
            .post(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        Ok(())
    }

    async fn add_reaction(&self, message_id: &str, reaction: &str) -> Result<(), AttaError> {
        // message_id format: "channel_id:message_id"
        let parts: Vec<&str> = message_id.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(AttaError::Validation(
                "Discord reaction requires 'channel_id:message_id' format".to_string(),
            ));
        }

        let encoded_emoji = urlencoding::encode(reaction);
        let path = format!(
            "/channels/{}/messages/{}/reactions/{}/@me",
            parts[0], parts[1], encoded_emoji
        );
        let url = format!("{}{}", self.api_base, path);

        self.client
            .put(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        Ok(())
    }

    async fn remove_reaction(&self, message_id: &str, reaction: &str) -> Result<(), AttaError> {
        let parts: Vec<&str> = message_id.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(AttaError::Validation(
                "Discord reaction requires 'channel_id:message_id' format".to_string(),
            ));
        }

        let encoded_emoji = urlencoding::encode(reaction);
        let path = format!(
            "/channels/{}/messages/{}/reactions/{}/@me",
            parts[0], parts[1], encoded_emoji
        );
        let url = format!("{}{}", self.api_base, path);

        self.client
            .delete(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        Ok(())
    }

    fn supports_draft_updates(&self) -> bool {
        true
    }

    async fn send_draft(&self, message: SendMessage) -> Result<String, AttaError> {
        // Send via REST API, return "{channel_id}:{message_id}"
        let body = serde_json::json!({
            "content": message.content,
        });

        let channel_id = &message.recipient;
        let url = format!("{}/channels/{}/messages", self.api_base, channel_id);

        let resp = self
            .client
            .post(&url)
            .header("Authorization", self.auth_header())
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AttaError::Other(anyhow::anyhow!("discord send_draft failed: {e}")))?;

        let data: serde_json::Value = resp.json().await.map_err(|e| {
            AttaError::Other(anyhow::anyhow!("discord send_draft parse failed: {e}"))
        })?;

        let message_id = data["id"].as_str().unwrap_or("0");
        Ok(format!("{channel_id}:{message_id}"))
    }

    async fn update_draft(&self, draft_id: &str, content: &str) -> Result<(), AttaError> {
        let parts: Vec<&str> = draft_id.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(AttaError::Validation("invalid draft_id format".to_string()));
        }

        // Discord message limit: 2000 characters
        let truncated = crate::draft::utf8_truncate(content, 2000);

        let url = format!(
            "{}/channels/{}/messages/{}",
            self.api_base, parts[0], parts[1]
        );
        let body = serde_json::json!({
            "content": truncated,
        });

        self.client
            .patch(&url)
            .header("Authorization", self.auth_header())
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AttaError::Other(anyhow::anyhow!("discord update_draft failed: {e}")))?;

        Ok(())
    }

    async fn finalize_draft(&self, _draft_id: &str) -> Result<(), AttaError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discord_channel_name() {
        let ch = DiscordChannel::new("test-token".to_string());
        assert_eq!(ch.name(), "discord");
    }

    #[test]
    fn test_auth_header() {
        let ch = DiscordChannel::new("my-bot-token".to_string());
        assert_eq!(ch.auth_header(), "Bot my-bot-token");
    }

    #[tokio::test]
    async fn test_health_check_invalid_token() {
        let ch = DiscordChannel::new("invalid".to_string());
        let _ = ch.health_check().await;
    }
}
