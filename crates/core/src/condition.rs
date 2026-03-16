//! 条件求值器
//!
//! [`ConditionEvaluator`] 负责解析 Flow 转移条件表达式并基于 Task 运行时数据求值。
//!
//! # 条件语法
//!
//! ```text
//! condition       = simple_cond | compound_cond
//! simple_cond     = identifier              // state_data 布尔值或内置条件
//!                 | identifier "==" value    // 等值比较
//! compound_cond   = condition "AND" condition
//!                 | condition "OR" condition
//!                 | "NOT" condition
//! value           = quoted_string | "true" | "false"
//! ```
//!
//! # 求值优先级
//!
//! 1. 内置条件函数（`approved`, `denied`, `needs_changes`, `timeout`, `has_high_risk_tools`, `all_tools_done`, `all_done`）
//! 2. `state_data` 中的键值
//! 3. 未找到则返回 `false`（Flow 停留当前状态）

use chrono::Utc;

use atta_types::{ApprovalStatus, AttaError, CondExpr, FlowState, RiskLevel, Task};

use atta_types::ToolRegistry;

/// 条件求值器
///
/// 无状态结构体，负责解析条件字符串并基于 Task / FlowState / ToolRegistry 求值。
/// 由 FlowEngine 持有实例，在状态推进时调用。
pub struct ConditionEvaluator;

impl ConditionEvaluator {
    /// 创建条件求值器
    pub fn new() -> Self {
        Self
    }

    /// 求值条件表达式
    ///
    /// # Arguments
    ///
    /// * `condition` - 条件表达式字符串（例如 `"approved"`, `"has_high_risk_tools AND NOT tests_passed"`）
    /// * `task` - 当前 Task（提供 `state_data`）
    /// * `flow_state` - 当前 FlowState（提供 `pending_approval` 等）
    /// * `tool_registry` - Tool 注册表（用于 `has_high_risk_tools` 等条件）
    pub fn evaluate(
        &self,
        condition: &str,
        task: &Task,
        flow_state: &FlowState,
        tool_registry: &dyn ToolRegistry,
    ) -> Result<bool, AttaError> {
        let expr = Self::parse(condition)?;
        self.eval_expr(&expr, task, flow_state, tool_registry)
    }

    /// 解析条件字符串为 AST
    ///
    /// 按运算符优先级拆分：OR < AND < NOT < 原子（标识符、等值比较）。
    ///
    /// NOTE: String values containing " AND ", " OR ", or " NOT " will be incorrectly
    /// split by the parser. Use simple identifiers in conditions; complex string matching
    /// should be done via tool calls rather than inline conditions.
    fn parse(condition: &str) -> Result<CondExpr, AttaError> {
        let condition = condition.trim();

        if condition.is_empty() {
            return Err(AttaError::Validation(
                "empty condition expression".to_string(),
            ));
        }

        // OR 优先级最低，先尝试拆分
        if let Some((left, right)) = condition.split_once(" OR ") {
            return Ok(CondExpr::Or(
                Box::new(Self::parse(left.trim())?),
                Box::new(Self::parse(right.trim())?),
            ));
        }

        // AND 优先级次低
        if let Some((left, right)) = condition.split_once(" AND ") {
            return Ok(CondExpr::And(
                Box::new(Self::parse(left.trim())?),
                Box::new(Self::parse(right.trim())?),
            ));
        }

        // NOT 前缀
        if let Some(rest) = condition.strip_prefix("NOT ") {
            return Ok(CondExpr::Not(Box::new(Self::parse(rest.trim())?)));
        }

        // 不等比较: key != value（先于 == 检查，避免 != 被跳过）
        if let Some((key, value)) = condition.split_once(" != ") {
            return Ok(CondExpr::Ne(
                key.trim().to_string(),
                value.trim().trim_matches('"').to_string(),
            ));
        }

        // 等值比较: key == value
        if let Some((key, value)) = condition.split_once(" == ") {
            return Ok(CondExpr::Eq(
                key.trim().to_string(),
                value.trim().trim_matches('"').to_string(),
            ));
        }

        // 简单标识符
        Ok(CondExpr::Ident(condition.to_string()))
    }

    /// 递归求值 AST 节点
    fn eval_expr(
        &self,
        expr: &CondExpr,
        task: &Task,
        flow_state: &FlowState,
        tool_registry: &dyn ToolRegistry,
    ) -> Result<bool, AttaError> {
        match expr {
            CondExpr::Ident(name) => self.eval_ident(name, task, flow_state, tool_registry),
            CondExpr::Eq(key, value) => {
                let actual = task
                    .state_data
                    .get(key.as_str())
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                Ok(actual == value)
            }
            CondExpr::Ne(key, value) => {
                let actual = task
                    .state_data
                    .get(key.as_str())
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                Ok(actual != value)
            }
            CondExpr::And(left, right) => {
                let l = self.eval_expr(left, task, flow_state, tool_registry)?;
                let r = self.eval_expr(right, task, flow_state, tool_registry)?;
                Ok(l && r)
            }
            CondExpr::Or(left, right) => {
                let l = self.eval_expr(left, task, flow_state, tool_registry)?;
                let r = self.eval_expr(right, task, flow_state, tool_registry)?;
                Ok(l || r)
            }
            CondExpr::Not(inner) => {
                let val = self.eval_expr(inner, task, flow_state, tool_registry)?;
                Ok(!val)
            }
        }
    }

