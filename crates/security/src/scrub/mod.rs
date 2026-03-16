//! Credential scrubbing and leak detection
//!
//! Filters API keys, tokens, and other secrets from tool outputs
//! before they enter the LLM context.

pub mod detector;
pub mod patterns;

pub use detector::LeakDetector;
pub use patterns::{scrub_json_value, scrub_secret_patterns};
