//! SQLite-backed SecretStore with AES-256-GCM encryption

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use chrono::Utc;
use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;
use sqlx::SqlitePool;
use tracing::debug;

use atta_types::AttaError;

use crate::traits::SecretStore;

/// SQLite-backed secret store with AES-256-GCM encryption
pub struct SqliteSecretStore {
    pool: SqlitePool,
    cipher: Aes256Gcm,
}

impl SqliteSecretStore {
    /// Create a new SqliteSecretStore
    ///
    /// The `master_key` is used to derive the encryption key via HKDF-SHA256.
    pub fn new(pool: SqlitePool, master_key: &[u8]) -> Self {
        let cipher = Self::derive_cipher(master_key);
        Self { pool, cipher }
    }

    /// Load or generate the master key from disk
    pub fn load_or_generate_master_key(path: &std::path::Path) -> Result<Vec<u8>, AttaError> {
        if path.exists() {
            let key = std::fs::read(path)?;
            if key.len() != 32 {
                return Err(AttaError::Validation(format!(
                    "master key at {} has invalid length: {} (expected 32)",
                    path.display(),
                    key.len()
                )));
            }
            Ok(key)
        } else {
            let mut key = vec![0u8; 32];
            rand::thread_rng().fill_bytes(&mut key);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, &key)?;
            debug!(path = %path.display(), "generated new master key");
            Ok(key)
        }
    }

    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, AttaError> {
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| AttaError::Other(anyhow::anyhow!("encryption failed: {}", e)))?;

        // Prepend nonce to ciphertext
        let mut result = nonce_bytes.to_vec();
        result.extend_from_slice(&ciphertext);
        Ok(result)
    }

    /// Derive an AES-256-GCM cipher from a master key using HKDF-SHA256.
    fn derive_cipher(master_key: &[u8]) -> Aes256Gcm {
        let hk = Hkdf::<Sha256>::new(None, master_key);
        let mut key_bytes = [0u8; 32];
        hk.expand(b"atta-secret-store-v1", &mut key_bytes)
            .expect("HKDF expand failed");
        Aes256Gcm::new_from_slice(&key_bytes).expect("AES-256-GCM key init failed")
    }

    /// Rotate the master key: re-encrypt all secrets with a new key.
    ///
    /// This is a transactional operation — if any re-encryption fails,
    /// the entire rotation is rolled back.
    pub async fn rotate_key(&mut self, new_master_key: &[u8]) -> Result<usize, AttaError> {
        // 1. Read all secrets (key, encrypted_value)
        let rows: Vec<(String, Vec<u8>)> =
            sqlx::query_as("SELECT key, value FROM secrets")
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AttaError::Other(e.into()))?;

        if rows.is_empty() {
            // No secrets to rotate, just update the cipher
            self.cipher = Self::derive_cipher(new_master_key);
            return Ok(0);
        }

        // 2. Decrypt each with current cipher
        let mut decrypted: Vec<(String, Vec<u8>)> = Vec::with_capacity(rows.len());
        for (key, encrypted) in &rows {
            let plaintext = self.decrypt(encrypted)?;
            decrypted.push((key.clone(), plaintext));
        }

        // 3. Create new cipher from new_master_key
        let new_cipher = Self::derive_cipher(new_master_key);

        // 4. Re-encrypt each value with new cipher
        let mut re_encrypted: Vec<(String, Vec<u8>)> = Vec::with_capacity(decrypted.len());
        for (key, plaintext) in &decrypted {
            let mut nonce_bytes = [0u8; 12];
            rand::thread_rng().fill_bytes(&mut nonce_bytes);
            let nonce = Nonce::from_slice(&nonce_bytes);

            let ciphertext = new_cipher
                .encrypt(nonce, plaintext.as_slice())
                .map_err(|e| AttaError::Other(anyhow::anyhow!("re-encryption failed: {}", e)))?;

            let mut result = nonce_bytes.to_vec();
            result.extend_from_slice(&ciphertext);
            re_encrypted.push((key.clone(), result));
        }

        // 5. Update all rows in a single transaction
        let mut tx = self.pool.begin().await.map_err(|e| AttaError::Other(e.into()))?;
        for (key, encrypted) in &re_encrypted {
            sqlx::query("UPDATE secrets SET value = ? WHERE key = ?")
                .bind(encrypted)
                .bind(key)
                .execute(&mut *tx)
                .await
                .map_err(|e| AttaError::Other(e.into()))?;
        }
        tx.commit().await.map_err(|e| AttaError::Other(e.into()))?;

        // 6. Replace self.cipher with new cipher
        let count = re_encrypted.len();
        self.cipher = new_cipher;

        debug!(count, "rotated master key, re-encrypted all secrets");
        Ok(count)
    }

    fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, AttaError> {
        if data.len() < 12 {
            return Err(AttaError::Other(anyhow::anyhow!(
                "encrypted data too short"
            )));
        }

        let (nonce_bytes, ciphertext) = data.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| AttaError::Other(anyhow::anyhow!("decryption failed: {}", e)))
    }
}

