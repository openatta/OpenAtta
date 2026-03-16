//! Slack channel
//!
//! Uses Socket Mode (WebSocket) for real-time event delivery and the
//! Web API for sending messages, reactions, and typing indicators.

use atta_types::AttaError;
use reqwest::Client;
use tracing::{debug, error, info, warn};

use crate::traits::{Channel, ChannelMessage, ChatType, SendMessage};

const SLACK_API_BASE: &str = "https://slack.com/api";

/// Slack channel using Socket Mode + Web API
pub struct SlackChannel {
    name: String,
    /// Bot OAuth token (xoxb-...)
    bot_token: String,
    /// App-level token for Socket Mode (xapp-...)
    app_token: String,
    /// HTTP client
    client: Client,
    /// API base URL (overridable for testing)
    api_base: String,
}

impl SlackChannel {
    /// Create a new Slack channel
    pub fn new(bot_token: String, app_token: String) -> Self {
        Self {
            name: "slack".to_string(),
            bot_token,
            app_token,
            client: Client::new(),
            api_base: SLACK_API_BASE.to_string(),
        }
    }

    /// Override the API base URL (for testing with mock servers)
    pub fn with_api_base(mut self, api_base: String) -> Self {
        self.api_base = api_base;
        self
    }

    /// Call a Slack Web API method with JSON body
    async fn api_call(
        &self,
        method: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, AttaError> {
        let url = format!("{}/{}", self.api_base, method);
        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.bot_token))
            .header("Content-Type", "application/json; charset=utf-8")
            .json(body)
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AttaError::Other(anyhow::anyhow!(
                "Slack {} HTTP {}: {}",
                method,
                status,
                text
            )));
        }

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        let ok = result.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
        if !ok {
            let err = result
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            return Err(AttaError::Other(anyhow::anyhow!(
                "Slack {} error: {}",
                method,
                err
            )));
        }

        Ok(result)
    }

    /// Request a Socket Mode WebSocket URL via apps.connections.open
    async fn get_ws_url(&self) -> Result<String, AttaError> {
        let url = format!("{}/apps.connections.open", self.api_base);
        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.app_token))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        let ok = result.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
        if !ok {
            let err = result
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            return Err(AttaError::Other(anyhow::anyhow!(
                "Slack apps.connections.open error: {}",
                err
            )));
        }

        result
            .get("url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| {
                AttaError::Other(anyhow::anyhow!(
                    "Slack: no url in connections.open response"
                ))
            })
    }
}

#[async_trait::async_trait]
impl Channel for SlackChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: SendMessage) -> Result<(), AttaError> {
        let mut body = serde_json::json!({
            "channel": message.recipient,
            "text": message.content,
        });

        if let Some(ref thread_ts) = message.thread_ts {
            body["thread_ts"] = serde_json::json!(thread_ts);
        }

        self.api_call("chat.postMessage", &body).await?;
        debug!("Slack message sent to {}", message.recipient);
        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        #[cfg(feature = "slack")]
        {
            use futures::stream::StreamExt;
            use futures::SinkExt;
            use tokio_tungstenite::{connect_async, tungstenite::Message};

            loop {
                // Step 1: Get WebSocket URL via apps.connections.open
                let ws_url = self.get_ws_url().await?;
                info!(url = %ws_url, "Slack Socket Mode connecting");

                let (ws_stream, _) = connect_async(&ws_url).await.map_err(|e| {
                    AttaError::Other(anyhow::anyhow!("Slack WS connect failed: {e}"))
                })?;

                let (mut write, mut read) = ws_stream.split();

                info!("Slack Socket Mode connected");

                while let Some(msg_result) = read.next().await {
                    let msg = match msg_result {
                        Ok(m) => m,
                        Err(e) => {
                            warn!(error = %e, "Slack WS read error, reconnecting");
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
                            info!("Slack WS closed, reconnecting");
                            break;
                        }
                        _ => continue,
                    };

                    let envelope: serde_json::Value = match serde_json::from_str(&text) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    // Handle hello message
                    let msg_type = envelope.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    if msg_type == "hello" {
                        debug!("Slack Socket Mode hello received");
                        continue;
                    }

                    // ACK envelope
                    if let Some(envelope_id) = envelope.get("envelope_id").and_then(|v| v.as_str())
                    {
                        let ack = serde_json::json!({"envelope_id": envelope_id});
                        let _ = write.send(Message::Text(ack.to_string())).await;
                    }

                    // Process events_api messages
                    if msg_type == "events_api" {
                        if let Some(event) = envelope.pointer("/payload/event") {
                            let event_type =
                                event.get("type").and_then(|v| v.as_str()).unwrap_or("");
                            if event_type == "message" {
                                // Skip bot messages to prevent loops
                                if event.get("bot_id").is_some() {
                                    continue;
                                }

                                let channel_msg = ChannelMessage {
                                    id: event
                                        .get("ts")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string(),
                                    sender: event
                                        .get("user")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string(),
                                    content: event
                                        .get("text")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string(),
                                    channel: "slack".to_string(),
                                    reply_target: None,
                                    timestamp: chrono::Utc::now(),
                                    thread_ts: event
                                        .get("thread_ts")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string()),
                                    metadata: {
                                        let mut m = serde_json::Map::new();
                                        if let Some(ch) =
                                            event.get("channel").and_then(|v| v.as_str())
                                        {
                                            m.insert("channel_id".to_string(), serde_json::Value::String(ch.to_string()));
                                        }
                                        serde_json::Value::Object(m)
                                    },
                                    chat_type: ChatType::default(),
                                    bot_mentioned: false,
                                    group_id: None,
                                };

                                if tx.send(channel_msg).await.is_err() {
                                    info!("Slack listener: receiver dropped, stopping");
                                    return Ok(());
                                }
                            }
                        }
                    }
                }

                // Reconnect after 1 second
                warn!("Slack Socket Mode disconnected, reconnecting in 1s");
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        }

