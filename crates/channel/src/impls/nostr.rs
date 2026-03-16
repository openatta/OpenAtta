//! Nostr channel
//!
//! Connects to Nostr relays via WebSocket, publishes and subscribes to events.
//! Supports NIP-01 (basic protocol) with real secp256k1 cryptography for
//! event ID computation and Schnorr signing.
//! NIP-04 (encrypted DMs) is not yet implemented (requires AES-CBC).

use std::sync::Arc;

use atta_types::AttaError;
use tracing::{debug, info, warn};

use crate::traits::{Channel, ChannelMessage, ChatType, SendMessage};

/// Nostr channel
pub struct NostrChannel {
    name: String,
    /// Relay WebSocket URLs (e.g., ["wss://relay.damus.io", "wss://nos.lol"])
    relay_urls: Vec<String>,
    /// Private key (hex-encoded 32-byte secp256k1 secret key)
    private_key_hex: String,
    /// Public key (hex-encoded x-only, derived from private key)
    public_key_hex: String,
    /// Shared WebSocket writer for send()
    #[cfg(feature = "nostr")]
    ws_writer: Arc<
        tokio::sync::Mutex<
            Option<
                futures::stream::SplitSink<
                    tokio_tungstenite::WebSocketStream<
                        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
                    >,
                    tokio_tungstenite::tungstenite::Message,
                >,
            >,
        >,
    >,
}

