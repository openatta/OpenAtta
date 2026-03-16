//! DingTalk channel
//!
//! Uses the DingTalk Open Platform APIs for bot messaging. Authentication
//! is via app_key / app_secret to obtain an access token. Incoming messages
//! are received via HTTP callback or long-polling.

use std::sync::Arc;

use atta_types::AttaError;
use reqwest::Client;
use tokio::sync::RwLock;
use tracing::{debug, error, warn};

use crate::traits::{Channel, ChannelMessage, ChatType, SendMessage};

const DINGTALK_API_BASE: &str = "https://oapi.dingtalk.com";
const DINGTALK_NEW_API_BASE: &str = "https://api.dingtalk.com";
const DINGTALK_TOKEN_URL: &str = "https://oapi.dingtalk.com/gettoken";

/// Cached access token with expiry
struct TokenCache {
    token: String,
    expires_at: chrono::DateTime<chrono::Utc>,
}

/// DingTalk channel
pub struct DingtalkChannel {
    name: String,
    /// App Key (formerly CorpId)
    app_key: String,
    /// App Secret
    app_secret: String,
    /// Optional webhook URL for outgoing notifications
    webhook_url: Option<String>,
    /// Token endpoint URL (overridable for testing)
    token_url: String,
    /// New API base URL (overridable for testing)
    new_api_base: String,
    /// HTTP client
    client: Client,
    /// Cached access token
    token_cache: Arc<RwLock<Option<TokenCache>>>,
    /// Incoming message sender (for webhook push model)
    incoming_tx: tokio::sync::Mutex<Option<tokio::sync::mpsc::Sender<ChannelMessage>>>,
}

impl DingtalkChannel {
    /// Create a new DingTalk channel
    pub fn new(app_key: String, app_secret: String, webhook_url: Option<String>) -> Self {
        Self {
            name: "dingtalk".to_string(),
            app_key,
            app_secret,
            webhook_url,
            token_url: DINGTALK_TOKEN_URL.to_string(),
            new_api_base: DINGTALK_NEW_API_BASE.to_string(),
            client: Client::new(),
            token_cache: Arc::new(RwLock::new(None)),
            incoming_tx: tokio::sync::Mutex::new(None),
        }
    }

    /// Override API base URLs (for testing with wiremock)
    pub fn with_api_base(mut self, api_base: String) -> Self {
        self.token_url = format!("{}/gettoken", api_base);
        self.new_api_base = api_base;
        self
    }

    /// Push an incoming message from the HTTP callback handler
    pub async fn push_incoming(&self, message: ChannelMessage) -> Result<(), AttaError> {
        let guard = self.incoming_tx.lock().await;
        if let Some(tx) = guard.as_ref() {
            tx.send(message).await.map_err(|_| {
                AttaError::ChannelCapacityExhausted("dingtalk incoming channel full".to_string())
            })?;
        }
        Ok(())
    }

    /// Get or refresh the access token
    async fn get_token(&self) -> Result<String, AttaError> {
        // Check cache
        {
            let cache = self.token_cache.read().await;
            if let Some(ref cached) = *cache {
                if cached.expires_at > chrono::Utc::now() {
                    return Ok(cached.token.clone());
                }
            }
        }

        // Refresh token
        let url = format!(
            "{}?appkey={}&appsecret={}",
            self.token_url, self.app_key, self.app_secret
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        let errcode = result.get("errcode").and_then(|v| v.as_i64()).unwrap_or(-1);
        if errcode != 0 {
            let errmsg = result
                .get("errmsg")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            return Err(AttaError::Other(anyhow::anyhow!(
                "DingTalk token error (errcode={}): {}",
                errcode,
                errmsg
            )));
        }

        let token = result
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AttaError::Other(anyhow::anyhow!(
                    "DingTalk: missing access_token in response"
                ))
            })?
            .to_string();

        let expires_in = result
            .get("expires_in")
            .and_then(|v| v.as_i64())
            .unwrap_or(7200);

        let expires_at =
            chrono::Utc::now() + chrono::Duration::seconds(expires_in.saturating_sub(300));

        *self.token_cache.write().await = Some(TokenCache {
            token: token.clone(),
            expires_at,
        });

        Ok(token)
    }
}

#[async_trait::async_trait]
impl Channel for DingtalkChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: SendMessage) -> Result<(), AttaError> {
        // If webhook_url is set, prefer sending via webhook (for group bots)
        if let Some(ref webhook_url) = self.webhook_url {
            let body = serde_json::json!({
                "msgtype": "text",
                "text": {
                    "content": message.content,
                },
            });

            let response = self
                .client
                .post(webhook_url)
                .json(&body)
                .send()
                .await
                .map_err(|e| AttaError::Other(e.into()))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                return Err(AttaError::Other(anyhow::anyhow!(
                    "DingTalk webhook send failed HTTP {}: {}",
                    status,
                    text
                )));
            }

            debug!("DingTalk webhook message sent");
            return Ok(());
        }

        // Otherwise, send via the DingTalk API (for 1:1 bot messages)
        let token = self.get_token().await?;
        let url = format!("{}/v1.0/robot/oToMessages/batchSend", self.new_api_base);

        let body = serde_json::json!({
            "robotCode": self.app_key,
            "userIds": [message.recipient],
            "msgKey": "sampleText",
            "msgParam": serde_json::json!({"content": message.content}).to_string(),
        });

        let response = self
            .client
            .post(&url)
            .header("x-acs-dingtalk-access-token", &token)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AttaError::Other(anyhow::anyhow!(
                "DingTalk API send failed HTTP {}: {}",
                status,
                text
            )));
        }

        debug!("DingTalk message sent to {}", message.recipient);
        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        // DingTalk primarily uses HTTP callback for incoming messages.
        // The HTTP handler in the API router should call push_incoming().
        //
        // Callback payload format:
        // {
        //   "msgtype": "text",
        //   "text": { "content": "..." },
        //   "msgId": "...",
        //   "createAt": 1234567890123,
        //   "conversationType": "1",       // 1=1:1, 2=group
        //   "conversationId": "...",
        //   "senderId": "...",
        //   "senderNick": "...",
        //   "chatbotUserId": "...",
        //   "atUsers": [...]
        // }
        //
        // For Enterprise bots, you can also use Stream mode (long connection)
        // via the DingTalk Stream SDK.

        *self.incoming_tx.lock().await = Some(tx);
        debug!("DingTalk listener started (webhook push model)");
        std::future::pending::<()>().await;
        Ok(())
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        self.get_token().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dingtalk_channel_name() {
        let ch = DingtalkChannel::new("app-key".to_string(), "app-secret".to_string(), None);
        assert_eq!(ch.name(), "dingtalk");
    }

    #[test]
    fn test_dingtalk_with_webhook() {
        let ch = DingtalkChannel::new(
            "key".to_string(),
            "secret".to_string(),
            Some("https://oapi.dingtalk.com/robot/send?access_token=xxx".to_string()),
        );
        assert_eq!(ch.name(), "dingtalk");
    }

    #[tokio::test]
    async fn test_get_token_invalid_creds() {
        let ch = DingtalkChannel::new("invalid".to_string(), "invalid".to_string(), None);
        let _ = ch.get_token().await;
    }
}
