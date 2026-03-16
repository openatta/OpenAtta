//! EventBus trait 定义
//!
//! 事件总线抽象，负责系统内所有事件的发布与订阅。
//! 所有状态变更通过 EventBus 流转，实现组件解耦。

use atta_types::{AttaError, EventEnvelope};
use futures::Stream;
use std::pin::Pin;

/// 事件流类型别名
pub type EventStream = Pin<Box<dyn Stream<Item = EventEnvelope> + Send>>;

/// 事件总线抽象
///
/// Desktop 版使用 [`InProcBus`](crate::InProcBus)（tokio broadcast），
/// Enterprise 版使用 NatsBus（NATS JetStream）。
///
/// # Topic 命名约定
///
/// 使用点分层级命名，例如：
/// - `atta.task.created`
/// - `atta.flow.advanced`
/// - `atta.agent.completed`
///
/// 支持通配符订阅：
/// - `atta.task.*` — 匹配所有 `atta.task.` 前缀的事件
/// - `atta.*` — 匹配所有 `atta.` 前缀的事件
#[async_trait::async_trait]
pub trait EventBus: Send + Sync + 'static {
    /// 发布事件到指定 topic
    async fn publish(&self, topic: &str, event: EventEnvelope) -> Result<(), AttaError>;

    /// 订阅指定 topic，返回事件流
    ///
    /// 支持通配符：topic 以 `*` 结尾时，匹配该前缀下的所有事件。
    async fn subscribe(&self, topic: &str) -> Result<EventStream, AttaError>;

    /// 带消费者组的订阅（Enterprise 专用，Desktop 退化为普通订阅）
    ///
    /// 在 Enterprise 版中，同一 group 内的多个消费者共享消息（负载均衡）。
    /// Desktop 版默认退化为普通订阅，所有订阅者都收到全量消息。
    async fn subscribe_group(&self, topic: &str, _group: &str) -> Result<EventStream, AttaError> {
        // 默认实现：退化为普通订阅
        self.subscribe(topic).await
    }
}
