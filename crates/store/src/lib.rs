//! AttaOS 状态存储层
//!
//! 定义 [`StateStore`] trait 及其实现。
//! - Desktop 版：[`SqliteStore`]（SQLite）
//! - Enterprise 版：PostgresStore（待实现）

mod common;
#[cfg(feature = "postgres")]
mod postgres;
#[cfg(feature = "sqlite")]
mod sqlite;
mod traits;

#[cfg(feature = "postgres")]
pub use postgres::PostgresStore;
#[cfg(feature = "sqlite")]
pub use sqlite::SqliteStore;
pub use traits::{
    ApprovalStore, CronStore, FlowStore, McpStore, NodeStore, PackageStore, RbacStore,
    RegistryStore, RemoteAgentStore, ServiceAccountStore, StateStore, TaskStore, UsageStore,
};
