#![no_main]
#![forbid(unsafe_code)]
//! Fuzz the prompt builder with random JSON as tool parameter schemas.
//! Verifies the JSON Schema extractor never panics.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Try to parse as JSON value — skip invalid JSON silently
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(s) {
            let tool = atta_types::ToolSchema {
                name: "fuzz_tool".to_string(),
                description: "fuzz".to_string(),
                parameters: value,
            };
            let ctx = atta_agent::PromptContext {
                tools: vec![tool],
                ..Default::default()
            };
            let builder = atta_agent::SystemPromptBuilder::with_defaults();
            // Must not panic regardless of parameter JSON shape
            let _ = builder.build(&ctx);
        }
    }
});
