//! SecretStore trait

use atta_types::AttaError;

/// Secret storage trait
///
/// Provides encrypted key-value storage for API keys, tokens, and other secrets.
///
/// # Examples
///
/// ```rust,no_run
/// use atta_secret::SecretStore;
///
/// # async fn example(store: impl SecretStore) -> Result<(), atta_types::AttaError> {
/// store.set("openai_key", "sk-...").await?;
/// let key = store.get("openai_key").await?;
/// assert_eq!(key.as_deref(), Some("sk-..."));
///
/// let keys = store.list_keys().await?;
/// assert!(keys.contains(&"openai_key".to_string()));
///
/// store.delete("openai_key").await?;
/// # Ok(())
/// # }
/// ```
#[async_trait::async_trait]
pub trait SecretStore: Send + Sync + 'static {
    /// Get a secret by key
    async fn get(&self, key: &str) -> Result<Option<String>, AttaError>;

    /// Set a secret (creates or updates)
    async fn set(&self, key: &str, value: &str) -> Result<(), AttaError>;

    /// Delete a secret
    async fn delete(&self, key: &str) -> Result<(), AttaError>;

    /// List all secret keys (values not returned)
    async fn list_keys(&self) -> Result<Vec<String>, AttaError>;
}
