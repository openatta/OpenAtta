//! Mattermost channel
//!
//! Connects to a Mattermost server using the WebSocket API for real-time
//! events and the REST API v4 for message sending and reactions.
//! Supports both token-based and session-based authentication.

use atta_types::AttaError;
use reqwest::Client;
use tracing::{debug, info, warn};

use crate::traits::{Channel, ChannelMessage, ChatType, SendMessage};

/// Mattermost channel
pub struct MattermostChannel {
    name: String,
    /// Mattermost server URL (e.g., "https://mattermost.example.com")
    server_url: String,
    /// Personal access token or bot token
    token: String,
    /// Bot user ID (set after authentication)
    bot_user_id: tokio::sync::Mutex<Option<String>>,
    /// HTTP client
    client: Client,
}

impl MattermostChannel {
    /// Create a new Mattermost channel
    pub fn new(server_url: String, token: String) -> Self {
        Self {
            name: "mattermost".to_string(),
            server_url: server_url.trim_end_matches('/').to_string(),
            token,
            bot_user_id: tokio::sync::Mutex::new(None),
            client: Client::new(),
        }
    }

    /// Build a REST API URL
    fn api_url(&self, path: &str) -> String {
        format!("{}/api/v4{}", self.server_url, path)
    }

    /// Build a WebSocket URL
    fn ws_url(&self) -> String {
        let ws_scheme = if self.server_url.starts_with("https") {
            "wss"
        } else {
            "ws"
        };
        let host = self
            .server_url
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        format!("{}://{}/api/v4/websocket", ws_scheme, host)
    }

