//! WhatsApp Business API (Storage-based) channel
//!
//! Integrates with the WhatsApp Business API using a storage-based approach
//! where messages and media are managed through REST APIs. Suitable for
//! on-premise WhatsApp Business API deployments.

use atta_types::AttaError;
use reqwest::Client;
use tracing::{debug, error, warn};

use crate::traits::{Channel, ChannelMessage, SendMessage};

/// WhatsApp Business API (storage-based) channel
pub struct WhatsappStorageChannel {
    name: String,
    /// API base URL (e.g., "https://waba.example.com/v1")
    api_url: String,
    /// API authentication token
    auth_token: String,
    /// HTTP client
    client: Client,
    /// Incoming message sender (for webhook push)
    incoming_tx: tokio::sync::Mutex<Option<tokio::sync::mpsc::Sender<ChannelMessage>>>,
}

impl WhatsappStorageChannel {
    /// Create a new WhatsApp Storage channel
    pub fn new(api_url: String, auth_token: String) -> Self {
        Self {
            name: "whatsapp_storage".to_string(),
            api_url: api_url.trim_end_matches('/').to_string(),
            auth_token,
            client: Client::new(),
            incoming_tx: tokio::sync::Mutex::new(None),
        }
    }

    /// Push an incoming webhook message
    pub async fn push_incoming(&self, message: ChannelMessage) -> Result<(), AttaError> {
        let guard = self.incoming_tx.lock().await;
        if let Some(tx) = guard.as_ref() {
            tx.send(message).await.map_err(|_| {
                AttaError::ChannelCapacityExhausted(
                    "whatsapp_storage incoming channel full".to_string(),
                )
            })?;
        }
        Ok(())
    }

    /// Upload media and return a media ID
    pub async fn upload_media(&self, data: &[u8], content_type: &str) -> Result<String, AttaError> {
        let url = format!("{}/media", self.api_url);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .header("Content-Type", content_type)
            .body(data.to_vec())
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AttaError::Other(anyhow::anyhow!(
                "WhatsApp media upload failed HTTP {}: {}",
                status,
                text
            )));
        }

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        result
            .pointer("/media/0/id")
            .or_else(|| result.get("id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| {
                AttaError::Other(anyhow::anyhow!(
                    "WhatsApp media upload: missing media ID in response"
                ))
            })
    }

    /// Download media by ID
    pub async fn download_media(&self, media_id: &str) -> Result<Vec<u8>, AttaError> {
        let url = format!("{}/media/{}", self.api_url, media_id);

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            return Err(AttaError::Other(anyhow::anyhow!(
                "WhatsApp media download failed: HTTP {}",
                response.status()
            )));
        }

        response
            .bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| AttaError::Other(e.into()))
    }
}

#[async_trait::async_trait]
impl Channel for WhatsappStorageChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: SendMessage) -> Result<(), AttaError> {
        let url = format!("{}/messages", self.api_url);

        let mut body = serde_json::json!({
            "to": message.recipient,
            "type": "text",
            "text": {
                "body": message.content,
            },
        });

        // Check metadata for media sending
        if let Some(media_type) = message.metadata.get("media_type").and_then(|v| v.as_str()) {
            if let Some(media_id) = message.metadata.get("media_id").and_then(|v| v.as_str()) {
                body = serde_json::json!({
                    "to": message.recipient,
                    "type": media_type,
                    media_type: {
                        "id": media_id,
                        "caption": message.content,
                    },
                });
            }
        }

        // Reply context
        if let Some(ref thread_ts) = message.thread_ts {
            body["context"] = serde_json::json!({
                "message_id": thread_ts,
            });
        }

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AttaError::Other(anyhow::anyhow!(
                "WhatsApp Storage send failed HTTP {}: {}",
                status,
                text
            )));
        }

        debug!("WhatsApp Storage message sent to {}", message.recipient);
        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        // On-premise WhatsApp Business API uses webhook callbacks.
        // The HTTP handler should be registered to receive:
        //
        // POST /v1/messages (incoming)
        // {
        //   "messages": [{
        //     "id": "ABGGFlA5FpafAgo6tHcNmNjXmuSf",
        //     "from": "1234567890",
        //     "timestamp": "1234567890",
        //     "type": "text",
        //     "text": { "body": "Hello" }
        //   }],
        //   "contacts": [{
        //     "profile": { "name": "User Name" },
        //     "wa_id": "1234567890"
        //   }]
        // }

        *self.incoming_tx.lock().await = Some(tx);
        debug!("WhatsApp Storage listener started (webhook push model)");
        std::future::pending::<()>().await;
        Ok(())
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        let url = format!("{}/health", self.api_url);
        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            return Err(AttaError::Other(anyhow::anyhow!(
                "WhatsApp Storage health check failed: HTTP {}",
                response.status()
            )));
        }

        Ok(())
    }

    async fn add_reaction(&self, message_id: &str, reaction: &str) -> Result<(), AttaError> {
        let parts: Vec<&str> = message_id.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(AttaError::Validation(
                "WhatsApp Storage reaction requires 'phone:message_id' format".to_string(),
            ));
        }

        let url = format!("{}/messages", self.api_url);
        let body = serde_json::json!({
            "to": parts[0],
            "type": "reaction",
            "reaction": {
                "message_id": parts[1],
                "emoji": reaction,
            },
        });

        self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.auth_token))
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
    fn test_whatsapp_storage_channel_name() {
        let ch = WhatsappStorageChannel::new(
            "https://waba.example.com/v1".to_string(),
            "token".to_string(),
        );
        assert_eq!(ch.name(), "whatsapp_storage");
    }

    #[test]
    fn test_api_url_trailing_slash_stripped() {
        let ch = WhatsappStorageChannel::new(
            "https://waba.example.com/v1/".to_string(),
            "token".to_string(),
        );
        assert_eq!(ch.api_url, "https://waba.example.com/v1");
    }

    #[tokio::test]
    async fn test_health_check_unreachable() {
        let ch = WhatsappStorageChannel::new("http://127.0.0.1:1".to_string(), "token".to_string());
        assert!(ch.health_check().await.is_err());
    }
}
