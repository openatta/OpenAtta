//! ReAct Agent 执行引擎
//!
//! 实现 Observe -> Think -> Act -> Observe 循环。

use std::sync::Arc;

use atta_types::{AttaError, TokenUsage, ToolRegistry, ToolSchema};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::context::ConversationContext;
use crate::dispatcher::{DispatchResult, ToolDispatcher};
use crate::llm::{ChatOptions, LlmProvider, LlmResponse, StreamChunk, ThinkingLevel, ToolCall};
use crate::research::{self, ResearchPhaseConfig};
use crate::tool_executor::{self, LoopDetector, ToolExecutionConfig};

/// Callback for recording LLM token usage after each API call.
///
/// Implementations receive the model name and token usage. The callback
/// is called from async context but is synchronous to keep it simple —
/// implementations can spawn tasks internally if needed.
pub type UsageCallback = Box<dyn Fn(&str, &TokenUsage) + Send + Sync>;

/// 应用层 delta 事件（UI 消费用）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentDelta {
    /// Agent 正在思考（每次 LLM 调用前发送）
    Thinking { iteration: u32 },
    /// 文本分块（~80 字符）
    TextChunk { text: String },
    /// 工具开始执行
    ToolStart { tool_name: String, call_id: String },
    /// 工具执行完成
    ToolComplete {
        tool_name: String,
        call_id: String,
        duration_ms: u64,
    },
    /// 工具执行失败
    ToolError {
        tool_name: String,
        call_id: String,
        error: String,
    },
    /// 工具等待审批
    ApprovalPending {
        tool_name: String,
        call_id: String,
        risk_level: String,
    },
    /// 工具审批通过
    ApprovalGranted { tool_name: String, call_id: String },
    /// 执行完成
    Done { iterations: u32 },
    /// 清除进度指示
    ClearProgress,
}

/// Agent 流式事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentStreamEvent {
    /// LLM 流式 token 增量
    StreamChunk(StreamChunk),
    /// 应用层 delta 事件
    Delta(AgentDelta),
}

/// ReAct Agent
///
/// 基于 ReAct（Reasoning + Acting）范式的 Agent 执行器。
/// 每次迭代执行 Observe -> Think -> Act 循环，直到 LLM 输出最终回答
/// 或达到最大迭代次数。
pub struct ReactAgent {
    /// LLM 提供者
    llm: Arc<dyn LlmProvider>,
    /// Tool 注册表
    tool_registry: Arc<dyn ToolRegistry>,
    /// 对话上下文
    context: ConversationContext,
    /// 最大迭代次数
    max_iterations: u32,
    /// 可用的 Tool Schema 列表
    tools: Vec<ToolSchema>,
    /// 思考深度
    thinking_level: ThinkingLevel,
    /// 研究阶段配置
    research_config: Option<ResearchPhaseConfig>,
    /// Tool call dispatcher (native vs XML)
    dispatcher: Option<Arc<dyn ToolDispatcher>>,
    /// Tool execution configuration (timeout, cancellation, hooks)
    tool_exec_config: ToolExecutionConfig,
    /// Loop detector for cycle prevention across ReAct iterations
    loop_detector: LoopDetector,
    /// Optional callback for recording LLM token usage
    usage_callback: Option<UsageCallback>,
}

