//! Lark (Feishu) channel
//!
//! Uses the Lark Open Platform APIs for messaging. Authentication is via
//! App ID / App Secret to obtain a tenant_access_token. Incoming messages
//! are received via WebSocket event subscription.

use std::sync::Arc;

use atta_types::AttaError;
use reqwest::Client;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::traits::{Channel, ChannelMessage, ChatType, SendMessage};

const LARK_API_BASE: &str = "https://open.feishu.cn/open-apis";
const LARK_TOKEN_URL: &str =
    "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal";

/// Cached tenant access token with expiry
struct TokenCache {
    token: String,
    expires_at: chrono::DateTime<chrono::Utc>,
}

/// Lark (Feishu) channel
pub struct LarkChannel {
    name: String,
    /// App ID
    app_id: String,
    /// App Secret
    app_secret: String,
    /// API base URL (overridable for testing)
    api_base: String,
    /// Token endpoint URL (overridable for testing)
    token_url: String,
    /// HTTP client
    client: Client,
    /// Cached tenant access token
    token_cache: Arc<RwLock<Option<TokenCache>>>,
}

impl LarkChannel {
    /// Create a new Lark channel
    pub fn new(app_id: String, app_secret: String) -> Self {
        Self {
            name: "lark".to_string(),
            app_id,
            app_secret,
            api_base: LARK_API_BASE.to_string(),
            token_url: LARK_TOKEN_URL.to_string(),
            client: Client::new(),
            token_cache: Arc::new(RwLock::new(None)),
        }
    }

    /// Override API base URLs (for testing with wiremock)
    pub fn with_api_base(mut self, api_base: String) -> Self {
        self.token_url = format!("{}/auth/v3/tenant_access_token/internal", api_base);
        self.api_base = api_base;
        self
    }

    /// Get or refresh the tenant_access_token
    async fn get_token(&self) -> Result<String, AttaError> {
        // Check cache first
        {
            let cache = self.token_cache.read().await;
            if let Some(ref cached) = *cache {
                if cached.expires_at > chrono::Utc::now() {
                    return Ok(cached.token.clone());
                }
            }
        }

        // Refresh token
        let body = serde_json::json!({
            "app_id": self.app_id,
            "app_secret": self.app_secret,
        });

        let response = self
            .client
            .post(&self.token_url)
            .header("Content-Type", "application/json; charset=utf-8")
            .json(&body)
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        let code = result.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            let msg = result
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            return Err(AttaError::Other(anyhow::anyhow!(
                "Lark token error (code={}): {}",
                code,
                msg
            )));
        }

        let token = result
            .get("tenant_access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AttaError::Other(anyhow::anyhow!("Lark: missing tenant_access_token")))?
            .to_string();

        let expire = result
            .get("expire")
            .and_then(|v| v.as_i64())
            .unwrap_or(7200);

        // Cache with a 5-minute safety margin
        let expires_at = chrono::Utc::now() + chrono::Duration::seconds(expire.saturating_sub(300));

        *self.token_cache.write().await = Some(TokenCache {
            token: token.clone(),
            expires_at,
        });

        Ok(token)
    }

    /// Make an authenticated API call
    async fn api_post(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, AttaError> {
        let token = self.get_token().await?;
        let url = format!("{}{}", self.api_base, path);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json; charset=utf-8")
            .json(body)
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AttaError::Other(anyhow::anyhow!(
                "Lark {} HTTP {}: {}",
                path,
                status,
                text
            )));
        }

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        let code = result.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            let msg = result
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            return Err(AttaError::Other(anyhow::anyhow!(
                "Lark {} error (code={}): {}",
                path,
                code,
                msg
            )));
        }

        Ok(result)
    }
}

#[async_trait::async_trait]
impl Channel for LarkChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: SendMessage) -> Result<(), AttaError> {
        // Determine receive_id_type from recipient format
        // Lark supports: open_id, user_id, union_id, email, chat_id
        let receive_id_type = if message.recipient.starts_with("oc_") {
            "chat_id"
        } else if message.recipient.starts_with("ou_") {
            "open_id"
        } else if message.recipient.contains('@') {
            "email"
        } else {
            "open_id"
        };

        let path = format!("/im/v1/messages?receive_id_type={}", receive_id_type);

        let mut body = serde_json::json!({
            "receive_id": message.recipient,
            "msg_type": "text",
            "content": serde_json::json!({"text": message.content}).to_string(),
        });

        // Reply in thread if thread_ts is provided
        if let Some(ref thread_ts) = message.thread_ts {
            // For replies, use the reply endpoint
            let reply_path = format!("/im/v1/messages/{}/reply", thread_ts);
            let reply_body = serde_json::json!({
                "msg_type": "text",
                "content": serde_json::json!({"text": message.content}).to_string(),
            });
            self.api_post(&reply_path, &reply_body).await?;
            debug!("Lark reply sent in thread {}", thread_ts);
            return Ok(());
        }

