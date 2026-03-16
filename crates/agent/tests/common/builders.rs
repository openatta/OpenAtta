//! Factory functions for building test agents and contexts

use std::sync::Arc;

use atta_agent::context::ConversationContext;
use atta_agent::llm::LlmProvider;
use atta_agent::react::ReactAgent;
use atta_types::ToolRegistry;

/// Build a ReactAgent with the given provider, registry, and system prompt
pub fn build_agent(
    llm: Arc<dyn LlmProvider>,
    registry: Arc<dyn ToolRegistry>,
    system_prompt: &str,
    user_message: &str,
) -> ReactAgent {
    let tools = registry.list_schemas();
    let mut ctx = ConversationContext::new(128_000);
    ctx.set_system(system_prompt);
    ctx.add_user(user_message);

    ReactAgent::new(llm, registry, ctx, 20).with_tools(tools)
}

/// Build a ReactAgent with default system prompt
pub fn build_agent_default(
    llm: Arc<dyn LlmProvider>,
    registry: Arc<dyn ToolRegistry>,
    user_message: &str,
) -> ReactAgent {
    build_agent(llm, registry, "You are a test agent.", user_message)
}

/// Build a minimal ConversationContext
pub fn build_context(system_prompt: &str, user_message: &str) -> ConversationContext {
    let mut ctx = ConversationContext::new(128_000);
    ctx.set_system(system_prompt);
    ctx.add_user(user_message);
    ctx
}
