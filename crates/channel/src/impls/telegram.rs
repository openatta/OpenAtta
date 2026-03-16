//! Telegram Bot channel
//!
//! Uses HTTP long-polling via `getUpdates` for incoming messages and the
//! Bot API for sending. Requires a bot token obtained from @BotFather.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};

use atta_types::AttaError;
use reqwest::Client;
use tracing::{debug, error, warn};

use crate::traits::{Channel, ChannelMessage, ChatType, SendMessage};

const TELEGRAM_API_BASE: &str = "https://api.telegram.org";
const POLL_TIMEOUT_SECS: u64 = 30;

/// Telegram Bot channel via HTTP long-polling
pub struct TelegramChannel {
    name: String,
    /// Bot token (e.g. "123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11")
    bot_token: String,
    /// HTTP client
    client: Client,
    /// Current getUpdates offset (next update_id to fetch)
    offset: AtomicI64,
    /// API base URL (overridable for testing)
    api_base: String,
    /// Webhook secret token for signature verification
    webhook_secret: Option<String>,
    /// Bot username for mention detection (e.g. "my_bot")
    bot_user: Option<String>,
}

impl TelegramChannel {
    /// Create a new Telegram channel with the given bot token
    pub fn new(bot_token: String) -> Self {
        Self {
            name: "telegram".to_string(),
            bot_token,
            client: Client::new(),
            offset: AtomicI64::new(0),
            api_base: TELEGRAM_API_BASE.to_string(),
            webhook_secret: None,
            bot_user: None,
        }
    }

    /// Override the API base URL (for testing with mock servers)
    pub fn with_api_base(mut self, api_base: String) -> Self {
        self.api_base = api_base;
        self
    }

    /// Set the webhook secret token for verifying inbound webhook requests.
    pub fn with_webhook_secret(mut self, secret: String) -> Self {
        self.webhook_secret = Some(secret);
        self
    }

    /// Set the bot username for mention detection in groups.
    pub fn with_bot_username(mut self, username: String) -> Self {
        self.bot_user = Some(username);
        self
    }

    /// Build the API URL for a given method
    fn api_url(&self, method: &str) -> String {
        format!("{}/bot{}/{}", self.api_base, self.bot_token, method)
    }

    /// Determine chat type from Telegram chat object
    fn parse_chat_type(chat: &serde_json::Value) -> ChatType {
        match chat.get("type").and_then(|v| v.as_str()) {
            Some("private") => ChatType::Dm,
            Some("group") => ChatType::Group,
            Some("supergroup") => ChatType::SuperGroup,
            Some("channel") => ChatType::Channel,
            _ => ChatType::Unknown,
        }
    }

    /// Check if the bot is mentioned in entities
    fn is_bot_mentioned(&self, msg: &serde_json::Value, text: &str) -> bool {
        let bot_user = match &self.bot_user {
            Some(u) => u,
            None => return false,
        };

        // Check @username mentions in entities
        if let Some(entities) = msg.get("entities").and_then(|v| v.as_array()) {
            for entity in entities {
                if entity.get("type").and_then(|v| v.as_str()) == Some("mention") {
                    // Extract mention text from the message
                    let offset = entity.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    let length = entity.get("length").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    if let Some(mention) = text.get(offset..offset + length) {
                        if mention.trim_start_matches('@').eq_ignore_ascii_case(bot_user) {
                            return true;
                        }
                    }
                }
                // Also check bot_command type which implies addressing the bot
                if entity.get("type").and_then(|v| v.as_str()) == Some("bot_command") {
                    return true;
                }
            }
        }

        // Fallback: check if text contains @bot_user
        text.to_lowercase()
            .contains(&format!("@{}", bot_user.to_lowercase()))
    }
}

#[async_trait::async_trait]
impl Channel for TelegramChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: SendMessage) -> Result<(), AttaError> {
        let mut body = serde_json::json!({
            "chat_id": message.recipient,
            "text": message.content,
        });

        if let Some(ref thread_ts) = message.thread_ts {
            if let Ok(msg_id) = thread_ts.parse::<i64>() {
                body["reply_to_message_id"] = serde_json::json!(msg_id);
            }
        }

