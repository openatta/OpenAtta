//! E-Stop types

use serde::{Deserialize, Serialize};

/// Emergency stop severity levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EstopLevel {
    /// Stop all agent execution
    KillAll,
    /// Block all network-accessing tools
    NetworkKill,
    /// Block specific domains
    DomainBlock(Vec<String>),
    /// Freeze specific tools
    ToolFreeze(Vec<String>),
}

/// Persisted E-Stop state
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EstopState {
    /// All execution halted
    pub kill_all: bool,
    /// Network access blocked
    pub network_kill: bool,
    /// Blocked domain list
    pub blocked_domains: Vec<String>,
    /// Frozen tool names
    pub frozen_tools: Vec<String>,
    /// When the E-Stop was activated
    pub activated_at: Option<String>,
    /// OTP required to resume after KillAll
    pub resume_otp: Option<String>,
}
