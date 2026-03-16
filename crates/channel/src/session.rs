//! Session routing — maps incoming messages to session contexts.
//!
//! A session key uniquely identifies a conversation scope (e.g., a DM with a
//! specific user, or a group thread). Each session can be bound to a specific
//! agent, flow, or human operator (ACP takeover).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::debug;

use crate::policy::SendPolicy;
use crate::traits::{Channel, ChannelMessage, SendMessage};

/// Scope for DM session isolation
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DmScope {
    /// All DMs collapse into one session per channel
    Main,
    /// One session per peer (default)
    #[default]
    PerPeer,
    /// One session per (channel, peer)
    PerChannelPeer,
}

/// Human takeover (ACP) state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TakeoverState {
    /// Operator who took over
    pub operator_id: String,
    /// When the takeover started
    pub started_at: DateTime<Utc>,
    /// Optional reason
    pub reason: Option<String>,
}

/// Configuration for a specific session
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionConfig {
    /// Optional agent ID to route messages to
    pub agent_id: Option<String>,
    /// Optional flow ID to route messages to
    pub flow_id: Option<String>,
    /// Send policy override
    #[serde(default)]
    pub send_policy: SendPolicyState,
    /// Human takeover state (ACP)
    pub takeover: Option<TakeoverState>,
}

/// Serializable send policy state
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SendPolicyState {
    #[default]
    Allow,
    Deny,
}

impl From<SendPolicyState> for SendPolicy {
    fn from(s: SendPolicyState) -> Self {
        match s {
            SendPolicyState::Allow => SendPolicy::Allow,
            SendPolicyState::Deny => SendPolicy::Deny,
        }
    }
}

/// Context attached to a message after session resolution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionContext {
    /// Unique session key
    pub key: String,
    /// Session configuration
    pub config: SessionConfig,
}

impl SessionContext {
    /// Whether this session is in human takeover mode
    pub fn is_takeover(&self) -> bool {
        self.config.takeover.is_some()
    }
}

/// Routes incoming messages to session contexts.
///
/// Maintains a registry of session configurations and builds session keys
/// based on message metadata (channel, sender, group).
pub struct SessionRouter {
    /// DM scope strategy
    dm_scope: DmScope,
    /// Per-session configuration overrides
    sessions: RwLock<HashMap<String, SessionConfig>>,
    /// Default config for new sessions
    default_config: SessionConfig,
}

impl SessionRouter {
    /// Create a new router with the given DM scope and default config.
    pub fn new(dm_scope: DmScope, default_config: SessionConfig) -> Self {
        Self {
            dm_scope,
            sessions: RwLock::new(HashMap::new()),
            default_config,
        }
    }

    /// Build a session key from a message.
    pub fn session_key(&self, msg: &ChannelMessage) -> String {
        match &msg.group_id {
            Some(gid) => {
                // Group messages: always per-group
                format!("{}:group:{}", msg.channel, gid)
            }
            None => {
                // DM messages: depends on scope
                match self.dm_scope {
                    DmScope::Main => format!("{}:dm:main", msg.channel),
                    DmScope::PerPeer => format!("{}:dm:{}", msg.channel, msg.sender),
                    DmScope::PerChannelPeer => {
                        format!("{}:dm:{}:{}", msg.channel, msg.channel, msg.sender)
                    }
                }
            }
        }
    }

    /// Resolve the session context for a message.
    pub async fn resolve(&self, msg: &ChannelMessage) -> SessionContext {
        let key = self.session_key(msg);
        let sessions = self.sessions.read().await;
        let config = sessions
            .get(&key)
            .cloned()
            .unwrap_or_else(|| self.default_config.clone());

        debug!(session_key = %key, "session resolved");
        SessionContext { key, config }
    }

    /// Set session configuration.
    pub async fn set_config(&self, key: &str, config: SessionConfig) {
        self.sessions
            .write()
            .await
            .insert(key.to_string(), config);
    }

    /// Get session configuration.
    pub async fn get_config(&self, key: &str) -> Option<SessionConfig> {
        self.sessions.read().await.get(key).cloned()
    }

    /// Remove session configuration (revert to defaults).
    pub async fn remove_config(&self, key: &str) {
        self.sessions.write().await.remove(key);
    }

    /// List all active session keys.
    pub async fn list_sessions(&self) -> Vec<String> {
        self.sessions.read().await.keys().cloned().collect()
    }

    /// Start a human takeover for a session.
    pub async fn start_takeover(
        &self,
        key: &str,
        operator_id: &str,
        reason: Option<String>,
    ) -> SessionConfig {
        let mut sessions = self.sessions.write().await;
        let config = sessions
            .entry(key.to_string())
            .or_insert_with(|| self.default_config.clone());
        config.takeover = Some(TakeoverState {
            operator_id: operator_id.to_string(),
            started_at: Utc::now(),
            reason,
        });
        debug!(session_key = %key, operator = %operator_id, "takeover started");
        config.clone()
    }

    /// End a human takeover for a session.
    pub async fn end_takeover(&self, key: &str) -> Option<SessionConfig> {
        let mut sessions = self.sessions.write().await;
        if let Some(config) = sessions.get_mut(key) {
            config.takeover = None;
            debug!(session_key = %key, "takeover ended");
            Some(config.clone())
        } else {
            None
        }
    }

