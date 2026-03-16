//! 共享辅助函数
//!
//! SQLite 和 PostgreSQL 实现共用的纯逻辑函数。

use atta_types::TaskStatus;

/// 将 TaskStatus 序列化为数据库存储的字符串
pub(crate) fn task_status_to_db(status: &TaskStatus) -> (String, Option<String>) {
    match status {
        TaskStatus::Running => ("running".to_string(), None),
        TaskStatus::WaitingApproval => ("waiting_approval".to_string(), None),
        TaskStatus::Completed => ("completed".to_string(), None),
        TaskStatus::Failed { error } => ("failed".to_string(), Some(error.clone())),
        TaskStatus::Cancelled => ("cancelled".to_string(), None),
    }
}

/// 从数据库字符串反序列化 TaskStatus
pub(crate) fn task_status_from_db(status: &str, error: Option<String>) -> TaskStatus {
    match status {
        "running" => TaskStatus::Running,
        "waiting_approval" => TaskStatus::WaitingApproval,
        "completed" => TaskStatus::Completed,
        "failed" => TaskStatus::Failed {
            error: error.unwrap_or_default(),
        },
        "cancelled" => TaskStatus::Cancelled,
        other => TaskStatus::Failed {
            error: format!("unknown status: {other}"),
        },
    }
}

/// JSON merge: merge `patch` into `base`
///
/// 递归合并两个 JSON 值。对于 Object 类型，逐键递归合并；
/// 对于其他类型，patch 值直接覆盖 base 值。
#[cfg(feature = "postgres")]
pub(crate) fn json_merge(base: serde_json::Value, patch: serde_json::Value) -> serde_json::Value {
    match (base, patch) {
        (serde_json::Value::Object(mut base_map), serde_json::Value::Object(patch_map)) => {
            for (key, value) in patch_map {
                let existing = base_map.remove(&key).unwrap_or(serde_json::Value::Null);
                base_map.insert(key, json_merge(existing, value));
            }
            serde_json::Value::Object(base_map)
        }
        (_, patch) => patch,
    }
}
