//! InProcBus — Desktop 版事件总线实现
//!
//! 基于 tokio broadcast channel，零外部依赖。
//! 支持通配符订阅：topic 以 `*` 结尾时匹配前缀。

use std::collections::HashMap;
use std::sync::RwLock;

use async_stream::stream;
use tokio::sync::broadcast;
use tracing::{debug, error, trace};

use atta_types::{AttaError, EventEnvelope};

use crate::traits::{EventBus, EventStream};

/// 默认 broadcast channel 容量
const DEFAULT_CAPACITY: usize = 1024;

/// 进程内事件总线（Desktop 版）
///
/// 使用 tokio broadcast channel 实现发布/订阅。
/// 每个 topic 对应一个独立的 broadcast channel。
///
/// # 通配符订阅
///
/// 订阅 topic 以 `*` 结尾时，发布到匹配前缀的任何 topic 的事件都会被投递。
/// 例如订阅 `atta.task.*` 会收到 `atta.task.created`、`atta.task.updated` 等事件。
///
/// 实现方式：通配符订阅创建一个独立的 broadcast channel，发布时遍历所有注册的
/// 通配符模式进行前缀匹配，匹配成功则同时向通配符 channel 投递事件。
pub struct InProcBus {
    /// topic -> broadcast::Sender 映射
    channels: RwLock<HashMap<String, broadcast::Sender<EventEnvelope>>>,
    /// 每个 channel 的容量
    capacity: usize,
}

impl InProcBus {
    /// 创建默认容量（1024）的 InProcBus
    pub fn new() -> Self {
        Self {
            channels: RwLock::new(HashMap::new()),
            capacity: DEFAULT_CAPACITY,
        }
    }

    /// 创建指定容量的 InProcBus
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            channels: RwLock::new(HashMap::new()),
            capacity,
        }
    }

    /// 获取或创建指定 topic 的 broadcast::Sender
    ///
    /// 使用读写锁实现快速路径（读锁命中）和慢速路径（写锁创建）。
    fn get_or_create(&self, topic: &str) -> broadcast::Sender<EventEnvelope> {
        // 快速路径：读锁检查
        {
            let read = match self.channels.read() {
                Ok(guard) => guard,
                Err(e) => {
                    tracing::warn!("bus channels RwLock poisoned, recovering");
                    e.into_inner()
                }
            };
            if let Some(tx) = read.get(topic) {
                return tx.clone();
            }
        }
        // 慢速路径：写锁创建
        let mut write = match self.channels.write() {
            Ok(guard) => guard,
            Err(e) => {
                tracing::warn!("bus channels RwLock poisoned, recovering");
                e.into_inner()
            }
        };
        if write.len() > 5_000 {
            tracing::warn!(count = write.len(), "high number of bus topics, possible leak");
        }
        write
            .entry(topic.to_string())
            .or_insert_with(|| {
                debug!(topic, "creating new broadcast channel");
                broadcast::channel(self.capacity).0
            })
            .clone()
    }

    /// 检查一个通配符模式是否匹配给定的 topic
    ///
    /// 规则：模式以 `*` 结尾，去掉 `*` 后的前缀必须是 topic 的前缀。
    /// 例如 `atta.task.*` 匹配 `atta.task.created` 但不匹配 `atta.task` 本身
    /// （因为前缀 `atta.task.` 不是 `atta.task` 的前缀——等长不算更长的 topic）。
    fn wildcard_matches(pattern: &str, topic: &str) -> bool {
        if let Some(prefix) = pattern.strip_suffix('*') {
            topic.starts_with(prefix) && topic.len() > prefix.len()
        } else {
            false
        }
    }
}

