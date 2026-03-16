//! Action rate tracking with sliding window

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Sliding window rate tracker
pub struct ActionTracker {
    /// All calls within the window
    calls: Mutex<VecDeque<Instant>>,
    /// High-risk calls within the window
    high_risk_calls: Mutex<VecDeque<Instant>>,
    /// Window duration
    window: Duration,
}

impl ActionTracker {
    /// Create a new tracker with a 60-second sliding window
    pub fn new() -> Self {
        Self {
            calls: Mutex::new(VecDeque::new()),
            high_risk_calls: Mutex::new(VecDeque::new()),
            window: Duration::from_secs(60),
        }
    }

    /// Record a normal call and return the count within the window
    pub fn record_call(&self) -> u32 {
        let mut calls = self.calls.lock().expect("tracker lock poisoned");
        let now = Instant::now();
        let cutoff = now - self.window;

        // Remove expired entries
        while calls.front().is_some_and(|t| *t < cutoff) {
            calls.pop_front();
        }

        calls.push_back(now);
        calls.len() as u32
    }

    /// Record a high-risk call and return the count within the window
    pub fn record_high_risk_call(&self) -> u32 {
        let mut calls = self
            .high_risk_calls
            .lock()
            .expect("high risk tracker lock poisoned");
        let now = Instant::now();
        let cutoff = now - self.window;

        while calls.front().is_some_and(|t| *t < cutoff) {
            calls.pop_front();
        }

        calls.push_back(now);
        calls.len() as u32
    }

    /// Get current call count within the window
    pub fn current_count(&self) -> u32 {
        let mut calls = self.calls.lock().expect("tracker lock poisoned");
        let cutoff = Instant::now() - self.window;
        while calls.front().is_some_and(|t| *t < cutoff) {
            calls.pop_front();
        }
        calls.len() as u32
    }

    /// Get current high-risk call count within the window
    pub fn current_high_risk_count(&self) -> u32 {
        let mut calls = self
            .high_risk_calls
            .lock()
            .expect("high risk tracker lock poisoned");
        let cutoff = Instant::now() - self.window;
        while calls.front().is_some_and(|t| *t < cutoff) {
            calls.pop_front();
        }
        calls.len() as u32
    }
}

impl Default for ActionTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_call() {
        let tracker = ActionTracker::new();
        assert_eq!(tracker.record_call(), 1);
        assert_eq!(tracker.record_call(), 2);
        assert_eq!(tracker.record_call(), 3);
        assert_eq!(tracker.current_count(), 3);
    }

    #[test]
    fn test_record_high_risk() {
        let tracker = ActionTracker::new();
        assert_eq!(tracker.record_high_risk_call(), 1);
        assert_eq!(tracker.record_high_risk_call(), 2);
        assert_eq!(tracker.current_high_risk_count(), 2);
    }

    #[test]
    fn test_independent_counters() {
        let tracker = ActionTracker::new();
        tracker.record_call();
        tracker.record_call();
        tracker.record_high_risk_call();

        assert_eq!(tracker.current_count(), 2);
        assert_eq!(tracker.current_high_risk_count(), 1);
    }
}
