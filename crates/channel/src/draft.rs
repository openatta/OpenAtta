//! Draft message manager with rate limiting and UTF-8 safe truncation

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Manages draft message accumulation with rate limiting
///
/// Ensures that update calls to messaging platforms respect rate limits
/// by accumulating text and only flushing when the minimum interval has elapsed.
pub struct DraftManager {
    /// Minimum interval between updates
    min_interval: Duration,
    /// Timestamp of the last flush
    last_update: Mutex<Option<Instant>>,
    /// Pending text not yet flushed
    pending_text: Mutex<String>,
}

impl DraftManager {
    /// Create a new DraftManager with the given minimum update interval
    pub fn new(min_interval: Duration) -> Self {
        Self {
            min_interval,
            last_update: Mutex::new(None),
            pending_text: Mutex::new(String::new()),
        }
    }

    /// Accumulate text and return the full pending content if enough time has elapsed.
    ///
    /// Returns `Some(text)` when the accumulated text should be flushed to the platform,
    /// or `None` if the rate limit hasn't elapsed yet.
    pub fn accumulate(&self, text: &str) -> Option<String> {
        let mut pending = self.pending_text.lock().unwrap();
        pending.push_str(text);

        let mut last = self.last_update.lock().unwrap();
        let now = Instant::now();

        let should_flush = match *last {
            None => true,
            Some(prev) => now.duration_since(prev) >= self.min_interval,
        };

        if should_flush {
            *last = Some(now);
            let content = pending.clone();
            // Don't clear pending -- we send the full accumulated text each time
            Some(content)
        } else {
            None
        }
    }

    /// Force flush all pending text regardless of rate limit
    pub fn flush(&self) -> String {
        let pending = self.pending_text.lock().unwrap();
        let mut last = self.last_update.lock().unwrap();
        *last = Some(Instant::now());
        pending.clone()
    }

    /// Get current accumulated text without flushing
    pub fn peek(&self) -> String {
        self.pending_text.lock().unwrap().clone()
    }

    /// Reset the manager (clear pending text and timer)
    pub fn reset(&self) {
        *self.pending_text.lock().unwrap() = String::new();
        *self.last_update.lock().unwrap() = None;
    }
}

/// UTF-8 safe truncation -- truncates to at most `max_bytes` bytes
/// without splitting a multi-byte character.
pub fn utf8_truncate(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }

    // Find the largest byte index <= max_bytes that is a char boundary
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_accumulate_first_call_flushes() {
        let mgr = DraftManager::new(Duration::from_millis(500));
        let result = mgr.accumulate("hello");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "hello");
    }

    #[test]
    fn test_accumulate_rate_limited() {
        let mgr = DraftManager::new(Duration::from_secs(60)); // very long interval
        let _ = mgr.accumulate("first");
        let result = mgr.accumulate(" second");
        // Should be rate limited
        assert!(result.is_none());
    }

    #[test]
    fn test_flush_always_returns() {
        let mgr = DraftManager::new(Duration::from_secs(60));
        let _ = mgr.accumulate("hello");
        mgr.accumulate(" world");
        let result = mgr.flush();
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_utf8_truncate_ascii() {
        assert_eq!(utf8_truncate("hello world", 5), "hello");
    }

    #[test]
    fn test_utf8_truncate_multibyte() {
        let s = "你好世界"; // Each char is 3 bytes, total 12 bytes
        assert_eq!(utf8_truncate(s, 6), "你好"); // 2 chars = 6 bytes
        assert_eq!(utf8_truncate(s, 7), "你好"); // Can't split third char
        assert_eq!(utf8_truncate(s, 9), "你好世"); // 3 chars = 9 bytes
    }

    #[test]
    fn test_utf8_truncate_no_op() {
        assert_eq!(utf8_truncate("hi", 10), "hi");
    }

    #[test]
    fn test_utf8_truncate_empty() {
        assert_eq!(utf8_truncate("", 5), "");
    }

    #[test]
    fn test_reset_clears_state() {
        let mgr = DraftManager::new(Duration::from_millis(100));
        mgr.accumulate("hello");
        mgr.reset();
        assert_eq!(mgr.peek(), "");
    }
}
