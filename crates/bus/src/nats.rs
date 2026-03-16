//! NatsBus — Enterprise 版事件总线实现
//!
//! 基于 NATS JetStream，支持跨节点事件发布/订阅。
//! Topic 命名自然映射到 NATS subject（`atta.task.created` -> `atta.task.created`）。
//! JetStream 提供持久化、消费者组和 at-least-once 投递保证。

use async_nats::jetstream::{self, consumer::PullConsumer, stream::Stream as JsStream};
use async_stream::stream;
use futures::StreamExt;
use tracing::{debug, error, info, warn};

use atta_types::error::BusError;
use atta_types::{AttaError, EventEnvelope};

use crate::traits::{EventBus, EventStream};

/// NATS JetStream stream name
const STREAM_NAME: &str = "ATTA_EVENTS";

/// NATS JetStream subject prefix
const SUBJECT_PREFIX: &str = "atta.>";

/// NATS-based event bus for Enterprise deployments
///
/// Uses NATS JetStream for durable message delivery across cluster nodes.
/// All events are published to the `ATTA_EVENTS` stream.
pub struct NatsBus {
    _client: async_nats::Client,
    jetstream: jetstream::Context,
    stream: JsStream,
}

impl NatsBus {
    /// Connect to a NATS server and set up JetStream stream
    ///
    /// # Arguments
    /// * `url` - NATS server URL (e.g., `nats://localhost:4222`)
    pub async fn connect(url: &str) -> Result<Self, AttaError> {
        info!(url = %url, "connecting to NATS");

        let client = async_nats::connect(url)
            .await
            .map_err(|e| BusError::SubscribeFailed {
                topic: "nats".to_string(),
                source: e.into(),
            })?;

        let jetstream = jetstream::new(client.clone());

        // Create or get the ATTA_EVENTS stream
        let stream = jetstream
            .get_or_create_stream(jetstream::stream::Config {
                name: STREAM_NAME.to_string(),
                subjects: vec![SUBJECT_PREFIX.to_string()],
                retention: jetstream::stream::RetentionPolicy::Limits,
                max_messages: 1_000_000,
                max_age: std::time::Duration::from_secs(7 * 24 * 3600), // 7 days
                ..Default::default()
            })
            .await
            .map_err(|e| BusError::SubscribeFailed {
                topic: STREAM_NAME.to_string(),
                source: e.into(),
            })?;

        info!("NATS JetStream stream '{}' ready", STREAM_NAME);

        Ok(Self {
            _client: client,
            jetstream,
            stream,
        })
    }
}

#[async_trait::async_trait]
impl EventBus for NatsBus {
    async fn publish(&self, topic: &str, event: EventEnvelope) -> Result<(), AttaError> {
        debug!(topic, event_type = %event.event_type, "publishing to NATS");

        let payload = serde_json::to_vec(&event).map_err(|e| BusError::PublishFailed {
            topic: topic.to_string(),
            source: e.into(),
        })?;

        self.jetstream
            .publish(topic.to_string(), payload.into())
            .await
            .map_err(|e| BusError::PublishFailed {
                topic: topic.to_string(),
                source: e.into(),
            })?
            .await
            .map_err(|e| BusError::PublishFailed {
                topic: topic.to_string(),
                source: e.into(),
            })?;

        Ok(())
    }

    async fn subscribe(&self, topic: &str) -> Result<EventStream, AttaError> {
        debug!(topic, "subscribing via NATS");

        // Convert wildcard: `atta.task.*` -> `atta.task.>`
        let nats_subject = if let Some(prefix) = topic.strip_suffix('*') {
            format!("{}>", prefix)
        } else {
            topic.to_string()
        };

        // Create an ephemeral pull consumer
        let consumer: PullConsumer = self
            .stream
            .create_consumer(jetstream::consumer::pull::Config {
                filter_subject: nats_subject.clone(),
                ..Default::default()
            })
            .await
            .map_err(|e| BusError::SubscribeFailed {
                topic: topic.to_string(),
                source: e.into(),
            })?;

        let topic_owned = topic.to_string();

        let stream = stream! {
            let mut messages = match consumer.messages().await {
                Ok(m) => m,
                Err(e) => {
                    error!(topic = %topic_owned, error = %e, "failed to create NATS message stream");
                    return;
                }
            };
            while let Some(Ok(msg)) = messages.next().await {
                match serde_json::from_slice::<EventEnvelope>(&msg.payload) {
                    Ok(event) => {
                        // Acknowledge message
                        if let Err(e) = msg.ack().await {
                            warn!(topic = %topic_owned, error = %e, "failed to ack NATS message");
                        }
                        yield event;
                    }
                    Err(e) => {
                        let payload_preview = String::from_utf8_lossy(&msg.payload);
                        let preview = if payload_preview.len() > 200 {
                            format!("{}...(truncated)", &payload_preview[..200])
                        } else {
                            payload_preview.to_string()
                        };
                        error!(
                            topic = %topic_owned,
                            payload_len = msg.payload.len(),
                            payload_preview = %preview,
                            error = %e,
                            "permanently undeliverable message (deserialization failed), acknowledging"
                        );
                        let _ = msg.ack().await;
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }

    async fn subscribe_group(&self, topic: &str, group: &str) -> Result<EventStream, AttaError> {
        debug!(topic, group, "subscribing with consumer group via NATS");

        let nats_subject = if let Some(prefix) = topic.strip_suffix('*') {
            format!("{}>", prefix)
        } else {
            topic.to_string()
        };

        // Create a durable pull consumer with a group name
        let consumer: PullConsumer = self
            .stream
            .create_consumer(jetstream::consumer::pull::Config {
                durable_name: Some(group.to_string()),
                filter_subject: nats_subject.clone(),
                ..Default::default()
            })
            .await
            .map_err(|e| BusError::SubscribeFailed {
                topic: topic.to_string(),
                source: e.into(),
            })?;

        let topic_owned = topic.to_string();

        let stream = stream! {
            let mut messages = match consumer.messages().await {
                Ok(m) => m,
                Err(e) => {
                    error!(topic = %topic_owned, error = %e, "failed to create NATS message stream");
                    return;
                }
            };
            while let Some(Ok(msg)) = messages.next().await {
                match serde_json::from_slice::<EventEnvelope>(&msg.payload) {
                    Ok(event) => {
                        if let Err(e) = msg.ack().await {
                            warn!(topic = %topic_owned, error = %e, "failed to ack NATS message");
                        }
                        yield event;
                    }
                    Err(e) => {
                        let payload_preview = String::from_utf8_lossy(&msg.payload);
                        let preview = if payload_preview.len() > 200 {
                            format!("{}...(truncated)", &payload_preview[..200])
                        } else {
                            payload_preview.to_string()
                        };
                        error!(
                            topic = %topic_owned,
                            payload_len = msg.payload.len(),
                            payload_preview = %preview,
                            error = %e,
                            "permanently undeliverable message (deserialization failed), acknowledging"
                        );
                        let _ = msg.ack().await;
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_name() {
        assert_eq!(STREAM_NAME, "ATTA_EVENTS");
    }

    #[test]
    fn test_wildcard_conversion() {
        // Verify our wildcard conversion logic
        let topic = "atta.task.*";
        if let Some(prefix) = topic.strip_suffix('*') {
            let nats_subject = format!("{}>", prefix);
            assert_eq!(nats_subject, "atta.task.>");
        }
    }
}
