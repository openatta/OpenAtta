//! Provider-specific JSON Schema cleaning
//!
//! Different LLM providers support different subsets of JSON Schema.
//! This module strips unsupported keywords before sending tool definitions
//! to the provider API.

use serde_json::Value;

/// Cleaning strategy per provider
pub enum CleaningStrategy {
    /// OpenAI: strip $ref, $defs
    OpenAi,
    /// Anthropic: strip $ref, $defs, definitions, default, examples
    Anthropic,
    /// Gemini: aggressive strip of most advanced JSON Schema features
    Gemini,
    /// Conservative: strip everything not in basic JSON Schema
    Conservative,
}

/// Keys to strip for each strategy
fn keys_for_strategy(strategy: &CleaningStrategy) -> &'static [&'static str] {
    match strategy {
        CleaningStrategy::OpenAi => &["$ref", "$defs"],
        CleaningStrategy::Anthropic => &["$ref", "$defs", "definitions", "default", "examples"],
        CleaningStrategy::Gemini => &[
            "$ref",
            "$defs",
            "$schema",
            "$id",
            "definitions",
            "additionalProperties",
            "patternProperties",
            "allOf",
            "anyOf",
            "oneOf",
            "not",
            "if",
            "then",
            "else",
            "format",
            "examples",
            "default",
            "minLength",
            "maxLength",
            "minimum",
            "maximum",
            "pattern",
            "multipleOf",
            "minItems",
            "maxItems",
            "uniqueItems",
            "minProperties",
            "maxProperties",
        ],
        CleaningStrategy::Conservative => &[
            "$ref",
            "$defs",
            "$schema",
            "$id",
            "definitions",
            "additionalProperties",
            "patternProperties",
            "allOf",
            "anyOf",
            "oneOf",
            "not",
            "if",
            "then",
            "else",
            "format",
            "examples",
            "default",
            "minLength",
            "maxLength",
            "minimum",
            "maximum",
            "pattern",
            "multipleOf",
            "minItems",
            "maxItems",
            "uniqueItems",
            "minProperties",
            "maxProperties",
        ],
    }
}

/// Clean a tool parameter schema by resolving $refs, flattening anyOf enums,
/// and stripping unsupported keys for the given strategy.
pub fn clean_tool_schema(parameters: &Value, strategy: &CleaningStrategy) -> Value {
    let mut result = parameters.clone();

    // Step 1: Collect $defs/definitions and resolve $ref inline
    let defs = collect_defs(&result);
    if !defs.is_null() {
        resolve_refs(&mut result, &defs, 0);
    }

    // Step 2: Flatten anyOf [{const: X}, ...] into enum: [X, ...]
    flatten_anyof_enums(&mut result);

    // Step 3: Strip unsupported keys
    let keys = keys_for_strategy(strategy);
    strip_keys_recursive(&mut result, keys);

    result
}

/// Collect $defs or definitions from a schema root
fn collect_defs(schema: &Value) -> Value {
    if let Some(defs) = schema.get("$defs") {
        return defs.clone();
    }
    if let Some(defs) = schema.get("definitions") {
        return defs.clone();
    }
    Value::Null
}

/// Recursively resolve $ref references by inlining definitions.
/// Max depth prevents infinite recursion from circular references.
const MAX_REF_DEPTH: u32 = 10;

fn resolve_refs(value: &mut Value, defs: &Value, depth: u32) {
    if depth > MAX_REF_DEPTH {
        return;
    }

    match value {
        Value::Object(map) => {
            // If this object is a $ref, replace it with the referenced definition
            if let Some(ref_val) = map.get("$ref").and_then(|v| v.as_str()) {
                if let Some(resolved) = resolve_ref_path(ref_val, defs) {
                    let mut resolved = resolved.clone();
                    resolve_refs(&mut resolved, defs, depth + 1);
                    *value = resolved;
                    return;
                }
            }
            // Otherwise recurse into children
            for v in map.values_mut() {
                resolve_refs(v, defs, depth);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                resolve_refs(v, defs, depth);
            }
        }
        _ => {}
    }
}

/// Resolve a $ref path like "#/$defs/Foo" or "#/definitions/Bar"
fn resolve_ref_path<'a>(ref_path: &str, defs: &'a Value) -> Option<&'a Value> {
    let name = ref_path
        .strip_prefix("#/$defs/")
        .or_else(|| ref_path.strip_prefix("#/definitions/"))?;
    defs.get(name)
}

