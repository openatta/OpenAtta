//! Channel trait and message types

use std::collections::HashMap;

use atta_types::AttaError;
use serde::{Deserialize, Serialize};

/// Type of chat (DM, group, etc.)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatType {
    /// Direct / private message
    Dm,
    /// Group chat
    Group,
    /// Supergroup (Telegram-specific)
    SuperGroup,
    /// Channel broadcast (Telegram channels, etc.)
    Channel,
    /// Unknown / not determined
    #[default]
    Unknown,
}

/// Incoming message from a channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMessage {
    /// Unique message ID
    pub id: String,
    /// Sender identifier (user ID, phone number, etc.)
    pub sender: String,
    /// Message content
    pub content: String,
    /// Channel name this message came from
    pub channel: String,
    /// Reply target message ID (for threaded conversations)
    pub reply_target: Option<String>,
    /// Message timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Thread identifier (Slack ts, Discord thread ID, etc.)
    pub thread_ts: Option<String>,
    /// Optional metadata (platform-specific data)
    pub metadata: serde_json::Value,
    /// Type of chat (DM, group, etc.)
    #[serde(default)]
    pub chat_type: ChatType,
    /// Whether the bot was mentioned in this message
    #[serde(default)]
    pub bot_mentioned: bool,
    /// Group/chat ID (for group-scoped sessions)
    #[serde(default)]
    pub group_id: Option<String>,
}

impl ChannelMessage {
    /// Create a new ChannelMessage with the required fields and defaults for new fields.
    ///
    /// This is a convenience constructor that sets `chat_type` to `Unknown`,
    /// `bot_mentioned` to `false`, and `group_id` to `None`.
    pub fn new(
        id: impl Into<String>,
        sender: impl Into<String>,
        content: impl Into<String>,
        channel: impl Into<String>,
        timestamp: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        Self {
            id: id.into(),
            sender: sender.into(),
            content: content.into(),
            channel: channel.into(),
            reply_target: None,
            timestamp,
            thread_ts: None,
            metadata: serde_json::json!({}),
            chat_type: ChatType::default(),
            bot_mentioned: false,
            group_id: None,
        }
    }
}

/// Outgoing message to a channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessage {
    /// Recipient identifier
    pub recipient: String,
    /// Message content
    pub content: String,
    /// Optional subject line (for email, etc.)
    pub subject: Option<String>,
    /// Thread identifier for threaded replies
    pub thread_ts: Option<String>,
    /// Optional metadata
    pub metadata: serde_json::Value,
}

/// Channel trait — bi-directional communication with external platforms.
///
/// Uses a **push model**: `listen()` receives a `Sender` and pushes incoming
/// messages through it. The method blocks until the connection is closed or
/// an error occurs. Returning `Ok(())` signals a normal disconnect; the
/// supervisor will automatically reconnect.
#[async_trait::async_trait]
pub trait Channel: Send + Sync + 'static {
    /// Channel name (e.g., "terminal", "webhook", "slack")
    fn name(&self) -> &str;

    /// Send a message through the channel
    async fn send(&self, message: SendMessage) -> Result<(), AttaError>;

    /// Start listening for incoming messages (push model).
    ///
    /// Pushes incoming messages to `tx`. Blocks until the connection ends.
    /// Returns `Ok(())` on normal disconnect, `Err` on failure.
    /// The supervisor wraps this with exponential backoff retry.
    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<(), AttaError>;

    /// Health check — returns Ok if the channel is operational
    async fn health_check(&self) -> Result<(), AttaError>;

    /// Signal typing indicator start (no-op by default)
    async fn start_typing(&self, _recipient: &str) -> Result<(), AttaError> {
        Ok(())
    }

    /// Signal typing indicator stop (no-op by default)
    async fn stop_typing(&self, _recipient: &str) -> Result<(), AttaError> {
        Ok(())
    }

    /// Whether this channel supports draft-based message updates
    fn supports_draft_updates(&self) -> bool {
        false
    }

    /// Send an initial draft message (returns draft ID)
    async fn send_draft(&self, message: SendMessage) -> Result<String, AttaError> {
        self.send(message).await?;
        Ok(String::new())
    }

    /// Update a previously sent draft
    async fn update_draft(&self, _draft_id: &str, _content: &str) -> Result<(), AttaError> {
        Ok(())
    }

    /// Finalize a draft (mark as complete)
    async fn finalize_draft(&self, _draft_id: &str) -> Result<(), AttaError> {
        Ok(())
    }

    /// Cancel a draft message
    async fn cancel_draft(&self, _draft_id: &str) -> Result<(), AttaError> {
        Ok(())
    }

    /// Send an approval prompt with interactive buttons
    async fn send_approval_prompt(
        &self,
        _recipient: &str,
        _request_id: &str,
        _tool_name: &str,
        _arguments: &serde_json::Value,
        _thread_ts: Option<String>,
    ) -> Result<(), AttaError> {
        Ok(())
    }

    /// Add a reaction/emoji to a message
    async fn add_reaction(&self, _message_id: &str, _reaction: &str) -> Result<(), AttaError> {
        Ok(())
    }

    /// Remove a reaction/emoji from a message
    async fn remove_reaction(&self, _message_id: &str, _reaction: &str) -> Result<(), AttaError> {
        Ok(())
    }

    /// Verify a webhook signature for inbound messages.
    ///
    /// Called by the HTTP handler before processing webhook payloads.
    /// Returns `Ok(true)` if the signature is valid, `Ok(false)` if invalid.
    /// Default implementation accepts all payloads (no verification).
    fn verify_webhook_signature(
        &self,
        _headers: &HashMap<String, String>,
        _body: &[u8],
    ) -> Result<bool, AttaError> {
        Ok(true)
    }

    /// The bot's username on this platform (for mention detection).
    ///
    /// If set, the dispatch pipeline can automatically detect mentions
    /// and set `bot_mentioned` on incoming messages.
    fn bot_username(&self) -> Option<&str> {
        None
    }
}