impl NostrChannel {
    /// Create a new Nostr channel
    ///
    /// `private_key_hex` is the 64-character hex-encoded secp256k1 private key.
    /// The public key is derived using real secp256k1 x-only public key derivation.
    pub fn new(relay_urls: Vec<String>, private_key_hex: String) -> Self {
        let public_key_hex = Self::derive_public_key(&private_key_hex);

        Self {
            name: "nostr".to_string(),
            relay_urls,
            private_key_hex,
            public_key_hex,
            #[cfg(feature = "nostr")]
            ws_writer: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    /// Derive the x-only public key from a hex-encoded private key
    #[cfg(feature = "nostr")]
    fn derive_public_key(private_key_hex: &str) -> String {
        use secp256k1::{Secp256k1, SecretKey};

        let bytes = match hex::decode(private_key_hex) {
            Ok(b) => b,
            Err(_) => {
                return format!(
                    "invalid_{}",
                    &private_key_hex[..8.min(private_key_hex.len())]
                )
            }
        };

        let secp = Secp256k1::new();
        let sk = match SecretKey::from_slice(&bytes) {
            Ok(k) => k,
            Err(_) => {
                return format!(
                    "invalid_{}",
                    &private_key_hex[..8.min(private_key_hex.len())]
                )
            }
        };

        let (xonly, _parity) = sk.public_key(&secp).x_only_public_key();
        hex::encode(xonly.serialize())
    }

    #[cfg(not(feature = "nostr"))]
    fn derive_public_key(private_key_hex: &str) -> String {
        format!("pub_{}", &private_key_hex[..8.min(private_key_hex.len())])
    }

    /// Build and sign a Nostr event (NIP-01)
    ///
    /// Computes event ID as sha256 of the canonical serialization
    /// and signs it with Schnorr signature.
    #[cfg(feature = "nostr")]
    fn build_signed_event(
        &self,
        kind: u64,
        content: &str,
        tags: Vec<Vec<String>>,
    ) -> Result<serde_json::Value, AttaError> {
        use secp256k1::{KeyPair, Secp256k1, SecretKey};
        use sha2::{Digest, Sha256};

        let created_at = chrono::Utc::now().timestamp();

        // Canonical serialization for event ID:
        // [0, <pubkey>, <created_at>, <kind>, <tags>, <content>]
        let canonical =
            serde_json::json!([0, self.public_key_hex, created_at, kind, tags, content,]);

        let canonical_bytes = serde_json::to_string(&canonical)
            .map_err(|e| AttaError::Channel(format!("failed to serialize event: {e}")))?;

        // Event ID = sha256(canonical_serialization)
        let mut hasher = Sha256::new();
        hasher.update(canonical_bytes.as_bytes());
        let event_id_bytes: [u8; 32] = hasher.finalize().into();
        let event_id = hex::encode(event_id_bytes);

        // Sign with Schnorr
        let sk_bytes = hex::decode(&self.private_key_hex)
            .map_err(|e| AttaError::Channel(format!("invalid private key hex: {e}")))?;

        let secp = Secp256k1::new();
        let sk = SecretKey::from_slice(&sk_bytes)
            .map_err(|e| AttaError::Channel(format!("invalid private key: {e}")))?;
        let keypair = KeyPair::from_secret_key(&secp, &sk);

        let msg = secp256k1::Message::from_digest(event_id_bytes);
        let sig = secp.sign_schnorr_no_aux_rand(&msg, &keypair);

        Ok(serde_json::json!({
            "id": event_id,
            "pubkey": self.public_key_hex,
            "created_at": created_at,
            "kind": kind,
            "tags": tags,
            "content": content,
            "sig": hex::encode(sig.as_ref()),
        }))
    }

    /// Build event without real crypto (feature disabled)
    #[cfg(not(feature = "nostr"))]
    fn build_signed_event(
        &self,
        kind: u64,
        content: &str,
        tags: Vec<Vec<String>>,
    ) -> Result<serde_json::Value, AttaError> {
        let created_at = chrono::Utc::now().timestamp();
        let event_id = uuid::Uuid::new_v4().to_string().replace('-', "");

        Ok(serde_json::json!({
            "id": event_id,
            "pubkey": self.public_key_hex,
            "created_at": created_at,
            "kind": kind,
            "tags": tags,
            "content": content,
            "sig": "0".repeat(128),
        }))
    }

    /// NIP-04: Encrypt a message for a recipient using ECDH shared secret + AES-256-CBC.
    ///
    /// Returns base64(ciphertext) + "?iv=" + base64(iv)
    #[cfg(feature = "nostr")]
    fn nip04_encrypt(
        &self,
        plaintext: &str,
        recipient_pubkey_hex: &str,
    ) -> Result<String, AttaError> {
        use aes::cipher::{BlockEncryptMut, KeyIvInit};
        use secp256k1::{PublicKey, Secp256k1, SecretKey};

        // Derive ECDH shared secret
        let sk_bytes = hex::decode(&self.private_key_hex)
            .map_err(|e| AttaError::Channel(format!("invalid private key hex: {e}")))?;
        let sk = SecretKey::from_slice(&sk_bytes)
            .map_err(|e| AttaError::Channel(format!("invalid private key: {e}")))?;

        // Recipient pubkey is x-only (32 bytes), need to prepend 0x02 for compressed
        let pk_bytes = hex::decode(recipient_pubkey_hex)
            .map_err(|e| AttaError::Channel(format!("invalid recipient pubkey hex: {e}")))?;
        let mut compressed = vec![0x02]; // assume even y
        compressed.extend_from_slice(&pk_bytes);
        let pk = PublicKey::from_slice(&compressed)
            .map_err(|e| AttaError::Channel(format!("invalid recipient pubkey: {e}")))?;

        let secp = Secp256k1::new();
        let shared_point = secp256k1::ecdh::shared_secret_point(&pk, &sk);
        // NIP-04 uses the x-coordinate (first 32 bytes) as AES key
        let shared_key: [u8; 32] = shared_point[1..33]
            .try_into()
            .map_err(|_| AttaError::Channel("ECDH shared point too short".to_string()))?;

        // Generate random IV
        let iv: [u8; 16] = rand::random();

        // PKCS7 pad plaintext
        let plaintext_bytes = plaintext.as_bytes();
        let padding_len = 16 - (plaintext_bytes.len() % 16);
        let mut padded = plaintext_bytes.to_vec();
        padded.extend(vec![padding_len as u8; padding_len]);

        // AES-256-CBC encrypt
        type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;
        let mut buf = padded;
        let encryptor = Aes256CbcEnc::new(&shared_key.into(), &iv.into());
        // Process blocks in-place
        for chunk in buf.chunks_mut(16) {
            let block = aes::cipher::generic_array::GenericArray::from_mut_slice(chunk);
            encryptor.clone().encrypt_block_mut(block);
        }

        use base64::Engine;
        let ct_b64 = base64::engine::general_purpose::STANDARD.encode(&buf);
        let iv_b64 = base64::engine::general_purpose::STANDARD.encode(iv);

        Ok(format!("{}?iv={}", ct_b64, iv_b64))
    }

    /// NIP-04: Decrypt a message from a sender using ECDH shared secret + AES-256-CBC.
    #[cfg(feature = "nostr")]
    fn nip04_decrypt(
        &self,
        ciphertext_with_iv: &str,
        sender_pubkey_hex: &str,
    ) -> Result<String, AttaError> {
        use aes::cipher::{BlockDecryptMut, KeyIvInit};
        use base64::Engine;
        use secp256k1::{PublicKey, Secp256k1, SecretKey};

        // Parse "base64(ciphertext)?iv=base64(iv)"
        let parts: Vec<&str> = ciphertext_with_iv.splitn(2, "?iv=").collect();
        if parts.len() != 2 {
            return Err(AttaError::Channel(
                "invalid NIP-04 ciphertext format".to_string(),
            ));
        }

        let ciphertext = base64::engine::general_purpose::STANDARD
            .decode(parts[0])
            .map_err(|e| AttaError::Channel(format!("invalid base64 ciphertext: {e}")))?;
        let iv: [u8; 16] = base64::engine::general_purpose::STANDARD
            .decode(parts[1])
            .map_err(|e| AttaError::Channel(format!("invalid base64 IV: {e}")))?
            .try_into()
            .map_err(|_| AttaError::Channel("IV must be 16 bytes".to_string()))?;

        // Derive ECDH shared secret (same as encrypt but with sender's pubkey)
        let sk_bytes = hex::decode(&self.private_key_hex)
            .map_err(|e| AttaError::Channel(format!("invalid private key hex: {e}")))?;
        let sk = SecretKey::from_slice(&sk_bytes)
            .map_err(|e| AttaError::Channel(format!("invalid private key: {e}")))?;

        let pk_bytes = hex::decode(sender_pubkey_hex)
            .map_err(|e| AttaError::Channel(format!("invalid sender pubkey hex: {e}")))?;
        let mut compressed = vec![0x02];
        compressed.extend_from_slice(&pk_bytes);
        let pk = PublicKey::from_slice(&compressed)
            .map_err(|e| AttaError::Channel(format!("invalid sender pubkey: {e}")))?;

        let secp = Secp256k1::new();
        let shared_point = secp256k1::ecdh::shared_secret_point(&pk, &sk);
        let shared_key: [u8; 32] = shared_point[1..33]
            .try_into()
            .map_err(|_| AttaError::Channel("ECDH shared point too short".to_string()))?;

        // AES-256-CBC decrypt
        type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;
        let mut buf = ciphertext;
        let decryptor = Aes256CbcDec::new(&shared_key.into(), &iv.into());
        for chunk in buf.chunks_mut(16) {
            let block = aes::cipher::generic_array::GenericArray::from_mut_slice(chunk);
            decryptor.clone().decrypt_block_mut(block);
        }

        // Remove PKCS7 padding
        let padding_len = *buf
            .last()
            .ok_or_else(|| AttaError::Channel("empty decrypted data".to_string()))?
            as usize;
        if padding_len == 0 || padding_len > 16 || padding_len > buf.len() {
            return Err(AttaError::Channel("invalid PKCS7 padding".to_string()));
        }
        buf.truncate(buf.len() - padding_len);

        String::from_utf8(buf)
            .map_err(|e| AttaError::Channel(format!("decrypted data is not valid UTF-8: {e}")))
    }

    /// Build a subscription filter
    fn build_subscription_filter(&self) -> serde_json::Value {
        serde_json::json!({
            "kinds": [1, 4],  // kind 1 = text note, kind 4 = encrypted DM
            "#p": [self.public_key_hex],  // events mentioning us
            "since": chrono::Utc::now().timestamp(),
        })
    }
}

#[async_trait::async_trait]
impl Channel for NostrChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: SendMessage) -> Result<(), AttaError> {
        #[cfg(feature = "nostr")]
        {
            use futures::SinkExt;
            use tokio_tungstenite::tungstenite::Message as WsMessage;

            // Determine event kind and tags based on recipient
            let (kind, tags, content) =
                if message.recipient.starts_with("npub") || message.recipient.len() == 64 {
                    // Direct message (NIP-04 encrypted DM)
                    let recipient_hex = if message.recipient.starts_with("npub") {
                        // Strip "npub1" prefix — in production would bech32 decode
                        // For now, treat the rest as hex (callers should pass hex pubkeys)
                        message.recipient.trim_start_matches("npub1").to_string()
                    } else {
                        message.recipient.clone()
                    };

                    // NIP-04: encrypt content with ECDH + AES-256-CBC
                    let encrypted = self.nip04_encrypt(&message.content, &recipient_hex)?;

                    (4_u64, vec![vec!["p".to_string(), recipient_hex]], encrypted)
                } else {
                    // Public text note (kind 1)
                    let mut tags = Vec::new();
                    if let Some(ref thread_ts) = message.thread_ts {
                        tags.push(vec!["e".to_string(), thread_ts.clone()]);
                    }
                    (1_u64, tags, message.content.clone())
                };

            let event = self.build_signed_event(kind, &content, tags)?;

            // Send ["EVENT", event] to connected relay
            let relay_msg = serde_json::json!(["EVENT", event]);

            let mut guard = self.ws_writer.lock().await;
            if let Some(writer) = guard.as_mut() {
                writer
                    .send(WsMessage::Text(relay_msg.to_string()))
                    .await
                    .map_err(|e| AttaError::Channel(format!("Nostr WS send failed: {e}")))?;
                debug!(kind, "Nostr event sent to relay");
            } else {
                return Err(AttaError::Channel(
                    "Nostr: not connected to any relay, cannot send".to_string(),
                ));
            }

            Ok(())
        }