/// Flatten anyOf patterns where all variants are const or single-value enums
/// into a simple enum array.
fn flatten_anyof_enums(value: &mut Value) {
    match value {
        Value::Object(map) => {
            // Check if anyOf can be flattened to enum
            if let Some(any_of) = map.get("anyOf").and_then(|v| v.as_array()) {
                let mut enum_values = Vec::new();
                let mut can_flatten = true;

                for variant in any_of {
                    if let Some(c) = variant.get("const") {
                        enum_values.push(c.clone());
                    } else if let Some(arr) = variant.get("enum").and_then(|v| v.as_array()) {
                        if arr.len() == 1 {
                            enum_values.push(arr[0].clone());
                        } else {
                            enum_values.extend(arr.iter().cloned());
                        }
                    } else {
                        can_flatten = false;
                        break;
                    }
                }

                if can_flatten && !enum_values.is_empty() {
                    map.remove("anyOf");
                    map.insert("enum".to_string(), Value::Array(enum_values));
                }
            }

            // Recurse into children
            for v in map.values_mut() {
                flatten_anyof_enums(v);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                flatten_anyof_enums(v);
            }
        }
        _ => {}
    }
}

/// Recursively strip specified keys from a JSON value
fn strip_keys_recursive(value: &mut Value, keys: &[&str]) {
    match value {
        Value::Object(map) => {
            for key in keys {
                map.remove(*key);
            }
            for v in map.values_mut() {
                strip_keys_recursive(v, keys);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                strip_keys_recursive(v, keys);
            }
        }
        _ => {}
    }
}

