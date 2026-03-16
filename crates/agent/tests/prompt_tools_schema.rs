//! TG3 — Tool parameter schema display integration tests
//!
//! Verifies that `ToolsSection` correctly formats tool names, descriptions,
//! parameter schemas (required/optional), handles various param types,
//! empty tools, empty properties, and the count header.

mod common;

use atta_agent::prompt::sections::ToolsSection;
use atta_agent::prompt::{PromptContext, PromptSection};
use common::fixtures::make_tool_schema;

// ---------------------------------------------------------------------------
// 1. Tool with required and optional parameters displayed correctly
// ---------------------------------------------------------------------------
#[test]
fn test_tool_with_required_and_optional_params() {
    let tool = make_tool_schema(
        "file_read",
        "Read file contents",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "encoding": { "type": "string" }
            },
            "required": ["path"]
        }),
    );

    let ctx = PromptContext {
        tools: vec![tool],
        ..Default::default()
    };

    let section = ToolsSection;
    let output = section.build(&ctx).unwrap();

    assert!(
        output.contains("path (string, required)"),
        "Required param 'path' must be marked as required. Output:\n{output}"
    );
    assert!(
        output.contains("encoding (string, optional)"),
        "Non-required param 'encoding' must be marked as optional. Output:\n{output}"
    );
}

// ---------------------------------------------------------------------------
// 2. Tool without parameters — no "Parameters:" line
// ---------------------------------------------------------------------------
#[test]
fn test_tool_without_parameters() {
    let tool = make_tool_schema("ping", "Ping the server", serde_json::json!({}));

    let ctx = PromptContext {
        tools: vec![tool],
        ..Default::default()
    };

    let section = ToolsSection;
    let output = section.build(&ctx).unwrap();

    assert!(
        output.contains("**ping**: Ping the server"),
        "Tool name and description must appear"
    );
    assert!(
        !output.contains("Parameters:"),
        "Tool with no parameters schema should not have a Parameters line"
    );
}

// ---------------------------------------------------------------------------
// 3. Tool with empty properties object — no Parameters line
// ---------------------------------------------------------------------------
#[test]
fn test_tool_with_empty_properties() {
    let tool = make_tool_schema(
        "noop",
        "Does nothing",
        serde_json::json!({
            "type": "object",
            "properties": {}
        }),
    );

    let ctx = PromptContext {
        tools: vec![tool],
        ..Default::default()
    };

    let section = ToolsSection;
    let output = section.build(&ctx).unwrap();

    assert!(output.contains("**noop**: Does nothing"));
    assert!(
        !output.contains("Parameters:"),
        "Empty properties should not produce a Parameters line"
    );
}

// ---------------------------------------------------------------------------
// 4. Multiple tools — all names, descriptions, and params shown
// ---------------------------------------------------------------------------
#[test]
fn test_multiple_tools_all_shown() {
    let tools = vec![
        make_tool_schema(
            "file_read",
            "Read file",
            serde_json::json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }),
        ),
        make_tool_schema(
            "web_fetch",
            "Fetch URL",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string" },
                    "method": { "type": "string" }
                },
                "required": ["url"]
            }),
        ),
        make_tool_schema("ping", "Ping server", serde_json::json!({})),
    ];

    let ctx = PromptContext {
        tools,
        ..Default::default()
    };

    let section = ToolsSection;
    let output = section.build(&ctx).unwrap();

    // All three tools present
    assert!(output.contains("**file_read**: Read file"));
    assert!(output.contains("**web_fetch**: Fetch URL"));
    assert!(output.contains("**ping**: Ping server"));

    // Parameters for the tools that have them
    assert!(output.contains("path (string, required)"));
    assert!(output.contains("url (string, required)"));
    assert!(output.contains("method (string, optional)"));
}

// ---------------------------------------------------------------------------
// 5. Various parameter types — string, integer, boolean, array displayed
// ---------------------------------------------------------------------------
#[test]
fn test_various_param_types() {
    let tool = make_tool_schema(
        "multi_type",
        "Tool with various types",
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "count": { "type": "integer" },
                "enabled": { "type": "boolean" },
                "items": { "type": "array" }
            },
            "required": ["name", "count"]
        }),
    );

    let ctx = PromptContext {
        tools: vec![tool],
        ..Default::default()
    };

    let section = ToolsSection;
    let output = section.build(&ctx).unwrap();

    assert!(
        output.contains("name (string, required)"),
        "string type must be displayed"
    );
    assert!(
        output.contains("count (integer, required)"),
        "integer type must be displayed"
    );
    assert!(
        output.contains("enabled (boolean, optional)"),
        "boolean type must be displayed"
    );
    assert!(
        output.contains("items (array, optional)"),
        "array type must be displayed"
    );
}

// ---------------------------------------------------------------------------
// 6. Tool count header — "You have access to N tools:"
// ---------------------------------------------------------------------------
#[test]
fn test_tool_count_header() {
    let tools = vec![
        make_tool_schema("tool_a", "A", serde_json::json!({})),
        make_tool_schema("tool_b", "B", serde_json::json!({})),
        make_tool_schema("tool_c", "C", serde_json::json!({})),
        make_tool_schema("tool_d", "D", serde_json::json!({})),
    ];

    let ctx = PromptContext {
        tools,
        ..Default::default()
    };

    let section = ToolsSection;
    let output = section.build(&ctx).unwrap();

    assert!(
        output.contains("You have access to 4 tools:"),
        "Header must report the correct tool count"
    );
}

// ---------------------------------------------------------------------------
// 7. Empty tools returns None (section skipped)
// ---------------------------------------------------------------------------
#[test]
fn test_empty_tools_returns_none() {
    let ctx = PromptContext {
        tools: vec![],
        ..Default::default()
    };

    let section = ToolsSection;
    let result = section.build(&ctx);

    assert!(
        result.is_none(),
        "ToolsSection must return None when there are no tools"
    );
}

// ---------------------------------------------------------------------------
// 8. Parameters format string — validate exact format "  Parameters: ..."
// ---------------------------------------------------------------------------
#[test]
fn test_parameters_format_string() {
    let tool = make_tool_schema(
        "exec",
        "Execute command",
        serde_json::json!({
            "type": "object",
            "properties": {
                "cmd": { "type": "string" },
                "timeout": { "type": "integer" }
            },
            "required": ["cmd"]
        }),
    );

    let ctx = PromptContext {
        tools: vec![tool],
        ..Default::default()
    };

    let section = ToolsSection;
    let output = section.build(&ctx).unwrap();

    // The parameters line should start with exactly two spaces and "Parameters: "
    let has_correct_format = output
        .lines()
        .any(|line| line.starts_with("  Parameters: ") && line.contains("cmd (string, required)"));

    assert!(
        has_correct_format,
        "Parameters line must be indented with 2 spaces and start with 'Parameters: '. Output:\n{output}"
    );
}
