#![no_main]
#![forbid(unsafe_code)]
//! Fuzz the XML tool-call dispatcher with random text.
//! Verifies XML parsing never panics on arbitrary LLM output.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        use atta_agent::dispatcher::ToolDispatcher;
        // Create an XML dispatcher and try to parse random text as LLM response
        let response = atta_agent::llm::LlmResponse::Message(s.to_string());
        let dispatcher = atta_agent::dispatcher::XmlToolDispatcher::default();
        let _ = dispatcher.parse_response(&response);
    }
});
