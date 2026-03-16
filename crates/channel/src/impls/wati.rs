//! WATI WhatsApp API channel
//!
//! Integrates with WATI (WhatsApp Team Inbox) for business messaging.
//! Uses the WATI REST API for sending messages and webhook callbacks
//! for receiving incoming messages.

use atta_types::AttaError;
use reqwest::Client;
use tracing::{debug, error, warn};

use crate::traits::{Channel, ChannelMessage, ChatType, SendMessage};

/// WATI WhatsApp API channel
pub struct WatiChannel {
    name: String,
    /// WATI API base URL (e.g., "https://live-server-123.wati.io")
    api_url: String,
    /// WATI API access token
    access_token: String,
    /// HTTP client
    client: Client,
    /// Incoming message sender (for webhook push)
    incoming_tx: tokio::sync::Mutex<Option<tokio::sync::mpsc::Sender<ChannelMessage>>>,
}

impl WatiChannel {
    /// Create a new WATI channel
    pub fn new(api_url: String, access_token: String) -> Self {
        Self {
            name: "wati".to_string(),
            api_url: api_url.trim_end_matches('/').to_string(),
            access_token,
            client: Client::new(),
            incoming_tx: tokio::sync::Mutex::new(None),
        }
    }

    /// Push an incoming webhook message (called by the HTTP handler)
    pub async fn push_incoming(&self, message: ChannelMessage) -> Result<(), AttaError> {
        let guard = self.incoming_tx.lock().await;
        if let Some(tx) = guard.as_ref() {
            tx.send(message).await.map_err(|_| {
                AttaError::ChannelCapacityExhausted("wati incoming channel full".to_string())
            })?;
        }
        Ok(())
    }

    /// Parse a WATI webhook payload into a ChannelMessage
    ///
    /// WATI webhook payload:
    /// ```json
    /// {
    ///   "id": "...",
    ///   "created": "2024-01-01T00:00:00Z",
    ///   "whatsappMessageId": "wamid.xxx",
    ///   "conversationId": "...",
    ///   "ticketId": "...",
    ///   "text": "Hello",
    ///   "type": "text",
    ///   "data": null,
    ///   "timestamp": "1700000000",
    ///   "owner": false,
    ///   "eventType": "message",
    ///   "statusString": "REPLY",
    ///   "avatarUrl": "...",
    ///   "assignedId": "...",
    ///   "operatorName": "...",
    ///   "waId": "1234567890"
    /// }
    /// ```
    pub fn parse_webhook_payload(payload: &serde_json::Value) -> Option<ChannelMessage> {
        // Only process non-owner (incoming) messages
        let owner = payload
            .get("owner")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        if owner {
            return None;
        }

        let event_type = payload
            .get("eventType")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if event_type != "message" {
            return None;
        }

        let id = payload
            .get("whatsappMessageId")
            .or_else(|| payload.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let sender = payload
            .get("waId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let text = payload
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if text.is_empty() || sender.is_empty() {
            return None;
        }

        let timestamp_str = payload
            .get("timestamp")
            .and_then(|v| v.as_str())
            .unwrap_or("0");
        let timestamp = timestamp_str
            .parse::<i64>()
            .ok()
            .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
            .unwrap_or_else(chrono::Utc::now);

        let conversation_id = payload
            .get("conversationId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Some(ChannelMessage {
            id,
            sender,
            content: text,
            channel: "wati".to_string(),
            reply_target: None,
            timestamp,
            thread_ts: conversation_id,
            metadata: serde_json::json!({
                "ticket_id": payload.get("ticketId").and_then(|v| v.as_str()).unwrap_or(""),
                "type": payload.get("type").and_then(|v| v.as_str()).unwrap_or("text"),
            }),
            chat_type: ChatType::default(),
            bot_mentioned: false,
            group_id: None,
        })
    }
}

#[async_trait::async_trait]
impl Channel for WatiChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: SendMessage) -> Result<(), AttaError> {
        // WATI API: POST /api/v1/sendSessionMessage/{whatsappNumber}
        let phone = &message.recipient;
        let url = format!("{}/api/v1/sendSessionMessage/{}", self.api_url, phone);

        let body = serde_json::json!({
            "messageText": message.content,
        });

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AttaError::Other(anyhow::anyhow!(
                "WATI send failed HTTP {}: {}",
                status,
                text
            )));
        }

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        let success = result
            .get("result")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if !success {
            let info = result
                .get("info")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            return Err(AttaError::Other(anyhow::anyhow!(
                "WATI send failed: {}",
                info
            )));
        }

        debug!("WATI message sent to {}", phone);
        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        // WATI uses webhook callbacks for incoming messages.
        // Configure the webhook URL in the WATI dashboard:
        //   Settings -> Webhook -> Message Webhook URL
        //
        // The HTTP handler should POST the payload to push_incoming()

        *self.incoming_tx.lock().await = Some(tx);
        debug!("WATI listener started (webhook push model)");
        std::future::pending::<()>().await;
        Ok(())
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        // Check WATI API health by fetching contacts (lightweight endpoint)
        let url = format!("{}/api/v1/getContacts?pageSize=1", self.api_url);

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            return Err(AttaError::Other(anyhow::anyhow!(
                "WATI health check failed: HTTP {}",
                response.status()
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wati_channel_name() {
        let ch = WatiChannel::new(
            "https://live-server-123.wati.io".to_string(),
            "token".to_string(),
        );
        assert_eq!(ch.name(), "wati");
    }

    #[test]
    fn test_parse_incoming_message() {
        let payload = serde_json::json!({
            "whatsappMessageId": "wamid.test",
            "waId": "1234567890",
            "text": "Hello WATI",
            "owner": false,
            "eventType": "message",
            "timestamp": "1700000000",
            "conversationId": "conv-123",
            "ticketId": "ticket-456",
            "type": "text",
        });

        let msg = WatiChannel::parse_webhook_payload(&payload).unwrap();
        assert_eq!(msg.sender, "1234567890");
        assert_eq!(msg.content, "Hello WATI");
        assert_eq!(msg.thread_ts, Some("conv-123".to_string()));
    }

    #[test]
    fn test_skip_owner_message() {
        let payload = serde_json::json!({
            "whatsappMessageId": "wamid.test",
            "waId": "1234567890",
            "text": "Hello",
            "owner": true,
            "eventType": "message",
        });

        assert!(WatiChannel::parse_webhook_payload(&payload).is_none());
    }

    #[test]
    fn test_skip_non_message_event() {
        let payload = serde_json::json!({
            "whatsappMessageId": "wamid.test",
            "waId": "1234567890",
            "text": "Hello",
            "owner": false,
            "eventType": "status",
        });

        assert!(WatiChannel::parse_webhook_payload(&payload).is_none());
    }

    #[tokio::test]
    async fn test_health_check_unreachable() {
        let ch = WatiChannel::new("http://127.0.0.1:1".to_string(), "token".to_string());
        assert!(ch.health_check().await.is_err());
    }
}