impl ReactAgent {
    /// 创建新的 ReactAgent
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        tool_registry: Arc<dyn ToolRegistry>,
        context: ConversationContext,
        max_iterations: u32,
    ) -> Self {
        Self {
            llm,
            tool_registry,
            context,
            max_iterations,
            tools: Vec::new(),
            thinking_level: ThinkingLevel::default(),
            research_config: None,
            dispatcher: None,
            tool_exec_config: ToolExecutionConfig::default(),
            loop_detector: LoopDetector::new(3),
            usage_callback: None,
        }
    }

    /// 注册可用的 Tool
    pub fn with_tools(mut self, tools: Vec<ToolSchema>) -> Self {
        self.tools = tools;
        self
    }

    /// 设置思考深度
    pub fn with_thinking_level(mut self, level: ThinkingLevel) -> Self {
        self.thinking_level = level;
        self
    }

    /// 设置研究阶段配置
    pub fn with_research(mut self, config: ResearchPhaseConfig) -> Self {
        self.research_config = Some(config);
        self
    }

    /// 设置 Tool Dispatcher（Native vs XML）
    pub fn with_dispatcher(mut self, dispatcher: Arc<dyn ToolDispatcher>) -> Self {
        self.dispatcher = Some(dispatcher);
        self
    }

    /// 设置 usage 回调（每次 LLM 调用后触发）
    pub fn with_usage_callback(mut self, cb: UsageCallback) -> Self {
        self.usage_callback = Some(cb);
        self
    }

    /// 设置工具执行配置（超时、取消、Hook 等）
    pub fn with_tool_exec_config(mut self, config: ToolExecutionConfig) -> Self {
        if config.max_repeated_calls > 0 {
            self.loop_detector = LoopDetector::new(config.max_repeated_calls);
        }
        self.tool_exec_config = config;
        self
    }

    /// Dispatch an LLM response through the configured dispatcher
    fn dispatch_response(&self, response: &LlmResponse) -> DispatchResult {
        match &self.dispatcher {
            Some(d) => d.parse_response(response),
            None => match response {
                LlmResponse::Message(text) => DispatchResult::FinalAnswer(text.clone()),
                LlmResponse::ToolCalls(calls) => DispatchResult::ToolCalls(calls.clone()),
            },
        }
    }

    /// Get the tool schemas to send to the LLM (empty for XML dispatcher)
    fn tools_for_llm(&self) -> Vec<ToolSchema> {
        match &self.dispatcher {
            Some(d) if !d.should_send_tool_specs() => vec![],
            _ => self.tools.clone(),
        }
    }

    /// 如果配置了研究阶段，在主循环前运行
    async fn maybe_run_research(&mut self) -> Result<(), AttaError> {
        let config = match &self.research_config {
            Some(c) => c.clone(),
            None => return Ok(()),
        };

        let user_message = self.context.last_user_message().unwrap_or_default();
        if !research::should_trigger(&config, &user_message) {
            return Ok(());
        }

        info!("running research phase before main ReAct loop");
        let result = research::run_research_phase(
            self.llm.as_ref(),
            Arc::clone(&self.tool_registry),
            &user_message,
            &config,
            &self.tools,
        )
        .await?;

        if !result.context.is_empty() {
            // Inject research context into conversation
            let research_msg = format!("[Research context]\n{}", result.context);
            self.context.add_user(&research_msg);
        }

        info!(
            tool_calls = result.tool_call_count,
            duration_ms = result.duration.as_millis() as u64,
            "research phase completed"
        );

        Ok(())
    }

    /// 执行 ReAct 循环
    ///
    /// Observe -> Think -> Act -> Observe ... 直到 LLM 输出最终文本回答
    /// 或达到最大迭代次数。
    pub async fn run(&mut self) -> Result<serde_json::Value, AttaError> {
        // Optional research phase
        self.maybe_run_research().await?;

        info!(
            max_iterations = self.max_iterations,
            tools = self.tools.len(),
            "ReAct loop starting"
        );

        let options = ChatOptions {
            thinking_level: self.thinking_level.clone(),
            temperature: None,
        };
        let tools_for_llm = self.tools_for_llm();

        for iteration in 1..=self.max_iterations {
            info!(iteration, "ReAct iteration begin — Think phase");

            // Truncate context to fit within token budget before LLM call
            self.context.truncate_to_fit();

            // Think: 调用 LLM (with usage tracking)
            let llm_result = self
                .llm
                .chat_with_usage(self.context.messages(), &tools_for_llm, &options)
                .await?;

            // Record usage if callback is set
            if let (Some(cb), Some(usage)) = (&self.usage_callback, &llm_result.usage) {
                let model_id = self.llm.model_info().model_id.clone();
                debug!(model = %model_id, input = usage.input_tokens, output = usage.output_tokens, "LLM usage recorded");
                cb(&model_id, usage);
            }

            let response = llm_result.response;

            // Dispatch through configured dispatcher (native or XML)
            let dispatch_result = self.dispatch_response(&response);

            match dispatch_result {
                DispatchResult::FinalAnswer(text) => {
                    // LLM 返回最终回答，循环结束
                    info!(
                        iteration,
                        "ReAct loop completed — LLM returned final answer"
                    );
                    self.context.add_assistant(&text);
                    return Ok(serde_json::json!({ "answer": text }));
                }
                DispatchResult::ToolCalls(tool_calls) => {
                    // Act: 执行 tool 调用
                    info!(
                        iteration,
                        tool_count = tool_calls.len(),
                        "ReAct Act phase — executing tool calls"
                    );

                    // 记录 assistant 的 tool call 请求到上下文
                    self.context.add_assistant_tool_calls(tool_calls.clone());

                    let results = tool_executor::execute_tools_configured(
                        &tool_calls,
                        Arc::clone(&self.tool_registry),
                        &self.tool_exec_config,
                        Some(&mut self.loop_detector),
                    )
                    .await;

                    for tr in &results {
                        let result_text = tool_executor::result_to_string(&tr.result);
                        // Observe: 将 tool 结果加入上下文
                        self.context.add_tool_result(&tr.tool_call_id, &result_text);
                    }
                }
            }
        }

        warn!(
            max_iterations = self.max_iterations,
            "ReAct loop exhausted max iterations"
        );

        Err(AttaError::Agent(atta_types::AgentError::MaxIterations(
            self.max_iterations,
        )))
    }

    /// 流式执行 ReAct 循环
    ///
    /// 与 `run()` 逻辑相同，但使用 `chat_stream()` 并通过 `delta_tx`
    /// 发送实时进度事件。
    pub async fn run_streaming(
        &mut self,
        delta_tx: mpsc::Sender<AgentStreamEvent>,
    ) -> Result<serde_json::Value, AttaError> {
        // Optional research phase
        self.maybe_run_research().await?;

        info!(
            max_iterations = self.max_iterations,
            tools = self.tools.len(),
            "ReAct streaming loop starting"
        );

        let tools_for_llm = self.tools_for_llm();

        for iteration in 1..=self.max_iterations {
            // Emit thinking delta
            let _ = delta_tx
                .send(AgentStreamEvent::Delta(AgentDelta::Thinking { iteration }))
                .await;

            info!(iteration, "ReAct iteration begin — streaming Think phase");

            // Truncate context to fit within token budget before LLM call
            self.context.truncate_to_fit();

            // Think: 流式调用 LLM (with options for thinking level)
            let options = ChatOptions {
                thinking_level: self.thinking_level.clone(),
                temperature: None,
            };
            let mut stream = self
                .llm
                .chat_stream_with_options(self.context.messages(), &tools_for_llm, &options)
                .await?;

            // Collect stream into LlmResponse
            let mut text_buf = String::new();
            let mut tool_calls: Vec<ToolCallBuilder> = Vec::new();

            use futures::StreamExt;
            while let Some(chunk_result) = stream.next().await {
                let chunk = chunk_result?;

                // Forward raw stream chunk
                let _ = delta_tx
                    .send(AgentStreamEvent::StreamChunk(chunk.clone()))
                    .await;

                match chunk {
                    StreamChunk::TextDelta { delta } => {
                        text_buf.push_str(&delta);
                    }
                    StreamChunk::ToolCallDelta {
                        index,
                        id,
                        name,
                        arguments_delta,
                    } => {
                        // Ensure we have enough slots
                        while tool_calls.len() <= index {
                            tool_calls.push(ToolCallBuilder::default());
                        }
                        if let Some(id) = id {
                            tool_calls[index].id = id;
                        }
                        if let Some(name) = name {
                            tool_calls[index].name = name;
                        }
                        tool_calls[index].arguments.push_str(&arguments_delta);
                    }
                    StreamChunk::Done => {
                        break;
                    }
                }
            }

            // Reconstruct LlmResponse from collected chunks, then dispatch
            let collected_response = if !tool_calls.is_empty() {
                let calls: Vec<ToolCall> = tool_calls.into_iter().map(|b| b.build()).collect();
                LlmResponse::ToolCalls(calls)
            } else {
                LlmResponse::Message(text_buf.clone())
            };
            let dispatch_result = self.dispatch_response(&collected_response);

            match dispatch_result {
                DispatchResult::ToolCalls(calls) => {
                    info!(
                        iteration,
                        tool_count = calls.len(),
                        "ReAct streaming Act phase — executing tool calls"
                    );

                    self.context.add_assistant_tool_calls(calls.clone());

                    // Execute tools with delta events via unified executor
                    let results = tool_executor::execute_tools_with_deltas(
                        &calls,
                        Arc::clone(&self.tool_registry),
                        &self.tool_exec_config,
                        &delta_tx,
                        Some(&mut self.loop_detector),
                    )
                    .await;

                    for tr in &results {
                        let result_text = tool_executor::result_to_string(&tr.result);
                        self.context.add_tool_result(&tr.tool_call_id, &result_text);
                    }
                }
                DispatchResult::FinalAnswer(final_text) => {
                    // Final text answer
                    info!(iteration, "ReAct streaming loop completed — final answer");
                    self.context.add_assistant(&final_text);

                    // Emit clear + text chunks + done
                    let _ = delta_tx
                        .send(AgentStreamEvent::Delta(AgentDelta::ClearProgress))
                        .await;

                    // Split into ~80 char chunks
                    for chunk in final_text.as_bytes().chunks(80) {
                        let text = String::from_utf8_lossy(chunk).to_string();
                        let _ = delta_tx
                            .send(AgentStreamEvent::Delta(AgentDelta::TextChunk { text }))
                            .await;
                    }

                    let _ = delta_tx
                        .send(AgentStreamEvent::Delta(AgentDelta::Done {
                            iterations: iteration,
                        }))
                        .await;

                    return Ok(serde_json::json!({ "answer": final_text }));
                }
            }
        }

        warn!(
            max_iterations = self.max_iterations,
            "ReAct streaming loop exhausted max iterations"
        );

        Err(AttaError::Agent(atta_types::AgentError::MaxIterations(
            self.max_iterations,
        )))
    }
}

/// Helper to build a ToolCall from streaming deltas
#[derive(Default)]
struct ToolCallBuilder {
    id: String,
    name: String,
    arguments: String,
}

impl ToolCallBuilder {
    fn build(self) -> ToolCall {
        let arguments = serde_json::from_str(&self.arguments).unwrap_or_else(|e| {
            tracing::warn!(error = %e, raw = %self.arguments, "failed to parse tool call arguments, defaulting to empty");
            serde_json::json!({})
        });
        ToolCall {
            id: self.id,
            name: self.name,
            arguments,
        }
    }
}
