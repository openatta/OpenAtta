//! WhatsApp Business Cloud API channel
//!
//! Uses the Meta Cloud API (graph.facebook.com) for sending messages and
//! receives incoming messages via webhook callbacks. Requires a WhatsApp
//! Business API access token and phone number ID.

use atta_types::AttaError;
use reqwest::Client;
use tracing::{debug, error, warn};

use crate::traits::{Channel, ChannelMessage, ChatType, SendMessage};

const META_GRAPH_API_BASE: &str = "https://graph.facebook.com/v18.0";

/// WhatsApp Business Cloud API channel
pub struct WhatsappChannel {
    name: String,
    /// Permanent or temporary access token
    access_token: String,
    /// Phone number ID (from Meta Developer dashboard)
    phone_number_id: String,
    /// API base URL (overridable for testing)
    api_base: String,
    /// HTTP client
    client: Client,
    /// Incoming message sender (populated when listen() is called)
    incoming_tx: tokio::sync::Mutex<Option<tokio::sync::mpsc::Sender<ChannelMessage>>>,
}

impl WhatsappChannel {
    /// Create a new WhatsApp Cloud API channel
    pub fn new(access_token: String, phone_number_id: String) -> Self {
        Self {
            name: "whatsapp".to_string(),
            access_token,
            phone_number_id,
            api_base: META_GRAPH_API_BASE.to_string(),
            client: Client::new(),
            incoming_tx: tokio::sync::Mutex::new(None),
        }
    }

    /// Override the API base URL (for testing with wiremock)
    pub fn with_api_base(mut self, api_base: String) -> Self {
        self.api_base = api_base;
        self
    }

    /// Push an incoming webhook message (called by the HTTP handler)
    ///
    /// The HTTP handler should verify the webhook signature and parse the
    /// payload before calling this method.
    pub async fn push_incoming(&self, message: ChannelMessage) -> Result<(), AttaError> {
        let guard = self.incoming_tx.lock().await;
        if let Some(tx) = guard.as_ref() {
            tx.send(message).await.map_err(|_| {
                AttaError::ChannelCapacityExhausted("whatsapp incoming channel full".to_string())
            })?;
        }
        Ok(())
    }

    /// Parse a webhook payload into ChannelMessages
    ///
    /// Meta webhook payload structure:
    /// ```json
    /// {
    ///   "object": "whatsapp_business_account",
    ///   "entry": [{
    ///     "id": "...",
    ///     "changes": [{
    ///       "value": {
    ///         "messaging_product": "whatsapp",
    ///         "metadata": { "phone_number_id": "..." },
    ///         "messages": [{
    ///           "id": "wamid.xxx",
    ///           "from": "1234567890",
    ///           "timestamp": "1234567890",
    ///           "type": "text",
    ///           "text": { "body": "Hello" }
    ///         }]
    ///       }
    ///     }]
    ///   }]
    /// }
    /// ```
    pub fn parse_webhook_payload(payload: &serde_json::Value) -> Vec<ChannelMessage> {
        let mut messages = Vec::new();

        let entries = match payload.get("entry").and_then(|v| v.as_array()) {
            Some(arr) => arr,
            None => return messages,
        };

        for entry in entries {
            let changes = match entry.get("changes").and_then(|v| v.as_array()) {
                Some(arr) => arr,
                None => continue,
            };

            for change in changes {
                let value = match change.get("value") {
                    Some(v) => v,
                    None => continue,
                };

                let msgs = match value.get("messages").and_then(|v| v.as_array()) {
                    Some(arr) => arr,
                    None => continue,
                };

                for msg in msgs {
                    let msg_id = msg
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    let from = msg
                        .get("from")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    let timestamp_str =
                        msg.get("timestamp").and_then(|v| v.as_str()).unwrap_or("0");
                    let timestamp = timestamp_str
                        .parse::<i64>()
                        .ok()
                        .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                        .unwrap_or_else(chrono::Utc::now);

                    let msg_type = msg.get("type").and_then(|v| v.as_str()).unwrap_or("text");

                    let content = match msg_type {
                        "text" => msg
                            .pointer("/text/body")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        "image" | "video" | "audio" | "document" => {
                            let caption = msg
                                .pointer(&format!("/{}/caption", msg_type))
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let media_id = msg
                                .pointer(&format!("/{}/id", msg_type))
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            format!("[{}: {}] {}", msg_type, media_id, caption)
                        }
                        _ => format!("[unsupported message type: {}]", msg_type),
                    };

                    let context_msg_id = msg
                        .pointer("/context/id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    messages.push(ChannelMessage {
                        id: msg_id,
                        sender: from,
                        content,
                        channel: "whatsapp".to_string(),
                        reply_target: context_msg_id,
                        timestamp,
                        thread_ts: None,
                        metadata: serde_json::json!({
                            "type": msg_type,
                        }),
                        chat_type: ChatType::default(),
                        bot_mentioned: false,
                        group_id: None,
                    });
                }
            }
        }

