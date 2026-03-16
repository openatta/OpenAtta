//! Preloaded skills — previously compiled into the binary via include_str!
//!
//! Skills are now loaded from `$ATTA_HOME/lib/skills/` at runtime.
//! This module is kept for backward compatibility but is a no-op.

use tracing::info;

use super::registry::SkillRegistry;

/// Register preloaded skills (no-op — skills are loaded from filesystem)
pub fn register_preloaded(_registry: &SkillRegistry) {
    info!("preloaded skills registration skipped — skills loaded from filesystem");
}
