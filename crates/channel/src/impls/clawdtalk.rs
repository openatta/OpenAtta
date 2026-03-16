//! ClawdTalk channel
//!
//! Custom REST API channel for ClawdTalk messaging platform.
//! Uses API key authentication, REST for sending, and long-polling
//! for receiving incoming messages.

use atta_types::AttaError;
use reqwest::Client;
use tracing::{debug, error, info, warn};

use crate::traits::{Channel, ChannelMessage, ChatType, SendMessage};

/// ClawdTalk channel
pub struct ClawdtalkChannel {
    name: String,
    /// ClawdTalk API base URL
    api_url: String,
    /// API key for authentication
    api_key: String,
    /// Bot/agent identifier
    bot_id: String,
    /// HTTP client
    client: Client,
    /// Cursor for long-poll pagination
    poll_cursor: tokio::sync::Mutex<Option<String>>,
}

impl ClawdtalkChannel {
    /// Create a new ClawdTalk channel
    pub fn new(api_url: String, api_key: String, bot_id: String) -> Self {
        Self {
            name: "clawdtalk".to_string(),
            api_url: api_url.trim_end_matches('/').to_string(),
            api_key,
            bot_id,
            client: Client::new(),
            poll_cursor: tokio::sync::Mutex::new(None),
        }
    }

