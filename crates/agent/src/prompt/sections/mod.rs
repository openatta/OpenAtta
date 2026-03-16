//! Default prompt sections

mod channel_media;
mod conversation;
mod datetime;
mod identity;
mod prompt_guard;
mod runtime;
mod safety;
mod skills;
mod tools;
mod workspace;

pub use channel_media::ChannelMediaSection;
pub use conversation::ConversationControlSection;
pub use datetime::DateTimeSection;
pub use identity::IdentitySection;
pub use prompt_guard::PromptGuardSection;
pub use runtime::RuntimeSection;
pub use safety::SafetySection;
pub use skills::SkillsSection;
pub use tools::ToolsSection;
pub use workspace::WorkspaceSection;
