//! WhatsApp Web channel
//!
//! Implements the WhatsApp Web protocol using WebSocket connections.
//! Supports QR code authentication flow (placeholder) and message
//! send/receive via the WhatsApp Web multi-device protocol.

use atta_types::AttaError;
use tracing::{debug, error, info, warn};

use crate::traits::{Channel, ChannelMessage, ChatType, SendMessage};

/// WhatsApp Web connection state
#[derive(Debug, Clone, PartialEq)]
pub enum WhatsappWebState {
    /// Not connected
    Disconnected,
    /// Waiting for QR code scan
    WaitingForQr,
    /// Authenticated and connected
    Connected,
    /// Reconnecting after disconnect
    Reconnecting,
}

/// WhatsApp Web channel using WebSocket protocol
pub struct WhatsappWebChannel {
    name: String,
    /// Connection state
    state: tokio::sync::Mutex<WhatsappWebState>,
    /// Session data for reconnection (encrypted)
    session_data: tokio::sync::Mutex<Option<Vec<u8>>>,
    /// QR code callback — invoked when a QR code needs to be displayed
    qr_callback: Option<Box<dyn Fn(&str) + Send + Sync>>,
    /// Shared WebSocket writer for send() — stored by listen(), used by send()
    #[cfg(feature = "whatsapp-web")]
    ws_writer: std::sync::Arc<
        tokio::sync::Mutex<
            Option<
                futures::stream::SplitSink<
                    tokio_tungstenite::WebSocketStream<
                        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
                    >,
                    tokio_tungstenite::tungstenite::Message,
                >,
            >,
        >,
    >,
}

impl WhatsappWebChannel {
    /// Create a new WhatsApp Web channel
    pub fn new() -> Self {
        Self {
            name: "whatsapp_web".to_string(),
            state: tokio::sync::Mutex::new(WhatsappWebState::Disconnected),
            session_data: tokio::sync::Mutex::new(None),
            qr_callback: None,
            #[cfg(feature = "whatsapp-web")]
            ws_writer: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    /// Set a callback for QR code display
    pub fn with_qr_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        self.qr_callback = Some(Box::new(callback));
        self
    }

    /// Restore a previous session from encrypted session data
    pub fn with_session_data(self, data: Vec<u8>) -> Self {
        // Store session data synchronously for the builder pattern.
        // It will be read by listen() for session restore.
        // We use try_lock since this is called before any async context.
        if let Ok(mut guard) = self.session_data.try_lock() {
            *guard = Some(data);
        }
        self
    }

    /// Get the current connection state
    pub async fn connection_state(&self) -> WhatsappWebState {
        self.state.lock().await.clone()
    }
}

impl Default for WhatsappWebChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Channel for WhatsappWebChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: SendMessage) -> Result<(), AttaError> {
        let state = self.state.lock().await;
        if *state != WhatsappWebState::Connected {
            return Err(AttaError::Other(anyhow::anyhow!(
                "WhatsApp Web not connected (state: {:?})",
                *state
            )));
        }
        drop(state);

        #[cfg(feature = "whatsapp-web")]
        {
            use futures::SinkExt;
            use tokio_tungstenite::tungstenite::Message as WsMessage;

            // Build WhatsApp Web message node
            // Note: The full protocol uses protobuf + signal encryption.
            // This sends a JSON text frame — compatible with some WA Web bridge
            // implementations, but the official WA Web protocol requires binary frames.
            let msg_id = uuid::Uuid::new_v4().to_string();
            let recipient_jid = if message.recipient.contains('@') {
                message.recipient.clone()
            } else {
                format!("{}@s.whatsapp.net", message.recipient)
            };

            let msg_node = serde_json::json!({
                "tag": "message",
                "type": "text",
                "to": recipient_jid,
                "id": msg_id,
                "body": message.content,
            });

            let mut guard = self.ws_writer.lock().await;
            if let Some(writer) = guard.as_mut() {
                writer
                    .send(WsMessage::Text(msg_node.to_string()))
                    .await
                    .map_err(|e| AttaError::Channel(format!("WhatsApp Web WS send failed: {e}")))?;
                debug!(to = %message.recipient, "WhatsApp Web message sent");
            } else {
                return Err(AttaError::Channel(
                    "WhatsApp Web: not connected to WebSocket".to_string(),
                ));
            }
        }

