//! TG15: Channel dispatch integration tests
//!
//! Tests the full agent-side dispatch flow: prompt building with channel context,
//! ConversationContext management, and ReactAgent execution.

mod common;

use std::sync::Arc;

use atta_agent::context::ConversationContext;
use atta_agent::llm::{LlmProvider, LlmResponse, Message};
use atta_agent::prompt::{PromptContext, SystemPromptBuilder};
use atta_agent::react::ReactAgent;
use atta_types::ToolRegistry;
use common::fixtures::{make_tool_call, make_tool_schema, text_response};
use common::mock_llm::MockLlmProvider;
use common::mock_tools::SimpleRegistry;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a PromptContext with a channel name and optional tools
fn prompt_ctx_with_channel(channel: &str) -> PromptContext {
    PromptContext {
        channel: Some(channel.to_string()),
        ..Default::default()
    }
}

/// Build a ReactAgent from an LLM provider, registry, system prompt, and user message
fn make_agent(
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// 1. SystemPromptBuilder includes channel information when channel is set.
#[test]
fn test_prompt_includes_channel_name() {
    let ctx = prompt_ctx_with_channel("telegram");
    let builder = SystemPromptBuilder::with_defaults();
    let prompt = builder.build(&ctx);

    assert!(
        prompt.contains("telegram"),
        "prompt should contain channel name 'telegram', got:\n{prompt}"
    );
    // The ChannelMediaSection should describe telegram's capabilities
    assert!(
        prompt.contains("Markdown"),
        "telegram prompt should mention Markdown support, got:\n{prompt}"
    );
}

/// 2. Prompt includes both tool descriptions and channel info when both are set.
#[test]
fn test_prompt_with_tools_and_channel() {
    let ctx = PromptContext {
        channel: Some("discord".to_string()),
        tools: vec![
            make_tool_schema(
                "web_search",
                "Search the web",
                serde_json::json!({"type": "object"}),
            ),
            make_tool_schema(
                "file_read",
                "Read a file",
                serde_json::json!({"type": "object"}),
            ),
        ],
        ..Default::default()
    };

    let builder = SystemPromptBuilder::with_defaults();
    let prompt = builder.build(&ctx);

    // Channel info
    assert!(
        prompt.contains("discord"),
        "prompt should contain channel name 'discord'"
    );
    // Tool info
    assert!(
        prompt.contains("web_search"),
        "prompt should mention web_search tool"
    );
    assert!(
        prompt.contains("file_read"),
        "prompt should mention file_read tool"
    );
}

/// 3. Full dispatch simulation: build prompt, create context, create agent, run, get answer.
#[tokio::test]
async fn test_full_dispatch_simulation() {
    let ctx = prompt_ctx_with_channel("terminal");
    let builder = SystemPromptBuilder::with_defaults();
    let system_prompt = builder.build(&ctx);

    let llm =
        Arc::new(MockLlmProvider::text("The capital of France is Paris.")) as Arc<dyn LlmProvider>;
    let registry = Arc::new(SimpleRegistry::empty()) as Arc<dyn ToolRegistry>;

    let mut agent = make_agent(
        llm,
        registry,
        &system_prompt,
        "What is the capital of France?",
    );

    let result = agent.run().await.unwrap();
    let answer = result["answer"].as_str().unwrap();
    assert_eq!(answer, "The capital of France is Paris.");
}

/// 4. Dispatch with tools: agent calls a tool then returns final answer.
#[tokio::test]
async fn test_dispatch_with_tools() {
    let tool_call = make_tool_call("echo", serde_json::json!({"message": "hello"}));
    let llm = Arc::new(MockLlmProvider::new(vec![
        LlmResponse::ToolCalls(vec![tool_call]),
        text_response("Tool returned: hello"),
    ])) as Arc<dyn LlmProvider>;

    let registry = Arc::new(common::mock_registry::echo_registry()) as Arc<dyn ToolRegistry>;

    let mut agent = make_agent(
        llm,
        registry,
        "You are a helpful assistant.",
        "Echo hello for me",
    );

    let result = agent.run().await.unwrap();
    let answer = result["answer"].as_str().unwrap();
    assert_eq!(answer, "Tool returned: hello");
}

/// 5. Dispatch with no tools registered: agent returns direct text response.
#[tokio::test]
async fn test_dispatch_empty_tools() {
    let llm = Arc::new(MockLlmProvider::text(
        "I have no tools available, but here is my answer.",
    )) as Arc<dyn LlmProvider>;
    let registry = Arc::new(SimpleRegistry::empty()) as Arc<dyn ToolRegistry>;

    let mut agent = make_agent(
        llm,
        registry,
        "You are a test assistant.",
        "Do something for me",
    );

    let result = agent.run().await.unwrap();
    let answer = result["answer"].as_str().unwrap();
    assert!(answer.contains("no tools available"));
}

/// 6. Agent returns MaxIterations error when LLM keeps requesting tool calls.
#[tokio::test]
async fn test_dispatch_agent_error_propagates() {
    // Create a provider that always returns tool calls (never a final answer)
    let tool_call = make_tool_call("echo", serde_json::json!({"message": "loop"}));
    let responses: Vec<LlmResponse> = (0..5)
        .map(|_| LlmResponse::ToolCalls(vec![tool_call.clone()]))
        .collect();

    let llm = Arc::new(MockLlmProvider::new(responses)) as Arc<dyn LlmProvider>;
    let registry = Arc::new(common::mock_registry::echo_registry()) as Arc<dyn ToolRegistry>;

    // Set max_iterations to 3 so it stops before exhausting all responses
    let tools = registry.list_schemas();
    let mut ctx = ConversationContext::new(128_000);
    ctx.set_system("You are a test agent.");
    ctx.add_user("Keep calling tools forever");

    let mut agent = ReactAgent::new(llm, registry, ctx, 3).with_tools(tools);

    let err = agent.run().await.unwrap_err();
    assert!(
        matches!(
            err,
            atta_types::AttaError::Agent(atta_types::AgentError::MaxIterations(3))
        ),
        "expected MaxIterations(3), got {err:?}"
    );
}

/// 7. `SystemPromptBuilder::with_defaults().build()` produces a non-empty prompt.
#[test]
fn test_prompt_builder_default_produces_valid_prompt() {
    let builder = SystemPromptBuilder::with_defaults();
    let prompt = builder.build(&PromptContext::default());

    assert!(
        !prompt.is_empty(),
        "default prompt builder should produce non-empty output"
    );
    // Should at least have the Identity and Safety sections
    assert!(
        prompt.contains("Identity"),
        "should contain Identity section"
    );
    assert!(prompt.contains("Safety"), "should contain Safety section");
}

/// 8. System message is first in the ConversationContext messages list.
#[test]
fn test_system_prompt_in_conversation_context() {
    let mut ctx = ConversationContext::new(128_000);
    ctx.add_user("user message first");
    ctx.set_system("I am the system prompt");
    ctx.add_user("another user message");

    let messages = ctx.messages();
    assert!(
        messages.len() >= 3,
        "should have at least 3 messages, got {}",
        messages.len()
    );

    // System is always at position 0 even if set after user messages
    match &messages[0] {
        Message::System(content) => {
            assert_eq!(content, "I am the system prompt");
        }
        other => panic!("expected System message at index 0, got {other:?}"),
    }
}

/// 9. Multiple messages build context in correct order.
#[test]
fn test_multiple_messages_build_context() {
    let mut ctx = ConversationContext::new(128_000);
    ctx.set_system("system");
    ctx.add_user("user-1");
    ctx.add_assistant("assistant-1");
    ctx.add_user("user-2");
    ctx.add_assistant("assistant-2");

    let messages = ctx.messages();
    assert_eq!(messages.len(), 5);

    assert!(matches!(&messages[0], Message::System(s) if s == "system"));
    assert!(matches!(&messages[1], Message::User(s) if s == "user-1"));
    assert!(matches!(&messages[2], Message::Assistant(s) if s == "assistant-1"));
    assert!(matches!(&messages[3], Message::User(s) if s == "user-2"));
    assert!(matches!(&messages[4], Message::Assistant(s) if s == "assistant-2"));
}

/// 10. Live LLM dispatch test (gated by ATTA_LIVE_TEST=1 environment variable).
#[tokio::test]
async fn test_live_llm_dispatch() {
    if !common::is_live_test_enabled() {
        eprintln!("Skipping live test (set ATTA_LIVE_TEST=1 to enable)");
        return;
    }

    // Read configuration from environment
    let api_key =
        std::env::var("ATTA_LIVE_API_KEY").expect("ATTA_LIVE_API_KEY required for live tests");
    let provider = std::env::var("ATTA_LIVE_PROVIDER").unwrap_or_else(|_| "openai".to_string());

    let key_preview_len = 8.min(api_key.len());
    eprintln!(
        "Live test: provider={provider}, key={}...",
        &api_key[..key_preview_len]
    );

    // TODO: Create actual LLM provider and run chat when provider factory is available.
    // For now, verify the env vars are set and the test infrastructure works.
    assert!(!api_key.is_empty(), "ATTA_LIVE_API_KEY should not be empty");
    assert!(
        ["openai", "anthropic"].contains(&provider.as_str()),
        "ATTA_LIVE_PROVIDER should be 'openai' or 'anthropic', got '{provider}'"
    );

    eprintln!("Live test env validation passed. Full LLM test pending provider factory.");
}
