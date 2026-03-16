//! Live integration tests for DeepSeek provider
//!
//! Gated by `ATTA_LIVE_TEST=1`. Requires `DEEPSEEK_API_KEY` in environment.
//! These tests make real API calls and consume token quota.

mod common;

use std::sync::Arc;

use atta_agent::react::{AgentDelta, AgentStreamEvent, ReactAgent};
use atta_agent::{
    ChatOptions, ConversationContext, DeepSeekProvider, LlmProvider, LlmResponse, Message,
    ModelInfo, StreamChunk, ThinkingLevel,
};
use atta_types::{ToolRegistry, ToolSchema};
use tokio::sync::mpsc;

use common::mock_tools::{echo_tool_def, CountingRegistry};

// ════════════════════════════════════════════════════════════════
// Helper: create provider or skip
// ════════════════════════════════════════════════════════════════

fn make_provider() -> DeepSeekProvider {
    DeepSeekProvider::from_env().expect("DeepSeekProvider::from_env failed")
}

// ════════════════════════════════════════════════════════════════
// 1. Simple text chat
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_deepseek_chat_simple() {
    skip_unless_live!();

    let provider = make_provider();
    let info = provider.model_info();
    eprintln!("provider={}, model={}", info.provider, info.model_id);

    let messages = vec![
        Message::System("You are a helpful assistant. Reply in one short sentence.".to_string()),
        Message::User("Say hello.".to_string()),
    ];

    let result = provider.chat(&messages, &[]).await;
    eprintln!("result: {result:?}");

    let response = result.expect("chat() should succeed");
    match response {
        LlmResponse::Message(text) => {
            assert!(!text.is_empty(), "response text should not be empty");
            eprintln!("assistant: {text}");
        }
        LlmResponse::ToolCalls(_) => panic!("expected Message, got ToolCalls"),
    }
}

// ════════════════════════════════════════════════════════════════
// 2. Chat with tool schema — verify ToolCalls
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_deepseek_chat_with_tools() {
    skip_unless_live!();

    let provider = make_provider();

    let tools = vec![ToolSchema {
        name: "get_weather".to_string(),
        description: "Get the current weather for a city".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "city": {
                    "type": "string",
                    "description": "The city name"
                }
            },
            "required": ["city"]
        }),
    }];

    let messages = vec![
        Message::System("You are a helpful assistant. Use the provided tools.".to_string()),
        Message::User("What's the weather in Tokyo?".to_string()),
    ];

    let result = provider.chat(&messages, &tools).await;
    eprintln!("result: {result:?}");

    let response = result.expect("chat() should succeed");
    match response {
        LlmResponse::ToolCalls(calls) => {
            assert!(!calls.is_empty(), "should have at least one tool call");
            let call = &calls[0];
            assert_eq!(call.name, "get_weather");
            assert!(!call.id.is_empty(), "tool call id should not be empty");
            let city = call.arguments.get("city").and_then(|v| v.as_str());
            assert!(city.is_some(), "arguments should contain 'city'");
            eprintln!(
                "tool_call: {} id={} args={}",
                call.name, call.id, call.arguments
            );
        }
        LlmResponse::Message(text) => {
            eprintln!("WARN: got text instead of tool call: {text}");
        }
    }
}

// ════════════════════════════════════════════════════════════════
// 3. Streaming chat — verify TextDelta + Done
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_deepseek_chat_stream() {
    skip_unless_live!();

    use tokio_stream::StreamExt;

    let provider = make_provider();

    let messages = vec![
        Message::System("You are a helpful assistant. Reply in one short sentence.".to_string()),
        Message::User("What is 2 + 2?".to_string()),
    ];

    let stream = provider
        .chat_stream(&messages, &[])
        .await
        .expect("chat_stream() should succeed");

    let mut stream = std::pin::pin!(stream);
    let mut text_deltas = Vec::new();
    let mut got_done = false;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.expect("stream chunk should be Ok");
        match chunk {
            StreamChunk::TextDelta { delta } => {
                text_deltas.push(delta);
            }
            StreamChunk::Done => {
                got_done = true;
                break;
            }
            StreamChunk::ToolCallDelta { .. } => {}
        }
    }

    let full_text: String = text_deltas.concat();
    eprintln!("streamed text: {full_text}");
    eprintln!("chunks: {}", text_deltas.len());

    assert!(!text_deltas.is_empty(), "should have received text deltas");
    assert!(got_done, "should have received Done chunk");
    assert!(
        !full_text.is_empty(),
        "concatenated text should not be empty"
    );
}

