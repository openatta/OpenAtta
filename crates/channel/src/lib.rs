//! Channel system for AttaOS
//!
//! Provides bi-directional communication channels (terminal, webhook, etc.)
//! that bridge external messaging platforms with the Agent execution engine.
//!
//! ## Architecture
//!
//! ```text
//! Channel.listen() → Supervisor → Dispatch Loop → Policy → Debounce → Session → Handler
//!                                                                                  ↓
//!                                                                           Agent / ACP
//! ```

pub mod debounce;
pub mod dispatch;
pub mod draft;
pub mod factory;
pub mod handler;
pub mod heartbeat;
pub mod impls;
pub mod persistence;
pub mod policy;
pub mod registry;
pub mod runtime;
pub mod session;
pub mod supervisor;
pub mod traits;

pub use debounce::{debounce_key, SimpleDebouncer};
pub use dispatch::process_channel_message;
pub use draft::{utf8_truncate, DraftManager};
pub use factory::{create_channel, ChannelConfig};
pub use handler::ChannelMessageHandler;
pub use heartbeat::HeartbeatMonitor;
pub use persistence::{InMemoryMessageStore, MessageStore};
pub use policy::{
    AccessControlPolicy, DeduplicationPolicy, MentionPolicy, MessagePolicy, PolicyChain,
    PolicyDecision, SendPolicyFilter,
};
pub use registry::ChannelRegistry;
pub use runtime::{start_channels, ChannelRuntimeContext};
pub use session::{SessionConfig, SessionContext, SessionRouter};
pub use traits::{Channel, ChannelMessage, ChatType, SendMessage};
