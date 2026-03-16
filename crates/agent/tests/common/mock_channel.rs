//! Mock channel for integration testing
//!
//! CapturingChannel records all send/draft operations for assertion.

// Note: This module is available for channel-level tests.
// Channel dispatch tests live in crates/channel/tests/ since they depend on atta-channel.
// We keep this here for reference and shared use.

use std::sync::Mutex;

/// Captured outgoing message (simplified for cross-crate use)
#[derive(Debug, Clone)]
pub struct CapturedMessage {
    pub recipient: String,
    pub content: String,
}

/// Records messages that would be sent through a channel
pub struct MessageCapture {
    pub messages: Mutex<Vec<CapturedMessage>>,
    pub typing_starts: Mutex<Vec<String>>,
    pub typing_stops: Mutex<Vec<String>>,
}

impl MessageCapture {
    pub fn new() -> Self {
        Self {
            messages: Mutex::new(Vec::new()),
            typing_starts: Mutex::new(Vec::new()),
            typing_stops: Mutex::new(Vec::new()),
        }
    }

    pub fn sent_messages(&self) -> Vec<CapturedMessage> {
        self.messages.lock().unwrap().clone()
    }

    pub fn sent_count(&self) -> usize {
        self.messages.lock().unwrap().len()
    }

    pub fn record(&self, recipient: &str, content: &str) {
        self.messages.lock().unwrap().push(CapturedMessage {
            recipient: recipient.to_string(),
            content: content.to_string(),
        });
    }
}