    /// Make an authenticated GET request
    async fn api_get(&self, path: &str) -> Result<serde_json::Value, AttaError> {
        let url = format!("{}{}", self.api_url, path);
        let response = self
            .client
            .get(&url)
            .header("X-API-Key", &self.api_key)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AttaError::Other(anyhow::anyhow!(
                "ClawdTalk GET {} HTTP {}: {}",
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
        let url = format!("{}{}", self.api_url, path);
        let response = self
            .client
            .post(&url)
            .header("X-API-Key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AttaError::Other(anyhow::anyhow!(
                "ClawdTalk POST {} HTTP {}: {}",
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
impl Channel for ClawdtalkChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: SendMessage) -> Result<(), AttaError> {
        let path = format!("/api/v1/bots/{}/messages", self.bot_id);

        let mut body = serde_json::json!({
            "recipient": message.recipient,
            "content": message.content,
        });

        if let Some(ref thread_ts) = message.thread_ts {
            body["thread_id"] = serde_json::json!(thread_ts);
        }

        if let Some(ref subject) = message.subject {
            body["subject"] = serde_json::json!(subject);
        }

        if !message.metadata.is_null() {
            body["metadata"] = message.metadata.clone();
        }

        self.api_post(&path, &body).await?;
        debug!("ClawdTalk message sent to {}", message.recipient);
        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        info!(
            api_url = %self.api_url,
            bot_id = %self.bot_id,
            "ClawdTalk long-poll listener starting"
        );

        loop {
            let cursor = self.poll_cursor.lock().await.clone();
            let mut path = format!("/api/v1/bots/{}/messages/poll?timeout=30", self.bot_id);

            if let Some(ref cursor) = cursor {
                path.push_str(&format!("&cursor={}", urlencoding::encode(cursor)));
            }

            let response = match self.api_get(&path).await {
                Ok(r) => r,
                Err(e) => {
                    warn!(error = %e, "ClawdTalk poll error, retrying");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            // Update cursor
            if let Some(new_cursor) = response.get("cursor").and_then(|v| v.as_str()) {
                *self.poll_cursor.lock().await = Some(new_cursor.to_string());
            }

            // Process messages
            let messages = match response.get("messages").and_then(|v| v.as_array()) {
                Some(arr) => arr,
                None => continue,
            };

            for msg in messages {
                let id = msg
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let sender = msg
                    .get("sender")
                    .or_else(|| msg.get("from"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let content = msg
                    .get("content")
                    .or_else(|| msg.get("text"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if content.is_empty() {
                    continue;
                }

                let timestamp_str = msg.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");

                let timestamp = chrono::DateTime::parse_from_rfc3339(timestamp_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now());

                let thread_id = msg
                    .get("thread_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let reply_to = msg
                    .get("reply_to")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let metadata = msg
                    .get("metadata")
                    .cloned()
                    .unwrap_or(serde_json::json!({}));

                let channel_msg = ChannelMessage {
                    id,
                    sender,
                    content,
                    channel: "clawdtalk".to_string(),
                    reply_target: reply_to,
                    timestamp,
                    thread_ts: thread_id,
                    metadata,
                    chat_type: ChatType::default(),
                    bot_mentioned: false,
                    group_id: None,
                };

                if tx.send(channel_msg).await.is_err() {
                    debug!("ClawdTalk listener: receiver dropped, stopping");
                    return Ok(());
                }
            }
        }
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        let path = format!("/api/v1/bots/{}/status", self.bot_id);
        self.api_get(&path).await?;
        Ok(())
    }

    async fn start_typing(&self, recipient: &str) -> Result<(), AttaError> {
        let path = format!("/api/v1/bots/{}/typing", self.bot_id);
        let body = serde_json::json!({
            "recipient": recipient,
            "typing": true,
        });
        let _ = self.api_post(&path, &body).await;
        Ok(())
    }

    async fn stop_typing(&self, recipient: &str) -> Result<(), AttaError> {
        let path = format!("/api/v1/bots/{}/typing", self.bot_id);
        let body = serde_json::json!({
            "recipient": recipient,
            "typing": false,
        });
        let _ = self.api_post(&path, &body).await;
        Ok(())
    }

    async fn add_reaction(&self, message_id: &str, reaction: &str) -> Result<(), AttaError> {
        let path = format!(
            "/api/v1/bots/{}/messages/{}/reactions",
            self.bot_id, message_id
        );
        let body = serde_json::json!({
            "emoji": reaction,
        });
        self.api_post(&path, &body).await?;
        Ok(())
    }

    async fn remove_reaction(&self, message_id: &str, reaction: &str) -> Result<(), AttaError> {
        let url = format!(
            "{}/api/v1/bots/{}/messages/{}/reactions/{}",
            self.api_url,
            self.bot_id,
            message_id,
            urlencoding::encode(reaction)
        );

        self.client
            .delete(&url)
            .header("X-API-Key", &self.api_key)
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        Ok(())
    }

    fn supports_draft_updates(&self) -> bool {
        true
    }

    async fn send_draft(&self, message: SendMessage) -> Result<String, AttaError> {
        let path = format!("/api/v1/bots/{}/messages", self.bot_id);

        let mut body = serde_json::json!({
            "recipient": message.recipient,
            "content": message.content,
            "draft": true,
        });

        if let Some(ref thread_ts) = message.thread_ts {
            body["thread_id"] = serde_json::json!(thread_ts);
        }

        let result = self.api_post(&path, &body).await?;
        let draft_id = result
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        Ok(draft_id)
    }

    async fn update_draft(&self, draft_id: &str, content: &str) -> Result<(), AttaError> {
        let path = format!("/api/v1/bots/{}/messages/{}", self.bot_id, draft_id);
        let body = serde_json::json!({
            "content": content,
        });

        let url = format!("{}{}", self.api_url, path);
        self.client
            .patch(&url)
            .header("X-API-Key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        Ok(())
    }

    async fn finalize_draft(&self, draft_id: &str) -> Result<(), AttaError> {
        let path = format!(
            "/api/v1/bots/{}/messages/{}/finalize",
            self.bot_id, draft_id
        );
        self.api_post(&path, &serde_json::json!({})).await?;
        Ok(())
    }

    async fn cancel_draft(&self, draft_id: &str) -> Result<(), AttaError> {
        let url = format!(
            "{}/api/v1/bots/{}/messages/{}",
            self.api_url, self.bot_id, draft_id
        );

        self.client
            .delete(&url)
            .header("X-API-Key", &self.api_key)
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
    fn test_clawdtalk_channel_name() {
        let ch = ClawdtalkChannel::new(
            "https://api.clawdtalk.example.com".to_string(),
            "api-key-123".to_string(),
            "bot-001".to_string(),
        );
        assert_eq!(ch.name(), "clawdtalk");
    }

    #[test]
    fn test_supports_draft() {
        let ch = ClawdtalkChannel::new(
            "https://api.example.com".to_string(),
            "key".to_string(),
            "bot".to_string(),
        );
        assert!(ch.supports_draft_updates());
    }

    #[tokio::test]
    async fn test_health_check_unreachable() {
        let ch = ClawdtalkChannel::new(
            "http://127.0.0.1:1".to_string(),
            "key".to_string(),
            "bot".to_string(),
        );
        assert!(ch.health_check().await.is_err());
    }
}
