//! Signal channel
//!
//! Uses the signal-cli REST API (typically running on localhost:8080) for
//! sending and receiving Signal messages. Requires signal-cli to be running
//! in REST API mode with a registered phone number.

use atta_types::AttaError;
use reqwest::Client;
use tracing::{debug, error, info, warn};

use crate::traits::{Channel, ChannelMessage, ChatType, SendMessage};

const DEFAULT_SIGNAL_API_URL: &str = "http://localhost:8080";

/// Signal channel via signal-cli REST API
pub struct SignalChannel {
    name: String,
    /// signal-cli REST API base URL
    api_url: String,
    /// Registered phone number (e.g., "+1234567890")
    phone_number: String,
    /// HTTP client
    client: Client,
}

impl SignalChannel {
    /// Create a new Signal channel with default API URL
    pub fn new(phone_number: String) -> Self {
        Self {
            name: "signal".to_string(),
            api_url: DEFAULT_SIGNAL_API_URL.to_string(),
            phone_number,
            client: Client::new(),
        }
    }

    /// Create a new Signal channel with a custom API URL
    pub fn with_api_url(mut self, api_url: String) -> Self {
        self.api_url = api_url;
        self
    }
}

#[async_trait::async_trait]
impl Channel for SignalChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: SendMessage) -> Result<(), AttaError> {
        let url = format!("{}/v2/send", self.api_url);

        let body = serde_json::json!({
            "message": message.content,
            "number": self.phone_number,
            "recipients": [message.recipient],
        });

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AttaError::Other(anyhow::anyhow!(
                "Signal send failed HTTP {}: {}",
                status,
                text
            )));
        }

        debug!("Signal message sent to {}", message.recipient);
        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        info!(
            api_url = %self.api_url,
            phone = %self.phone_number,
            "Signal listener starting (long-poll)"
        );

        let url = format!("{}/v1/receive/{}", self.api_url, self.phone_number);

        loop {
            let response = match self.client.get(&url).send().await {
                Ok(r) => r,
                Err(e) => {
                    warn!(error = %e, "Signal receive request failed, retrying");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            if !response.status().is_success() {
                let status = response.status();
                error!("Signal receive HTTP {}", status);
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }

            let messages: serde_json::Value = match response.json().await {
                Ok(v) => v,
                Err(e) => {
                    error!(error = %e, "Signal response parse error");
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                }
            };

            // signal-cli REST API returns an array of message envelopes
            let envelopes = match messages.as_array() {
                Some(arr) => arr,
                None => {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                }
            };

            for envelope in envelopes {
                // Extract data message
                let data_message = match envelope.get("envelope").and_then(|e| e.get("dataMessage"))
                {
                    Some(dm) => dm,
                    None => continue,
                };

                let message_text = data_message
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if message_text.is_empty() {
                    continue;
                }

                let source = envelope
                    .pointer("/envelope/source")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let timestamp_ms = data_message
                    .get("timestamp")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);

                let timestamp = chrono::DateTime::from_timestamp_millis(timestamp_ms)
                    .unwrap_or_else(chrono::Utc::now);

                let group_id = envelope
                    .pointer("/envelope/dataMessage/groupInfo/groupId")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let channel_msg = ChannelMessage {
                    id: uuid::Uuid::new_v4().to_string(),
                    sender: source,
                    content: message_text,
                    channel: "signal".to_string(),
                    reply_target: None,
                    timestamp,
                    thread_ts: None,
                    metadata: serde_json::json!({
                        "timestamp_ms": timestamp_ms,
                        "group_id": group_id,
                    }),
                    chat_type: ChatType::default(),
                    bot_mentioned: false,
                    group_id: None,
                };

                if tx.send(channel_msg).await.is_err() {
                    debug!("Signal listener: receiver dropped, stopping");
                    return Ok(());
                }
            }

            // Short delay between polls
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        let url = format!("{}/v1/about", self.api_url);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            return Err(AttaError::Other(anyhow::anyhow!(
                "Signal health check failed: HTTP {}",
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
    fn test_signal_channel_name() {
        let ch = SignalChannel::new("+1234567890".to_string());
        assert_eq!(ch.name(), "signal");
    }

    #[test]
    fn test_custom_api_url() {
        let ch = SignalChannel::new("+1234567890".to_string())
            .with_api_url("http://signal-api:9090".to_string());
        assert_eq!(ch.api_url, "http://signal-api:9090");
    }

    #[tokio::test]
    async fn test_health_check_unreachable() {
        let ch = SignalChannel::new("+1234567890".to_string())
            .with_api_url("http://127.0.0.1:1".to_string());
        assert!(ch.health_check().await.is_err());
    }
}