        #[cfg(not(feature = "nostr"))]
        {
            let _ = message;
            Err(AttaError::Channel(
                "Nostr channel requires the 'nostr' feature".to_string(),
            ))
        }
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        #[cfg(feature = "nostr")]
        {
            use futures::stream::StreamExt;
            use futures::SinkExt;
            use std::collections::HashSet;
            use tokio::sync::Mutex;
            use tokio_tungstenite::{connect_async, tungstenite::Message};

            // Track seen event IDs to deduplicate across relays
            let seen_ids: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));

            info!(
                relays = ?self.relay_urls,
                pubkey = %self.public_key_hex,
                "Nostr relay listener starting"
            );

            if self.relay_urls.is_empty() {
                return Err(AttaError::Validation(
                    "Nostr: no relay URLs configured".to_string(),
                ));
            }

            // Connect to first relay in list (multi-relay support can be added with tokio::select!)
            let mut relay_index = 0;

            loop {
                let relay_url = &self.relay_urls[relay_index % self.relay_urls.len()];
                info!(relay = %relay_url, "Nostr connecting to relay");

                let (ws_stream, _) = match connect_async(relay_url).await {
                    Ok(conn) => conn,
                    Err(e) => {
                        warn!(relay = %relay_url, error = %e, "Nostr WS connect failed, trying next relay in 5s");
                        relay_index += 1;
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                        continue;
                    }
                };

                let (write, mut read) = ws_stream.split();

                // Store writer for send() to use
                {
                    let mut guard = self.ws_writer.lock().await;
                    *guard = Some(write);
                }

                // Send subscription request: ["REQ", "<sub_id>", <filter>]
                let sub_id = format!(
                    "atta_{}",
                    &uuid::Uuid::new_v4().to_string().replace('-', "")[..8]
                );
                let filter = self.build_subscription_filter();
                let req_msg = serde_json::json!(["REQ", sub_id, filter]);

                {
                    let mut guard = self.ws_writer.lock().await;
                    if let Some(w) = guard.as_mut() {
                        if let Err(e) = w.send(Message::Text(req_msg.to_string())).await {
                            warn!(error = %e, "Nostr failed to send REQ, reconnecting");
                            *guard = None;
                            relay_index += 1;
                            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                            continue;
                        }
                    }
                }

                info!(sub_id = %sub_id, relay = %relay_url, "Nostr subscribed");

                // Read events
                while let Some(msg_result) = read.next().await {
                    let msg = match msg_result {
                        Ok(m) => m,
                        Err(e) => {
                            warn!(error = %e, "Nostr WS read error, reconnecting");
                            break;
                        }
                    };

                    let text = match msg {
                        Message::Text(t) => t,
                        Message::Ping(data) => {
                            let mut guard = self.ws_writer.lock().await;
                            if let Some(w) = guard.as_mut() {
                                let _ = w.send(Message::Pong(data)).await;
                            }
                            continue;
                        }
                        Message::Close(_) => {
                            info!("Nostr WS closed, reconnecting");
                            break;
                        }
                        _ => continue,
                    };

                    // Nostr messages are JSON arrays
                    let payload: serde_json::Value = match serde_json::from_str(&text) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    let arr = match payload.as_array() {
                        Some(a) => a,
                        None => continue,
                    };

                    if arr.is_empty() {
                        continue;
                    }

                    let msg_type = arr[0].as_str().unwrap_or("");

                    match msg_type {
                        "EVENT" => {
                            if arr.len() < 3 {
                                continue;
                            }

                            let event = &arr[2];
                            let event_id = event.get("id").and_then(|v| v.as_str()).unwrap_or("");

                            // Deduplicate
                            {
                                let mut seen = seen_ids.lock().await;
                                if seen.contains(event_id) {
                                    continue;
                                }
                                seen.insert(event_id.to_string());
                                if seen.len() > 10000 {
                                    seen.clear();
                                }
                            }

                            let kind = event.get("kind").and_then(|v| v.as_u64()).unwrap_or(0);
                            let pubkey = event.get("pubkey").and_then(|v| v.as_str()).unwrap_or("");

                            // Skip our own events
                            if pubkey == self.public_key_hex {
                                continue;
                            }

                            let content =
                                event.get("content").and_then(|v| v.as_str()).unwrap_or("");

                            // For kind 4 (encrypted DM), decrypt with NIP-04
                            let decrypted_content = if kind == 4 {
                                match self.nip04_decrypt(content, pubkey) {
                                    Ok(plaintext) => plaintext,
                                    Err(e) => {
                                        warn!(error = %e, "NIP-04 decryption failed, showing raw");
                                        format!("[encrypted DM] {}", content)
                                    }
                                }
                            } else {
                                content.to_string()
                            };

                            // Extract reply target from "e" tags
                            let reply_target = event
                                .get("tags")
                                .and_then(|v| v.as_array())
                                .and_then(|tags| {
                                    tags.iter().find_map(|tag| {
                                        let tag_arr = tag.as_array()?;
                                        if tag_arr.first()?.as_str()? == "e" {
                                            tag_arr.get(1)?.as_str().map(|s| s.to_string())
                                        } else {
                                            None
                                        }
                                    })
                                });

                            let created_at = event
                                .get("created_at")
                                .and_then(|v| v.as_i64())
                                .unwrap_or(0);
                            let timestamp = chrono::DateTime::from_timestamp(created_at, 0)
                                .unwrap_or_else(chrono::Utc::now);

                            let channel_msg = ChannelMessage {
                                id: event_id.to_string(),
                                sender: pubkey.to_string(),
                                content: decrypted_content,
                                channel: "nostr".to_string(),
                                reply_target,
                                timestamp,
                                thread_ts: None,
                                metadata: serde_json::json!({
                                    "kind": kind,
                                    "relay": relay_url,
                                    "sig": event.get("sig").and_then(|v| v.as_str()).unwrap_or(""),
                                }),
                                chat_type: ChatType::default(),
                                bot_mentioned: false,
                                group_id: None,
                            };

                            if tx.send(channel_msg).await.is_err() {
                                info!("Nostr listener: receiver dropped, stopping");
                                return Ok(());
                            }
                        }
                        "EOSE" => {
                            debug!("Nostr EOSE received, now listening for live events");
                        }
                        "OK" => {
                            if arr.len() >= 3 {
                                let success = arr.get(2).and_then(|v| v.as_bool()).unwrap_or(false);
                                debug!(success, "Nostr OK received");
                            }
                        }
                        "NOTICE" => {
                            let notice = arr.get(1).and_then(|v| v.as_str()).unwrap_or("");
                            warn!(notice, "Nostr relay NOTICE");
                        }
                        _ => {
                            debug!(msg_type, "Nostr unknown message type");
                        }
                    }
                }

                // Clear writer on disconnect
                {
                    let mut guard = self.ws_writer.lock().await;
                    *guard = None;
                }

                relay_index += 1;
                warn!("Nostr relay disconnected, reconnecting in 5s");
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }

