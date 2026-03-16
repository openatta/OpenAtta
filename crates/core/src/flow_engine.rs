//! Flow 状态机引擎
//!
//! [`FlowEngine`] 是 AttaOS 控制面的核心组件，负责：
//!
//! - 管理 FlowDef 模板的注册和查询
//! - 创建 Task（Flow 运行实例）
//! - 推进 Task 状态机（条件求值 → 状态转移）
//! - 强制状态转移（ErrorPolicy fallback / 管理员干预）
//!
//! # 事件驱动
//!
//! 所有状态变更通过 EventBus 发布事件，CoreCoordinator 订阅事件后调用 `advance()` 推进。

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;

use chrono::Utc;
use tracing::{error, info, warn};
use uuid::Uuid;

use atta_bus::EventBus;
use atta_store::StateStore;
use atta_types::flow::{JoinStrategy, StateDef};
use atta_types::{
    Actor, AttaError, EventEnvelope, FlowDef, FlowState, OnEnterAction, StateTransition, StateType,
    Task, TaskStatus,
};

use crate::condition::ConditionEvaluator;
use atta_types::ToolRegistry;

/// Flow 状态机引擎
///
/// 控制面核心组件，管理 FlowDef 模板并驱动 Task 状态机推进。
/// 通过 DI 注入 `StateStore`、`EventBus`、`ToolRegistry`。
pub struct FlowEngine {
    /// 状态存储
    store: Arc<dyn StateStore>,
    /// 事件总线
    bus: Arc<dyn EventBus>,
    /// Flow 定义缓存（启动时从 StateStore 加载）
    flow_registry: RwLock<HashMap<String, FlowDef>>,
    /// 条件求值器
    condition_evaluator: ConditionEvaluator,
    /// Tool 注册表（供条件求值使用）
    tool_registry: Arc<dyn ToolRegistry>,
}

impl FlowEngine {
    /// 创建 FlowEngine 实例
    ///
    /// # Arguments
    ///
    /// * `store` - 状态存储实例
    /// * `bus` - 事件总线实例
    /// * `tool_registry` - Tool 注册表实例
    pub fn new(
        store: Arc<dyn StateStore>,
        bus: Arc<dyn EventBus>,
        tool_registry: Arc<dyn ToolRegistry>,
    ) -> Self {
        Self {
            store,
            bus,
            flow_registry: RwLock::new(HashMap::new()),
            condition_evaluator: ConditionEvaluator::new(),
            tool_registry,
        }
    }

    /// 从 StateStore 加载所有 FlowDef 到内存缓存
    ///
    /// 应在系统启动时调用。后续通过 API 注册的 FlowDef 也会同步写入缓存。
    pub async fn load_flows(&self) -> Result<(), AttaError> {
        let flows = self.store.list_flow_defs().await?;
        let mut registry = self.flow_registry.write().map_err(|_| {
            AttaError::Runtime(atta_types::RuntimeError::ResourceExceeded(
                "flow registry lock poisoned".to_string(),
            ))
        })?;
        for flow in flows {
            info!(flow_id = %flow.id, "loaded flow definition");
            registry.insert(flow.id.clone(), flow);
        }
        info!(count = registry.len(), "flow definitions loaded");
        Ok(())
    }

    /// Load FlowDefs from YAML files in a directory.
    ///
    /// Scans the directory for `.yaml` / `.yml` files, parses each as a [`FlowDef`],
    /// validates it, and registers it. Invalid files are skipped with a warning.
    /// Later directories override earlier ones (exts overrides lib).
    pub async fn load_flows_from_dir(&self, dir: &std::path::Path) -> Result<usize, AttaError> {
        if !dir.exists() {
            return Ok(0);
        }

        let mut count = 0;
        let mut read_dir = tokio::fs::read_dir(dir)
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext != "yaml" && ext != "yml" {
                continue;
            }

            match tokio::fs::read_to_string(&path).await {
                Ok(content) => match serde_yml::from_str::<FlowDef>(&content) {
                    Ok(flow_def) => {
                        if let Err(e) = Self::validate_flow_def(&flow_def) {
                            warn!(
                                path = %path.display(),
                                error = %e,
                                "invalid flow definition, skipping"
                            );
                            continue;
                        }
                        info!(flow_id = %flow_def.id, path = %path.display(), "loaded flow from file");
                        self.flow_registry
                            .write()
                            .map_err(|_| {
                                AttaError::Runtime(
                                    atta_types::RuntimeError::ResourceExceeded(
                                        "flow registry lock poisoned".to_string(),
                                    ),
                                )
                            })?
                            .insert(flow_def.id.clone(), flow_def);
                        count += 1;
                    }
                    Err(e) => {
                        warn!(
                            path = %path.display(),
                            error = %e,
                            "failed to parse flow YAML, skipping"
                        );
                    }
                },
                Err(e) => {
                    warn!(
                        path = %path.display(),
                        error = %e,
                        "failed to read flow file, skipping"
                    );
                }
            }
        }

