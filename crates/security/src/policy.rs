//! Security policy definitions

use serde::{Deserialize, Serialize};

/// Tool profile — restricts which categories of tools are available
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolProfile {
    /// Minimal: only read operations
    Minimal,
    /// Coding: file + shell + search tools
    Coding,
    /// Messaging: communication tools only
    Messaging,
    /// Research: web + search + memory tools
    Research,
    /// Full: all tools (default)
    Full,
    /// Custom: explicit tool name list
    Custom(Vec<String>),
}

impl Default for ToolProfile {
    fn default() -> Self {
        Self::Full
    }
}

/// Agent autonomy level
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyLevel {
    /// Read-only: no write/execute operations allowed
    ReadOnly,
    /// Supervised (default): high-risk operations require approval
    Supervised,
    /// Full autonomy: all operations allowed (with audit trail)
    Full,
}

impl Default for AutonomyLevel {
    fn default() -> Self {
        Self::Supervised
    }
}

/// Security policy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityPolicy {
    /// Agent autonomy level
    #[serde(default)]
    pub autonomy_level: AutonomyLevel,
    /// Shell command allowlist (glob patterns)
    #[serde(default)]
    pub command_allowlist: Vec<String>,
    /// Forbidden file system paths
    #[serde(default)]
    pub forbidden_paths: Vec<String>,
    /// Maximum tool calls per minute
    #[serde(default = "default_max_calls")]
    pub max_calls_per_minute: u32,
    /// Maximum high-risk calls per minute
    #[serde(default = "default_max_high_risk")]
    pub max_high_risk_per_minute: u32,
    /// Whether network access is allowed
    #[serde(default = "default_true")]
    pub allow_network: bool,
    /// Maximum file write size in bytes
    #[serde(default = "default_max_write_size")]
    pub max_write_size: u64,
    /// Workspace root — file operations are restricted to this directory tree
    #[serde(default)]
    pub workspace_root: Option<String>,
    /// Additional allowed paths beyond workspace_root (e.g., /tmp)
    #[serde(default)]
    pub allowed_roots: Vec<String>,
    /// URL domain allowlist — empty means all allowed
    #[serde(default)]
    pub url_allowlist: Vec<String>,
    /// URL domain blocklist — takes priority over allowlist
    #[serde(default)]
    pub url_blocklist: Vec<String>,
    /// Tool profile restricting which tools are exposed
    #[serde(default)]
    pub tool_profile: ToolProfile,
}

fn default_max_calls() -> u32 {
    60
}

fn default_max_high_risk() -> u32 {
    10
}

fn default_true() -> bool {
    true
}

fn default_max_write_size() -> u64 {
    10 * 1024 * 1024 // 10MB
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        Self {
            autonomy_level: AutonomyLevel::default(),
            command_allowlist: Vec::new(),
            forbidden_paths: vec![
                "/etc/shadow".to_string(),
                "/etc/passwd".to_string(),
                "~/.ssh/id_*".to_string(),
            ],
            max_calls_per_minute: default_max_calls(),
            max_high_risk_per_minute: default_max_high_risk(),
            allow_network: true,
            max_write_size: default_max_write_size(),
            workspace_root: None,
            allowed_roots: Vec::new(),
            url_allowlist: Vec::new(),
            url_blocklist: Vec::new(),
            tool_profile: ToolProfile::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_policy() {
        let policy = SecurityPolicy::default();
        assert_eq!(policy.autonomy_level, AutonomyLevel::Supervised);
        assert_eq!(policy.max_calls_per_minute, 60);
        assert!(policy.allow_network);
    }

    #[test]
    fn test_autonomy_level_serde() {
        let json = serde_json::to_string(&AutonomyLevel::ReadOnly).unwrap();
        assert_eq!(json, "\"read_only\"");

        let level: AutonomyLevel = serde_json::from_str("\"full\"").unwrap();
        assert_eq!(level, AutonomyLevel::Full);
    }
}