    /// 求值标识符
    ///
    /// 按优先级查找：内置条件函数 → state_data 布尔值 → 默认 false。
    fn eval_ident(
        &self,
        name: &str,
        task: &Task,
        flow_state: &FlowState,
        tool_registry: &dyn ToolRegistry,
    ) -> Result<bool, AttaError> {
        // 优先级 1：内置条件函数
        match name {
            "approved" => {
                return Ok(flow_state
                    .pending_approval
                    .as_ref()
                    .map(|a| a.status == ApprovalStatus::Approved)
                    .unwrap_or(false));
            }
            "denied" => {
                return Ok(flow_state
                    .pending_approval
                    .as_ref()
                    .map(|a| a.status == ApprovalStatus::Denied)
                    .unwrap_or(false));
            }
            "needs_changes" => {
                return Ok(flow_state
                    .pending_approval
                    .as_ref()
                    .map(|a| a.status == ApprovalStatus::RequestChanges)
                    .unwrap_or(false));
            }
            "timeout" => {
                return Ok(flow_state
                    .pending_approval
                    .as_ref()
                    .map(|a| Utc::now() > a.timeout_at)
                    .unwrap_or(false));
            }
            "has_high_risk_tools" => {
                let empty = vec![];
                let tools = task
                    .state_data
                    .get("planned_tools")
                    .and_then(|v| v.as_array())
                    .unwrap_or(&empty);
                for tool_name in tools {
                    if let Some(name) = tool_name.as_str() {
                        if let Some(tool) = tool_registry.get(name) {
                            if tool.risk_level == RiskLevel::High {
                                return Ok(true);
                            }
                        }
                    }
                }
                return Ok(false);
            }
            "all_tools_done" => {
                let pending = task
                    .state_data
                    .get("pending_tools")
                    .and_then(|v| v.as_array());
                return Ok(pending.map(|a| a.is_empty()).unwrap_or(true));
            }
            "all_done" => {
                return Ok(task.state_data.get("agent_status").and_then(|v| v.as_str())
                    == Some("completed"));
            }
            _ => {}
        }

        // 优先级 2：查找 state_data 中的布尔值
        if let Some(value) = task.state_data.get(name) {
            return Ok(value.as_bool().unwrap_or(false));
        }

        // 未找到条件 → false（不报错，保持 Flow 停留当前状态）
        Ok(false)
    }
}

