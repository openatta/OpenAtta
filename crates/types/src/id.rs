//! Base58-encoded UUID identifiers
//!
//! All AttaOS entities use base58-encoded UUIDs as their primary identifiers.
//! This produces compact, URL-safe, human-readable 22-character strings.
//!
//! # Examples
//!
//! ```
//! use atta_types::id::new_id;
//!
//! let id = new_id();
//! assert!(id.len() <= 22);
//! assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));
//! ```

use uuid::Uuid;

/// Generate a new base58-encoded UUID.
///
/// Returns a compact string (typically 22 characters) suitable for use as
/// an entity identifier in flows, skills, tasks, etc.
pub fn new_id() -> String {
    let uuid = Uuid::new_v4();
    bs58::encode(uuid.as_bytes()).into_string()
}

/// Encode an existing UUID to base58.
pub fn uuid_to_base58(uuid: &Uuid) -> String {
    bs58::encode(uuid.as_bytes()).into_string()
}

/// Decode a base58 string back to a UUID.
///
/// Returns `None` if the string is not a valid base58-encoded UUID.
pub fn base58_to_uuid(s: &str) -> Option<Uuid> {
    let bytes = bs58::decode(s).into_vec().ok()?;
    if bytes.len() != 16 {
        return None;
    }
    Some(Uuid::from_bytes(bytes.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_id_produces_valid_base58() {
        let id = new_id();
        assert!(!id.is_empty());
        assert!(id.len() <= 22);
        // base58 alphabet: 123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn round_trip_uuid_base58() {
        let uuid = Uuid::new_v4();
        let encoded = uuid_to_base58(&uuid);
        let decoded = base58_to_uuid(&encoded).expect("should decode");
        assert_eq!(decoded, uuid);
    }

    #[test]
    fn invalid_base58_returns_none() {
        assert!(base58_to_uuid("too-short").is_none());
        assert!(base58_to_uuid("").is_none());
    }

    #[test]
    fn ids_are_unique() {
        let a = new_id();
        let b = new_id();
        assert_ne!(a, b);
    }
}
