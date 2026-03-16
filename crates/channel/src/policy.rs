//! Message policy — decision pipeline between channel transport and agent handler.
//!
//! Policies are evaluated in order. The first `Deny` short-circuits. `Buffer`
//! delegates to the debouncer. `Allow` continues to the next policy.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::RwLock;
use tracing::debug;

use crate::traits::{ChannelMessage, ChatType};

/// Result of policy evaluation
#[derive(Debug, Clone)]
pub enum PolicyDecision {
    /// Allow the message through
    Allow,
    /// Deny the message (drop silently)
    Deny { reason: String },
    /// Buffer for debounce aggregation
    Buffer { key: String },
}

/// Trait for evaluating incoming channel messages.
///
/// Implementations decide whether a message should be processed, dropped, or
/// buffered for aggregation.
#[async_trait]
pub trait MessagePolicy: Send + Sync + 'static {
    /// Evaluate a message against this policy.
    async fn evaluate(&self, msg: &ChannelMessage) -> PolicyDecision;
}

// ---------------------------------------------------------------------------
// PolicyChain — run multiple policies in sequence
// ---------------------------------------------------------------------------

/// Chains multiple policies; first `Deny` wins, first `Buffer` wins.
pub struct PolicyChain {
    policies: Vec<Arc<dyn MessagePolicy>>,
}

impl PolicyChain {
    /// Create an empty chain.
    pub fn new() -> Self {
        Self {
            policies: Vec::new(),
        }
    }

    /// Append a policy to the chain.
    #[allow(clippy::should_implement_trait)]
    pub fn add(mut self, policy: Arc<dyn MessagePolicy>) -> Self {
        self.policies.push(policy);
        self
    }

    /// Build a default chain suitable for most deployments.
    ///
    /// Includes deduplication, mention filter, and access control (empty lists).
    pub fn default_chain() -> Self {
        Self::new()
            .add(Arc::new(DeduplicationPolicy::new(Duration::from_secs(60))))
            .add(Arc::new(MentionPolicy::new()))
    }
}

