//! QQ Bot channel
//!
//! Implements the QQ Bot API v2 using WebSocket Gateway for real-time events
//! and REST API for message sending. Requires a QQ Bot app ID and token.

use atta_types::AttaError;
use reqwest::Client;
use tracing::{debug, info, warn};

use crate::traits::{Channel, ChannelMessage, ChatType, SendMessage};

const QQ_API_BASE: &str = "https://api.sgroup.qq.com";
const QQ_SANDBOX_API_BASE: &str = "https://sandbox.api.sgroup.qq.com";

/// QQ Bot channel (API v2)
pub struct QqChannel {
    name: String,
    /// Bot App ID
    app_id: String,
    /// Bot Token
    token: String,
    /// Whether to use sandbox environment
    sandbox: bool,
    /// HTTP client
    client: Client,
}

impl QqChannel {
    /// Create a new QQ Bot channel
    pub fn new(app_id: String, token: String, sandbox: bool) -> Self {
        Self {
            name: "qq".to_string(),
            app_id,
            token,
            sandbox,
            client: Client::new(),
        }
    }

    /// Get the base API URL
    fn api_base(&self) -> &str {
        if self.sandbox {
            QQ_SANDBOX_API_BASE
        } else {
            QQ_API_BASE
        }
    }

    /// Build the authorization header
    fn auth_header(&self) -> String {
        format!("Bot {}.{}", self.app_id, self.token)
    }

