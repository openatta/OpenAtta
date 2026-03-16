//! Shared SSE (Server-Sent Events) line parser
//!
//! Both OpenAI and Anthropic use SSE for streaming. This module provides
//! common parsing utilities for `data: {...}` lines.

use atta_types::AttaError;

/// Parse a single SSE line and return the JSON data if present.
///
/// Returns:
/// - `Ok(Some(value))` — parsed a `data: {...}` line
/// - `Ok(None)` — line is empty, a comment, or event/retry field (skip)
/// - `Err(_)` — JSON parse failure
pub fn parse_sse_line(line: &str) -> Result<Option<serde_json::Value>, AttaError> {
    let line = line.trim();

    // Empty line (event boundary) or comment
    if line.is_empty() || line.starts_with(':') {
        return Ok(None);
    }

    // data: [DONE] sentinel (OpenAI)
    if line == "data: [DONE]" {
        return Ok(None);
    }

    // data: {...} — the actual payload
    if let Some(data) = line.strip_prefix("data: ") {
        let data = data.trim();
        if data.is_empty() {
            return Ok(None);
        }
        let value: serde_json::Value = serde_json::from_str(data).map_err(|e| {
            AttaError::Llm(atta_types::LlmError::InvalidResponse(format!(
                "SSE JSON parse error: {e}"
            )))
        })?;
        return Ok(Some(value));
    }

    // event: / retry: / id: fields — skip
    Ok(None)
}

/// Check if an SSE event signals end of stream.
///
/// - OpenAI: `data: [DONE]`
/// - Anthropic: `event: message_stop`
pub fn is_stream_done(line: &str) -> bool {
    let line = line.trim();
    line == "data: [DONE]" || line == "event: message_stop"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_data_line() {
        let line = r#"data: {"type":"content_block_delta","delta":{"text":"hello"}}"#;
        let result = parse_sse_line(line).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap()["delta"]["text"], "hello");
    }

    #[test]
    fn test_parse_empty_line() {
        assert!(parse_sse_line("").unwrap().is_none());
        assert!(parse_sse_line("  ").unwrap().is_none());
    }

    #[test]
    fn test_parse_comment() {
        assert!(parse_sse_line(": comment").unwrap().is_none());
    }

    #[test]
    fn test_parse_done() {
        assert!(parse_sse_line("data: [DONE]").unwrap().is_none());
    }

    #[test]
    fn test_parse_event_field() {
        assert!(parse_sse_line("event: message_start").unwrap().is_none());
    }

    #[test]
    fn test_is_done_openai() {
        assert!(is_stream_done("data: [DONE]"));
    }

    #[test]
    fn test_is_done_anthropic() {
        assert!(is_stream_done("event: message_stop"));
    }

    #[test]
    fn test_is_not_done() {
        assert!(!is_stream_done("data: {}"));
    }
}