    /// Forward a message from a human operator to the channel.
    ///
    /// Used during ACP takeover — the operator's reply is sent through
    /// the channel to the original user.
    pub async fn forward_takeover_reply(
        &self,
        session_key: &str,
        content: &str,
        channel: &dyn Channel,
        original_msg: &ChannelMessage,
    ) -> Result<(), atta_types::AttaError> {
        let sessions = self.sessions.read().await;
        let has_takeover = sessions
            .get(session_key)
            .and_then(|c| c.takeover.as_ref())
            .is_some();
        if !has_takeover {
            return Err(atta_types::AttaError::Validation(
                "session is not in takeover mode".to_string(),
            ));
        }

        let reply = SendMessage {
            recipient: original_msg.sender.clone(),
            content: content.to_string(),
            subject: None,
            thread_ts: original_msg.thread_ts.clone(),
            metadata: serde_json::json!({}),
        };

        channel.send(reply).await
    }
}

impl Default for SessionRouter {
    fn default() -> Self {
        Self::new(DmScope::PerPeer, SessionConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::ChatType;

    fn dm_msg(channel: &str, sender: &str) -> ChannelMessage {
        ChannelMessage {
            id: "1".to_string(),
            sender: sender.to_string(),
            content: "hi".to_string(),
            channel: channel.to_string(),
            reply_target: None,
            timestamp: Utc::now(),
            thread_ts: None,
            metadata: serde_json::json!({}),
            chat_type: ChatType::Dm,
            bot_mentioned: false,
            group_id: None,
        }
    }

    fn group_msg(channel: &str, sender: &str, group: &str) -> ChannelMessage {
        ChannelMessage {
            id: "1".to_string(),
            sender: sender.to_string(),
            content: "hi".to_string(),
            channel: channel.to_string(),
            reply_target: None,
            timestamp: Utc::now(),
            thread_ts: None,
            metadata: serde_json::json!({}),
            chat_type: ChatType::Group,
            bot_mentioned: true,
            group_id: Some(group.to_string()),
        }
    }

    #[test]
    fn test_session_key_dm_per_peer() {
        let router = SessionRouter::new(DmScope::PerPeer, SessionConfig::default());
        let msg = dm_msg("telegram", "user123");
        assert_eq!(router.session_key(&msg), "telegram:dm:user123");
    }

    #[test]
    fn test_session_key_dm_main() {
        let router = SessionRouter::new(DmScope::Main, SessionConfig::default());
        let msg = dm_msg("telegram", "user123");
        assert_eq!(router.session_key(&msg), "telegram:dm:main");
    }

    #[test]
    fn test_session_key_group() {
        let router = SessionRouter::default();
        let msg = group_msg("telegram", "user1", "group42");
        assert_eq!(router.session_key(&msg), "telegram:group:group42");
    }

    #[tokio::test]
    async fn test_resolve_default_config() {
        let router = SessionRouter::default();
        let msg = dm_msg("tg", "user1");
        let ctx = router.resolve(&msg).await;
        assert_eq!(ctx.key, "tg:dm:user1");
        assert!(ctx.config.agent_id.is_none());
    }

    #[tokio::test]
    async fn test_set_and_resolve_config() {
        let router = SessionRouter::default();
        let msg = dm_msg("tg", "user1");
        let key = router.session_key(&msg);

        router
            .set_config(
                &key,
                SessionConfig {
                    agent_id: Some("agent-42".to_string()),
                    ..Default::default()
                },
            )
            .await;

        let ctx = router.resolve(&msg).await;
        assert_eq!(ctx.config.agent_id.as_deref(), Some("agent-42"));
    }

    #[tokio::test]
    async fn test_takeover_lifecycle() {
        let router = SessionRouter::default();
        let key = "tg:dm:user1";

        // Start takeover
        let config = router
            .start_takeover(key, "ops-alice", Some("customer escalation".to_string()))
            .await;
        assert!(config.takeover.is_some());
        assert_eq!(config.takeover.as_ref().unwrap().operator_id, "ops-alice");

        // Verify active
        let msg = dm_msg("tg", "user1");
        let ctx = router.resolve(&msg).await;
        assert!(ctx.is_takeover());

        // End takeover
        let config = router.end_takeover(key).await;
        assert!(config.is_some());
        assert!(config.unwrap().takeover.is_none());

        // Verify cleared
        let ctx = router.resolve(&msg).await;
        assert!(!ctx.is_takeover());
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let router = SessionRouter::default();
        router
            .set_config("a", SessionConfig::default())
            .await;
        router
            .set_config("b", SessionConfig::default())
            .await;

        let mut keys = router.list_sessions().await;
        keys.sort();
        assert_eq!(keys, vec!["a", "b"]);
    }

    #[tokio::test]
    async fn test_remove_config() {
        let router = SessionRouter::default();
        router
            .set_config(
                "key",
                SessionConfig {
                    agent_id: Some("x".to_string()),
                    ..Default::default()
                },
            )
            .await;
        assert!(router.get_config("key").await.is_some());

        router.remove_config("key").await;
        assert!(router.get_config("key").await.is_none());
    }
}
