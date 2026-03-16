//! AttaOS SecretStore
//!
//! Provides encrypted key-value secret storage.
//!
//! - [`traits::SecretStore`] — Secret storage trait
//! - [`sqlite::SqliteSecretStore`] — SQLite-backed implementation with AES-256-GCM encryption

pub mod sqlite;
pub mod traits;

pub use sqlite::SqliteSecretStore;
pub use traits::SecretStore;