        Ok(count)
    }

    /// 列出所有已加载的 FlowDef
    pub fn list_flow_defs(&self) -> Result<Vec<FlowDef>, AttaError> {
        let registry = self.flow_registry.read().map_err(|_| {
            AttaError::Runtime(atta_types::RuntimeError::ResourceExceeded(
                "flow registry lock poisoned".to_string(),
            ))
        })?;
        Ok(registry.values().cloned().collect())
    }

    /// 根据 ID 获取 FlowDef
    pub fn get_flow_def(&self, id: &str) -> Result<FlowDef, AttaError> {
        let registry = self.flow_registry.read().map_err(|_| {
            AttaError::Runtime(atta_types::RuntimeError::ResourceExceeded(
                "flow registry lock poisoned".to_string(),
            ))
        })?;
        registry
            .get(id)
            .cloned()
            .ok_or_else(|| AttaError::FlowNotFound(id.to_string()))
    }

    /// 注册 FlowDef 到缓存（不持久化，持久化由调用方负责）
    ///
    /// 注册前会执行 [`validate_flow_def`] 校验。
    pub fn register_flow_def(&self, flow_def: FlowDef) -> Result<(), AttaError> {
        Self::validate_flow_def(&flow_def)?;
        let mut registry = self.flow_registry.write().map_err(|_| {
            AttaError::Runtime(atta_types::RuntimeError::ResourceExceeded(
                "flow registry lock poisoned".to_string(),
            ))
        })?;
        registry.insert(flow_def.id.clone(), flow_def);
        Ok(())
    }

    /// 校验 FlowDef 的结构完整性
    ///
    /// - 恰好一个 `StateType::Start` 状态
    /// - 至少一个 `StateType::End` 状态
    /// - 所有 `transitions.to` 引用的目标状态必须存在
    /// - `StateType::Gate` 状态必须定义 `gate` 字段
    pub fn validate_flow_def(flow_def: &FlowDef) -> Result<(), AttaError> {
        let start_count = flow_def
            .states
            .values()
            .filter(|s| s.state_type == StateType::Start)
            .count();
        if start_count != 1 {
            return Err(AttaError::Validation(format!(
                "flow '{}' must have exactly one Start state, found {}",
                flow_def.id, start_count
            )));
        }

        let end_count = flow_def
            .states
            .values()
            .filter(|s| s.state_type == StateType::End)
            .count();
        if end_count == 0 {
            return Err(AttaError::Validation(format!(
                "flow '{}' must have at least one End state",
                flow_def.id
            )));
        }

        for (state_name, state_def) in &flow_def.states {
            // Gate 状态必须有 gate 字段
            if state_def.state_type == StateType::Gate && state_def.gate.is_none() {
                return Err(AttaError::Validation(format!(
                    "gate state '{}' in flow '{}' must have a 'gate' field",
                    state_name, flow_def.id
                )));
            }

            // Gate states must not have auto transitions (would bypass approval)
            if state_def.state_type == StateType::Gate {
                for transition in &state_def.transitions {
                    if transition.auto.unwrap_or(false) {
                        return Err(AttaError::Validation(format!(
                            "gate state '{}' in flow '{}' must not have auto transitions (bypasses approval)",
                            state_name, flow_def.id
                        )));
                    }
                }
            }

            // 所有 transitions.to 必须引用存在的状态
            for transition in &state_def.transitions {
                if !flow_def.states.contains_key(&transition.to) {
                    return Err(AttaError::Validation(format!(
                        "transition target '{}' from state '{}' not found in flow '{}'",
                        transition.to, state_name, flow_def.id
                    )));
                }
            }
        }

        // Validate ErrorPolicy fallback state exists
        if let Some(error_policy) = &flow_def.on_error {
            if !flow_def.states.contains_key(&error_policy.fallback) {
                return Err(AttaError::Validation(format!(
                    "error policy fallback state '{}' not found in flow '{}'",
                    error_policy.fallback, flow_def.id
                )));
            }
            // Validate retry_states all exist
            for state_name in &error_policy.retry_states {
                if !flow_def.states.contains_key(state_name) {
                    return Err(AttaError::Validation(format!(
                        "error policy retry state '{}' not found in flow '{}'",
                        state_name, flow_def.id
                    )));
                }
            }
        }

        Ok(())
    }

    /// 创建新 Task（Flow 运行实例）
    ///
    /// 1. 从缓存获取 FlowDef
    /// 2. 初始化 Task 和 FlowState
    /// 3. 持久化到 StateStore（事务）
    /// 4. 发布 `atta.task.created` 事件
    /// 5. 执行初始状态的自动推进
    pub async fn create_task(
        &self,
        flow_id: &str,
        input: serde_json::Value,
        actor: Actor,
    ) -> Result<Task, AttaError> {
        let flow_def = self.get_flow_def(flow_id)?;

        let now = Utc::now();
        let task_id = Uuid::new_v4();

        let task = Task {
            id: task_id,
            flow_id: flow_id.to_string(),
            current_state: flow_def.initial_state.clone(),
            state_data: serde_json::json!({}),
            input,
            output: None,
            status: TaskStatus::Running,
            created_by: actor,
            created_at: now,
            updated_at: now,
            completed_at: None,
            version: 0,
        };

        let flow_state = FlowState {
            task_id,
            current_state: flow_def.initial_state.clone(),
            history: Vec::new(),
            pending_approval: None,
            retry_count: 0,
        };

        // 持久化 Task + FlowState（事务）
        self.store.create_task_with_flow(&task, &flow_state).await?;

        // 发布创建事件
        self.bus
            .publish("atta.task.created", EventEnvelope::task_created(&task)?)
            .await?;

        info!(task_id = %task.id, flow_id = %flow_id, "task created");

        // 尝试初始自动推进
        self.advance_by_id(&task.id).await?;

        Ok(task)
    }

    /// 推进 Task 状态机
    ///
    /// 遍历当前状态的转移列表，按序求值条件。
    /// - `auto: true` → 无条件转移
    /// - `when: <expr>` → 条件为 true 时转移
    /// - 无匹配条件 → 停留当前状态
    ///
    /// 转移成功后会从 StateStore 重新加载 Task，继续推进（cascading auto-transitions），
    /// 直到到达 End 状态或无匹配条件。
    ///
    /// 若推进过程中发生错误且 FlowDef 配置了 ErrorPolicy，会自动重试或跳转到 fallback 状态。
    ///
    /// 由 CoreCoordinator 在收到事件后调用。
    pub async fn advance(&self, task: &Task) -> Result<(), AttaError> {
        self.advance_by_id(&task.id).await
    }

    /// 根据 task_id 推进 Task 状态机（支持 cascading auto-transitions）
    pub async fn advance_by_id(&self, task_id: &Uuid) -> Result<(), AttaError> {
        const MAX_CASCADE_DEPTH: usize = 100;
        for _iteration in 0..MAX_CASCADE_DEPTH {
            // 每次循环从 store 重新加载最新的 Task
            let task = self
                .store
                .get_task(task_id)
                .await?
                .ok_or_else(|| AttaError::NotFound {
                    entity_type: "task".to_string(),
                    id: task_id.to_string(),
                })?;

            let flow_def = self.get_flow_def(&task.flow_id)?;

            let state_def = flow_def.states.get(&task.current_state).ok_or_else(|| {
                AttaError::Validation(format!(
                    "state '{}' not found in flow '{}'",
                    task.current_state, task.flow_id
                ))
            })?;

            // End 状态不再推进
            if state_def.state_type == StateType::End {
                return Ok(());
            }

            // Parallel 状态：分支 spawn / 汇合
            if state_def.state_type == StateType::Parallel {
                self.handle_parallel_state(&task, state_def).await?;
                return Ok(());
            }

            let flow_state = self
                .store
                .get_flow_state(&task.id)
                .await?
                .unwrap_or_default();

            let mut transitioned = false;

            for transition in &state_def.transitions {
                // auto: true -> 无条件转移
                if transition.auto.unwrap_or(false) {
                    match self.transition(&task, &transition.to).await {
                        Ok(()) => {
                            transitioned = true;
                            break;
                        }
                        Err(e) => {
                            // 尝试 ErrorPolicy 处理
                            if self
                                .handle_error_policy(&task, &flow_def, &flow_state, &e)
                                .await?
                            {
                                return Ok(());
                            }
                            return Err(e);
                        }
                    }
                }

                // when 条件求值
                if let Some(condition) = &transition.when {
                    let result = self.condition_evaluator.evaluate(
                        condition,
                        &task,
                        &flow_state,
                        self.tool_registry.as_ref(),
                    )?;
                    if result {
                        match self.transition(&task, &transition.to).await {
                            Ok(()) => {
                                transitioned = true;
                                break;
                            }
                            Err(e) => {
                                if self
                                    .handle_error_policy(&task, &flow_def, &flow_state, &e)
                                    .await?
                                {
                                    return Ok(());
                                }
                                return Err(e);
                            }
                        }
                    }
                }
            }

            if !transitioned {
                // 无匹配条件，停留当前状态
                return Ok(());
            }
            // transitioned == true → 循环继续，重新加载 Task 推进下一步
        }
        // If we exhaust iterations, warn and return error
        error!(task_id = %task_id, max_depth = MAX_CASCADE_DEPTH,
            "cascading auto-transitions exceeded limit — possible infinite loop in flow definition");
        Err(AttaError::Validation(format!(
            "flow advance exceeded maximum cascade depth ({MAX_CASCADE_DEPTH}) for task {task_id}"
        )))
    }

    /// 处理 ErrorPolicy：重试或跳转到 fallback 状态
    ///
    /// 返回 `Ok(true)` 表示 ErrorPolicy 已接管处理（调用方应 return Ok），
    /// 返回 `Ok(false)` 表示无 ErrorPolicy 或当前状态不在 retry_states 中。
    async fn handle_error_policy(
        &self,
        task: &Task,
        flow_def: &FlowDef,
        flow_state: &FlowState,
        error: &AttaError,
    ) -> Result<bool, AttaError> {
        let policy = match &flow_def.on_error {
            Some(p) => p,
            None => return Ok(false),
        };

        // 只对 retry_states 中列出的状态应用重试策略
        if !policy.retry_states.contains(&task.current_state) {
            return Ok(false);
        }

        if flow_state.retry_count < policy.max_retries {
            // 增加重试计数
            let new_retry_count = flow_state.retry_count + 1;
            let mut updated_flow_state = flow_state.clone();
            updated_flow_state.retry_count = new_retry_count;
            self.store
                .save_flow_state(&task.id, &updated_flow_state)
                .await?;

            warn!(
                task_id = %task.id,
                state = %task.current_state,
                attempt = new_retry_count,
                max_retries = policy.max_retries,
                error = %error,
                "retrying after error"
            );

            // 发布重试事件
            self.bus
                .publish(
                    "atta.flow.retry",
                    EventEnvelope::flow_retry(&task.id, &task.current_state, new_retry_count)?,
                )
                .await?;

            return Ok(true);
        }

        // 重试次数耗尽，跳转 fallback 状态
        warn!(
            task_id = %task.id,
            state = %task.current_state,
            retries = flow_state.retry_count,
            fallback = %policy.fallback,
            error = %error,
            "retries exhausted, transitioning to fallback"
        );
        self.force_transition(task, &policy.fallback).await?;
        Ok(true)
    }

    /// 强制状态转移
    ///
    /// 跳过条件求值，直接转移到指定状态。
    /// 用于 ErrorPolicy fallback、管理员干预、超时处理等场景。
    pub async fn force_transition(&self, task: &Task, target_state: &str) -> Result<(), AttaError> {
        let flow_def = self.get_flow_def(&task.flow_id)?;

        // 校验目标状态存在
        if !flow_def.states.contains_key(target_state) {
            return Err(AttaError::Validation(format!(
                "target state '{}' not found in flow '{}'",
                target_state, task.flow_id
            )));
        }

        warn!(
            task_id = %task.id,
            from = %task.current_state,
            to = %target_state,
            "force transition"
        );

        self.transition(task, target_state).await
    }

    /// 执行状态转移
    ///
    /// 1. 构造 StateTransition 记录
    /// 2. 确定新的 TaskStatus（End 状态根据 state_data 中是否有 "error" 键判断成功/失败）
    /// 3. 通过 StateStore 事务更新 Task + FlowState + Transition
    /// 4. 发布 `atta.flow.advanced` 事件
    /// 5. 执行目标状态的 `on_enter` 动作
    async fn transition(&self, task: &Task, target: &str) -> Result<(), AttaError> {
        let flow_def = self.get_flow_def(&task.flow_id)?;

        let transition_record = StateTransition {
            from: task.current_state.clone(),
            to: target.to_string(),
            reason: format!("transition from {} to {}", task.current_state, target),
            timestamp: Utc::now(),
        };

        // 根据目标状态类型确定 TaskStatus
        let target_state_def = flow_def.states.get(target);
        let new_status = if let Some(state_def) = target_state_def {
            match state_def.state_type {
                StateType::End => {
                    // 通过 state_data 中是否有 "error" 键来判断成功/失败
                    if task.state_data.get("error").is_some() {
                        TaskStatus::Failed {
                            error: task
                                .state_data
                                .get("error")
                                .and_then(|v| v.as_str())
                                .unwrap_or("flow reached failure state")
                                .to_string(),
                        }
                    } else {
                        TaskStatus::Completed
                    }
                }
                StateType::Gate => TaskStatus::WaitingApproval,
                _ => TaskStatus::Running,
            }
        } else {
            TaskStatus::Running
        };

        // 事务更新 Task + FlowState + Transition (optimistic locking)
        self.store
            .advance_task(&task.id, new_status, target, &transition_record, task.version)
            .await?;

        // 发布事件
        self.bus
            .publish(
                "atta.flow.advanced",
                EventEnvelope::flow_advanced(&task.id, &task.current_state, target)?,
            )
            .await?;

        info!(
            task_id = %task.id,
            from = %task.current_state,
            to = %target,
            "flow advanced"
        );

        // 执行目标状态的 on_enter 动作
        if let Some(state_def) = target_state_def {
            self.execute_on_enter(task, target, state_def.on_enter.as_deref())
                .await?;
        }

        Ok(())
    }

    /// Parallel 状态处理
    ///
    /// 分两阶段执行：
    /// 1. **首次进入**：为每个 branch 创建子任务，将 branch task ID 记录到
    ///    `state_data["_parallel"]["branch_tasks"]`，发布事件由 Coordinator 调度执行。
    /// 2. **后续调用**：检查所有子任务完成状态，按 JoinStrategy 汇合：
    ///    - `All`：全部完成后合并结果，auto-transition 到下一状态
    ///    - `FailFast`：任一失败即标记父任务失败
    async fn handle_parallel_state(
        &self,
        task: &Task,
        state_def: &StateDef,
    ) -> Result<(), AttaError> {
        let branches = state_def.branches.as_deref().unwrap_or_default();
        if branches.is_empty() {
            warn!(task_id = %task.id, "parallel state has no branches, skipping");
            return Ok(());
        }

        let join_strategy = state_def.join_strategy.clone().unwrap_or(JoinStrategy::All);

        // 检查是否已经 spawn 了分支任务
        let existing_branch_ids: Vec<String> = task
            .state_data
            .get("_parallel")
            .and_then(|p| p.get("branch_tasks"))
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        if existing_branch_ids.is_empty() {
            // Phase 1: spawn branch sub-tasks
            let mut branch_task_ids = Vec::new();

            for (i, branch) in branches.iter().enumerate() {
                let branch_task_id = Uuid::new_v4();
                let mut input = task.state_data.clone();

                // 应用 input_mapping（从 parent state_data 提取子集）
                if let Some(mapping) = &branch.input_mapping {
                    let mut mapped = serde_json::Map::new();
                    for (target_key, source_key) in mapping {
                        if let Some(val) = task.state_data.get(source_key) {
                            mapped.insert(target_key.clone(), val.clone());
                        }
                    }
                    input = serde_json::Value::Object(mapped);
                }

                let branch_task = Task {
                    id: branch_task_id,
                    flow_id: format!("{}/_parallel/{}", task.flow_id, branch.skill),
                    current_state: "start".to_string(),
                    state_data: serde_json::json!({}),
                    input,
                    output: None,
                    status: TaskStatus::Running,
                    created_by: Actor::system(),
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                    completed_at: None,
                    version: 0,
                };

                self.store.create_task(&branch_task).await?;
                branch_task_ids.push(branch_task_id.to_string());

                // 发布事件通知 Coordinator 调度执行
                let event = EventEnvelope::new(
                    "atta.parallel.branch_spawned",
                    atta_types::EntityRef::task(&task.id),
                    Actor::system(),
                    task.id,
                    serde_json::json!({
                        "parent_task_id": task.id,
                        "branch_task_id": branch_task_id,
                        "branch_index": i,
                        "agent": branch.agent,
                        "skill": branch.skill,
                    }),
                )?;
                self.bus
                    .publish("atta.parallel.branch_spawned", event)
                    .await?;

                info!(
                    parent = %task.id,
                    branch = %branch_task_id,
                    agent = %branch.agent,
                    skill = %branch.skill,
                    "spawned parallel branch"
                );
            }

            // 记录 branch task IDs 到 parent 的 state_data
            self.store
                .merge_task_state_data(
                    &task.id,
                    serde_json::json!({
                        "_parallel": {
                            "branch_tasks": branch_task_ids,
                            "join_strategy": serde_json::to_value(&join_strategy)
                                .unwrap_or(serde_json::Value::String("all".into())),
                            "spawned_at": Utc::now().to_rfc3339(),
                        }
                    }),
                )
                .await?;

            return Ok(());
        }

        // Phase 2: check branch completion
        let mut all_completed = true;
        let mut any_failed = false;
        let mut branch_results = serde_json::Map::new();

        for branch_id_str in &existing_branch_ids {
            let branch_id = branch_id_str
                .parse::<Uuid>()
                .map_err(|e| AttaError::Validation(format!("invalid branch task id: {e}")))?;

            if let Some(branch_task) = self.store.get_task(&branch_id).await? {
                match &branch_task.status {
                    TaskStatus::Completed => {
                        branch_results.insert(
                            branch_id_str.clone(),
                            branch_task
                                .output
                                .clone()
                                .unwrap_or_else(|| serde_json::json!({"_no_output": true})),
                        );
                    }
                    TaskStatus::Failed { error } => {
                        any_failed = true;
                        branch_results
                            .insert(branch_id_str.clone(), serde_json::json!({ "error": error }));
                        if matches!(join_strategy, JoinStrategy::FailFast) {
                            break;
                        }
                    }
                    _ => {
                        all_completed = false;
                    }
                }
            } else {
                all_completed = false;
            }
        }

        // Timeout check: if timeout_secs is set and elapsed, force-fail incomplete branches
        if !all_completed {
            if let Some(timeout_secs) = state_def.timeout_secs {
                let spawned_at_str = task
                    .state_data
                    .get("_parallel")
                    .and_then(|p| p.get("spawned_at"))
                    .and_then(|v| v.as_str());
                if let Some(spawned_str) = spawned_at_str {
                    if let Ok(spawned_at) =
                        chrono::DateTime::parse_from_rfc3339(spawned_str)
                    {
                        let elapsed = Utc::now()
                            .signed_duration_since(spawned_at.with_timezone(&Utc));
                        if elapsed.num_seconds() >= timeout_secs as i64 {
                            warn!(
                                task_id = %task.id,
                                timeout_secs,
                                elapsed_secs = elapsed.num_seconds(),
                                "parallel branches timed out, force-failing incomplete branches"
                            );
                            // Force-fail any incomplete branch tasks
                            for branch_id_str in &existing_branch_ids {
                                if branch_results.contains_key(branch_id_str) {
                                    continue; // already completed or failed
                                }
                                let branch_id = branch_id_str
                                    .parse::<Uuid>()
                                    .map_err(|e| {
                                        AttaError::Validation(format!(
                                            "invalid branch task id: {e}"
                                        ))
                                    })?;
                                self.store
                                    .update_task_status(
                                        &branch_id,
                                        TaskStatus::Failed {
                                            error: format!(
                                                "parallel branch timed out after {}s",
                                                timeout_secs
                                            ),
                                        },
                                    )
                                    .await?;
                                branch_results.insert(
                                    branch_id_str.clone(),
                                    serde_json::json!({
                                        "error": format!(
                                            "parallel branch timed out after {}s",
                                            timeout_secs
                                        )
                                    }),
                                );
                                any_failed = true;
                            }
                            all_completed = true;
                        }
                    }
                }
            }
        }

        // FailFast: 任一分支失败 → 父任务失败
        if any_failed && matches!(join_strategy, JoinStrategy::FailFast) {
            // Before merge, verify task hasn't been modified concurrently
            let current_task = self.store.get_task(&task.id).await?.ok_or_else(|| {
                AttaError::NotFound {
                    entity_type: "task".to_string(),
                    id: task.id.to_string(),
                }
            })?;
            if current_task.version != task.version {
                warn!(task_id = %task.id, "parallel state task modified concurrently, deferring");
                return Ok(());
            }
            self.store
                .merge_task_state_data(
                    &task.id,
                    serde_json::json!({
                        "_parallel_results": branch_results,
                        "error": "parallel branch failed (fail_fast)",
                    }),
                )
                .await?;

            // 如果有 auto transition，尝试执行（可能指向 error/end 状态）
            for transition in &state_def.transitions {
                if transition.auto.unwrap_or(false) {
                    self.transition(task, &transition.to).await?;
                    return Ok(());
                }
            }
            return Ok(());
        }

        // All: 全部完成 → 合并结果并 auto-transition
        if all_completed && !any_failed {
            // Before merge, verify task hasn't been modified concurrently
            let current_task = self.store.get_task(&task.id).await?.ok_or_else(|| {
                AttaError::NotFound {
                    entity_type: "task".to_string(),
                    id: task.id.to_string(),
                }
            })?;
            if current_task.version != task.version {
                warn!(task_id = %task.id, "parallel state task modified concurrently, deferring");
                return Ok(());
            }
            self.store
                .merge_task_state_data(
                    &task.id,
                    serde_json::json!({
                        "_parallel_results": branch_results,
                    }),
                )
                .await?;

            // auto-transition 到下一状态
            for transition in &state_def.transitions {
                if transition.auto.unwrap_or(false) {
                    self.transition(task, &transition.to).await?;
                    return Ok(());
                }
            }
        }

        // 仍在等待分支完成
        Ok(())
    }

    /// 执行状态的 `on_enter` 动作列表
    ///
    /// 支持三种动作类型：
    /// - `PublishEvent` — 发布事件到 EventBus
    /// - `SetVariable` — 合并键值到 task.state_data
    /// - `Log` — 输出 tracing 日志
    async fn execute_on_enter(
        &self,
        task: &Task,
        state_name: &str,
        actions: Option<&[OnEnterAction]>,
    ) -> Result<(), AttaError> {
        let actions = match actions {
            Some(a) => a,
            None => return Ok(()),
        };

        for action in actions {
            match action {
                OnEnterAction::PublishEvent { event_type } => {
                    let event = EventEnvelope::new(
                        event_type.as_str(),
                        atta_types::EntityRef::task(&task.id),
                        Actor::system(),
                        task.id,
                        serde_json::json!({
                            "task_id": task.id,
                            "state": state_name,
                        }),
                    )?;
                    self.bus.publish(event_type, event).await?;
                    info!(
                        task_id = %task.id,
                        state = state_name,
                        event_type = event_type,
                        "on_enter: published event"
                    );
                }
                OnEnterAction::SetVariable { key, value } => {
                    let mut map = serde_json::Map::new();
                    map.insert(key.clone(), value.clone());
                    let patch = serde_json::Value::Object(map);
                    self.store.merge_task_state_data(&task.id, patch).await?;
                    info!(
                        task_id = %task.id,
                        state = state_name,
                        key = key,
                        "on_enter: set variable"
                    );
                }
                OnEnterAction::Log { message } => {
                    info!(
                        task_id = %task.id,
                        state = state_name,
                        message = message,
                        "on_enter: log"
                    );
                }
            }
        }

        Ok(())
    }
}

