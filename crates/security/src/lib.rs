//! AttaOS Security System
//!
//! Provides security policies, command classification, rate limiting,
//! and a guard wrapper for the ToolRegistry.

pub mod approval;
pub mod classifier;
pub mod estop;
pub mod guard;
pub mod policy;
pub mod policy_pipeline;
pub mod scrub;
pub mod tracker;

pub use approval::cli::CliApprovalBackend;
pub use approval::webhook::{WebhookApprovalBackend, WebhookApprovalConfig};
pub use approval::ws::WsApprovalBackend;
pub use approval::{ApprovalBackend, ApprovalManager, ToolApprovalRequest, ToolApprovalResponse};
pub use classifier::CommandClassifier;
pub use estop::EstopManager;
pub use guard::{ApprovalEvent, SecurityGuard};
pub use policy::{AutonomyLevel, SecurityPolicy, ToolProfile};
pub use policy_pipeline::filter_tools_by_profile;
pub use scrub::{scrub_json_value, scrub_secret_patterns};
pub use tracker::ActionTracker;
