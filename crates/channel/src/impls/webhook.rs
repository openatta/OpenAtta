//! Generic webhook channel
//!
//! Sends outgoing messages via HTTP POST to a configured URL.
//! Incoming messages are received via the webhook endpoint (not implemented here;
//! the HTTP handler should be added to the API router).

use atta_types::AttaError;
use tracing::debug;

use crate::traits::{Channel, ChannelMessage, ChatType, SendMessage};

/// Webhook channel configuration
pub struct WebhookChannel {
    name: String,
    /// URL to POST outgoing messages to
    outgoing_url: String,
    /// HTTP client
    client: reqwest::Client,
    /// Incoming message sender (populated when listen() is called)
    incoming_tx: tokio::sync::Mutex<Option<tokio::sync::mpsc::Sender<ChannelMessage>>>,
}

impl WebhookChannel {
    /// Create a new webhook channel
    pub fn new(name: String, outgoing_url: String) -> Self {
        Self {
            name,
            outgoing_url,
            client: reqwest::Client::new(),
            incoming_tx: tokio::sync::Mutex::new(None),
        }
    }

    /// Push an incoming message (called by the HTTP handler)
    pub async fn push_incoming(&self, message: ChannelMessage) -> Result<(), AttaError> {
        let guard = self.incoming_tx.lock().await;
        if let Some(tx) = guard.as_ref() {
            tx.send(message).await.map_err(|_| {
                AttaError::ChannelCapacityExhausted("webhook incoming channel full".to_string())
            })?;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl Channel for WebhookChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: SendMessage) -> Result<(), AttaError> {
        debug!(url = %self.outgoing_url, "sending webhook message");

        let response = self
            .client
            .post(&self.outgoing_url)
            .json(&message)
            .send()
            .await
            .map_err(|e| {
                AttaError::Llm(atta_types::LlmError::RequestFailed(format!(
                    "webhook send failed: {}",
                    e
                )))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AttaError::Llm(atta_types::LlmError::RequestFailed(
                format!("webhook returned HTTP {}: {}", status, text),
            )));
        }

        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        // Store the tx so push_incoming() can forward messages
        *self.incoming_tx.lock().await = Some(tx);
        // Block forever — webhook messages arrive via push_incoming()
        std::future::pending::<()>().await;
        Ok(())
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        // Simple connectivity check — HEAD request
        let _response = self
            .client
            .head(&self.outgoing_url)
            .send()
            .await
            .map_err(|e| {
                AttaError::Llm(atta_types::LlmError::RequestFailed(format!(
                    "webhook health check failed: {}",
                    e
                )))
            })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webhook_channel_name() {
        let ch = WebhookChannel::new("test-webhook".into(), "http://localhost:8080".into());
        assert_eq!(ch.name(), "test-webhook");
    }
}