#[async_trait::async_trait]
impl SecretStore for SqliteSecretStore {
    async fn get(&self, key: &str) -> Result<Option<String>, AttaError> {
        let row: Option<(Vec<u8>,)> = sqlx::query_as("SELECT value FROM secrets WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        match row {
            Some((encrypted,)) => {
                let plaintext = self.decrypt(&encrypted)?;
                let value = String::from_utf8(plaintext).map_err(|e| {
                    AttaError::Other(anyhow::anyhow!("invalid UTF-8 secret: {}", e))
                })?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    async fn set(&self, key: &str, value: &str) -> Result<(), AttaError> {
        let encrypted = self.encrypt(value.as_bytes())?;
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO secrets (key, value, created_at, updated_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        )
        .bind(key)
        .bind(&encrypted)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| AttaError::Other(e.into()))?;

        debug!(key, "secret stored");
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), AttaError> {
        sqlx::query("DELETE FROM secrets WHERE key = ?")
            .bind(key)
            .execute(&self.pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        debug!(key, "secret deleted");
        Ok(())
    }

    async fn list_keys(&self) -> Result<Vec<String>, AttaError> {
        let rows: Vec<(String,)> = sqlx::query_as("SELECT key FROM secrets ORDER BY key")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        Ok(rows.into_iter().map(|(k,)| k).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup() -> SqliteSecretStore {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();

        // Create secrets table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS secrets (
                key TEXT PRIMARY KEY,
                value BLOB NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        let master_key = [42u8; 32];
        SqliteSecretStore::new(pool, &master_key)
    }

    #[tokio::test]
    async fn test_set_and_get() {
        let store = setup().await;
        store.set("api_key", "sk-test-123").await.unwrap();
        let value = store.get("api_key").await.unwrap();
        assert_eq!(value.unwrap(), "sk-test-123");
    }

    #[tokio::test]
    async fn test_get_nonexistent() {
        let store = setup().await;
        let value = store.get("missing").await.unwrap();
        assert!(value.is_none());
    }

    #[tokio::test]
    async fn test_update() {
        let store = setup().await;
        store.set("key", "value1").await.unwrap();
        store.set("key", "value2").await.unwrap();
        let value = store.get("key").await.unwrap();
        assert_eq!(value.unwrap(), "value2");
    }

    #[tokio::test]
    async fn test_delete() {
        let store = setup().await;
        store.set("key", "value").await.unwrap();
        store.delete("key").await.unwrap();
        let value = store.get("key").await.unwrap();
        assert!(value.is_none());
    }

    #[tokio::test]
    async fn test_list_keys() {
        let store = setup().await;
        store.set("b_key", "val").await.unwrap();
        store.set("a_key", "val").await.unwrap();
        let keys = store.list_keys().await.unwrap();
        assert_eq!(keys, vec!["a_key", "b_key"]);
    }

    #[tokio::test]
    async fn test_empty_key() {
        let store = setup().await;
        store.set("", "value").await.unwrap();
        let value = store.get("").await.unwrap();
        assert_eq!(value.unwrap(), "value");
    }

    #[tokio::test]
    async fn test_empty_value() {
        let store = setup().await;
        store.set("key", "").await.unwrap();
        let value = store.get("key").await.unwrap();
        assert_eq!(value.unwrap(), "");
    }

    #[tokio::test]
    async fn test_large_value() {
        let store = setup().await;
        let large = "x".repeat(100_000);
        store.set("big", &large).await.unwrap();
        let value = store.get("big").await.unwrap();
        assert_eq!(value.unwrap(), large);
    }

    #[tokio::test]
    async fn test_unicode_value() {
        let store = setup().await;
        let unicode = "你好世界🌍";
        store.set("lang", unicode).await.unwrap();
        let value = store.get("lang").await.unwrap();
        assert_eq!(value.unwrap(), unicode);
    }

    #[tokio::test]
    async fn test_different_master_keys_cannot_decrypt() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS secrets (
                key TEXT PRIMARY KEY,
                value BLOB NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        let store1 = SqliteSecretStore::new(pool.clone(), &[1u8; 32]);
        store1.set("key", "secret-data").await.unwrap();

        // Different master key should fail to decrypt
        let store2 = SqliteSecretStore::new(pool, &[2u8; 32]);
        let result = store2.get("key").await;
        assert!(result.is_err(), "decryption with wrong key should fail");
    }

    #[tokio::test]
    async fn test_decrypt_corrupted_data() {
        let store = setup().await;
        // Manually corrupt the ciphertext
        let short_data = vec![0u8; 5]; // too short for nonce (12 bytes)
        let result = store.decrypt(&short_data);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_nonexistent_key() {
        let store = setup().await;
        // Deleting a key that doesn't exist should not error
        store.delete("nonexistent").await.unwrap();
    }

    #[tokio::test]
    async fn test_list_keys_empty() {
        let store = setup().await;
        let keys = store.list_keys().await.unwrap();
        assert!(keys.is_empty());
    }

    #[test]
    fn test_load_or_generate_master_key_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("master.key");

        // First call generates
        let key1 = SqliteSecretStore::load_or_generate_master_key(&key_path).unwrap();
        assert_eq!(key1.len(), 32);

        // Second call loads same key
        let key2 = SqliteSecretStore::load_or_generate_master_key(&key_path).unwrap();
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_load_master_key_invalid_length() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("bad.key");
        std::fs::write(&key_path, [0u8; 16]).unwrap(); // wrong length

        let result = SqliteSecretStore::load_or_generate_master_key(&key_path);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_rotate_key() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS secrets (
                key TEXT PRIMARY KEY,
                value BLOB NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        let old_key = [42u8; 32];
        let new_key = [99u8; 32];

        let mut store = SqliteSecretStore::new(pool.clone(), &old_key);

        // Set some secrets
        store.set("secret_a", "value_a").await.unwrap();
        store.set("secret_b", "value_b").await.unwrap();
        store.set("secret_c", "value_c").await.unwrap();

        // Rotate to new key
        let count = store.rotate_key(&new_key).await.unwrap();
        assert_eq!(count, 3);

        // All secrets should still be readable through the rotated store
        assert_eq!(store.get("secret_a").await.unwrap().unwrap(), "value_a");
        assert_eq!(store.get("secret_b").await.unwrap().unwrap(), "value_b");
        assert_eq!(store.get("secret_c").await.unwrap().unwrap(), "value_c");

        // A store with the OLD key should NOT be able to read the secrets
        let old_store = SqliteSecretStore::new(pool.clone(), &old_key);
        let result = old_store.get("secret_a").await;
        assert!(result.is_err(), "old key should not decrypt rotated secrets");

        // A store with the NEW key should be able to read the secrets
        let new_store = SqliteSecretStore::new(pool, &new_key);
        assert_eq!(
            new_store.get("secret_a").await.unwrap().unwrap(),
            "value_a"
        );
        assert_eq!(
            new_store.get("secret_b").await.unwrap().unwrap(),
            "value_b"
        );
    }

    #[tokio::test]
    async fn test_rotate_key_empty_store() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS secrets (
                key TEXT PRIMARY KEY,
                value BLOB NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut store = SqliteSecretStore::new(pool, &[42u8; 32]);
        let count = store.rotate_key(&[99u8; 32]).await.unwrap();
        assert_eq!(count, 0);
    }
}