/// Infer a cleaning strategy from a provider name
pub fn strategy_for_provider(provider: &str) -> CleaningStrategy {
    match provider.to_lowercase().as_str() {
        "openai" | "azure" | "vllm" | "deepseek" => CleaningStrategy::OpenAi,
        "anthropic" | "claude" => CleaningStrategy::Anthropic,
        "gemini" | "google" | "vertex" => CleaningStrategy::Gemini,
        _ => CleaningStrategy::Conservative,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_openai_strips_ref() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "$ref": "#/defs/Name" }
            },
            "$defs": { "Name": { "type": "string" } }
        });
        let cleaned = clean_tool_schema(&schema, &CleaningStrategy::OpenAi);
        assert!(cleaned.get("$defs").is_none());
        assert!(cleaned["properties"]["name"].get("$ref").is_none());
        assert_eq!(cleaned["properties"]["name"]["type"], "string");
    }

    #[test]
    fn test_anthropic_strips_defaults() {
        let schema = json!({
            "type": "object",
            "properties": {
                "count": {
                    "type": "integer",
                    "default": 10,
                    "examples": [1, 2, 3]
                }
            },
            "definitions": {}
        });
        let cleaned = clean_tool_schema(&schema, &CleaningStrategy::Anthropic);
        assert!(cleaned.get("definitions").is_none());
        assert!(cleaned["properties"]["count"].get("default").is_none());
        assert!(cleaned["properties"]["count"].get("examples").is_none());
        assert_eq!(cleaned["properties"]["count"]["type"], "integer");
    }

    #[test]
    fn test_gemini_aggressive_strip() {
        let schema = json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "query": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 100,
                    "pattern": "^[a-z]+$",
                    "format": "email"
                }
            },
            "allOf": [],
            "anyOf": [],
            "$schema": "http://json-schema.org/draft-07/schema#"
        });
        let cleaned = clean_tool_schema(&schema, &CleaningStrategy::Gemini);
        assert!(cleaned.get("additionalProperties").is_none());
        assert!(cleaned.get("allOf").is_none());
        assert!(cleaned.get("anyOf").is_none());
        assert!(cleaned.get("$schema").is_none());
        let query = &cleaned["properties"]["query"];
        assert!(query.get("minLength").is_none());
        assert!(query.get("maxLength").is_none());
        assert!(query.get("pattern").is_none());
        assert!(query.get("format").is_none());
        assert_eq!(query["type"], "string");
    }

    #[test]
    fn test_strategy_for_provider() {
        assert!(matches!(
            strategy_for_provider("openai"),
            CleaningStrategy::OpenAi
        ));
        assert!(matches!(
            strategy_for_provider("anthropic"),
            CleaningStrategy::Anthropic
        ));
        assert!(matches!(
            strategy_for_provider("gemini"),
            CleaningStrategy::Gemini
        ));
        assert!(matches!(
            strategy_for_provider("unknown"),
            CleaningStrategy::Conservative
        ));
    }

    #[test]
    fn test_ref_resolution() {
        let schema = json!({
            "type": "object",
            "$defs": {
                "Color": { "type": "string", "description": "A color" }
            },
            "properties": {
                "bg": { "$ref": "#/$defs/Color" },
                "fg": { "$ref": "#/$defs/Color" }
            }
        });
        let cleaned = clean_tool_schema(&schema, &CleaningStrategy::OpenAi);
        // $ref should be resolved to inline definition
        assert_eq!(cleaned["properties"]["bg"]["type"], "string");
        assert_eq!(cleaned["properties"]["bg"]["description"], "A color");
        assert_eq!(cleaned["properties"]["fg"]["type"], "string");
        // $defs should be stripped
        assert!(cleaned.get("$defs").is_none());
    }

    #[test]
    fn test_ref_resolution_with_definitions() {
        let schema = json!({
            "type": "object",
            "definitions": {
                "Size": { "type": "integer", "minimum": 0 }
            },
            "properties": {
                "width": { "$ref": "#/definitions/Size" }
            }
        });
        let cleaned = clean_tool_schema(&schema, &CleaningStrategy::Anthropic);
        assert_eq!(cleaned["properties"]["width"]["type"], "integer");
        assert!(cleaned.get("definitions").is_none());
    }

    #[test]
    fn test_anyof_flatten_const() {
        let schema = json!({
            "type": "object",
            "properties": {
                "action": {
                    "anyOf": [
                        { "const": "read" },
                        { "const": "write" },
                        { "const": "delete" }
                    ]
                }
            }
        });
        let cleaned = clean_tool_schema(&schema, &CleaningStrategy::OpenAi);
        let action = &cleaned["properties"]["action"];
        assert!(action.get("anyOf").is_none());
        let enum_vals = action["enum"].as_array().unwrap();
        assert_eq!(enum_vals.len(), 3);
        assert_eq!(enum_vals[0], "read");
        assert_eq!(enum_vals[1], "write");
        assert_eq!(enum_vals[2], "delete");
    }

    #[test]
    fn test_anyof_flatten_enum() {
        let schema = json!({
            "type": "object",
            "properties": {
                "level": {
                    "anyOf": [
                        { "enum": ["low"] },
                        { "enum": ["medium"] },
                        { "enum": ["high"] }
                    ]
                }
            }
        });
        let cleaned = clean_tool_schema(&schema, &CleaningStrategy::OpenAi);
        let level = &cleaned["properties"]["level"];
        assert!(level.get("anyOf").is_none());
        let enum_vals = level["enum"].as_array().unwrap();
        assert_eq!(enum_vals.len(), 3);
    }

    #[test]
    fn test_anyof_not_flattened_if_complex() {
        // anyOf with non-const/enum variants should NOT be flattened
        let schema = json!({
            "type": "object",
            "properties": {
                "value": {
                    "anyOf": [
                        { "type": "string" },
                        { "type": "integer" }
                    ]
                }
            }
        });
        let cleaned = clean_tool_schema(&schema, &CleaningStrategy::OpenAi);
        // anyOf should remain (not flattened)
        assert!(cleaned["properties"]["value"].get("anyOf").is_some());
    }

    #[test]
    fn test_nested_strip() {
        let schema = json!({
            "type": "object",
            "properties": {
                "nested": {
                    "type": "object",
                    "properties": {
                        "deep": {
                            "type": "string",
                            "$ref": "#/bad",
                            "default": "hello"
                        }
                    }
                }
            }
        });
        let cleaned = clean_tool_schema(&schema, &CleaningStrategy::Anthropic);
        let deep = &cleaned["properties"]["nested"]["properties"]["deep"];
        assert!(deep.get("$ref").is_none());
        assert!(deep.get("default").is_none());
        assert_eq!(deep["type"], "string");
    }
}
