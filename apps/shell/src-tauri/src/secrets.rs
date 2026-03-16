//! API key encryption using ChaCha20-Poly1305 AEAD.
//!
//! Master key is stored at `$ATTA_HOME/etc/.keys_master` (0o600 on Unix).
//! Encrypted values are prefixed with `enc:` followed by hex-encoded nonce‖ciphertext‖tag.

use std::path::Path;

use chacha20poly1305::{
    aead::{Aead, KeyInit, OsRng},
    ChaCha20Poly1305, Nonce,
};
use rand::RngCore;
use zeroize::Zeroize;

/// 32-byte master encryption key.
pub struct MasterKey {
    key: [u8; 32],
}

impl Drop for MasterKey {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

impl MasterKey {
    /// Load the master key from `$ATTA_HOME/etc/.keys_master`.
    ///
    /// If the file does not exist, generates a new random key and writes it (Unix: 0o600).
    pub fn load_or_create(home: &Path) -> Result<Self, String> {
        let etc_dir = home.join("etc");
        std::fs::create_dir_all(&etc_dir)
            .map_err(|e| format!("[SECURITY] Cannot create etc directory: {e}"))?;

        let key_path = etc_dir.join(".keys_master");
        if key_path.exists() {
            let data = std::fs::read(&key_path)
                .map_err(|e| format!("[SECURITY] Cannot read master key: {e}"))?;
            if data.len() != 32 {
                return Err("[SECURITY] Master key file is corrupted (expected 32 bytes)".into());
            }
            let mut key = [0u8; 32];
            key.copy_from_slice(&data);
            Ok(Self { key })
        } else {
            let mut key = [0u8; 32];
            OsRng.fill_bytes(&mut key);
            std::fs::write(&key_path, key)
                .map_err(|e| format!("[SECURITY] Cannot write master key: {e}"))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(0o600);
                std::fs::set_permissions(&key_path, perms)
                    .map_err(|e| format!("[SECURITY] Cannot set key file permissions: {e}"))?;
            }
            Ok(Self { key })
        }
    }
}

/// Encrypt plaintext using ChaCha20-Poly1305 AEAD.
///
/// Returns `enc:` + hex(nonce ‖ ciphertext ‖ tag).
pub fn encrypt(master: &MasterKey, plaintext: &[u8]) -> Result<String, String> {
    let cipher = ChaCha20Poly1305::new_from_slice(&master.key)
        .map_err(|e| format!("[SECURITY] Cipher init failed: {e}"))?;

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| format!("[SECURITY] Encryption failed: {e}"))?;

    // nonce (12) + ciphertext + tag (16, appended by AEAD)
    let mut combined = Vec::with_capacity(12 + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);

    Ok(format!("enc:{}", hex::encode(&combined)))
}

/// Decrypt a value previously encrypted with [`encrypt`].
///
/// Expects format: `enc:` + hex(nonce ‖ ciphertext ‖ tag).
pub fn decrypt(master: &MasterKey, encoded: &str) -> Result<Vec<u8>, String> {
    let hex_str = encoded
        .strip_prefix("enc:")
        .ok_or_else(|| "[SECURITY] Not an encrypted value (missing enc: prefix)".to_string())?;

    let combined =
        hex::decode(hex_str).map_err(|e| format!("[SECURITY] Invalid hex encoding: {e}"))?;

    if combined.len() < 12 + 16 {
        return Err("[SECURITY] Encrypted data too short".into());
    }

    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);

    let cipher = ChaCha20Poly1305::new_from_slice(&master.key)
        .map_err(|e| format!("[SECURITY] Cipher init failed: {e}"))?;

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("[SECURITY] Decryption failed: {e}"))
}

/// Check whether a string value is already encrypted (has `enc:` prefix).
pub fn is_encrypted(value: &str) -> bool {
    value.starts_with("enc:")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let master = MasterKey::load_or_create(tmp.path()).unwrap();
        let plaintext = b"sk-test-1234567890";
        let encrypted = encrypt(&master, plaintext).unwrap();
        assert!(encrypted.starts_with("enc:"));
        let decrypted = decrypt(&master, &encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_key_persistence() {
        let tmp = TempDir::new().unwrap();
        let key1 = MasterKey::load_or_create(tmp.path()).unwrap();
        let key2 = MasterKey::load_or_create(tmp.path()).unwrap();
        assert_eq!(key1.key, key2.key);
    }
}