    /// Make an authenticated GET request
    async fn api_get(&self, path: &str) -> Result<serde_json::Value, AttaError> {
        let url = self.api_url(path);
        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AttaError::Other(anyhow::anyhow!(
                "Mattermost GET {} HTTP {}: {}",
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

    /// Make an authenticated POST request
    async fn api_post(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, AttaError> {
        let url = self.api_url(path);
        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AttaError::Other(anyhow::anyhow!(
                "Mattermost POST {} HTTP {}: {}",
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

    /// Fetch and cache the bot's user ID
    async fn ensure_user_id(&self) -> Result<String, AttaError> {
        let mut user_id = self.bot_user_id.lock().await;
        if let Some(ref id) = *user_id {
            return Ok(id.clone());
        }

        let result = self.api_get("/users/me").await?;
        let id = result
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AttaError::Other(anyhow::anyhow!("Mattermost: missing user ID in /users/me"))
            })?
            .to_string();

        *user_id = Some(id.clone());
        Ok(id)
    }
}

#[async_trait::async_trait]
impl Channel for MattermostChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: SendMessage) -> Result<(), AttaError> {
        let channel_id = &message.recipient;

        let mut body = serde_json::json!({
            "channel_id": channel_id,
            "message": message.content,
        });

        if let Some(ref thread_ts) = message.thread_ts {
            body["root_id"] = serde_json::json!(thread_ts);
        }

        self.api_post("/posts", &body).await?;
        debug!("Mattermost message sent to channel {}", channel_id);
        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        #[cfg(feature = "mattermost")]
        {
            use futures::stream::StreamExt;
            use futures::SinkExt;
            use tokio_tungstenite::{connect_async, tungstenite::Message};

            let bot_user_id = self.ensure_user_id().await?;

            loop {
                let ws_url = self.ws_url();
                info!(url = %ws_url, "Mattermost WebSocket connecting");

                let (ws_stream, _) = match connect_async(&ws_url).await {
                    Ok(conn) => conn,
                    Err(e) => {
                        warn!(error = %e, "Mattermost WS connect failed, retrying in 5s");
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                        continue;
                    }
                };

                let (mut write, mut read) = ws_stream.split();

                // Step 1: Send authentication challenge
                let auth_msg = serde_json::json!({
                    "seq": 1,
                    "action": "authentication_challenge",
                    "data": {
                        "token": self.token,
                    }
                });

                if let Err(e) = write.send(Message::Text(auth_msg.to_string())).await {
                    warn!(error = %e, "Mattermost WS auth send failed, reconnecting");
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    continue;
                }

                info!("Mattermost WebSocket authenticated, starting event loop");

                // Step 2: Read events
                while let Some(msg_result) = read.next().await {
                    let msg = match msg_result {
                        Ok(m) => m,
                        Err(e) => {
                            warn!(error = %e, "Mattermost WS read error, reconnecting");
                            break;
                        }
                    };

                    let text = match msg {
                        Message::Text(t) => t,
                        Message::Ping(data) => {
                            let _ = write.send(Message::Pong(data)).await;
                            continue;
                        }
                        Message::Close(_) => {
                            info!("Mattermost WS closed, reconnecting");
                            break;
                        }
                        _ => continue,
                    };

                    let payload: serde_json::Value = match serde_json::from_str(&text) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    let event = payload.get("event").and_then(|v| v.as_str()).unwrap_or("");

                    match event {
                        "hello" => {
                            debug!("Mattermost WS hello received");
                        }
                        "posted" => {
                            // The "post" field in data is a JSON-encoded string
                            let post_str = payload
                                .pointer("/data/post")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");

                            let post: serde_json::Value = match serde_json::from_str(post_str) {
                                Ok(v) => v,
                                Err(_) => continue,
                            };

                            // Skip messages from our bot
                            let user_id =
                                post.get("user_id").and_then(|v| v.as_str()).unwrap_or("");
                            if user_id == bot_user_id {
                                continue;
                            }

                            let channel_id = post
                                .get("channel_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");

                            let root_id = post
                                .get("root_id")
                                .and_then(|v| v.as_str())
                                .filter(|s| !s.is_empty())
                                .map(|s| s.to_string());

                            let parent_id = post
                                .get("parent_id")
                                .and_then(|v| v.as_str())
                                .filter(|s| !s.is_empty())
                                .map(|s| s.to_string());

                            let channel_msg = ChannelMessage {
                                id: post
                                    .get("id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                sender: user_id.to_string(),
                                content: post
                                    .get("message")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                channel: "mattermost".to_string(),
                                reply_target: parent_id,
                                timestamp: chrono::Utc::now(),
                                thread_ts: root_id,
                                metadata: serde_json::json!({
                                    "channel_id": channel_id,
                                    "sender_name": payload.pointer("/data/sender_name")
                                        .and_then(|v| v.as_str()).unwrap_or(""),
                                    "channel_type": payload.pointer("/data/channel_type")
                                        .and_then(|v| v.as_str()).unwrap_or(""),
                                    "team_id": payload.pointer("/data/team_id")
                                        .and_then(|v| v.as_str()).unwrap_or(""),
                                }),
                                chat_type: ChatType::default(),
                                bot_mentioned: false,
                                group_id: None,
                            };

                            if tx.send(channel_msg).await.is_err() {
                                info!("Mattermost listener: receiver dropped, stopping");
                                return Ok(());
                            }
                        }
                        "" => {
                            // Status response (e.g., auth response)
                            let status =
                                payload.get("status").and_then(|v| v.as_str()).unwrap_or("");
                            if status == "OK" {
                                debug!("Mattermost WS status OK");
                            } else if !status.is_empty() {
                                warn!(status, "Mattermost WS unexpected status");
                            }
                        }
                        _ => {
                            debug!(event, "Mattermost event (ignored)");
                        }
                    }
                }

                warn!("Mattermost WebSocket disconnected, reconnecting in 5s");
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }

        #[cfg(not(feature = "mattermost"))]
        {
            let _ = tx;
            warn!("Mattermost channel requires the 'mattermost' feature");
            std::future::pending::<()>().await;
            Ok(())
        }
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        self.ensure_user_id().await?;
        Ok(())
    }

    async fn start_typing(&self, recipient: &str) -> Result<(), AttaError> {
        // Typing is only indicated via WebSocket, not REST
        // No-op until WebSocket is wired
        debug!(channel = %recipient, "Mattermost typing (no-op without WebSocket)");
        Ok(())
    }

    async fn add_reaction(&self, message_id: &str, reaction: &str) -> Result<(), AttaError> {
        let user_id = self.ensure_user_id().await?;

        let body = serde_json::json!({
            "user_id": user_id,
            "post_id": message_id,
            "emoji_name": reaction,
        });

        self.api_post("/reactions", &body).await?;
        Ok(())
    }

    async fn remove_reaction(&self, message_id: &str, reaction: &str) -> Result<(), AttaError> {
        let user_id = self.ensure_user_id().await?;
        let url = self.api_url(&format!(
            "/users/{}/posts/{}/reactions/{}",
            user_id, message_id, reaction
        ));

        self.client
            .delete(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        Ok(())
    }

    fn supports_draft_updates(&self) -> bool {
        true
    }

    async fn send_draft(&self, message: SendMessage) -> Result<String, AttaError> {
        let channel_id = &message.recipient;

        let mut body = serde_json::json!({
            "channel_id": channel_id,
            "message": message.content,
        });

        if let Some(ref thread_ts) = message.thread_ts {
            body["root_id"] = serde_json::json!(thread_ts);
        }

        let result = self.api_post("/posts", &body).await?;
        let post_id = result
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        Ok(post_id)
    }

    async fn update_draft(&self, draft_id: &str, content: &str) -> Result<(), AttaError> {
        let url = self.api_url(&format!("/posts/{}/patch", draft_id));

        let body = serde_json::json!({
            "message": content,
        });

        self.client
            .put(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Content-Type", "application/json")
            .json(&body)
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
    fn test_mattermost_channel_name() {
        let ch = MattermostChannel::new(
            "https://mattermost.example.com".to_string(),
            "token".to_string(),
        );
        assert_eq!(ch.name(), "mattermost");
    }

    #[test]
    fn test_ws_url_https() {
        let ch = MattermostChannel::new("https://mm.example.com".to_string(), "token".to_string());
        assert_eq!(ch.ws_url(), "wss://mm.example.com/api/v4/websocket");
    }

    #[test]
    fn test_ws_url_http() {
        let ch = MattermostChannel::new("http://localhost:8065".to_string(), "token".to_string());
        assert_eq!(ch.ws_url(), "ws://localhost:8065/api/v4/websocket");
    }

    #[test]
    fn test_supports_draft() {
        let ch = MattermostChannel::new("https://mm.test".to_string(), "t".to_string());
        assert!(ch.supports_draft_updates());
    }

    #[tokio::test]
    async fn test_health_check_unreachable() {
        let ch = MattermostChannel::new("http://127.0.0.1:1".to_string(), "token".to_string());
        assert!(ch.health_check().await.is_err());
    }
}
