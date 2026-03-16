//! AttaOS 审计模块
//!
//! 提供 `AuditSink` trait 及其实现：
//! - `NoopAudit`：Desktop 版默认，仅 debug 日志
//! - （Enterprise 版将提供 `AuditStore`，持久化到数据库）

pub mod noop;
#[cfg(feature = "sqlite")]
pub mod store;
pub mod traits;

pub use noop::NoopAudit;
#[cfg(feature = "sqlite")]
pub use store::AuditStore;
pub use traits::{AuditEntry, AuditFilter, AuditOutcome, AuditSink};
