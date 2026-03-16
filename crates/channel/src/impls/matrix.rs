//! Matrix channel
//!
//! Integrates with the Matrix protocol using the Client-Server API.
//! Supports room joining, message sync, and E2EE (placeholder).
//! In production, use the `matrix-sdk` crate for full protocol support.

use atta_types::AttaError;
use reqwest::Client;
use tracing::{debug, error, info, warn};

use crate::traits::{Channel, ChannelMessage, ChatType, SendMessage};

/// Matrix channel using Client-Server API
pub struct MatrixChannel {
    name: String,
    /// Homeserver URL (e.g., "https://matrix.org")
    homeserver_url: String,
    /// Access token
    access_token: String,
    /// User ID (e.g., "@bot:matrix.org")
    user_id: String,
    /// HTTP client
    client: Client,
    /// Last sync batch token for incremental sync
    next_batch: tokio::sync::Mutex<Option<String>>,
}

impl MatrixChannel {
    /// Create a new Matrix channel
    pub fn new(homeserver_url: String, access_token: String, user_id: String) -> Self {
        Self {
            name: "matrix".to_string(),
            homeserver_url: homeserver_url.trim_end_matches('/').to_string(),
            access_token,
            user_id,
            client: Client::new(),
            next_batch: tokio::sync::Mutex::new(None),
        }
    }

    /// Build an API URL
    fn api_url(&self, path: &str) -> String {
        format!("{}/_matrix/client/v3{}", self.homeserver_url, path)
    }