        self.api_post(&path, &body).await?;
        debug!("Lark message sent to {}", message.recipient);
        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        #[cfg(feature = "lark")]
        {
            use futures::stream::StreamExt;
            use futures::SinkExt;
            use tokio_tungstenite::{connect_async, tungstenite::Message};

            loop {
                let token = self.get_token().await?;
                let ws_url = format!(
                    "wss://open.feishu.cn/event/ws?app_id={}&token={}",
                    self.app_id, token
                );
                info!("Lark WebSocket connecting");

                let connect_result = connect_async(&ws_url).await;
                let (ws_stream, _) = match connect_result {
                    Ok(s) => s,
                    Err(e) => {
                        warn!(error = %e, "Lark WS connect failed, retrying in 5s");
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                        continue;
                    }
                };

                let (mut write, mut read) = ws_stream.split();

                info!("Lark WebSocket connected");

                while let Some(msg_result) = read.next().await {
                    let msg = match msg_result {
                        Ok(m) => m,
                        Err(e) => {
                            warn!(error = %e, "Lark WS read error, reconnecting");
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
                            info!("Lark WS closed, reconnecting");
                            break;
                        }
                        _ => continue,
                    };

                    let payload: serde_json::Value = match serde_json::from_str(&text) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    // Check for im.message.receive_v1 events
                    let event_type = payload
                        .pointer("/header/event_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");

                    if event_type == "im.message.receive_v1" {
                        if let Some(event) = payload.get("event") {
                            let message = event.get("message").unwrap_or(&serde_json::Value::Null);
                            let sender = event.get("sender").unwrap_or(&serde_json::Value::Null);

                            let message_id = message
                                .get("message_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();

                            let sender_id = sender
                                .pointer("/sender_id/open_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();

                            // Parse content JSON to extract text
                            let content_str = message
                                .get("content")
                                .and_then(|v| v.as_str())
                                .unwrap_or("{}");
                            let content_json: serde_json::Value =
                                serde_json::from_str(content_str).unwrap_or_default();
                            let text_content = content_json
                                .get("text")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();

                            let thread_ts = message
                                .get("root_id")
                                .and_then(|v| v.as_str())
                                .filter(|s| !s.is_empty())
                                .map(|s| s.to_string());

                            let channel_msg = ChannelMessage {
                                id: message_id,
                                sender: sender_id,
                                content: text_content,
                                channel: "lark".to_string(),
                                reply_target: message
                                    .get("parent_id")
                                    .and_then(|v| v.as_str())
                                    .filter(|s| !s.is_empty())
                                    .map(|s| s.to_string()),
                                timestamp: chrono::Utc::now(),
                                thread_ts,
                                metadata: serde_json::json!({
                                    "chat_id": message.get("chat_id")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or(""),
                                    "chat_type": message.get("chat_type")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or(""),
                                }),
                                chat_type: ChatType::default(),
                                bot_mentioned: false,
                                group_id: None,
                            };

                            if tx.send(channel_msg).await.is_err() {
                                info!("Lark listener: receiver dropped, stopping");
                                return Ok(());
                            }
                        }
                    }
                }

                warn!("Lark WebSocket disconnected, reconnecting in 5s");
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }

        #[cfg(not(feature = "lark"))]
        {
            let _ = tx;
            warn!("Lark WebSocket requires the 'lark' feature");
            std::future::pending::<()>().await;
            Ok(())
        }
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        // Verify we can obtain a valid token
        self.get_token().await?;
        Ok(())
    }

    async fn add_reaction(&self, message_id: &str, reaction: &str) -> Result<(), AttaError> {
        let path = format!("/im/v1/messages/{}/reactions", message_id);
        let body = serde_json::json!({
            "reaction_type": {
                "emoji_type": reaction,
            }
        });

        self.api_post(&path, &body).await?;
        Ok(())
    }

    async fn remove_reaction(&self, message_id: &str, reaction: &str) -> Result<(), AttaError> {
        // Lark doesn't have a direct "remove reaction by emoji" API;
        // you need the reaction_id. This is a best-effort implementation.
        let path = format!(
            "/im/v1/messages/{}/reactions?reaction_type.emoji_type={}",
            message_id, reaction
        );

        // DELETE request for reaction removal
        let token = self.get_token().await?;
        let url = format!("{}{}", self.api_base, path);

        self.client
            .delete(&url)
            .header("Authorization", format!("Bearer {}", token))
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
    fn test_lark_channel_name() {
        let ch = LarkChannel::new("app-id".to_string(), "app-secret".to_string());
        assert_eq!(ch.name(), "lark");
    }

    #[tokio::test]
    async fn test_get_token_invalid_creds() {
        let ch = LarkChannel::new("invalid".to_string(), "invalid".to_string());
        // Should fail with invalid credentials — just verify no panic
        let _ = ch.get_token().await;
    }
}