        let response = self
            .client
            .post(self.api_url("sendMessage"))
            .json(&body)
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AttaError::Other(anyhow::anyhow!(
                "Telegram sendMessage failed HTTP {}: {}",
                status,
                text
            )));
        }

        debug!("Telegram message sent to {}", message.recipient);
        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        debug!("Telegram long-polling started");

        loop {
            let current_offset = self.offset.load(Ordering::Relaxed);
            let url = self.api_url("getUpdates");

            let mut params = serde_json::json!({
                "timeout": POLL_TIMEOUT_SECS,
                "allowed_updates": ["message"],
            });
            if current_offset > 0 {
                params["offset"] = serde_json::json!(current_offset);
            }

            let response = match self.client.post(&url).json(&params).send().await {
                Ok(r) => r,
                Err(e) => {
                    warn!(error = %e, "Telegram getUpdates request failed, retrying");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                error!("Telegram getUpdates HTTP {}: {}", status, text);
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }

            let body: serde_json::Value = match response.json().await {
                Ok(v) => v,
                Err(e) => {
                    error!(error = %e, "Telegram response parse error");
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                }
            };

            let ok = body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
            if !ok {
                error!("Telegram API returned ok=false: {}", body);
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }

            let updates = match body.get("result").and_then(|v| v.as_array()) {
                Some(arr) => arr,
                None => continue,
            };

            for update in updates {
                let update_id = update
                    .get("update_id")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);

                // Always advance offset past the latest update
                self.offset.fetch_max(update_id + 1, Ordering::Relaxed);

                // Extract message (could be "message" or "edited_message")
                let msg = update
                    .get("message")
                    .or_else(|| update.get("edited_message"));

                let msg = match msg {
                    Some(m) => m,
                    None => continue,
                };

                let text = msg
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if text.is_empty() {
                    continue;
                }

                let chat_id = msg
                    .pointer("/chat/id")
                    .and_then(|v| v.as_i64())
                    .map(|id| id.to_string())
                    .unwrap_or_default();

                let sender = msg
                    .pointer("/from/id")
                    .and_then(|v| v.as_i64())
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| chat_id.clone());

                let message_id = msg
                    .get("message_id")
                    .and_then(|v| v.as_i64())
                    .map(|id| id.to_string())
                    .unwrap_or_default();

                let reply_to = msg
                    .pointer("/reply_to_message/message_id")
                    .and_then(|v| v.as_i64())
                    .map(|id| id.to_string());

                let timestamp = msg
                    .get("date")
                    .and_then(|v| v.as_i64())
                    .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                    .unwrap_or_else(chrono::Utc::now);

                // Determine chat type and group ID
                let chat = msg.get("chat").unwrap_or(&serde_json::Value::Null);
                let chat_type = Self::parse_chat_type(chat);
                let group_id = match chat_type {
                    ChatType::Group | ChatType::SuperGroup => Some(chat_id.clone()),
                    _ => None,
                };

                // Detect bot mention
                let bot_mentioned = self.is_bot_mentioned(msg, &text);

                let channel_msg = ChannelMessage {
                    id: message_id.clone(),
                    sender,
                    content: text,
                    channel: "telegram".to_string(),
                    reply_target: reply_to,
                    timestamp,
                    thread_ts: Some(message_id),
                    metadata: serde_json::json!({
                        "chat_id": chat_id,
                        "update_id": update_id,
                    }),
                    chat_type,
                    bot_mentioned,
                    group_id,
                };

                if tx.send(channel_msg).await.is_err() {
                    debug!("Telegram listener: receiver dropped, stopping");
                    return Ok(());
                }
            }
        }
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        let response = self
            .client
            .get(self.api_url("getMe"))
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            return Err(AttaError::Other(anyhow::anyhow!(
                "Telegram health check failed: HTTP {}",
                response.status()
            )));
        }

        Ok(())
    }

    async fn start_typing(&self, recipient: &str) -> Result<(), AttaError> {
        let body = serde_json::json!({
            "chat_id": recipient,
            "action": "typing",
        });

        self.client
            .post(self.api_url("sendChatAction"))
            .json(&body)
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        Ok(())
    }

    async fn add_reaction(&self, message_id: &str, reaction: &str) -> Result<(), AttaError> {
        // Telegram setMessageReaction requires chat_id; we encode it in message_id as "chat:msg"
        let parts: Vec<&str> = message_id.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(AttaError::Validation(
                "Telegram reaction requires message_id in 'chat_id:message_id' format".to_string(),
            ));
        }

        let body = serde_json::json!({
            "chat_id": parts[0],
            "message_id": parts[1].parse::<i64>().unwrap_or(0),
            "reaction": [{
                "type": "emoji",
                "emoji": reaction,
            }],
        });

        self.client
            .post(self.api_url("setMessageReaction"))
            .json(&body)
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        Ok(())
    }

    fn supports_draft_updates(&self) -> bool {
        true
    }

    async fn send_draft(&self, message: SendMessage) -> Result<String, AttaError> {
        // Send message and return "{chat_id}:{message_id}" as draft_id
        let body = serde_json::json!({
            "chat_id": message.recipient,
            "text": message.content,
            "parse_mode": "Markdown",
        });

        let resp = self
            .client
            .post(&self.api_url("sendMessage"))
            .json(&body)
            .send()
            .await
            .map_err(|e| AttaError::Other(anyhow::anyhow!("telegram send_draft failed: {e}")))?;

        let data: serde_json::Value = resp.json().await.map_err(|e| {
            AttaError::Other(anyhow::anyhow!("telegram send_draft parse failed: {e}"))
        })?;

        let chat_id = data["result"]["chat"]["id"].as_i64().unwrap_or(0);
        let message_id = data["result"]["message_id"].as_i64().unwrap_or(0);

        Ok(format!("{chat_id}:{message_id}"))
    }

    async fn update_draft(&self, draft_id: &str, content: &str) -> Result<(), AttaError> {
        let parts: Vec<&str> = draft_id.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(AttaError::Validation("invalid draft_id format".to_string()));
        }

        // Telegram message limit: 4096 bytes
        let truncated = crate::draft::utf8_truncate(content, 4096);

        let body = serde_json::json!({
            "chat_id": parts[0],
            "message_id": parts[1].parse::<i64>().unwrap_or(0),
            "text": truncated,
            "parse_mode": "Markdown",
        });

        self.client
            .post(&self.api_url("editMessageText"))
            .json(&body)
            .send()
            .await
            .map_err(|e| AttaError::Other(anyhow::anyhow!("telegram update_draft failed: {e}")))?;

        Ok(())
    }

    async fn finalize_draft(&self, _draft_id: &str) -> Result<(), AttaError> {
        // No special finalization needed for Telegram
        Ok(())
    }

    fn verify_webhook_signature(
        &self,
        headers: &HashMap<String, String>,
        _body: &[u8],
    ) -> Result<bool, AttaError> {
        let Some(ref secret) = self.webhook_secret else {
            // No secret configured — accept all
            return Ok(true);
        };

        // Telegram sends the secret in X-Telegram-Bot-Api-Secret-Token header
        match headers.get("x-telegram-bot-api-secret-token") {
            Some(token) if token == secret => Ok(true),
            Some(_) => Ok(false),
            None => Ok(false),
        }
    }

    fn bot_username(&self) -> Option<&str> {
        self.bot_user.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telegram_channel_name() {
        let ch = TelegramChannel::new("test-token".to_string());
        assert_eq!(ch.name(), "telegram");
    }

    #[test]
    fn test_api_url() {
        let ch = TelegramChannel::new("123:ABC".to_string());
        assert_eq!(
            ch.api_url("getUpdates"),
            "https://api.telegram.org/bot123:ABC/getUpdates"
        );
    }

    #[tokio::test]
    async fn test_health_check_requires_valid_token() {
        // With a fake token, health_check should fail (connection or 401)
        let ch = TelegramChannel::new("invalid-token".to_string());
        // We don't assert success — just verify it doesn't panic
        let _ = ch.health_check().await;
    }
}