        messages
    }
}

#[async_trait::async_trait]
impl Channel for WhatsappChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: SendMessage) -> Result<(), AttaError> {
        let url = format!("{}/{}/messages", self.api_base, self.phone_number_id);

        let mut body = serde_json::json!({
            "messaging_product": "whatsapp",
            "to": message.recipient,
            "type": "text",
            "text": {
                "body": message.content,
            },
        });

        // If replying to a specific message
        if let Some(ref thread_ts) = message.thread_ts {
            body["context"] = serde_json::json!({
                "message_id": thread_ts,
            });
        }

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
                "WhatsApp send failed HTTP {}: {}",
                status,
                text
            )));
        }

        debug!("WhatsApp message sent to {}", message.recipient);
        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        // WhatsApp Cloud API uses webhooks for incoming messages.
        // The HTTP handler should:
        // 1. Verify the webhook signature (X-Hub-Signature-256 header)
        // 2. Handle the GET verification challenge
        // 3. Parse POST payloads and call push_incoming()
        *self.incoming_tx.lock().await = Some(tx);
        debug!("WhatsApp listener started (webhook push model)");
        std::future::pending::<()>().await;
        Ok(())
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        // Verify the access token by fetching phone number info
        let url = format!("{}/{}", self.api_base, self.phone_number_id);
        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            return Err(AttaError::Other(anyhow::anyhow!(
                "WhatsApp health check failed: HTTP {}",
                response.status()
            )));
        }

        Ok(())
    }

    async fn add_reaction(&self, message_id: &str, reaction: &str) -> Result<(), AttaError> {
        let url = format!("{}/{}/messages", self.api_base, self.phone_number_id);

        // WhatsApp reactions require the recipient phone number.
        // We encode it as "phone:wamid" in message_id.
        let parts: Vec<&str> = message_id.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(AttaError::Validation(
                "WhatsApp reaction requires 'phone:message_id' format".to_string(),
            ));
        }

        let body = serde_json::json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": parts[0],
            "type": "reaction",
            "reaction": {
                "message_id": parts[1],
                "emoji": reaction,
            },
        });

        self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
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
    fn test_whatsapp_channel_name() {
        let ch = WhatsappChannel::new("token".to_string(), "123".to_string());
        assert_eq!(ch.name(), "whatsapp");
    }

    #[test]
    fn test_parse_webhook_text_message() {
        let payload = serde_json::json!({
            "object": "whatsapp_business_account",
            "entry": [{
                "id": "123",
                "changes": [{
                    "value": {
                        "messaging_product": "whatsapp",
                        "messages": [{
                            "id": "wamid.test123",
                            "from": "1234567890",
                            "timestamp": "1700000000",
                            "type": "text",
                            "text": { "body": "Hello from WhatsApp" }
                        }]
                    }
                }]
            }]
        });

        let messages = WhatsappChannel::parse_webhook_payload(&payload);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].sender, "1234567890");
        assert_eq!(messages[0].content, "Hello from WhatsApp");
        assert_eq!(messages[0].id, "wamid.test123");
    }

    #[test]
    fn test_parse_webhook_with_context() {
        let payload = serde_json::json!({
            "object": "whatsapp_business_account",
            "entry": [{
                "changes": [{
                    "value": {
                        "messages": [{
                            "id": "wamid.reply",
                            "from": "999",
                            "timestamp": "1700000000",
                            "type": "text",
                            "text": { "body": "Reply" },
                            "context": { "id": "wamid.original" }
                        }]
                    }
                }]
            }]
        });

        let messages = WhatsappChannel::parse_webhook_payload(&payload);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].reply_target, Some("wamid.original".to_string()));
    }

    #[test]
    fn test_parse_empty_webhook() {
        let payload = serde_json::json!({});
        let messages = WhatsappChannel::parse_webhook_payload(&payload);
        assert!(messages.is_empty());
    }
}