// ════════════════════════════════════════════════════════════════
// 4. ReactAgent full ReAct cycle — LLM calls tool, gets result, gives final answer
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_deepseek_react_agent_tool_cycle() {
    skip_unless_live!();

    let provider = make_provider();
    let llm: Arc<dyn LlmProvider> = Arc::new(provider);

    // Register a real echo tool in CountingRegistry
    let registry = Arc::new(CountingRegistry::new(vec![echo_tool_def()]));
    let count = registry.invocation_count.clone();

    let reg: Arc<dyn ToolRegistry> = registry;
    let tools = reg.list_schemas();

    let mut ctx = ConversationContext::new(64_000);
    ctx.set_system(
        "You are a helpful assistant. You have an 'echo' tool that echoes messages back. \
         When the user asks you to echo something, use the echo tool with the message parameter, \
         then report the result to the user. Keep your final answer short.",
    );
    ctx.add_user("Please use the echo tool to echo 'hello world', then tell me what it returned.");

    let mut agent = ReactAgent::new(llm, reg, ctx, 5).with_tools(tools);

    let result = agent.run().await.expect("ReactAgent.run() should succeed");

    eprintln!("result: {}", serde_json::to_string_pretty(&result).unwrap());

    // The echo tool should have been invoked at least once
    let invocations = *count.lock().unwrap();
    eprintln!("echo tool invocations: {invocations}");
    assert!(invocations >= 1, "echo tool should have been invoked");

    // Should have a final answer
    let answer = result["answer"].as_str().expect("should have answer key");
    assert!(!answer.is_empty(), "answer should not be empty");
    eprintln!("final answer: {answer}");
}

// ════════════════════════════════════════════════════════════════
// 5. ReactAgent streaming — full cycle with delta events
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_deepseek_react_agent_streaming() {
    skip_unless_live!();

    let provider = make_provider();
    let llm: Arc<dyn LlmProvider> = Arc::new(provider);

    let registry = Arc::new(CountingRegistry::new(vec![echo_tool_def()]));
    let count = registry.invocation_count.clone();

    let reg: Arc<dyn ToolRegistry> = registry;
    let tools = reg.list_schemas();

    let mut ctx = ConversationContext::new(64_000);
    ctx.set_system(
        "You have an 'echo' tool. Use it to echo the user's message, then give a short answer.",
    );
    ctx.add_user("Echo 'ping' for me.");

    let mut agent = ReactAgent::new(llm, reg, ctx, 5).with_tools(tools);

    let (tx, mut rx) = mpsc::channel::<AgentStreamEvent>(256);

    // Run agent in a spawned task so we can collect events
    let handle = tokio::spawn(async move { agent.run_streaming(tx).await });

    // Collect all events
    let mut events = Vec::new();
    while let Some(evt) = rx.recv().await {
        events.push(evt);
    }

    let result = handle.await.unwrap().expect("run_streaming should succeed");

    eprintln!("result: {}", serde_json::to_string_pretty(&result).unwrap());
    eprintln!("total events: {}", events.len());

    // Should have Thinking events
    let thinking_count = events
        .iter()
        .filter(|e| matches!(e, AgentStreamEvent::Delta(AgentDelta::Thinking { .. })))
        .count();
    eprintln!("Thinking events: {thinking_count}");
    assert!(thinking_count >= 1, "should have at least 1 Thinking event");

    // Should have Done event
    let has_done = events
        .iter()
        .any(|e| matches!(e, AgentStreamEvent::Delta(AgentDelta::Done { .. })));
    assert!(has_done, "should have a Done event");

    // Tool should have been invoked
    let invocations = *count.lock().unwrap();
    eprintln!("echo invocations: {invocations}");
    assert!(invocations >= 1, "echo tool should have been invoked");

    // Should have ToolStart + ToolComplete delta events
    let tool_starts = events
        .iter()
        .filter(|e| matches!(e, AgentStreamEvent::Delta(AgentDelta::ToolStart { .. })))
        .count();
    let tool_completes = events
        .iter()
        .filter(|e| matches!(e, AgentStreamEvent::Delta(AgentDelta::ToolComplete { .. })))
        .count();
    eprintln!("ToolStart={tool_starts}, ToolComplete={tool_completes}");
    assert!(tool_starts >= 1, "should have ToolStart events");
    assert!(tool_completes >= 1, "should have ToolComplete events");
}