    /// Make an authenticated GET request
    async fn api_get(&self, path: &str) -> Result<serde_json::Value, AttaError> {
        let url = self.api_url(path);
        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AttaError::Other(anyhow::anyhow!(
                "Matrix GET {} HTTP {}: {}",
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

    /// Make an authenticated PUT request
    async fn api_put(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, AttaError> {
        let url = self.api_url(path);
        let response = self
            .client
            .put(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AttaError::Other(anyhow::anyhow!(
                "Matrix PUT {} HTTP {}: {}",
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
        let url = self.api_url(path);
        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AttaError::Other(anyhow::anyhow!(
                "Matrix POST {} HTTP {}: {}",
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

    /// Join a room by ID or alias
    pub async fn join_room(&self, room_id_or_alias: &str) -> Result<String, AttaError> {
        let encoded = urlencoding::encode(room_id_or_alias);
        let path = format!("/join/{}", encoded);
        let result = self.api_post(&path, &serde_json::json!({})).await?;

        result
            .get("room_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| {
                AttaError::Other(anyhow::anyhow!("Matrix: missing room_id in join response"))
            })
    }
}

#[async_trait::async_trait]
impl Channel for MatrixChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: SendMessage) -> Result<(), AttaError> {
        let room_id = &message.recipient;
        let txn_id = uuid::Uuid::new_v4().to_string();
        let encoded_room = urlencoding::encode(room_id);

        let body = if let Some(ref thread_ts) = message.thread_ts {
            // Send as a threaded reply
            serde_json::json!({
                "msgtype": "m.text",
                "body": message.content,
                "m.relates_to": {
                    "rel_type": "m.thread",
                    "event_id": thread_ts,
                    "is_falling_back": true,
                    "m.in_reply_to": {
                        "event_id": thread_ts,
                    }
                }
            })
        } else {
            serde_json::json!({
                "msgtype": "m.text",
                "body": message.content,
            })
        };

        let path = format!("/rooms/{}/send/m.room.message/{}", encoded_room, txn_id);

        self.api_put(&path, &body).await?;
        debug!("Matrix message sent to room {}", room_id);
        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        info!(
            homeserver = %self.homeserver_url,
            user = %self.user_id,
            "Matrix sync listener starting"
        );

        // Long-poll /sync endpoint
        loop {
            let mut url = format!(
                "{}/_matrix/client/v3/sync?timeout=30000",
                self.homeserver_url
            );

            {
                let batch = self.next_batch.lock().await;
                if let Some(ref since) = *batch {
                    url.push_str(&format!("&since={}", urlencoding::encode(since)));
                }
            }

            let response = match self
                .client
                .get(&url)
                .header("Authorization", format!("Bearer {}", self.access_token))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    warn!(error = %e, "Matrix sync request failed, retrying");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            if !response.status().is_success() {
                let status = response.status();
                error!("Matrix sync HTTP {}", status);
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }

            let sync_response: serde_json::Value = match response.json().await {
                Ok(v) => v,
                Err(e) => {
                    error!(error = %e, "Matrix sync response parse error");
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                }
            };

            // Update next_batch token
            if let Some(batch) = sync_response.get("next_batch").and_then(|v| v.as_str()) {
                *self.next_batch.lock().await = Some(batch.to_string());
            }

            // Process joined rooms
            let rooms = sync_response
                .pointer("/rooms/join")
                .and_then(|v| v.as_object());

            if let Some(rooms) = rooms {
                for (room_id, room_data) in rooms {
                    let events = room_data
                        .pointer("/timeline/events")
                        .and_then(|v| v.as_array());

                    if let Some(events) = events {
                        for event in events {
                            let event_type =
                                event.get("type").and_then(|v| v.as_str()).unwrap_or("");

                            if event_type != "m.room.message" {
                                continue;
                            }

                            let sender = event
                                .get("sender")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();

                            // Skip own messages
                            if sender == self.user_id {
                                continue;
                            }

                            let event_id = event
                                .get("event_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();

                            let body = event
                                .pointer("/content/body")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();

                            if body.is_empty() {
                                continue;
                            }

                            let origin_ts = event
                                .get("origin_server_ts")
                                .and_then(|v| v.as_i64())
                                .unwrap_or(0);

                            let timestamp = chrono::DateTime::from_timestamp_millis(origin_ts)
                                .unwrap_or_else(chrono::Utc::now);

                            // Check for reply
                            let reply_target = event
                                .pointer("/content/m.relates_to/m.in_reply_to/event_id")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());

                            // Check for thread
                            let thread_ts = event
                                .pointer("/content/m.relates_to/event_id")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());

                            let channel_msg = ChannelMessage {
                                id: event_id,
                                sender,
                                content: body,
                                channel: "matrix".to_string(),
                                reply_target,
                                timestamp,
                                thread_ts,
                                metadata: serde_json::json!({
                                    "room_id": room_id,
                                }),
                                chat_type: ChatType::default(),
                                bot_mentioned: false,
                                group_id: None,
                            };

                            if tx.send(channel_msg).await.is_err() {
                                debug!("Matrix listener: receiver dropped, stopping");
                                return Ok(());
                            }
                        }
                    }
                }
            }
        }
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        // Check whoami endpoint
        self.api_get("/account/whoami").await?;
        Ok(())
    }

    async fn start_typing(&self, recipient: &str) -> Result<(), AttaError> {
        let encoded_room = urlencoding::encode(recipient);
        let encoded_user = urlencoding::encode(&self.user_id);
        let path = format!("/rooms/{}/typing/{}", encoded_room, encoded_user);

        self.api_put(
            &path,
            &serde_json::json!({
                "typing": true,
                "timeout": 30000,
            }),
        )
        .await?;

        Ok(())
    }

    async fn stop_typing(&self, recipient: &str) -> Result<(), AttaError> {
        let encoded_room = urlencoding::encode(recipient);
        let encoded_user = urlencoding::encode(&self.user_id);
        let path = format!("/rooms/{}/typing/{}", encoded_room, encoded_user);

        self.api_put(
            &path,
            &serde_json::json!({
                "typing": false,
            }),
        )
        .await?;

        Ok(())
    }

    async fn add_reaction(&self, message_id: &str, reaction: &str) -> Result<(), AttaError> {
        // message_id format: "room_id:event_id"
        let parts: Vec<&str> = message_id.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(AttaError::Validation(
                "Matrix reaction requires 'room_id:event_id' format".to_string(),
            ));
        }

        let encoded_room = urlencoding::encode(parts[0]);
        let txn_id = uuid::Uuid::new_v4().to_string();
        let path = format!("/rooms/{}/send/m.reaction/{}", encoded_room, txn_id);

        let body = serde_json::json!({
            "m.relates_to": {
                "rel_type": "m.annotation",
                "event_id": parts[1],
                "key": reaction,
            }
        });

        self.api_put(&path, &body).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matrix_channel_name() {
        let ch = MatrixChannel::new(
            "https://matrix.org".to_string(),
            "syt_token".to_string(),
            "@bot:matrix.org".to_string(),
        );
        assert_eq!(ch.name(), "matrix");
    }

    #[test]
    fn test_api_url_construction() {
        let ch = MatrixChannel::new(
            "https://matrix.org".to_string(),
            "token".to_string(),
            "@bot:matrix.org".to_string(),
        );
        assert_eq!(
            ch.api_url("/sync"),
            "https://matrix.org/_matrix/client/v3/sync"
        );
    }

    #[test]
    fn test_api_url_trailing_slash() {
        let ch = MatrixChannel::new(
            "https://matrix.org/".to_string(),
            "token".to_string(),
            "@bot:matrix.org".to_string(),
        );
        assert_eq!(
            ch.api_url("/sync"),
            "https://matrix.org/_matrix/client/v3/sync"
        );
    }

    #[tokio::test]
    async fn test_health_check_invalid() {
        let ch = MatrixChannel::new(
            "http://127.0.0.1:1".to_string(),
            "invalid".to_string(),
            "@bot:localhost".to_string(),
        );
        assert!(ch.health_check().await.is_err());
    }
}