impl Default for PolicyChain {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MessagePolicy for PolicyChain {
    async fn evaluate(&self, msg: &ChannelMessage) -> PolicyDecision {
        for policy in &self.policies {
            let decision = policy.evaluate(msg).await;
            match &decision {
                PolicyDecision::Allow => continue,
                PolicyDecision::Deny { .. } | PolicyDecision::Buffer { .. } => return decision,
            }
        }
        PolicyDecision::Allow
    }
}

// ---------------------------------------------------------------------------
// DeduplicationPolicy — skip messages already seen within a time window
// ---------------------------------------------------------------------------

/// Drops duplicate messages based on `(channel, message_id)` within a sliding
/// window. Automatically evicts stale entries on every check to prevent
/// unbounded memory growth.
pub struct DeduplicationPolicy {
    seen: RwLock<HashMap<String, Instant>>,
    window: Duration,
    /// Counter for periodic full eviction (every N evaluations)
    eval_count: std::sync::atomic::AtomicU64,
}

impl DeduplicationPolicy {
    /// Create with the given de-duplication window.
    pub fn new(window: Duration) -> Self {
        Self {
            seen: RwLock::new(HashMap::new()),
            window,
            eval_count: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Evict entries older than the window.
    async fn evict(&self) {
        let cutoff = Instant::now() - self.window;
        let mut guard = self.seen.write().await;
        guard.retain(|_, ts| *ts > cutoff);
    }
}

#[async_trait]
impl MessagePolicy for DeduplicationPolicy {
    async fn evaluate(&self, msg: &ChannelMessage) -> PolicyDecision {
        let key = format!("{}:{}", msg.channel, msg.id);
        let now = Instant::now();

        // Periodic eviction: every 100 evaluations or when size exceeds 1000
        let count = self.eval_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if count % 100 == 99 {
            self.evict().await;
        } else {
            let guard = self.seen.read().await;
            if guard.len() > 1000 {
                drop(guard);
                self.evict().await;
            }
        }

        let mut guard = self.seen.write().await;
        if let Some(ts) = guard.get(&key) {
            if now.duration_since(*ts) < self.window {
                debug!(key, "dedup: dropping duplicate message");
                return PolicyDecision::Deny {
                    reason: "duplicate message".to_string(),
                };
            }
        }
        guard.insert(key, now);
        PolicyDecision::Allow
    }
}

// ---------------------------------------------------------------------------
// MentionPolicy — require @mention in group chats
// ---------------------------------------------------------------------------

/// In group chats, require the bot to be mentioned. DMs always pass.
pub struct MentionPolicy {
    _priv: (),
}

impl MentionPolicy {
    pub fn new() -> Self {
        Self { _priv: () }
    }
}

impl Default for MentionPolicy {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MessagePolicy for MentionPolicy {
    async fn evaluate(&self, msg: &ChannelMessage) -> PolicyDecision {
        match msg.chat_type {
            ChatType::Dm | ChatType::Unknown => PolicyDecision::Allow,
            ChatType::Group | ChatType::SuperGroup | ChatType::Channel => {
                if msg.bot_mentioned {
                    PolicyDecision::Allow
                } else {
                    debug!(
                        channel = msg.channel,
                        sender = msg.sender,
                        "mention: group message without bot mention, dropping"
                    );
                    PolicyDecision::Deny {
                        reason: "group message without bot mention".to_string(),
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// AccessControlPolicy — sender allowlist / blocklist
// ---------------------------------------------------------------------------

/// Controls which senders are permitted to interact per channel.
///
/// If the allowlist is non-empty, only listed senders may send.
/// The blocklist always takes precedence.
pub struct AccessControlPolicy {
    /// Per-channel allowlist: channel_name → set of sender IDs.
    /// Empty means "allow all" for that channel.
    allowlist: RwLock<HashMap<String, Vec<String>>>,
    /// Per-channel blocklist: channel_name → set of sender IDs.
    blocklist: RwLock<HashMap<String, Vec<String>>>,
}

impl AccessControlPolicy {
    pub fn new() -> Self {
        Self {
            allowlist: RwLock::new(HashMap::new()),
            blocklist: RwLock::new(HashMap::new()),
        }
    }

    /// Set the allowlist for a channel.
    pub async fn set_allowlist(&self, channel: &str, senders: Vec<String>) {
        self.allowlist
            .write()
            .await
            .insert(channel.to_string(), senders);
    }

    /// Set the blocklist for a channel.
    pub async fn set_blocklist(&self, channel: &str, senders: Vec<String>) {
        self.blocklist
            .write()
            .await
            .insert(channel.to_string(), senders);
    }

    /// Add a sender to the allowlist for a channel.
    pub async fn allow_sender(&self, channel: &str, sender: &str) {
        self.allowlist
            .write()
            .await
            .entry(channel.to_string())
            .or_default()
            .push(sender.to_string());
    }

    /// Add a sender to the blocklist for a channel.
    pub async fn block_sender(&self, channel: &str, sender: &str) {
        self.blocklist
            .write()
            .await
            .entry(channel.to_string())
            .or_default()
            .push(sender.to_string());
    }
}

impl Default for AccessControlPolicy {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MessagePolicy for AccessControlPolicy {
    async fn evaluate(&self, msg: &ChannelMessage) -> PolicyDecision {
        // Blocklist takes precedence
        let blocklist = self.blocklist.read().await;
        if let Some(blocked) = blocklist.get(&msg.channel) {
            if blocked.contains(&msg.sender) {
                debug!(
                    channel = msg.channel,
                    sender = msg.sender,
                    "access: sender is blocklisted"
                );
                return PolicyDecision::Deny {
                    reason: "sender is blocklisted".to_string(),
                };
            }
        }
        drop(blocklist);

        // Allowlist (if non-empty, only listed senders pass)
        let allowlist = self.allowlist.read().await;
        if let Some(allowed) = allowlist.get(&msg.channel) {
            if !allowed.is_empty() && !allowed.contains(&msg.sender) {
                debug!(
                    channel = msg.channel,
                    sender = msg.sender,
                    "access: sender not in allowlist"
                );
                return PolicyDecision::Deny {
                    reason: "sender not in allowlist".to_string(),
                };
            }
        }

        PolicyDecision::Allow
    }
}

// ---------------------------------------------------------------------------
// SendPolicy — per-session allow/deny override
// ---------------------------------------------------------------------------

/// Runtime send policy state — can be toggled via API or chat commands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SendPolicy {
    /// Auto-reply enabled
    #[default]
    Allow,
    /// Auto-reply disabled for this session
    Deny,
}

/// Policy that checks per-session send policy from the session router.
///
/// Session keys must match the format produced by [`SessionRouter::session_key()`].
/// Use `SessionRouter::session_key()` to generate keys when calling `set()`/`get()`.
pub struct SendPolicyFilter {
    /// session_key → SendPolicy
    overrides: RwLock<HashMap<String, SendPolicy>>,
}

impl SendPolicyFilter {
    pub fn new() -> Self {
        Self {
            overrides: RwLock::new(HashMap::new()),
        }
    }

    /// Set send policy for a session.
    ///
    /// The `session_key` must match the format from `SessionRouter::session_key()`,
    /// e.g. `"channel:dm:sender"` or `"channel:group:group_id"`.
    pub async fn set(&self, session_key: &str, policy: SendPolicy) {
        self.overrides
            .write()
            .await
            .insert(session_key.to_string(), policy);
    }

    /// Get the send policy for a session.
    pub async fn get(&self, session_key: &str) -> SendPolicy {
        self.overrides
            .read()
            .await
            .get(session_key)
            .copied()
            .unwrap_or_default()
    }

    /// Remove override for a session (revert to default Allow).
    pub async fn remove(&self, session_key: &str) {
        self.overrides.write().await.remove(session_key);
    }

    /// Build a session key from a message.
    ///
    /// Uses the same format as [`SessionRouter`] with `PerPeer` scope:
    /// - DM: `"channel:dm:sender"`
    /// - Group: `"channel:group:group_id"`
    pub fn session_key(msg: &ChannelMessage) -> String {
        match &msg.group_id {
            Some(gid) => format!("{}:group:{}", msg.channel, gid),
            None => format!("{}:dm:{}", msg.channel, msg.sender),
        }
    }
}

impl Default for SendPolicyFilter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MessagePolicy for SendPolicyFilter {
    async fn evaluate(&self, msg: &ChannelMessage) -> PolicyDecision {
        let key = Self::session_key(msg);
        match self.get(&key).await {
            SendPolicy::Allow => PolicyDecision::Allow,
            SendPolicy::Deny => PolicyDecision::Deny {
                reason: format!("send policy denied for session {key}"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::ChannelMessage;

    fn test_msg(channel: &str, sender: &str, id: &str) -> ChannelMessage {
        ChannelMessage {
            id: id.to_string(),
            sender: sender.to_string(),
            content: "hello".to_string(),
            channel: channel.to_string(),
            reply_target: None,
            timestamp: chrono::Utc::now(),
            thread_ts: None,
            metadata: serde_json::json!({}),
            chat_type: ChatType::Dm,
            bot_mentioned: false,
            group_id: None,
        }
    }

    fn group_msg(channel: &str, sender: &str, mentioned: bool) -> ChannelMessage {
        ChannelMessage {
            id: "1".to_string(),
            sender: sender.to_string(),
            content: "hello".to_string(),
            channel: channel.to_string(),
            reply_target: None,
            timestamp: chrono::Utc::now(),
            thread_ts: None,
            metadata: serde_json::json!({}),
            chat_type: ChatType::Group,
            bot_mentioned: mentioned,
            group_id: Some("group1".to_string()),
        }
    }

    #[tokio::test]
    async fn test_dedup_allows_first_drops_second() {
        let policy = DeduplicationPolicy::new(Duration::from_secs(60));
        let msg = test_msg("tg", "user1", "msg1");

        assert!(matches!(
            policy.evaluate(&msg).await,
            PolicyDecision::Allow
        ));
        assert!(matches!(
            policy.evaluate(&msg).await,
            PolicyDecision::Deny { .. }
        ));
    }

    #[tokio::test]
    async fn test_dedup_different_ids_allowed() {
        let policy = DeduplicationPolicy::new(Duration::from_secs(60));
        let msg1 = test_msg("tg", "user1", "msg1");
        let msg2 = test_msg("tg", "user1", "msg2");

        assert!(matches!(
            policy.evaluate(&msg1).await,
            PolicyDecision::Allow
        ));
        assert!(matches!(
            policy.evaluate(&msg2).await,
            PolicyDecision::Allow
        ));
    }

    #[tokio::test]
    async fn test_mention_dm_always_passes() {
        let policy = MentionPolicy::new();
        let msg = test_msg("tg", "user1", "1");
        assert!(matches!(
            policy.evaluate(&msg).await,
            PolicyDecision::Allow
        ));
    }

    #[tokio::test]
    async fn test_mention_group_requires_mention() {
        let policy = MentionPolicy::new();

        let no_mention = group_msg("tg", "user1", false);
        assert!(matches!(
            policy.evaluate(&no_mention).await,
            PolicyDecision::Deny { .. }
        ));

        let mentioned = group_msg("tg", "user1", true);
        assert!(matches!(
            policy.evaluate(&mentioned).await,
            PolicyDecision::Allow
        ));
    }

    #[tokio::test]
    async fn test_access_control_blocklist() {
        let policy = AccessControlPolicy::new();
        policy.block_sender("tg", "spammer").await;

        let blocked = test_msg("tg", "spammer", "1");
        assert!(matches!(
            policy.evaluate(&blocked).await,
            PolicyDecision::Deny { .. }
        ));

        let allowed = test_msg("tg", "friend", "2");
        assert!(matches!(
            policy.evaluate(&allowed).await,
            PolicyDecision::Allow
        ));
    }

    #[tokio::test]
    async fn test_access_control_allowlist() {
        let policy = AccessControlPolicy::new();
        policy
            .set_allowlist("tg", vec!["vip".to_string()])
            .await;

        let allowed = test_msg("tg", "vip", "1");
        assert!(matches!(
            policy.evaluate(&allowed).await,
            PolicyDecision::Allow
        ));

        let denied = test_msg("tg", "random", "2");
        assert!(matches!(
            policy.evaluate(&denied).await,
            PolicyDecision::Deny { .. }
        ));
    }

    #[tokio::test]
    async fn test_send_policy_filter() {
        let policy = SendPolicyFilter::new();
        let msg = test_msg("tg", "user1", "1");
        let key = SendPolicyFilter::session_key(&msg);

        // Default: allow
        assert!(matches!(
            policy.evaluate(&msg).await,
            PolicyDecision::Allow
        ));

        // Set deny
        policy.set(&key, SendPolicy::Deny).await;
        assert!(matches!(
            policy.evaluate(&msg).await,
            PolicyDecision::Deny { .. }
        ));

        // Revert
        policy.remove(&key).await;
        assert!(matches!(
            policy.evaluate(&msg).await,
            PolicyDecision::Allow
        ));
    }

    #[tokio::test]
    async fn test_policy_chain() {
        let chain = PolicyChain::new()
            .add(Arc::new(DeduplicationPolicy::new(Duration::from_secs(60))))
            .add(Arc::new(MentionPolicy::new()));

        // DM message — passes both
        let dm = test_msg("tg", "user1", "1");
        assert!(matches!(chain.evaluate(&dm).await, PolicyDecision::Allow));

        // Group without mention — denied by MentionPolicy
        let group = group_msg("tg", "user1", false);
        assert!(matches!(
            chain.evaluate(&group).await,
            PolicyDecision::Deny { .. }
        ));

        // Duplicate DM — denied by DeduplicationPolicy
        assert!(matches!(
            chain.evaluate(&dm).await,
            PolicyDecision::Deny { .. }
        ));
    }

    #[tokio::test]
    async fn test_blocklist_overrides_allowlist() {
        let policy = AccessControlPolicy::new();
        policy
            .set_allowlist("tg", vec!["user1".to_string()])
            .await;
        policy.block_sender("tg", "user1").await;

        let msg = test_msg("tg", "user1", "1");
        assert!(matches!(
            policy.evaluate(&msg).await,
            PolicyDecision::Deny { .. }
        ));
    }
}