        #[cfg(not(feature = "nostr"))]
        {
            let _ = tx;
            warn!("Nostr channel requires the 'nostr' feature");
            std::future::pending::<()>().await;
            Ok(())
        }
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        if self.relay_urls.is_empty() {
            return Err(AttaError::Validation(
                "Nostr channel: no relay URLs configured".to_string(),
            ));
        }
        if self.private_key_hex.is_empty() {
            return Err(AttaError::Validation(
                "Nostr channel: private key not configured".to_string(),
            ));
        }
        Ok(())
    }

    async fn add_reaction(&self, message_id: &str, reaction: &str) -> Result<(), AttaError> {
        #[cfg(feature = "nostr")]
        {
            use futures::SinkExt;
            use tokio_tungstenite::tungstenite::Message as WsMessage;

            // NIP-25: Reactions — kind 7
            let tags = vec![vec!["e".to_string(), message_id.to_string()]];
            let event = self.build_signed_event(7, reaction, tags)?;

            let relay_msg = serde_json::json!(["EVENT", event]);

            let mut guard = self.ws_writer.lock().await;
            if let Some(writer) = guard.as_mut() {
                writer
                    .send(WsMessage::Text(relay_msg.to_string()))
                    .await
                    .map_err(|e| AttaError::Channel(format!("Nostr reaction send failed: {e}")))?;
                debug!(event_id = %message_id, reaction, "Nostr reaction sent");
            } else {
                return Err(AttaError::Channel(
                    "Nostr: not connected, cannot send reaction".to_string(),
                ));
            }

            Ok(())
        }

        #[cfg(not(feature = "nostr"))]
        {
            let _ = (message_id, reaction);
            Err(AttaError::Channel(
                "Nostr channel requires the 'nostr' feature".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nostr_channel_name() {
        let ch = NostrChannel::new(vec!["wss://relay.damus.io".to_string()], "a".repeat(64));
        assert_eq!(ch.name(), "nostr");
    }

    #[test]
    fn test_build_signed_event_kind_1() {
        let ch = NostrChannel::new(vec!["wss://relay.damus.io".to_string()], "a".repeat(64));
        let event = ch.build_signed_event(1, "Hello Nostr!", vec![]).unwrap();
        assert_eq!(event["kind"], 1);
        assert_eq!(event["content"], "Hello Nostr!");
        assert!(event["id"].as_str().is_some());
        assert!(event["sig"].as_str().is_some());
    }

    #[tokio::test]
    async fn test_health_check_no_relays() {
        let ch = NostrChannel::new(vec![], "a".repeat(64));
        assert!(ch.health_check().await.is_err());
    }

    #[tokio::test]
    async fn test_health_check_valid() {
        let ch = NostrChannel::new(vec!["wss://relay.damus.io".to_string()], "a".repeat(64));
        assert!(ch.health_check().await.is_ok());
    }
}
