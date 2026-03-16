//! Re-export and convenience wrappers for mock registries
//!
//! Most mock registries live in mock_tools.rs. This module provides
//! additional convenience constructors.

use super::mock_tools::{echo_tool_def, failing_tool_def, high_risk_tool_def};
pub use super::mock_tools::{CountingRegistry, SimpleRegistry};

/// Create a registry with a single echo tool
pub fn echo_registry() -> CountingRegistry {
    CountingRegistry::new(vec![echo_tool_def()])
}

/// Create a registry with echo + failing tools
pub fn mixed_registry() -> CountingRegistry {
    CountingRegistry::new(vec![echo_tool_def(), failing_tool_def()]).with_failing("failing_tool")
}

/// Create a registry with echo + high-risk tool
pub fn with_high_risk_registry() -> CountingRegistry {
    CountingRegistry::new(vec![echo_tool_def(), high_risk_tool_def()])
}
