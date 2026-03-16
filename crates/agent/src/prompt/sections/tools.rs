//! Tools section — lists available tools with descriptions and parameter schemas

use super::super::section::{PromptContext, PromptSection};

/// Format parameter information from a JSON Schema `parameters` value.
///
/// Extracts `properties` and `required` fields to produce a human-readable
/// parameter list like: `path (string, required), encoding (string, optional)`.
fn format_parameters(params: &serde_json::Value) -> String {
    let properties = match params.get("properties").and_then(|p| p.as_object()) {
        Some(props) => props,
        None => return String::new(),
    };

    let required: Vec<&str> = params
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let mut param_parts: Vec<String> = Vec::new();
    for (name, schema) in properties {
        let type_str = schema.get("type").and_then(|t| t.as_str()).unwrap_or("any");
        let req = if required.contains(&name.as_str()) {
            "required"
        } else {
            "optional"
        };
        param_parts.push(format!("{name} ({type_str}, {req})"));
    }

    if param_parts.is_empty() {
        String::new()
    } else {
        format!("  Parameters: {}", param_parts.join(", "))
    }
}

/// Tools section — lists all available tool names, descriptions, and parameters
pub struct ToolsSection;

impl PromptSection for ToolsSection {
    fn name(&self) -> &str {
        "Available Tools"
    }

    fn priority(&self) -> u32 {
        20
    }

    fn build(&self, ctx: &PromptContext) -> Option<String> {
        if ctx.tools.is_empty() {
            return None;
        }

        let mut lines = vec![format!("You have access to {} tools:", ctx.tools.len())];
        for tool in &ctx.tools {
            lines.push(format!("- **{}**: {}", tool.name, tool.description));
            let params = format_parameters(&tool.parameters);
            if !params.is_empty() {
                lines.push(params);
            }
        }
        lines.push(
            "\nUse the appropriate tool when needed. You can call multiple tools in sequence."
                .to_string(),
        );

        Some(lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::section::PromptContext;
    use atta_types::ToolSchema;

    fn make_tool(name: &str, desc: &str, params: serde_json::Value) -> ToolSchema {
        ToolSchema {
            name: name.to_string(),
            description: desc.to_string(),
            parameters: params,
        }
    }

    #[test]
    fn test_tools_section_shows_parameters() {
        let tool = make_tool(
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

        assert!(output.contains("**file_read**: Read file contents"));
        assert!(output.contains("path (string, required)"));
        assert!(output.contains("encoding (string, optional)"));
    }

    #[test]
    fn test_tools_section_no_parameters() {
        let tool = make_tool("ping", "Ping the server", serde_json::json!({}));

        let ctx = PromptContext {
            tools: vec![tool],
            ..Default::default()
        };

        let section = ToolsSection;
        let output = section.build(&ctx).unwrap();

        assert!(output.contains("**ping**: Ping the server"));
        assert!(!output.contains("Parameters:"));
    }

    #[test]
    fn test_tools_section_empty() {
        let ctx = PromptContext::default();
        let section = ToolsSection;
        assert!(section.build(&ctx).is_none());
    }

    #[test]
    fn test_format_parameters_helper() {
        let params = serde_json::json!({
            "type": "object",
            "properties": {
                "url": { "type": "string" },
                "method": { "type": "string" }
            },
            "required": ["url"]
        });

        let result = format_parameters(&params);
        assert!(result.contains("url (string, required)"));
        assert!(result.contains("method (string, optional)"));
    }
}
