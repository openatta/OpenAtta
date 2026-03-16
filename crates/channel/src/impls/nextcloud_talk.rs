//! Nextcloud Talk channel
//!
//! Integrates with Nextcloud Talk (Spreed) via the REST API.
//! Uses long-polling for incoming messages and POST for sending.
//! Authentication is via token-based auth (app password or login token).

use atta_types::AttaError;
use reqwest::Client;
use tracing::{debug, error, info, warn};

use crate::traits::{Channel, ChannelMessage, ChatType, SendMessage};

/// Nextcloud Talk channel
pub struct NextcloudTalkChannel {
    name: String,
    /// Nextcloud server URL (e.g., "https://cloud.example.com")
    server_url: String,
    /// Username
    username: String,
    /// Password or app-specific password
    password: String,
    /// Conversation token (room) to monitor
    conversation_token: String,
    /// HTTP client
    client: Client,
    /// Last known message ID for incremental polling
    last_known_message_id: tokio::sync::Mutex<Option<i64>>,
}

impl NextcloudTalkChannel {
    /// Create a new Nextcloud Talk channel
    pub fn new(
        server_url: String,
        username: String,
        password: String,
        conversation_token: String,
    ) -> Self {
        Self {
            name: "nextcloud_talk".to_string(),
            server_url: server_url.trim_end_matches('/').to_string(),
            username,
            password,
            conversation_token,
            client: Client::new(),
            last_known_message_id: tokio::sync::Mutex::new(None),
        }
    }

    /// Build an OCS API URL
    fn api_url(&self, path: &str) -> String {
        format!("{}/ocs/v2.php/apps/spreed/api/v1{}", self.server_url, path)
    }

    /// Make an authenticated GET request
    async fn api_get(&self, path: &str) -> Result<serde_json::Value, AttaError> {
        let url = self.api_url(path);
        let response = self
            .client
            .get(&url)
            .basic_auth(&self.username, Some(&self.password))
            .header("OCS-APIRequest", "true")
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AttaError::Other(anyhow::anyhow!(
                "Nextcloud Talk GET {} HTTP {}: {}",
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
            .basic_auth(&self.username, Some(&self.password))
            .header("OCS-APIRequest", "true")
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AttaError::Other(anyhow::anyhow!(
                "Nextcloud Talk POST {} HTTP {}: {}",
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
impl Channel for NextcloudTalkChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: SendMessage) -> Result<(), AttaError> {
        let token = if message.recipient.is_empty() {
            &self.conversation_token
        } else {
            &message.recipient
        };

        let path = format!("/chat/{}", token);

        let mut body = serde_json::json!({
            "message": message.content,
        });

        if let Some(ref thread_ts) = message.thread_ts {
            if let Ok(reply_to) = thread_ts.parse::<i64>() {
                body["replyTo"] = serde_json::json!(reply_to);
            }
        }

        self.api_post(&path, &body).await?;
        debug!("Nextcloud Talk message sent to conversation {}", token);
        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        info!(
            server = %self.server_url,
            conversation = %self.conversation_token,
            "Nextcloud Talk listener starting"
        );

        // First, get the latest message ID to only receive new messages
        let initial_path = format!("/chat/{}?lookIntoFuture=0&limit=1", self.conversation_token);

        match self.api_get(&initial_path).await {
            Ok(result) => {
                if let Some(messages) = result.pointer("/ocs/data").and_then(|v| v.as_array()) {
                    if let Some(last) = messages.last() {
                        if let Some(id) = last.get("id").and_then(|v| v.as_i64()) {
                            *self.last_known_message_id.lock().await = Some(id);
                        }
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "Failed to get initial message ID");
            }
        }

        // Long-poll for new messages
        loop {
            let last_id = self.last_known_message_id.lock().await.unwrap_or(0);

            let path = format!(
                "/chat/{}?lookIntoFuture=1&timeout=30&lastKnownMessageId={}",
                self.conversation_token, last_id
            );

            let response = match self.api_get(&path).await {
                Ok(r) => r,
                Err(e) => {
                    // 304 Not Modified is expected when no new messages
                    warn!(error = %e, "Nextcloud Talk poll error");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            let messages = match response.pointer("/ocs/data").and_then(|v| v.as_array()) {
                Some(arr) => arr,
                None => {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                }
            };

            for msg in messages {
                let msg_id = msg.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
                let msg_type = msg
                    .get("messageType")
                    .and_then(|v| v.as_str())
                    .unwrap_or("comment");

                // Only process regular comments, not system messages
                if msg_type != "comment" {
                    *self.last_known_message_id.lock().await = Some(msg_id);
                    continue;
                }

                let actor_id = msg
                    .get("actorId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                // Skip own messages
                if actor_id == self.username {
                    *self.last_known_message_id.lock().await = Some(msg_id);
                    continue;
                }

                let content = msg
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if content.is_empty() {
                    *self.last_known_message_id.lock().await = Some(msg_id);
                    continue;
                }

                let timestamp_epoch = msg.get("timestamp").and_then(|v| v.as_i64()).unwrap_or(0);

                let timestamp = chrono::DateTime::from_timestamp(timestamp_epoch, 0)
                    .unwrap_or_else(chrono::Utc::now);

                let reply_to = msg
                    .get("parent")
                    .and_then(|v| v.get("id"))
                    .and_then(|v| v.as_i64())
                    .map(|id| id.to_string());

                let channel_msg = ChannelMessage {
                    id: msg_id.to_string(),
                    sender: actor_id,
                    content,
                    channel: "nextcloud_talk".to_string(),
                    reply_target: reply_to,
                    timestamp,
                    thread_ts: None,
                    metadata: serde_json::json!({
                        "conversation_token": self.conversation_token,
                        "actor_display_name": msg.get("actorDisplayName")
                            .and_then(|v| v.as_str()).unwrap_or(""),
                    }),
                    chat_type: ChatType::default(),
                    bot_mentioned: false,
                    group_id: None,
                };

                if tx.send(channel_msg).await.is_err() {
                    debug!("Nextcloud Talk listener: receiver dropped, stopping");
                    return Ok(());
                }

                *self.last_known_message_id.lock().await = Some(msg_id);
            }
        }
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        // Check conversation exists and we have access
        let path = format!("/room/{}", self.conversation_token);
        self.api_get(&path).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nextcloud_talk_channel_name() {
        let ch = NextcloudTalkChannel::new(
            "https://cloud.example.com".to_string(),
            "admin".to_string(),
            "password".to_string(),
            "abcdefgh".to_string(),
        );
        assert_eq!(ch.name(), "nextcloud_talk");
    }

    #[test]
    fn test_api_url() {
        let ch = NextcloudTalkChannel::new(
            "https://cloud.example.com".to_string(),
            "admin".to_string(),
            "pass".to_string(),
            "token".to_string(),
        );
        assert_eq!(
            ch.api_url("/chat/token"),
            "https://cloud.example.com/ocs/v2.php/apps/spreed/api/v1/chat/token"
        );
    }

    #[tokio::test]
    async fn test_health_check_unreachable() {
        let ch = NextcloudTalkChannel::new(
            "http://127.0.0.1:1".to_string(),
            "admin".to_string(),
            "pass".to_string(),
            "token".to_string(),
        );
        assert!(ch.health_check().await.is_err());
    }
}