impl Default for ConditionEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_registry::DefaultToolRegistry;
    use atta_types::{Actor, FlowState, RiskLevel, Task, TaskStatus, ToolBinding, ToolDef};
    use chrono::Utc;
    use uuid::Uuid;

    fn make_task(state_data: serde_json::Value) -> Task {
        Task {
            id: Uuid::new_v4(),
            flow_id: "test_flow".to_string(),
            current_state: "execute".to_string(),
            state_data,
            input: serde_json::json!({}),
            output: None,
            status: TaskStatus::Running,
            created_by: Actor::system(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            completed_at: None,
            version: 0,
        }
    }

    fn make_flow_state() -> FlowState {
        FlowState::default()
    }

    fn make_registry() -> DefaultToolRegistry {
        DefaultToolRegistry::new()
    }

    fn make_high_risk_tool(name: &str) -> ToolDef {
        ToolDef {
            name: name.to_string(),
            description: format!("High risk tool: {}", name),
            binding: ToolBinding::Native {
                handler_name: name.to_string(),
            },
            risk_level: RiskLevel::High,
            parameters: serde_json::json!({}),
        }
    }

    #[test]
    fn test_simple_ident_true() {
        let evaluator = ConditionEvaluator::new();
        let task = make_task(serde_json::json!({"plan_ready": true}));
        let flow_state = make_flow_state();
        let registry = make_registry();

        let result = evaluator
            .evaluate("plan_ready", &task, &flow_state, &registry)
            .unwrap();
        assert!(result);
    }

    #[test]
    fn test_simple_ident_false() {
        let evaluator = ConditionEvaluator::new();
        let task = make_task(serde_json::json!({"plan_ready": false}));
        let flow_state = make_flow_state();
        let registry = make_registry();

        let result = evaluator
            .evaluate("plan_ready", &task, &flow_state, &registry)
            .unwrap();
        assert!(!result);
    }

    #[test]
    fn test_missing_ident_is_false() {
        let evaluator = ConditionEvaluator::new();
        let task = make_task(serde_json::json!({}));
        let flow_state = make_flow_state();
        let registry = make_registry();

        let result = evaluator
            .evaluate("nonexistent", &task, &flow_state, &registry)
            .unwrap();
        assert!(!result);
    }

    #[test]
    fn test_eq_condition() {
        let evaluator = ConditionEvaluator::new();
        let task = make_task(serde_json::json!({"deploy_status": "unhealthy"}));
        let flow_state = make_flow_state();
        let registry = make_registry();

        let result = evaluator
            .evaluate(
                "deploy_status == \"unhealthy\"",
                &task,
                &flow_state,
                &registry,
            )
            .unwrap();
        assert!(result);

        let result = evaluator
            .evaluate(
                "deploy_status == \"healthy\"",
                &task,
                &flow_state,
                &registry,
            )
            .unwrap();
        assert!(!result);
    }

    #[test]
    fn test_and_condition() {
        let evaluator = ConditionEvaluator::new();
        let task = make_task(serde_json::json!({"a": true, "b": true}));
        let flow_state = make_flow_state();
        let registry = make_registry();

        let result = evaluator
            .evaluate("a AND b", &task, &flow_state, &registry)
            .unwrap();
        assert!(result);

        let task2 = make_task(serde_json::json!({"a": true, "b": false}));
        let result = evaluator
            .evaluate("a AND b", &task2, &flow_state, &registry)
            .unwrap();
        assert!(!result);
    }

    #[test]
    fn test_or_condition() {
        let evaluator = ConditionEvaluator::new();
        let task = make_task(serde_json::json!({"a": false, "b": true}));
        let flow_state = make_flow_state();
        let registry = make_registry();

        let result = evaluator
            .evaluate("a OR b", &task, &flow_state, &registry)
            .unwrap();
        assert!(result);
    }

    #[test]
    fn test_not_condition() {
        let evaluator = ConditionEvaluator::new();
        let task = make_task(serde_json::json!({"tests_passed": false}));
        let flow_state = make_flow_state();
        let registry = make_registry();

        let result = evaluator
            .evaluate("NOT tests_passed", &task, &flow_state, &registry)
            .unwrap();
        assert!(result);
    }

    #[test]
    fn test_all_tools_done() {
        let evaluator = ConditionEvaluator::new();
        let registry = make_registry();
        let flow_state = make_flow_state();

        // pending_tools 为空 → all_tools_done = true
        let task = make_task(serde_json::json!({"pending_tools": []}));
        let result = evaluator
            .evaluate("all_tools_done", &task, &flow_state, &registry)
            .unwrap();
        assert!(result);

        // pending_tools 不为空 → all_tools_done = false
        let task2 = make_task(serde_json::json!({"pending_tools": ["git.push"]}));
        let result = evaluator
            .evaluate("all_tools_done", &task2, &flow_state, &registry)
            .unwrap();
        assert!(!result);

        // 无 pending_tools 键 → all_tools_done = true (默认)
        let task3 = make_task(serde_json::json!({}));
        let result = evaluator
            .evaluate("all_tools_done", &task3, &flow_state, &registry)
            .unwrap();
        assert!(result);
    }

    #[test]
    fn test_all_done() {
        let evaluator = ConditionEvaluator::new();
        let registry = make_registry();
        let flow_state = make_flow_state();

        let task = make_task(serde_json::json!({"agent_status": "completed"}));
        let result = evaluator
            .evaluate("all_done", &task, &flow_state, &registry)
            .unwrap();
        assert!(result);

        let task2 = make_task(serde_json::json!({"agent_status": "running"}));
        let result = evaluator
            .evaluate("all_done", &task2, &flow_state, &registry)
            .unwrap();
        assert!(!result);
    }

    #[test]
    fn test_has_high_risk_tools() {
        let evaluator = ConditionEvaluator::new();
        let flow_state = make_flow_state();

        let registry = make_registry();
        registry.register(make_high_risk_tool("git.push"));

        let task = make_task(serde_json::json!({"planned_tools": ["git.push"]}));
        let result = evaluator
            .evaluate("has_high_risk_tools", &task, &flow_state, &registry)
            .unwrap();
        assert!(result);

        // Tool 不在注册表中 → 不算高风险
        let task2 = make_task(serde_json::json!({"planned_tools": ["unknown.tool"]}));
        let result = evaluator
            .evaluate("has_high_risk_tools", &task2, &flow_state, &registry)
            .unwrap();
        assert!(!result);
    }

    #[test]
    fn test_compound_all_tools_done_and_tests_passed() {
        let evaluator = ConditionEvaluator::new();
        let flow_state = make_flow_state();
        let registry = make_registry();

        let task = make_task(serde_json::json!({
            "pending_tools": [],
            "tests_passed": true,
        }));
        let result = evaluator
            .evaluate(
                "all_tools_done AND tests_passed",
                &task,
                &flow_state,
                &registry,
            )
            .unwrap();
        assert!(result);
    }

    #[test]
    fn test_empty_condition_is_error() {
        let evaluator = ConditionEvaluator::new();
        let task = make_task(serde_json::json!({}));
        let flow_state = make_flow_state();
        let registry = make_registry();

        let result = evaluator.evaluate("", &task, &flow_state, &registry);
        assert!(result.is_err());
    }
}
