//! Channel message handler trait
//!
//! Defines the abstraction that decouples the channel layer from the agent
//! layer. The channel crate dispatches incoming messages to an implementation
//! of [`ChannelMessageHandler`] without knowing about LLM providers or tool
//! registries.
//!
//! Session context (agent routing, flow binding, ACP takeover) is conveyed
//! via enriched metadata fields on the [`ChannelMessage`]:
//!
//! - `_session_key`: unique session identifier
//! - `_agent_id`: optional agent override for this session
//! - `_flow_id`: optional flow override for this session

use async_trait::async_trait;

use crate::traits::{Channel, ChannelMessage};

/// Handler for incoming channel messages.
///
/// Implementations receive a channel message together with a reference to the
/// originating [`Channel`] and are responsible for processing the message
/// (typically by running it through an agent pipeline and sending a response).
///
/// The dispatch pipeline enriches `msg.metadata` with session context before
/// calling this handler. Implementations can read:
/// - `msg.metadata["_session_key"]` — session key string
/// - `msg.metadata["_agent_id"]` — optional agent ID override
/// - `msg.metadata["_flow_id"]` — optional flow ID override
#[async_trait]
pub trait ChannelMessageHandler: Send + Sync + 'static {
    /// Process a single incoming channel message.
    async fn handle(&self, msg: &ChannelMessage, channel: &dyn Channel);
}
