//! Test data fixtures and constructors

use atta_agent::llm::{LlmResponse, ToolCall};
use atta_types::{SkillDef, ToolSchema};

/// Create a text LlmResponse
pub fn text_response(text: &str) -> LlmResponse {
    LlmResponse::Message(text.to_string())
}

/// Create a tool-calls LlmResponse
pub fn tool_response(calls: Vec<ToolCall>) -> LlmResponse {
    LlmResponse::ToolCalls(calls)
}

/// Create a single ToolCall
pub fn make_tool_call(name: &str, args: serde_json::Value) -> ToolCall {
    ToolCall {
        id: format!("tc_{name}"),
        name: name.to_string(),
        arguments: args,
    }
}

/// Create a ToolSchema with parameters
pub fn make_tool_schema(name: &str, desc: &str, params: serde_json::Value) -> ToolSchema {
    ToolSchema {
        name: name.to_string(),
        description: desc.to_string(),
        parameters: params,
    }
}

/// Create a SkillDef for testing
pub fn make_skill(id: &str) -> SkillDef {
    SkillDef {
        id: id.to_string(),
        version: "1.0".to_string(),
        name: Some(id.to_string()),
        description: Some(format!("{id} description")),
        system_prompt: format!("You are a {id} expert"),
        tools: vec!["web_fetch".to_string(), "file_read".to_string()],
        steps: None,
        output_format: None,
        requires_approval: false,
        risk_level: Default::default(),
        tags: vec![],
        variables: None,
        author: None,
        source: "builtin".to_string(),
    }
}

/// Create a SkillDef with custom tools
pub fn make_skill_with_tools(id: &str, tools: Vec<&str>) -> SkillDef {
    SkillDef {
        tools: tools.into_iter().map(String::from).collect(),
        ..make_skill(id)
    }
}