/// FlowRunner implementation — bridges FlowEngine to the trait used by start_flow tool.
#[async_trait::async_trait]
impl atta_types::FlowRunner for FlowEngine {
    async fn start_flow(
        &self,
        flow_id: &str,
        input: serde_json::Value,
        actor: Actor,
    ) -> Result<Task, AttaError> {
        self.create_task(flow_id, input, actor).await
    }

    async fn list_flows(&self) -> Result<Vec<(String, Option<String>)>, AttaError> {
        let registry = self.flow_registry.read().map_err(|_| {
            AttaError::Runtime(atta_types::RuntimeError::ResourceExceeded(
                "flow registry lock poisoned".to_string(),
            ))
        })?;
        Ok(registry
            .values()
            .map(|f| (f.id.clone(), f.name.clone()))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_registry::DefaultToolRegistry;
    use atta_store::{
        ApprovalStore, CronStore, FlowStore, McpStore, NodeStore, PackageStore, RbacStore,
        RegistryStore, ServiceAccountStore, TaskStore,
    };
    use atta_types::{StateDef, TransitionDef};

    // 构造一个最简 FlowDef 用于测试
    fn make_simple_flow() -> FlowDef {
        let mut states = HashMap::new();

        states.insert(
            "init".to_string(),
            StateDef {
                state_type: StateType::Start,
                agent: None,
                skill: None,
                gate: None,
                on_enter: None,
                transitions: vec![TransitionDef {
                    to: "complete".to_string(),
                    when: None,
                    auto: Some(true),
                }],
                branches: None,
                join_strategy: None,
                timeout_secs: None,
            },
        );

        states.insert(
            "complete".to_string(),
            StateDef {
                state_type: StateType::End,
                agent: None,
                skill: None,
                gate: None,
                on_enter: None,
                transitions: vec![],
                branches: None,
                join_strategy: None,
                timeout_secs: None,
            },
        );

        FlowDef {
            id: "test_flow".to_string(),
            version: "1.0".to_string(),
            name: Some("Test Flow".to_string()),
            description: None,
            initial_state: "init".to_string(),
            states,
            on_error: None,
            skills: vec![],
            source: "builtin".to_string(),
        }
    }

    /// 构造 FlowEngine 测试实例（使用 InProcBus + InProcBus 作为 store 占位）
    fn make_engine() -> FlowEngine {
        let bus: Arc<dyn EventBus> = Arc::new(atta_bus::InProcBus::new());
        let tool_registry: Arc<dyn ToolRegistry> = Arc::new(DefaultToolRegistry::new());

        // 注意：这里 FlowEngine 的同步方法（register/get）不使用 store，
        // 但构造函数需要 Arc<dyn StateStore>。
        // 使用 InProcBus 作为 bus，store 需要异步构造。
        // 这里我们先只测试不需要 store 的方法，使用 flow_registry 直接操作。

        // 直接构造 FlowEngine（绕过 store）
        FlowEngine {
            store: {
                // 创建一个不会被用到的占位 store
                // 在同步测试中我们不调用任何 async 方法
                panic_store()
            },
            bus,
            flow_registry: RwLock::new(HashMap::new()),
            condition_evaluator: ConditionEvaluator::new(),
            tool_registry,
        }
    }

    /// 创建一个占位 StateStore（同步测试中不会实际调用）
    fn panic_store() -> Arc<dyn StateStore> {
        struct PanicStore;

        #[async_trait::async_trait]
        impl TaskStore for PanicStore {
            async fn create_task(&self, _: &Task) -> Result<(), AttaError> {
                unreachable!()
            }
            async fn get_task(&self, _: &Uuid) -> Result<Option<Task>, AttaError> {
                unreachable!()
            }
            async fn update_task_status(&self, _: &Uuid, _: TaskStatus) -> Result<(), AttaError> {
                unreachable!()
            }
            async fn list_tasks(&self, _: &atta_types::TaskFilter) -> Result<Vec<Task>, AttaError> {
                unreachable!()
            }
            async fn merge_task_state_data(
                &self,
                _: &Uuid,
                _: serde_json::Value,
            ) -> Result<(), AttaError> {
                unreachable!()
            }
        }

        #[async_trait::async_trait]
        impl FlowStore for PanicStore {
            async fn save_flow_def(&self, _: &FlowDef) -> Result<(), AttaError> {
                unreachable!()
            }
            async fn get_flow_def(&self, _: &str) -> Result<Option<FlowDef>, AttaError> {
                unreachable!()
            }
            async fn save_flow_state(&self, _: &Uuid, _: &FlowState) -> Result<(), AttaError> {
                unreachable!()
            }
            async fn get_flow_state(&self, _: &Uuid) -> Result<Option<FlowState>, AttaError> {
                unreachable!()
            }
            async fn list_flow_defs(&self) -> Result<Vec<FlowDef>, AttaError> {
                unreachable!()
            }
            async fn delete_flow_def(&self, _: &str) -> Result<(), AttaError> {
                unreachable!()
            }
            async fn list_skill_defs(&self) -> Result<Vec<atta_types::SkillDef>, AttaError> {
                unreachable!()
            }
            async fn create_task_with_flow(
                &self,
                _: &Task,
                _: &FlowState,
            ) -> Result<(), AttaError> {
                unreachable!()
            }
            async fn advance_task(
                &self,
                _: &Uuid,
                _: TaskStatus,
                _: &str,
                _: &StateTransition,
                _: u64,
            ) -> Result<(), AttaError> {
                unreachable!()
            }
        }

        #[async_trait::async_trait]
        impl RegistryStore for PanicStore {
            async fn register_plugin(
                &self,
                _: &atta_types::PluginManifest,
            ) -> Result<(), AttaError> {
                unreachable!()
            }
            async fn unregister_plugin(&self, _: &str) -> Result<(), AttaError> {
                unreachable!()
            }
            async fn list_plugins(&self) -> Result<Vec<atta_types::PluginManifest>, AttaError> {
                unreachable!()
            }
            async fn register_tool(&self, _: &atta_types::ToolDef) -> Result<(), AttaError> {
                unreachable!()
            }
            async fn list_tools(&self) -> Result<Vec<atta_types::ToolDef>, AttaError> {
                unreachable!()
            }
            async fn register_skill(&self, _: &atta_types::SkillDef) -> Result<(), AttaError> {
                unreachable!()
            }
            async fn list_skills(&self) -> Result<Vec<atta_types::SkillDef>, AttaError> {
                unreachable!()
            }
            async fn get_tool(&self, _: &str) -> Result<Option<atta_types::ToolDef>, AttaError> {
                unreachable!()
            }
            async fn get_skill(&self, _: &str) -> Result<Option<atta_types::SkillDef>, AttaError> {
                unreachable!()
            }
            async fn get_plugin(
                &self,
                _: &str,
            ) -> Result<Option<atta_types::PluginManifest>, AttaError> {
                unreachable!()
            }
            async fn delete_skill(&self, _: &str) -> Result<(), AttaError> {
                unreachable!()
            }
        }

        #[async_trait::async_trait]
        impl PackageStore for PanicStore {
            async fn register_package(
                &self,
                _: &atta_types::PackageRecord,
            ) -> Result<(), AttaError> {
                unreachable!()
            }
            async fn get_package(
                &self,
                _: &str,
            ) -> Result<Option<atta_types::PackageRecord>, AttaError> {
                unreachable!()
            }
        }

        #[async_trait::async_trait]
        impl ServiceAccountStore for PanicStore {
            async fn get_service_account_by_key(
                &self,
                _: &str,
            ) -> Result<Option<atta_types::package::ServiceAccount>, AttaError> {
                unreachable!()
            }
        }

        #[async_trait::async_trait]
        impl NodeStore for PanicStore {
            async fn upsert_node(&self, _: &atta_types::NodeInfo) -> Result<(), AttaError> {
                unreachable!()
            }
            async fn get_node(&self, _: &str) -> Result<Option<atta_types::NodeInfo>, AttaError> {
                unreachable!()
            }
            async fn list_nodes(&self) -> Result<Vec<atta_types::NodeInfo>, AttaError> {
                unreachable!()
            }
            async fn list_nodes_after(
                &self,
                _: chrono::DateTime<Utc>,
            ) -> Result<Vec<atta_types::NodeInfo>, AttaError> {
                unreachable!()
            }
            async fn update_node_status(
                &self,
                _: &str,
                _: atta_types::NodeStatus,
            ) -> Result<(), AttaError> {
                unreachable!()
            }
        }

        #[async_trait::async_trait]
        impl ApprovalStore for PanicStore {
            async fn save_approval(
                &self,
                _: &atta_types::ApprovalRequest,
            ) -> Result<(), AttaError> {
                unreachable!()
            }
            async fn get_approval(
                &self,
                _: &Uuid,
            ) -> Result<Option<atta_types::ApprovalRequest>, AttaError> {
                unreachable!()
            }
            async fn list_approvals(
                &self,
                _: &atta_types::ApprovalFilter,
            ) -> Result<Vec<atta_types::ApprovalRequest>, AttaError> {
                unreachable!()
            }
            async fn update_approval_status(
                &self,
                _: &Uuid,
                _: atta_types::ApprovalStatus,
                _: &Actor,
                _: Option<&str>,
            ) -> Result<(), AttaError> {
                unreachable!()
            }
        }

        #[async_trait::async_trait]
        impl McpStore for PanicStore {
            async fn register_mcp(&self, _: &atta_types::McpServerConfig) -> Result<(), AttaError> {
                unreachable!()
            }
            async fn list_mcp_servers(
                &self,
            ) -> Result<Vec<atta_types::McpServerConfig>, AttaError> {
                unreachable!()
            }
            async fn unregister_mcp(&self, _: &str) -> Result<(), AttaError> {
                unreachable!()
            }
        }

        #[async_trait::async_trait]
        impl CronStore for PanicStore {
            async fn save_cron_job(&self, _: &atta_types::CronJob) -> Result<(), AttaError> {
                unreachable!()
            }
            async fn get_cron_job(
                &self,
                _: &str,
            ) -> Result<Option<atta_types::CronJob>, AttaError> {
                unreachable!()
            }
            async fn list_cron_jobs(
                &self,
                _: Option<&str>,
            ) -> Result<Vec<atta_types::CronJob>, AttaError> {
                unreachable!()
            }
            async fn delete_cron_job(&self, _: &str) -> Result<(), AttaError> {
                unreachable!()
            }
            async fn save_cron_run(&self, _: &atta_types::CronRun) -> Result<(), AttaError> {
                unreachable!()
            }
            async fn list_cron_runs(
                &self,
                _: &str,
                _: usize,
            ) -> Result<Vec<atta_types::CronRun>, AttaError> {
                unreachable!()
            }
        }

        #[async_trait::async_trait]
        impl RbacStore for PanicStore {
            async fn get_roles_for_actor(
                &self,
                _: &str,
            ) -> Result<Vec<atta_types::Role>, AttaError> {
                unreachable!()
            }
            async fn bind_role(&self, _: &str, _: &atta_types::Role) -> Result<(), AttaError> {
                unreachable!()
            }
            async fn unbind_role(&self, _: &str, _: &atta_types::Role) -> Result<(), AttaError> {
                unreachable!()
            }
        }

        #[async_trait::async_trait]
        impl atta_store::UsageStore for PanicStore {
            async fn record_usage(
                &self,
                _: &atta_types::usage::UsageRecord,
            ) -> Result<(), AttaError> {
                unreachable!()
            }
            async fn get_usage_summary(
                &self,
                _: chrono::DateTime<chrono::Utc>,
            ) -> Result<atta_types::usage::UsageSummary, AttaError> {
                unreachable!()
            }
            async fn get_usage_daily(
                &self,
                _: chrono::DateTime<chrono::Utc>,
                _: chrono::DateTime<chrono::Utc>,
            ) -> Result<Vec<atta_types::usage::UsageDaily>, AttaError> {
                unreachable!()
            }
        }

        #[async_trait::async_trait]
        impl atta_store::RemoteAgentStore for PanicStore {
            async fn register_remote_agent(
                &self,
                _: &atta_types::RemoteAgent,
                _: &str,
            ) -> Result<(), AttaError> {
                unreachable!()
            }
            async fn get_remote_agent(
                &self,
                _: &str,
            ) -> Result<Option<atta_types::RemoteAgent>, AttaError> {
                unreachable!()
            }
            async fn get_remote_agent_by_token(
                &self,
                _: &str,
            ) -> Result<Option<atta_types::RemoteAgent>, AttaError> {
                unreachable!()
            }
            async fn list_remote_agents(
                &self,
            ) -> Result<Vec<atta_types::RemoteAgent>, AttaError> {
                unreachable!()
            }
            async fn update_remote_agent_status(
                &self,
                _: &str,
                _: &atta_types::RemoteAgentStatus,
            ) -> Result<(), AttaError> {
                unreachable!()
            }
            async fn update_remote_agent_heartbeat(&self, _: &str) -> Result<(), AttaError> {
                unreachable!()
            }
            async fn delete_remote_agent(&self, _: &str) -> Result<(), AttaError> {
                unreachable!()
            }
            async fn rotate_remote_agent_token(&self, _id: &str, _new_token_hash: &str, _expires_at: Option<chrono::DateTime<chrono::Utc>>) -> Result<(), AttaError> {
                unreachable!()
            }
        }

        impl StateStore for PanicStore {}

        Arc::new(PanicStore)
    }

    #[test]
    fn test_register_and_get_flow_def() {
        let engine = make_engine();
        let flow_def = make_simple_flow();

        engine.register_flow_def(flow_def).unwrap();

        let got = engine.get_flow_def("test_flow").unwrap();
        assert_eq!(got.id, "test_flow");
        assert_eq!(got.states.len(), 2);
    }

    #[test]
    fn test_get_flow_def_not_found() {
        let engine = make_engine();

        let result = engine.get_flow_def("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_register_overwrites() {
        let engine = make_engine();
        let flow = make_simple_flow();
        engine.register_flow_def(flow).unwrap();

        // 注册同 ID 不同 version
        let mut flow2 = make_simple_flow();
        flow2.version = "2.0".to_string();
        engine.register_flow_def(flow2).unwrap();

        let got = engine.get_flow_def("test_flow").unwrap();
        assert_eq!(got.version, "2.0");
    }

    #[test]
    fn test_validate_flow_def_valid() {
        let flow = make_simple_flow();
        assert!(FlowEngine::validate_flow_def(&flow).is_ok());
    }

    #[test]
    fn test_validate_flow_def_no_start() {
        let mut states = HashMap::new();
        states.insert(
            "done".to_string(),
            StateDef {
                state_type: StateType::End,
                agent: None,
                skill: None,
                gate: None,
                on_enter: None,
                transitions: vec![],
                branches: None,
                join_strategy: None,
                timeout_secs: None,
            },
        );
        let flow = FlowDef {
            id: "bad".to_string(),
            version: "1.0".to_string(),
            name: None,
            description: None,
            initial_state: "done".to_string(),
            states,
            on_error: None,
            skills: vec![],
            source: "builtin".to_string(),
        };
        let err = FlowEngine::validate_flow_def(&flow).unwrap_err();
        assert!(err.to_string().contains("exactly one Start state"));
    }

    #[test]
    fn test_validate_flow_def_no_end() {
        let mut states = HashMap::new();
        states.insert(
            "init".to_string(),
            StateDef {
                state_type: StateType::Start,
                agent: None,
                skill: None,
                gate: None,
                on_enter: None,
                transitions: vec![],
                branches: None,
                join_strategy: None,
                timeout_secs: None,
            },
        );
        let flow = FlowDef {
            id: "bad".to_string(),
            version: "1.0".to_string(),
            name: None,
            description: None,
            initial_state: "init".to_string(),
            states,
            on_error: None,
            skills: vec![],
            source: "builtin".to_string(),
        };
        let err = FlowEngine::validate_flow_def(&flow).unwrap_err();
        assert!(err.to_string().contains("at least one End state"));
    }

    #[test]
    fn test_validate_flow_def_bad_transition_target() {
        let mut states = HashMap::new();
        states.insert(
            "init".to_string(),
            StateDef {
                state_type: StateType::Start,
                agent: None,
                skill: None,
                gate: None,
                on_enter: None,
                transitions: vec![TransitionDef {
                    to: "nonexistent".to_string(),
                    when: None,
                    auto: Some(true),
                }],
                branches: None,
                join_strategy: None,
                timeout_secs: None,
            },
        );
        states.insert(
            "done".to_string(),
            StateDef {
                state_type: StateType::End,
                agent: None,
                skill: None,
                gate: None,
                on_enter: None,
                transitions: vec![],
                branches: None,
                join_strategy: None,
                timeout_secs: None,
            },
        );
        let flow = FlowDef {
            id: "bad".to_string(),
            version: "1.0".to_string(),
            name: None,
            description: None,
            initial_state: "init".to_string(),
            states,
            on_error: None,
            skills: vec![],
            source: "builtin".to_string(),
        };
        let err = FlowEngine::validate_flow_def(&flow).unwrap_err();
        assert!(err.to_string().contains("not found in flow"));
    }

    #[test]
    fn test_validate_flow_def_gate_without_gate_field() {
        let mut states = HashMap::new();
        states.insert(
            "init".to_string(),
            StateDef {
                state_type: StateType::Start,
                agent: None,
                skill: None,
                gate: None,
                on_enter: None,
                transitions: vec![TransitionDef {
                    to: "approve".to_string(),
                    when: None,
                    auto: Some(true),
                }],
                branches: None,
                join_strategy: None,
                timeout_secs: None,
            },
        );
        states.insert(
            "approve".to_string(),
            StateDef {
                state_type: StateType::Gate,
                agent: None,
                skill: None,
                gate: None, // missing gate field
                on_enter: None,
                transitions: vec![TransitionDef {
                    to: "done".to_string(),
                    when: Some("approved".to_string()),
                    auto: None,
                }],
                branches: None,
                join_strategy: None,
                timeout_secs: None,
            },
        );
        states.insert(
            "done".to_string(),
            StateDef {
                state_type: StateType::End,
                agent: None,
                skill: None,
                gate: None,
                on_enter: None,
                transitions: vec![],
                branches: None,
                join_strategy: None,
                timeout_secs: None,
            },
        );
        let flow = FlowDef {
            id: "bad".to_string(),
            version: "1.0".to_string(),
            name: None,
            description: None,
            initial_state: "init".to_string(),
            states,
            on_error: None,
            skills: vec![],
            source: "builtin".to_string(),
        };
        let err = FlowEngine::validate_flow_def(&flow).unwrap_err();
        assert!(err.to_string().contains("must have a 'gate' field"));
    }

    #[test]
    fn test_validate_flow_def_valid_error_policy() {
        use atta_types::ErrorPolicy;

        let mut states = HashMap::new();
        states.insert(
            "init".to_string(),
            StateDef {
                state_type: StateType::Start,
                agent: None,
                skill: None,
                gate: None,
                on_enter: None,
                transitions: vec![TransitionDef {
                    to: "done".to_string(),
                    when: None,
                    auto: Some(true),
                }],
                branches: None,
                join_strategy: None,
                timeout_secs: None,
            },
        );
        states.insert(
            "done".to_string(),
            StateDef {
                state_type: StateType::End,
                agent: None,
                skill: None,
                gate: None,
                on_enter: None,
                transitions: vec![],
                branches: None,
                join_strategy: None,
                timeout_secs: None,
            },
        );
        let flow = FlowDef {
            id: "good".to_string(),
            version: "1.0".to_string(),
            name: None,
            description: None,
            initial_state: "init".to_string(),
            states,
            on_error: Some(ErrorPolicy {
                max_retries: 3,
                retry_states: vec!["init".to_string()],
                fallback: "done".to_string(),
            }),
            skills: vec![],
            source: "builtin".to_string(),
        };
        assert!(FlowEngine::validate_flow_def(&flow).is_ok());
    }

    #[test]
    fn test_validate_flow_def_invalid_error_policy_fallback() {
        use atta_types::ErrorPolicy;

        let mut states = HashMap::new();
        states.insert(
            "init".to_string(),
            StateDef {
                state_type: StateType::Start,
                agent: None,
                skill: None,
                gate: None,
                on_enter: None,
                transitions: vec![TransitionDef {
                    to: "done".to_string(),
                    when: None,
                    auto: Some(true),
                }],
                branches: None,
                join_strategy: None,
                timeout_secs: None,
            },
        );
        states.insert(
            "done".to_string(),
            StateDef {
                state_type: StateType::End,
                agent: None,
                skill: None,
                gate: None,
                on_enter: None,
                transitions: vec![],
                branches: None,
                join_strategy: None,
                timeout_secs: None,
            },
        );
        let flow = FlowDef {
            id: "bad".to_string(),
            version: "1.0".to_string(),
            name: None,
            description: None,
            initial_state: "init".to_string(),
            states,
            on_error: Some(ErrorPolicy {
                max_retries: 3,
                retry_states: vec![],
                fallback: "nonexistent".to_string(),
            }),
            skills: vec![],
            source: "builtin".to_string(),
        };
        let err = FlowEngine::validate_flow_def(&flow).unwrap_err();
        assert!(err.to_string().contains("error policy fallback state"));
        assert!(err.to_string().contains("not found in flow"));
    }
}