impl Default for InProcBus {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl EventBus for InProcBus {
    /// 发布事件到指定 topic
    ///
    /// 除了精确匹配的 topic channel，还会向所有匹配的通配符 channel 投递。
    async fn publish(&self, topic: &str, event: EventEnvelope) -> Result<(), AttaError> {
        trace!(topic, event_type = %event.event_type, "publishing event");

        let tx = self.get_or_create(topic);

        // 向精确 topic channel 发送
        // 如果没有 receiver 也不算错误（可能暂时没人订阅）
        match tx.send(event.clone()) {
            Ok(n) => {
                trace!(topic, receivers = n, "event delivered to exact subscribers");
            }
            Err(_) => {
                trace!(topic, "no active receivers for exact topic");
            }
        }

        // NOTE: Each send() clones the event. For high-throughput scenarios with large payloads,
        // consider wrapping EventEnvelope in Arc to reduce memory pressure.

        // 向所有匹配的通配符 channel 发送
        // TODO: For high topic counts, consider replacing linear wildcard scan
        // with a trie or BTreeMap-based prefix index for O(log n) matching.
        let wildcard_topics: Vec<(String, broadcast::Sender<EventEnvelope>)> = {
            let read = match self.channels.read() {
                Ok(guard) => guard,
                Err(e) => {
                    tracing::warn!("bus channels RwLock poisoned, recovering");
                    e.into_inner()
                }
            };
            read.iter()
                .filter(|(pattern, _)| {
                    pattern.ends_with('*') && Self::wildcard_matches(pattern, topic)
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        };

        for (pattern, wtx) in wildcard_topics {
            match wtx.send(event.clone()) {
                Ok(n) => {
                    trace!(
                        topic,
                        pattern = %pattern,
                        receivers = n,
                        "event delivered to wildcard subscribers"
                    );
                }
                Err(_) => {
                    trace!(
                        topic,
                        pattern = %pattern,
                        "no active receivers for wildcard pattern"
                    );
                }
            }
        }

        Ok(())
    }

    /// 订阅指定 topic，返回事件流
    ///
    /// 如果 topic 以 `*` 结尾，则为通配符订阅。
    /// 通配符订阅会收到所有匹配前缀的事件。
    async fn subscribe(&self, topic: &str) -> Result<EventStream, AttaError> {
        debug!(topic, "subscribing to topic");

        let tx = self.get_or_create(topic);
        let mut rx = tx.subscribe();
        let topic_owned = topic.to_string();

        let stream = stream! {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        yield event;
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        error!(
                            topic = %topic_owned,
                            skipped = n,
                            "subscriber lagged — {n} events permanently lost (consider increasing bus capacity)"
                        );
                        // Continue receiving subsequent events
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        debug!(topic = %topic_owned, "broadcast channel closed");
                        break;
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
    use atta_types::{Actor, EntityRef};
    use futures::StreamExt;
    use uuid::Uuid;

    /// 创建测试用 EventEnvelope
    fn make_event(event_type: &str) -> EventEnvelope {
        EventEnvelope::new(
            event_type,
            EntityRef::task(&Uuid::new_v4()),
            Actor::system(),
            Uuid::new_v4(),
            serde_json::json!({"test": true}),
        )
        .expect("failed to create test event")
    }

    #[tokio::test]
    async fn test_publish_subscribe_exact() {
        let bus = InProcBus::new();
        let mut stream = bus.subscribe("atta.task.created").await.unwrap();

        let event = make_event("atta.task.created");
        let event_id = event.event_id;
        bus.publish("atta.task.created", event).await.unwrap();

        let received = stream.next().await.unwrap();
        assert_eq!(received.event_id, event_id);
    }

    #[tokio::test]
    async fn test_wildcard_subscribe() {
        let bus = InProcBus::new();
        let mut stream = bus.subscribe("atta.task.*").await.unwrap();

        let event = make_event("atta.task.created");
        let event_id = event.event_id;
        bus.publish("atta.task.created", event).await.unwrap();

        let received = stream.next().await.unwrap();
        assert_eq!(received.event_id, event_id);
    }

    #[tokio::test]
    async fn test_wildcard_no_match_different_prefix() {
        let bus = InProcBus::new();
        let mut stream = bus.subscribe("atta.flow.*").await.unwrap();

        let event = make_event("atta.task.created");
        bus.publish("atta.task.created", event).await.unwrap();

        // 使用 tokio timeout 确认不会收到不匹配的事件
        let result =
            tokio::time::timeout(std::time::Duration::from_millis(50), stream.next()).await;
        assert!(
            result.is_err(),
            "should not receive event for non-matching prefix"
        );
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let bus = InProcBus::new();
        let mut stream1 = bus.subscribe("atta.task.created").await.unwrap();
        let mut stream2 = bus.subscribe("atta.task.created").await.unwrap();

        let event = make_event("atta.task.created");
        let event_id = event.event_id;
        bus.publish("atta.task.created", event).await.unwrap();

        let r1 = stream1.next().await.unwrap();
        let r2 = stream2.next().await.unwrap();
        assert_eq!(r1.event_id, event_id);
        assert_eq!(r2.event_id, event_id);
    }

    #[tokio::test]
    async fn test_subscribe_group_degrades_to_subscribe() {
        let bus = InProcBus::new();
        let mut stream = bus
            .subscribe_group("atta.task.created", "group1")
            .await
            .unwrap();

        let event = make_event("atta.task.created");
        let event_id = event.event_id;
        bus.publish("atta.task.created", event).await.unwrap();

        let received = stream.next().await.unwrap();
        assert_eq!(received.event_id, event_id);
    }

    #[tokio::test]
    async fn test_publish_no_subscribers_ok() {
        let bus = InProcBus::new();
        // Publishing with no subscribers should not error
        let event = make_event("atta.task.created");
        let result = bus.publish("atta.task.created", event).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wildcard_matches_helper() {
        assert!(InProcBus::wildcard_matches(
            "atta.task.*",
            "atta.task.created"
        ));
        assert!(InProcBus::wildcard_matches(
            "atta.task.*",
            "atta.task.updated"
        ));
        assert!(InProcBus::wildcard_matches("atta.*", "atta.task.created"));
        assert!(!InProcBus::wildcard_matches(
            "atta.flow.*",
            "atta.task.created"
        ));
        // 精确匹配不是通配符
        assert!(!InProcBus::wildcard_matches(
            "atta.task.created",
            "atta.task.created"
        ));
        // 前缀等长不匹配（topic 必须比前缀长）
        assert!(!InProcBus::wildcard_matches("atta.task.*", "atta.task."));
    }

    #[tokio::test]
    async fn test_with_capacity() {
        let bus = InProcBus::with_capacity(16);
        assert_eq!(bus.capacity, 16);
    }

    #[tokio::test]
    async fn test_default() {
        let bus = InProcBus::default();
        assert_eq!(bus.capacity, DEFAULT_CAPACITY);
    }
}
