//! MQTT channel
//!
//! Uses MQTT protocol for pub/sub messaging. Subscribes to a topic for
//! incoming messages and publishes to a topic for outgoing messages.
//! Designed for IoT and lightweight messaging scenarios.

use atta_types::AttaError;
use tracing::{debug, error, info, warn};

use crate::traits::{Channel, ChannelMessage, ChatType, SendMessage};

/// MQTT Quality of Service level
#[derive(Debug, Clone, Copy)]
pub enum MqttQos {
    /// At most once (fire and forget)
    AtMostOnce = 0,
    /// At least once (acknowledged delivery)
    AtLeastOnce = 1,
    /// Exactly once (assured delivery)
    ExactlyOnce = 2,
}

/// MQTT channel configuration
pub struct MqttChannel {
    name: String,
    /// MQTT broker host
    host: String,
    /// MQTT broker port (1883 for plain, 8883 for TLS)
    port: u16,
    /// Client ID
    client_id: String,
    /// Subscribe topic for incoming messages
    subscribe_topic: String,
    /// Publish topic for outgoing messages
    publish_topic: String,
    /// Optional username
    username: Option<String>,
    /// Optional password
    password: Option<String>,
    /// QoS level
    qos: MqttQos,
    /// Use TLS
    use_tls: bool,
    /// Shared MQTT client for send()
    #[cfg(feature = "mqtt")]
    client: std::sync::Arc<tokio::sync::Mutex<Option<rumqttc::AsyncClient>>>,
}

