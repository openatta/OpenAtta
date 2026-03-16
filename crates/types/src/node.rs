//! Node 与执行相关类型

use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::AttaError;

/// 资源限制
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub max_memory_bytes: u64,
    #[serde(with = "crate::serde_util::duration_as_secs")]
    pub timeout: Duration,
    pub network_allowed: bool,
    pub fs_sandbox_root: Option<PathBuf>,
}

impl ResourceLimits {
    /// Validate resource limits bounds
    pub fn validate(&self) -> Result<(), AttaError> {
        if self.max_memory_bytes == 0 {
            return Err(AttaError::Validation("max_memory_bytes must be > 0".to_string()));
        }
        if self.timeout.as_secs() == 0 {
            return Err(AttaError::Validation("timeout must be > 0".to_string()));
        }
        Ok(())
    }
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_memory_bytes: 256 * 1024 * 1024, // 256 MB
            timeout: Duration::from_secs(300),
            network_allowed: false,
            fs_sandbox_root: None,
        }
    }
}

/// 执行状态
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Running { started_at: DateTime<Utc> },
    Completed { result: serde_json::Value },
    Failed { error: String },
    Timeout,
    Killed,
}

/// 节点容量
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCapacity {
    pub total_memory: u64,
    pub available_memory: u64,
    pub running_agents: usize,
    pub max_concurrent: usize,
}

/// 节点信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub id: String,
    pub hostname: String,
    pub labels: Vec<String>,
    pub status: NodeStatus,
    pub capacity: NodeCapacity,
    pub last_heartbeat: DateTime<Utc>,
}

/// 节点状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    Online,
    Draining,
    Offline,
}
