//! Modular system prompt builder
//!
//! Provides a composable prompt system with 10 default sections:
//! PromptGuard, Identity, Safety, Tools, Skills, Workspace, Runtime,
//! DateTime, ChannelMedia, ConversationControl.

pub mod builder;
pub mod guard;
pub mod section;
pub mod sections;

pub use builder::SystemPromptBuilder;
pub use guard::{GuardAction, GuardCategory, PromptGuard};
pub use section::{PromptContext, PromptSection, SkillsPromptMode};
