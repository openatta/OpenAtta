//! DateTime section — current time and timezone

use super::super::section::{PromptContext, PromptSection};

/// DateTime section — injects current date/time
pub struct DateTimeSection;

impl PromptSection for DateTimeSection {
    fn name(&self) -> &str {
        "Date & Time"
    }

    fn priority(&self) -> u32 {
        80
    }

    fn build(&self, _ctx: &PromptContext) -> Option<String> {
        let now = chrono::Local::now();
        Some(format!(
            "Current time: {} ({})",
            now.format("%Y-%m-%d %H:%M:%S"),
            now.format("%Z")
        ))
    }
}
