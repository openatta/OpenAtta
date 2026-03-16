#![no_main]
#![forbid(unsafe_code)]
//! Fuzz PromptGuard::check() with random UTF-8 input.
//! Verifies the regex engine never panics on arbitrary input.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let guard = atta_agent::PromptGuard::default();
        // Must not panic regardless of input
        let _ = guard.check(s);
    }
});