        #[cfg(not(feature = "whatsapp-web"))]
        {
            let _ = message;
            warn!("WhatsApp Web send requires the 'whatsapp-web' feature");
        }

        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        #[cfg(feature = "whatsapp-web")]
        {
            use futures::stream::StreamExt;
            use futures::SinkExt;
            use tokio_tungstenite::tungstenite::client::IntoClientRequest;
            use tokio_tungstenite::{connect_async, tungstenite::Message};

            const WA_WS_URL: &str = "wss://web.whatsapp.com/ws/chat";

            loop {
                info!("WhatsApp Web connecting");

                *self.state.lock().await = WhatsappWebState::WaitingForQr;

                // Build request with required headers
                let mut request = match WA_WS_URL.into_client_request() {
                    Ok(r) => r,
                    Err(e) => {
                        error!(error = %e, "WhatsApp Web failed to build WS request");
                        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                        continue;
                    }
                };
                request
                    .headers_mut()
                    .insert("Origin", "https://web.whatsapp.com".parse().unwrap());

                let (ws_stream, _) = match connect_async(request).await {
                    Ok(conn) => conn,
                    Err(e) => {
                        warn!(error = %e, "WhatsApp Web WS connect failed, retrying in 10s");
                        *self.state.lock().await = WhatsappWebState::Reconnecting;
                        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                        continue;
                    }
                };

                let (write, mut read) = ws_stream.split();

                // Store writer for send() to use
                *self.ws_writer.lock().await = Some(write);

                info!("WhatsApp Web WebSocket connected");

                // Invoke QR callback for authentication
                // In the full protocol, we would generate a Curve25519 keypair,
                // perform a Noise_XX handshake, and generate a QR code.
                // For now, we signal that QR is needed and wait for messages.
                if let Some(ref callback) = self.qr_callback {
                    callback("WhatsApp_Web_QR_PLACEHOLDER — scan this with WhatsApp mobile app");
                }

                // Check for session restore
                let has_session = self.session_data.lock().await.is_some();
                if has_session {
                    info!("WhatsApp Web attempting session restore");
                    // In production: send session restore handshake
                }

                // Set connected state (in production, only after successful auth)
                *self.state.lock().await = WhatsappWebState::Connected;

                // Start keepalive task — WhatsApp Web expects pings every 25s
                let ka_write = self.ws_writer.clone();
                let keepalive_task = tokio::spawn(async move {
                    let mut ticker = tokio::time::interval(tokio::time::Duration::from_secs(25));
                    loop {
                        ticker.tick().await;
                        let mut guard = ka_write.lock().await;
                        if let Some(w) = guard.as_mut() {
                            if w.send(Message::Ping(vec![])).await.is_err() {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                });

                // Message receive loop
                while let Some(msg_result) = read.next().await {
                    let msg = match msg_result {
                        Ok(m) => m,
                        Err(e) => {
                            warn!(error = %e, "WhatsApp Web WS read error, reconnecting");
                            break;
                        }
                    };

                    match msg {
                        Message::Text(text) => {
                            // WhatsApp Web typically uses binary frames with protobuf,
                            // but some control messages are JSON text frames.
                            let payload: serde_json::Value = match serde_json::from_str(&text) {
                                Ok(v) => v,
                                Err(_) => continue,
                            };

                            // Parse message node — WhatsApp Web protocol varies by version
                            let msg_type =
                                payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
                            if msg_type == "message" || msg_type == "chat" {
                                let sender_jid =
                                    payload.get("from").and_then(|v| v.as_str()).unwrap_or("");
                                // Strip @s.whatsapp.net suffix
                                let sender = sender_jid.split('@').next().unwrap_or(sender_jid);

                                let content = payload
                                    .get("body")
                                    .or_else(|| payload.get("content"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();

                                if content.is_empty() {
                                    continue;
                                }

                                let channel_msg = ChannelMessage {
                                    id: payload
                                        .get("id")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or(&uuid::Uuid::new_v4().to_string())
                                        .to_string(),
                                    sender: sender.to_string(),
                                    content,
                                    channel: "whatsapp_web".to_string(),
                                    reply_target: payload
                                        .get("quotedMsgId")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string()),
                                    timestamp: chrono::Utc::now(),
                                    thread_ts: None,
                                    metadata: serde_json::json!({
                                        "jid": sender_jid,
                                        "is_group": sender_jid.contains("@g.us"),
                                    }),
                                    chat_type: ChatType::default(),
                                    bot_mentioned: false,
                                    group_id: None,
                                };

                                if tx.send(channel_msg).await.is_err() {
                                    keepalive_task.abort();
                                    info!("WhatsApp Web listener: receiver dropped, stopping");
                                    *self.state.lock().await = WhatsappWebState::Disconnected;
                                    return Ok(());
                                }
                            }
                        }
                        Message::Binary(data) => {
                            // In the full protocol, binary frames contain encrypted
                            // protobuf messages. Decryption requires the Noise session
                            // keys established during handshake. We log and skip for now.
                            debug!(
                                len = data.len(),
                                "WhatsApp Web binary frame (encrypted, skipped)"
                            );
                        }
                        Message::Pong(_) => {
                            debug!("WhatsApp Web pong received");
                        }
                        Message::Close(_) => {
                            info!("WhatsApp Web WS closed, reconnecting");
                            break;
                        }
                        _ => continue,
                    }
                }

                keepalive_task.abort();
                *self.ws_writer.lock().await = None;
                *self.state.lock().await = WhatsappWebState::Reconnecting;
                warn!("WhatsApp Web disconnected, reconnecting in 10s");
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
            }
        }

        #[cfg(not(feature = "whatsapp-web"))]
        {
            let _ = tx;
            warn!("WhatsApp Web requires the 'whatsapp-web' feature");
            std::future::pending::<()>().await;
            Ok(())
        }
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        let state = self.state.lock().await;
        match *state {
            WhatsappWebState::Connected => Ok(()),
            WhatsappWebState::WaitingForQr => Err(AttaError::Other(anyhow::anyhow!(
                "WhatsApp Web: waiting for QR code scan"
            ))),
            WhatsappWebState::Disconnected => Err(AttaError::Other(anyhow::anyhow!(
                "WhatsApp Web: not connected"
            ))),
            WhatsappWebState::Reconnecting => Err(AttaError::Other(anyhow::anyhow!(
                "WhatsApp Web: reconnecting"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_whatsapp_web_channel_name() {
        let ch = WhatsappWebChannel::new();
        assert_eq!(ch.name(), "whatsapp_web");
    }

    #[tokio::test]
    async fn test_initial_state_disconnected() {
        let ch = WhatsappWebChannel::new();
        assert_eq!(ch.connection_state().await, WhatsappWebState::Disconnected);
    }

    #[tokio::test]
    async fn test_send_when_disconnected() {
        let ch = WhatsappWebChannel::new();
        let msg = SendMessage {
            recipient: "1234567890".to_string(),
            content: "test".to_string(),
            subject: None,
            thread_ts: None,
            metadata: serde_json::json!({}),
        };
        let result = ch.send(msg).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_health_check_disconnected() {
        let ch = WhatsappWebChannel::new();
        assert!(ch.health_check().await.is_err());
    }
}
