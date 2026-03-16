//! AttaOS 事件总线抽象
//!
//! 本 crate 定义 [`EventBus`] trait 以及 Desktop 版实现 [`InProcBus`]。
//!
//! # 架构
//!
//! EventBus 是 AttaOS 5 个核心 trait 之一，负责系统内所有事件的发布与订阅。
//! 所有状态变更通过 [`EventEnvelope`](atta_types::EventEnvelope) 在总线上流转，实现组件解耦。
//!
//! # 双版本实现
//!
//! | 版本 | 实现 | 依赖 |
//! |------|------|------|
//! | Desktop | [`InProcBus`] | tokio broadcast（进程内） |
//! | Enterprise | NatsBus | NATS JetStream（跨节点） |
//!
//! # 使用示例
//!
//! ```rust,no_run
//! use atta_bus::{EventBus, InProcBus};
//! use atta_types::EventEnvelope;
//! use futures::StreamExt;
//!
//! # async fn example() -> Result<(), atta_types::AttaError> {
//! let bus = InProcBus::new();
//!
//! // 订阅
//! let mut stream = bus.subscribe("atta.task.*").await?;
//!
//! // 发布
//! let event = EventEnvelope::system_started("desktop")?;
//! bus.publish("atta.system.started", event).await?;
//! # Ok(())
//! # }
//! ```

#[cfg(feature = "inproc")]
pub mod inproc;
#[cfg(feature = "nats")]
pub mod nats;
pub mod traits;

#[cfg(feature = "inproc")]
pub use inproc::InProcBus;
#[cfg(feature = "nats")]
pub use nats::NatsBus;
pub use traits::{EventBus, EventStream};