// ════════════════════════════════════════════════════════════════
// 6. Multi-turn conversation context — DeepSeek remembers prior turns
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_deepseek_multi_turn_context() {
    skip_unless_live!();

    let provider = make_provider();

    // Turn 1: establish a fact
    let messages_1 = vec![
        Message::System("You are a helpful assistant. Answer concisely.".to_string()),
        Message::User("My name is Zephyr. Remember it.".to_string()),
    ];
    let resp_1 = provider
        .chat(&messages_1, &[])
        .await
        .expect("turn 1 should succeed");
    let text_1 = match resp_1 {
        LlmResponse::Message(t) => t,
        _ => panic!("expected text response"),
    };
    eprintln!("turn 1: {text_1}");

    // Turn 2: ask about the fact with full conversation history
    let messages_2 = vec![
        Message::System("You are a helpful assistant. Answer concisely.".to_string()),
        Message::User("My name is Zephyr. Remember it.".to_string()),
        Message::Assistant(text_1),
        Message::User("What is my name?".to_string()),
    ];
    let resp_2 = provider
        .chat(&messages_2, &[])
        .await
        .expect("turn 2 should succeed");
    let text_2 = match resp_2 {
        LlmResponse::Message(t) => t,
        _ => panic!("expected text response"),
    };
    eprintln!("turn 2: {text_2}");

    // The model should remember the name
    let lower = text_2.to_lowercase();
    assert!(
        lower.contains("zephyr"),
        "model should remember the name 'Zephyr', got: {text_2}"
    );
}

// ════════════════════════════════════════════════════════════════
// 7. chat_with_options — temperature + thinking level
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_deepseek_chat_with_options() {
    skip_unless_live!();

    let provider = make_provider();

    let messages = vec![
        Message::System("You are a math tutor. Be precise and concise.".to_string()),
        Message::User("What is the square root of 144?".to_string()),
    ];

    let options = ChatOptions {
        thinking_level: ThinkingLevel::Low,
        temperature: Some(0.0),
    };

    let result = provider
        .chat_with_options(&messages, &[], &options)
        .await
        .expect("chat_with_options should succeed");

    match result {
        LlmResponse::Message(text) => {
            eprintln!("answer: {text}");
            assert!(
                text.contains("12"),
                "answer should contain '12', got: {text}"
            );
        }
        _ => panic!("expected text response"),
    }
}

// ════════════════════════════════════════════════════════════════
// 8. model_info correctness
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_deepseek_model_info() {
    skip_unless_live!();

    let provider = make_provider();
    let info: ModelInfo = provider.model_info();

    eprintln!("model_info: {:?}", info);

    assert_eq!(info.provider, "deepseek");
    assert_eq!(info.model_id, "deepseek-chat");
    assert_eq!(info.context_window, 64_000);
    assert!(info.supports_tools);
    assert!(info.supports_streaming);
}

