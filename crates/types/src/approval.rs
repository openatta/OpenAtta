//! 审批相关类型

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{Actor, Role};
use crate::tool::ToolDef;

/// 审批请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: Uuid,
    pub task_id: Uuid,
    pub requested_by: Actor,
    pub approver_role: Role,
    pub context: ApprovalContext,
    pub status: ApprovalStatus,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub resolved_by: Option<Actor>,
    #[serde(with = "crate::serde_util::duration_as_secs")]
    pub timeout: Duration,
    pub timeout_at: DateTime<Utc>,
}

/// 审批上下文
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalContext {
    pub summary: String,
    pub diff_summary: Option<String>,
    pub test_results: Option<String>,
    pub risk_assessment: String,
    pub pending_tools: Vec<ToolInfo>,
}

/// 待审批的 Tool 信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub risk_level: String,
}

impl From<&ToolDef> for ToolInfo {
    fn from(t: &ToolDef) -> Self {
        Self {
            name: t.name.clone(),
            description: t.description.clone(),
            risk_level: format!("{:?}", t.risk_level),
        }
    }
}

/// 审批状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Denied,
    RequestChanges,
    Expired,
}

/// 审批列表查询过滤
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApprovalFilter {
    pub status: Option<ApprovalStatus>,
    pub approver_role: Option<String>,
    pub task_id: Option<Uuid>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
}

fn default_limit() -> usize {
    20
}
