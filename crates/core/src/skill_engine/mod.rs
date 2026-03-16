//! Skill Engine
//!
//! Loads, parses, registers, and executes SKILL.md-based skills.
//! Skills define reusable agent behaviors with tool constraints
//! and system prompt injection.

pub mod executor;
pub mod parser;
pub mod preloaded;
pub mod registry;
pub mod sync;
pub mod validator;

pub use executor::{build_skill_system_prompt, filter_tools_for_skill};
pub use parser::parse_skill_md;
pub use preloaded::register_preloaded;
pub use registry::SkillRegistry;
pub use sync::{SkillSync, SkillSyncConfig};
pub use validator::{has_critical_warnings, validate_skill, ValidationWarning};