// ════════════════════════════════════════════════════════════════
// 9. Schema cleaning strategy — deepseek uses OpenAi strategy
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_deepseek_schema_cleaning() {
    skip_unless_live!();

    use atta_agent::provider::schema::strategy_for_provider;

    let strategy = strategy_for_provider("deepseek");
    assert!(
        matches!(
            strategy,
            atta_agent::provider::schema::CleaningStrategy::OpenAi
        ),
        "deepseek should use OpenAi cleaning strategy"
    );

    // Verify a tool with $ref/$defs is cleaned and still works in a real call
    let provider = make_provider();
    let tools = vec![ToolSchema {
        name: "lookup".to_string(),
        description: "Look up an item by category".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "$defs": {
                "Category": { "type": "string", "enum": ["book", "movie", "music"] }
            },
            "properties": {
                "category": { "$ref": "#/$defs/Category" },
                "query": { "type": "string" }
            },
            "required": ["category", "query"]
        }),
    }];

    let messages = vec![
        Message::System("Use the lookup tool to find items.".to_string()),
        Message::User("Look up a book about Rust programming.".to_string()),
    ];

    let result = provider.chat(&messages, &tools).await;
    eprintln!("schema cleaning result: {result:?}");

    // Should succeed without API error (schema was accepted)
    let response = result.expect("chat with cleaned schema should succeed");
    match response {
        LlmResponse::ToolCalls(calls) => {
            assert!(!calls.is_empty());
            assert_eq!(calls[0].name, "lookup");
            eprintln!("tool call args: {}", calls[0].arguments);
        }
        LlmResponse::Message(text) => {
            eprintln!("WARN: got text instead of tool call: {text}");
        }
    }
}

// ════════════════════════════════════════════════════════════════
// 10. Streaming with tool calls — verify ToolCallDelta chunks
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_deepseek_stream_with_tool_calls() {
    skip_unless_live!();

    use tokio_stream::StreamExt;

    let provider = make_provider();

    let tools = vec![ToolSchema {
        name: "calculate".to_string(),
        description: "Evaluate a math expression and return the result".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "expression": {
                    "type": "string",
                    "description": "The math expression to evaluate"
                }
            },
            "required": ["expression"]
        }),
    }];

    let messages = vec![
        Message::System("You are a math assistant. Always use the calculate tool.".to_string()),
        Message::User("Calculate 17 * 23.".to_string()),
    ];

    let stream = provider
        .chat_stream(&messages, &tools)
        .await
        .expect("chat_stream with tools should succeed");

    let mut stream = std::pin::pin!(stream);
    let mut text_deltas = Vec::new();
    let mut tool_deltas = Vec::new();
    let mut got_done = false;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.expect("chunk should be Ok");
        match chunk {
            StreamChunk::TextDelta { delta } => text_deltas.push(delta),
            StreamChunk::ToolCallDelta {
                index,
                id,
                name,
                arguments_delta,
            } => {
                tool_deltas.push((index, id, name, arguments_delta));
            }
            StreamChunk::Done => {
                got_done = true;
                break;
            }
        }
    }

    eprintln!(
        "text_deltas={}, tool_deltas={}, done={}",
        text_deltas.len(),
        tool_deltas.len(),
        got_done
    );

    assert!(got_done, "should have received Done");

    // Model should either use tool or respond with text — both are valid
    if !tool_deltas.is_empty() {
        // Reconstruct tool call from deltas
        let first_id = tool_deltas.iter().find_map(|(_, id, _, _)| id.clone());
        let first_name = tool_deltas.iter().find_map(|(_, _, name, _)| name.clone());
        let args: String = tool_deltas.iter().map(|(_, _, _, a)| a.as_str()).collect();

        eprintln!("tool id={first_id:?} name={first_name:?} args={args}");
        assert!(
            first_name.as_deref() == Some("calculate"),
            "tool name should be 'calculate', got: {first_name:?}"
        );
        assert!(!args.is_empty(), "tool arguments should not be empty");
    } else {
        let full = text_deltas.concat();
        eprintln!("model responded with text instead of tool: {full}");
    }
}
