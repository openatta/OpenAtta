//! Shared test infrastructure for integration tests
//!
//! Provides mock implementations, builders, and fixtures used across all test groups.
//!
//! Not every test file uses every helper — suppress dead-code warnings for the shared module.
#![allow(dead_code, unused_imports)]

pub mod builders;
pub mod fixtures;
pub mod mock_channel;
pub mod mock_llm;
pub mod mock_registry;
pub mod mock_tools;

/// Check if live tests are enabled via ATTA_LIVE_TEST=1 environment variable
pub fn is_live_test_enabled() -> bool {
    std::env::var("ATTA_LIVE_TEST")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Skip test if live tests are not enabled. Use at the start of live test functions.
#[macro_export]
macro_rules! skip_unless_live {
    () => {
        if !common::is_live_test_enabled() {
            eprintln!("Skipping live test (set ATTA_LIVE_TEST=1 to enable)");
            return;
        }
    };
}