    /// Get the WebSocket Gateway URL
    async fn get_gateway_url(&self) -> Result<String, AttaError> {
        let url = format!("{}/gateway", self.api_base());
        let response = self
            .client
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AttaError::Other(anyhow::anyhow!(
                "QQ gateway request failed HTTP {}: {}",
                status,
                text
            )));
        }

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        result
            .get("url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| AttaError::Other(anyhow::anyhow!("QQ: missing url in gateway response")))
    }

    /// Make an authenticated API POST request
    async fn api_post(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, AttaError> {
        let url = format!("{}{}", self.api_base(), path);
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
                "QQ API {} failed HTTP {}: {}",
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
impl Channel for QqChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: SendMessage) -> Result<(), AttaError> {
        let channel_id = &message.recipient;
        let path = format!("/channels/{}/messages", channel_id);

        let mut body = serde_json::json!({
            "content": message.content,
        });

        if let Some(ref thread_ts) = message.thread_ts {
            body["msg_id"] = serde_json::json!(thread_ts);
        }

        self.api_post(&path, &body).await?;
        debug!("QQ message sent to channel {}", channel_id);
        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        #[cfg(feature = "qq")]
        {
            use futures::stream::StreamExt;
            use futures::SinkExt;
            use std::sync::Arc;
            use tokio::sync::Mutex;
            use tokio_tungstenite::{connect_async, tungstenite::Message};

            let mut _last_sequence: Option<u64> = None;

            loop {
                // Step 1: Get the Gateway URL
                let gateway_url = match self.get_gateway_url().await {
                    Ok(url) => url,
                    Err(e) => {
                        warn!(error = %e, "QQ failed to get gateway URL, retrying in 5s");
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                        continue;
                    }
                };
                info!(url = %gateway_url, "QQ Gateway connecting");

                let (ws_stream, _) = match connect_async(&gateway_url).await {
                    Ok(conn) => conn,
                    Err(e) => {
                        warn!(error = %e, "QQ WS connect failed, retrying in 5s");
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                        continue;
                    }
                };

                let (write, mut read) = ws_stream.split();
                let write = Arc::new(Mutex::new(write));

                // Step 2: Receive Hello (opcode 10) with heartbeat_interval
                let mut heartbeat_interval_ms: u64 = 41250;
                if let Some(Ok(Message::Text(text))) = read.next().await {
                    if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&text) {
                        if msg.get("op").and_then(|v| v.as_u64()) == Some(10) {
                            heartbeat_interval_ms = msg
                                .pointer("/d/heartbeat_interval")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(41250);
                            debug!(interval_ms = heartbeat_interval_ms, "QQ Hello received");
                        }
                    }
                }

                // Step 3: Send Identify (opcode 2)
                // intents: PUBLIC_GUILD_MESSAGES (1 << 30) | DIRECT_MESSAGE (1 << 12)
                let intents: u64 = (1 << 30) | (1 << 12);
                let identify = serde_json::json!({
                    "op": 2,
                    "d": {
                        "token": self.auth_header(),
                        "intents": intents,
                        "shard": [0, 1],
                    }
                });
                {
                    let mut w = write.lock().await;
                    if let Err(e) = w.send(Message::Text(identify.to_string())).await {
                        warn!(error = %e, "QQ failed to send Identify, reconnecting");
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                        continue;
                    }
                }

                info!("QQ Gateway identified, starting event loop");

                // Step 4: Start heartbeat task
                let hb_write = write.clone();
                let heartbeat_task = {
                    let interval = heartbeat_interval_ms;
                    tokio::spawn(async move {
                        let mut ticker =
                            tokio::time::interval(tokio::time::Duration::from_millis(interval));
                        loop {
                            ticker.tick().await;
                            let heartbeat =
                                serde_json::json!({"op": 1, "d": serde_json::Value::Null});
                            let mut w = hb_write.lock().await;
                            if w.send(Message::Text(heartbeat.to_string())).await.is_err() {
                                break;
                            }
                        }
                    })
                };

                // Step 5: Read events
                while let Some(msg_result) = read.next().await {
                    let msg = match msg_result {
                        Ok(m) => m,
                        Err(e) => {
                            warn!(error = %e, "QQ WS read error, reconnecting");
                            break;
                        }
                    };

                    let text = match msg {
                        Message::Text(t) => t,
                        Message::Close(_) => {
                            info!("QQ WS closed, reconnecting");
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
                        _last_sequence = Some(s);
                    }

                    match op {
                        0 => {
                            // Dispatch event
                            let event_type =
                                payload.get("t").and_then(|v| v.as_str()).unwrap_or("");

                            match event_type {
                                "AT_MESSAGE_CREATE"
                                | "MESSAGE_CREATE"
                                | "DIRECT_MESSAGE_CREATE" => {
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
                                            channel: "qq".to_string(),
                                            reply_target: None,
                                            timestamp: chrono::Utc::now(),
                                            thread_ts: None,
                                            metadata: serde_json::json!({
                                                "channel_id": d.get("channel_id").and_then(|v| v.as_str()).unwrap_or(""),
                                                "guild_id": d.get("guild_id").and_then(|v| v.as_str()).unwrap_or(""),
                                                "event_type": event_type,
                                            }),
                                            chat_type: ChatType::default(),
                                            bot_mentioned: false,
                                            group_id: None,
                                        };

                                        if tx.send(channel_msg).await.is_err() {
                                            heartbeat_task.abort();
                                            info!("QQ listener: receiver dropped, stopping");
                                            return Ok(());
                                        }
                                    }
                                }
                                "READY" => {
                                    info!("QQ Gateway READY");
                                }
                                _ => {
                                    debug!(event = event_type, "QQ event (ignored)");
                                }
                            }
                        }
                        7 => {
                            // Reconnect requested
                            info!("QQ Gateway reconnect requested");
                            break;
                        }
                        9 => {
                            // Invalid session
                            warn!("QQ Gateway invalid session");
                            break;
                        }
                        11 => {
                            // Heartbeat ACK
                            debug!("QQ heartbeat ACK");
                        }
                        _ => {}
                    }
                }

                heartbeat_task.abort();
                warn!("QQ Gateway disconnected, reconnecting in 5s");
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }

        #[cfg(not(feature = "qq"))]
        {
            let _ = tx;
            warn!("QQ channel requires the 'qq' feature");
            std::future::pending::<()>().await;
            Ok(())
        }
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        let url = format!("{}/users/@me", self.api_base());
        let response = self
            .client
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            return Err(AttaError::Other(anyhow::anyhow!(
                "QQ health check failed: HTTP {}",
                response.status()
            )));
        }

        Ok(())
    }

    async fn add_reaction(&self, message_id: &str, reaction: &str) -> Result<(), AttaError> {
        // message_id format: "channel_id:message_id"
        let parts: Vec<&str> = message_id.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(AttaError::Validation(
                "QQ reaction requires 'channel_id:message_id' format".to_string(),
            ));
        }

        // QQ uses emoji type 1 for system emoji, 2 for custom emoji
        let path = format!(
            "/channels/{}/messages/{}/reactions/1/{}",
            parts[0], parts[1], reaction
        );
        let url = format!("{}{}", self.api_base(), path);

        self.client
            .put(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qq_channel_name() {
        let ch = QqChannel::new("12345".to_string(), "token".to_string(), false);
        assert_eq!(ch.name(), "qq");
    }

    #[test]
    fn test_qq_sandbox_base() {
        let ch = QqChannel::new("12345".to_string(), "token".to_string(), true);
        assert_eq!(ch.api_base(), QQ_SANDBOX_API_BASE);
    }

    #[test]
    fn test_qq_production_base() {
        let ch = QqChannel::new("12345".to_string(), "token".to_string(), false);
        assert_eq!(ch.api_base(), QQ_API_BASE);
    }

    #[test]
    fn test_auth_header_format() {
        let ch = QqChannel::new("123".to_string(), "abc".to_string(), false);
        assert_eq!(ch.auth_header(), "Bot 123.abc");
    }
}