        #[cfg(not(feature = "slack"))]
        {
            let _ = tx;
            warn!("Slack Socket Mode requires the 'slack' feature");
            std::future::pending::<()>().await;
            Ok(())
        }
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        self.api_call("auth.test", &serde_json::json!({})).await?;
        Ok(())
    }

    async fn start_typing(&self, _recipient: &str) -> Result<(), AttaError> {
        // Slack does not have a dedicated typing API for bots.
        // Typing indicators are only shown via the RTM API (deprecated).
        // No-op for Web API / Socket Mode.
        Ok(())
    }

    async fn add_reaction(&self, message_id: &str, reaction: &str) -> Result<(), AttaError> {
        // message_id format: "channel:ts"
        let parts: Vec<&str> = message_id.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(AttaError::Validation(
                "Slack reaction requires 'channel:ts' format".to_string(),
            ));
        }

        let body = serde_json::json!({
            "channel": parts[0],
            "timestamp": parts[1],
            "name": reaction,
        });

        self.api_call("reactions.add", &body).await?;
        Ok(())
    }

    async fn remove_reaction(&self, message_id: &str, reaction: &str) -> Result<(), AttaError> {
        let parts: Vec<&str> = message_id.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(AttaError::Validation(
                "Slack reaction requires 'channel:ts' format".to_string(),
            ));
        }

        let body = serde_json::json!({
            "channel": parts[0],
            "timestamp": parts[1],
            "name": reaction,
        });

        self.api_call("reactions.remove", &body).await?;
        Ok(())
    }

    fn supports_draft_updates(&self) -> bool {
        true
    }

    async fn send_draft(&self, message: SendMessage) -> Result<String, AttaError> {
        let mut body = serde_json::json!({
            "channel": message.recipient,
            "text": message.content,
        });

        if let Some(ref thread_ts) = message.thread_ts {
            body["thread_ts"] = serde_json::json!(thread_ts);
        }

        let result = self.api_call("chat.postMessage", &body).await?;
        let ts = result
            .get("ts")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let channel = result
            .get("channel")
            .and_then(|v| v.as_str())
            .unwrap_or(&message.recipient)
            .to_string();

        // Return "channel:ts" as draft ID
        Ok(format!("{}:{}", channel, ts))
    }

    async fn update_draft(&self, draft_id: &str, content: &str) -> Result<(), AttaError> {
        let parts: Vec<&str> = draft_id.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(AttaError::Validation(
                "Slack draft_id must be 'channel:ts' format".to_string(),
            ));
        }

        // Slack message limit: 40,000 characters
        let truncated = crate::draft::utf8_truncate(content, 40_000);

        let body = serde_json::json!({
            "channel": parts[0],
            "ts": parts[1],
            "text": truncated,
        });

        self.api_call("chat.update", &body).await?;
        Ok(())
    }

    async fn finalize_draft(&self, _draft_id: &str) -> Result<(), AttaError> {
        Ok(())
    }

    async fn send_approval_prompt(
        &self,
        recipient: &str,
        request_id: &str,
        tool_name: &str,
        arguments: &serde_json::Value,
        thread_ts: Option<String>,
    ) -> Result<(), AttaError> {
        let mut body = serde_json::json!({
            "channel": recipient,
            "text": format!("Approval required for tool: {}", tool_name),
            "blocks": [
                {
                    "type": "section",
                    "text": {
                        "type": "mrkdwn",
                        "text": format!(
                            "*Approval Required*\nTool: `{}`\nArguments:\n```{}```",
                            tool_name,
                            serde_json::to_string_pretty(arguments).unwrap_or_default()
                        )
                    }
                },
                {
                    "type": "actions",
                    "block_id": format!("approval_{}", request_id),
                    "elements": [
                        {
                            "type": "button",
                            "text": { "type": "plain_text", "text": "Approve" },
                            "style": "primary",
                            "action_id": "approve",
                            "value": request_id,
                        },
                        {
                            "type": "button",
                            "text": { "type": "plain_text", "text": "Deny" },
                            "style": "danger",
                            "action_id": "deny",
                            "value": request_id,
                        }
                    ]
                }
            ]
        });

        if let Some(ref ts) = thread_ts {
            body["thread_ts"] = serde_json::json!(ts);
        }

        self.api_call("chat.postMessage", &body).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slack_channel_name() {
        let ch = SlackChannel::new("xoxb-test".to_string(), "xapp-test".to_string());
        assert_eq!(ch.name(), "slack");
    }

    #[test]
    fn test_slack_supports_draft() {
        let ch = SlackChannel::new("xoxb-test".to_string(), "xapp-test".to_string());
        assert!(ch.supports_draft_updates());
    }

    #[tokio::test]
    async fn test_health_check_invalid_token() {
        let ch = SlackChannel::new("invalid".to_string(), "invalid".to_string());
        let result = ch.health_check().await;
        assert!(result.is_err());
    }
}