impl MqttChannel {
    /// Create a new MQTT channel
    pub fn new(
        host: String,
        port: u16,
        client_id: String,
        subscribe_topic: String,
        publish_topic: String,
    ) -> Self {
        Self {
            name: "mqtt".to_string(),
            host,
            port,
            client_id,
            subscribe_topic,
            publish_topic,
            username: None,
            password: None,
            qos: MqttQos::AtLeastOnce,
            use_tls: false,
            #[cfg(feature = "mqtt")]
            client: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    /// Set authentication credentials
    pub fn with_credentials(mut self, username: String, password: String) -> Self {
        self.username = Some(username);
        self.password = Some(password);
        self
    }

    /// Set QoS level
    pub fn with_qos(mut self, qos: MqttQos) -> Self {
        self.qos = qos;
        self
    }

    /// Enable TLS
    pub fn with_tls(mut self, use_tls: bool) -> Self {
        self.use_tls = use_tls;
        self
    }

    /// Convert our QoS to rumqttc QoS
    #[cfg(feature = "mqtt")]
    fn to_rumqttc_qos(&self) -> rumqttc::QoS {
        match self.qos {
            MqttQos::AtMostOnce => rumqttc::QoS::AtMostOnce,
            MqttQos::AtLeastOnce => rumqttc::QoS::AtLeastOnce,
            MqttQos::ExactlyOnce => rumqttc::QoS::ExactlyOnce,
        }
    }

    /// Parse an MQTT payload into a ChannelMessage
    fn parse_payload(topic: &str, payload: &[u8]) -> Option<ChannelMessage> {
        let payload_str = String::from_utf8_lossy(payload);

        // Try to parse as JSON first
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&payload_str) {
            let content = json
                .get("content")
                .or_else(|| json.get("message"))
                .or_else(|| json.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or(&payload_str)
                .to_string();

            let sender = json
                .get("sender")
                .or_else(|| json.get("from"))
                .or_else(|| json.get("client_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("mqtt-unknown")
                .to_string();

            let id = json
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

            return Some(ChannelMessage {
                id,
                sender,
                content,
                channel: "mqtt".to_string(),
                reply_target: None,
                timestamp: chrono::Utc::now(),
                thread_ts: None,
                metadata: serde_json::json!({
                    "topic": topic,
                    "raw_json": true,
                }),
                chat_type: ChatType::default(),
                bot_mentioned: false,
                group_id: None,
            });
        }

        // Fall back to plain text
        Some(ChannelMessage {
            id: uuid::Uuid::new_v4().to_string(),
            sender: "mqtt-unknown".to_string(),
            content: payload_str.to_string(),
            channel: "mqtt".to_string(),
            reply_target: None,
            timestamp: chrono::Utc::now(),
            thread_ts: None,
            metadata: serde_json::json!({
                "topic": topic,
                "raw_json": false,
            }),
            chat_type: ChatType::default(),
            bot_mentioned: false,
            group_id: None,
        })
    }
}

#[async_trait::async_trait]
impl Channel for MqttChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: SendMessage) -> Result<(), AttaError> {
        #[cfg(feature = "mqtt")]
        {
            let topic = if message.recipient.is_empty() {
                self.publish_topic.clone()
            } else {
                message.recipient.clone()
            };

            let payload = serde_json::json!({
                "content": message.content,
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "metadata": message.metadata,
            });

            let qos = self.to_rumqttc_qos();

            let guard = self.client.lock().await;
            let client = guard.as_ref().ok_or_else(|| {
                AttaError::Channel("MQTT: not connected, cannot publish".to_string())
            })?;

            client
                .publish(&topic, qos, false, payload.to_string().as_bytes())
                .await
                .map_err(|e| AttaError::Channel(format!("MQTT publish failed: {e}")))?;

            debug!(topic = %topic, qos = self.qos as u8, "MQTT message published");
            Ok(())
        }

        #[cfg(not(feature = "mqtt"))]
        {
            let _ = message;
            Err(AttaError::Channel(
                "MQTT channel requires the 'mqtt' feature".to_string(),
            ))
        }
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        #[cfg(feature = "mqtt")]
        {
            use rumqttc::{AsyncClient, Event, Incoming, MqttOptions};

            loop {
                info!(
                    host = %self.host,
                    port = self.port,
                    subscribe = %self.subscribe_topic,
                    client_id = %self.client_id,
                    "MQTT connecting"
                );

                let mut options = MqttOptions::new(&self.client_id, &self.host, self.port);
                options.set_keep_alive(std::time::Duration::from_secs(30));

                if let (Some(ref user), Some(ref pass)) = (&self.username, &self.password) {
                    options.set_credentials(user, pass);
                }

                let (client, mut eventloop) = AsyncClient::new(options, 100);

                let qos = self.to_rumqttc_qos();

                if let Err(e) = client.subscribe(&self.subscribe_topic, qos).await {
                    warn!(error = %e, "MQTT subscribe failed, retrying in 5s");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }

                // Store client for send() to use
                {
                    let mut guard = self.client.lock().await;
                    *guard = Some(client);
                }

                info!(topic = %self.subscribe_topic, "MQTT subscribed");

                loop {
                    match eventloop.poll().await {
                        Ok(Event::Incoming(Incoming::Publish(publish))) => {
                            let topic = publish.topic.clone();
                            if let Some(msg) = Self::parse_payload(&topic, &publish.payload) {
                                if tx.send(msg).await.is_err() {
                                    info!("MQTT listener: receiver dropped, stopping");
                                    return Ok(());
                                }
                            }
                        }
                        Ok(Event::Incoming(Incoming::ConnAck(_))) => {
                            info!("MQTT connection acknowledged");
                        }
                        Ok(_) => {}
                        Err(e) => {
                            error!(error = %e, "MQTT eventloop error, reconnecting");
                            break;
                        }
                    }
                }

                // Clear client on disconnect
                {
                    let mut guard = self.client.lock().await;
                    *guard = None;
                }

                warn!("MQTT disconnected, reconnecting in 5s");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }

        #[cfg(not(feature = "mqtt"))]
        {
            let _ = tx;
            warn!("MQTT channel requires the 'mqtt' feature");
            std::future::pending::<()>().await;
            Ok(())
        }
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        if self.host.is_empty() {
            return Err(AttaError::Validation(
                "MQTT channel: broker host not configured".to_string(),
            ));
        }
        if self.subscribe_topic.is_empty() && self.publish_topic.is_empty() {
            return Err(AttaError::Validation(
                "MQTT channel: no topics configured".to_string(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mqtt_channel_name() {
        let ch = MqttChannel::new(
            "mqtt.example.com".to_string(),
            1883,
            "atta-bot".to_string(),
            "atta/inbox".to_string(),
            "atta/outbox".to_string(),
        );
        assert_eq!(ch.name(), "mqtt");
    }

    #[test]
    fn test_parse_json_payload() {
        let payload = br#"{"sender": "device-1", "content": "temperature: 25C"}"#;
        let msg = MqttChannel::parse_payload("sensors/temp", payload).unwrap();
        assert_eq!(msg.sender, "device-1");
        assert_eq!(msg.content, "temperature: 25C");
        assert_eq!(msg.channel, "mqtt");
    }

    #[test]
    fn test_parse_plain_text_payload() {
        let payload = b"Hello from MQTT";
        let msg = MqttChannel::parse_payload("test/topic", payload).unwrap();
        assert_eq!(msg.content, "Hello from MQTT");
        assert_eq!(msg.sender, "mqtt-unknown");
    }

    #[tokio::test]
    async fn test_health_check_valid() {
        let ch = MqttChannel::new(
            "mqtt.example.com".to_string(),
            1883,
            "test".to_string(),
            "in".to_string(),
            "out".to_string(),
        );
        assert!(ch.health_check().await.is_ok());
    }

    #[tokio::test]
    async fn test_health_check_empty_host() {
        let ch = MqttChannel::new(
            "".to_string(),
            1883,
            "test".to_string(),
            "in".to_string(),
            "out".to_string(),
        );
        assert!(ch.health_check().await.is_err());
    }
}
