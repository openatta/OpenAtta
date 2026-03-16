//! Research Phase — pre-ReAct fact-gathering
//!
//! Runs an optional "research" mini-agent loop before the main ReAct cycle
//! to gather facts with a restricted system prompt and tool set.

use std::sync::Arc;
use std::time::{Duration, Instant};

use atta_types::{AttaError, ToolRegistry, ToolSchema};
use tracing::info;

use crate::context::ConversationContext;
use crate::llm::{LlmProvider, LlmResponse};
use crate::tool_executor;

/// How to trigger the research phase
#[derive(Debug, Clone)]
pub enum ResearchTrigger {
    /// Never run research
    Never,
    /// Always run research
    Always,
    /// Run if user message contains any of the keywords
    Keywords,
    /// Run if user message exceeds N characters
    Length(usize),
}

impl Default for ResearchTrigger {
    fn default() -> Self {
        Self::Never
    }
}

/// Configuration for the research phase
#[derive(Debug, Clone)]
pub struct ResearchPhaseConfig {
    /// Whether research is enabled
    pub enabled: bool,
    /// Trigger condition
    pub trigger: ResearchTrigger,
    /// Keywords that trigger research (used with `Keywords` trigger)
    pub keywords: Vec<String>,
    /// Maximum iterations for the research mini-agent
    pub max_iterations: u32,
    /// Tool name filter — empty means use all tools
    pub tool_filter: Vec<String>,
}

impl Default for ResearchPhaseConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            trigger: ResearchTrigger::Never,
            keywords: Vec::new(),
            max_iterations: 3,
            tool_filter: Vec::new(),
        }
    }
}

/// Result of the research phase
pub struct ResearchResult {
    /// Collected context to inject into main conversation
    pub context: String,
    /// Number of tool calls made
    pub tool_call_count: u32,
    /// Total duration of the research phase
    pub duration: Duration,
    /// Summaries of tool call results
    pub tool_summaries: Vec<String>,
}

/// Check if the research phase should be triggered
pub fn should_trigger(config: &ResearchPhaseConfig, user_message: &str) -> bool {
    if !config.enabled {
        return false;
    }
    match &config.trigger {
        ResearchTrigger::Never => false,
        ResearchTrigger::Always => true,
        ResearchTrigger::Keywords => config
            .keywords
            .iter()
            .any(|kw| user_message.to_lowercase().contains(&kw.to_lowercase())),
        ResearchTrigger::Length(min_len) => user_message.len() >= *min_len,
    }
}

/// Filter tools based on config
fn filter_tools(all_tools: &[ToolSchema], filter: &[String]) -> Vec<ToolSchema> {
    if filter.is_empty() {
        return all_tools.to_vec();
    }
    all_tools
        .iter()
        .filter(|t| filter.contains(&t.name))
        .cloned()
        .collect()
}

/// Run the research phase mini-agent loop
pub async fn run_research_phase(
    llm: &dyn LlmProvider,
    tool_registry: Arc<dyn ToolRegistry>,
    user_message: &str,
    config: &ResearchPhaseConfig,
    all_tools: &[ToolSchema],
) -> Result<ResearchResult, AttaError> {
    let start = Instant::now();
    let tools = filter_tools(all_tools, &config.tool_filter);

    info!(
        tools = tools.len(),
        max_iterations = config.max_iterations,
        "starting research phase"
    );

    let mut ctx = ConversationContext::new(128_000);
    ctx.set_system(
        "You are a research assistant. Your job is to gather relevant facts \
         and context using the available tools. Do NOT provide a final answer — \
         only collect information that will help answer the user's question.",
    );
    ctx.add_user(user_message);

    let mut tool_call_count: u32 = 0;
    let mut tool_summaries = Vec::new();

    for iteration in 1..=config.max_iterations {
        let response = llm.chat(ctx.messages(), &tools).await?;

        match response {
            LlmResponse::Message(text) => {
                // Research agent returned text — use as context
                info!(iteration, "research phase completed with text response");
                return Ok(ResearchResult {
                    context: text,
                    tool_call_count,
                    duration: start.elapsed(),
                    tool_summaries,
                });
            }
            LlmResponse::ToolCalls(calls) => {
                ctx.add_assistant_tool_calls(calls.clone());
                let results =
                    tool_executor::execute_tools(&calls, Arc::clone(&tool_registry)).await;

                for tr in &results {
                    let result_text = tool_executor::result_to_string(&tr.result);
                    ctx.add_tool_result(&tr.tool_call_id, &result_text);
                    tool_call_count += 1;
                    tool_summaries.push(format!(
                        "{}({}): {}",
                        tr.tool_name,
                        tr.tool_call_id,
                        if result_text.len() > 200 {
                            format!("{}...", &result_text[..200])
                        } else {
                            result_text
                        }
                    ));
                }
            }
        }
    }

    // Exhausted iterations — collect whatever we have
    let context = tool_summaries.join("\n");
    Ok(ResearchResult {
        context,
        tool_call_count,
        duration: start.elapsed(),
        tool_summaries: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trigger_never() {
        let config = ResearchPhaseConfig::default();
        assert!(!should_trigger(&config, "anything"));
    }

    #[test]
    fn test_trigger_always() {
        let config = ResearchPhaseConfig {
            enabled: true,
            trigger: ResearchTrigger::Always,
            ..Default::default()
        };
        assert!(should_trigger(&config, "anything"));
    }

    #[test]
    fn test_trigger_keywords() {
        let config = ResearchPhaseConfig {
            enabled: true,
            trigger: ResearchTrigger::Keywords,
            keywords: vec!["research".to_string(), "analyze".to_string()],
            ..Default::default()
        };
        assert!(should_trigger(&config, "please research this topic"));
        assert!(!should_trigger(&config, "just do it"));
    }

    #[test]
    fn test_trigger_length() {
        let config = ResearchPhaseConfig {
            enabled: true,
            trigger: ResearchTrigger::Length(20),
            ..Default::default()
        };
        assert!(!should_trigger(&config, "short"));
        assert!(should_trigger(
            &config,
            "this is a longer message that exceeds 20 chars"
        ));
    }

    #[test]
    fn test_filter_tools_empty() {
        let tools = vec![ToolSchema {
            name: "search".to_string(),
            description: "Search".to_string(),
            parameters: serde_json::json!({}),
        }];
        let filtered = filter_tools(&tools, &[]);
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_filter_tools_specific() {
        let tools = vec![
            ToolSchema {
                name: "search".to_string(),
                description: "Search".to_string(),
                parameters: serde_json::json!({}),
            },
            ToolSchema {
                name: "exec".to_string(),
                description: "Execute".to_string(),
                parameters: serde_json::json!({}),
            },
        ];
        let filtered = filter_tools(&tools, &["search".to_string()]);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "search");
    }
}
