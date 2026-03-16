//! Regex-based leak detector for classified secret patterns

use regex::Regex;

/// Compiled regex patterns for detecting various secret types
pub struct LeakDetector {
    patterns: Vec<(String, Regex)>,
}

impl LeakDetector {
    /// Create a new LeakDetector with pre-compiled patterns
    pub fn new() -> Self {
        let patterns = vec![
            // API keys (generic: long alphanumeric strings in key= or token= context)
            (
                "generic_api_key".to_string(),
                Regex::new(
                    r#"(?i)(api[_-]?key|api[_-]?secret|access[_-]?token)\s*[=:]\s*["']?([a-zA-Z0-9_\-]{20,})["']?"#,
                )
                .unwrap(),
            ),
            // AWS secret key
            (
                "aws_secret".to_string(),
                Regex::new(
                    r#"(?i)(aws[_-]?secret[_-]?access[_-]?key)\s*[=:]\s*["']?([a-zA-Z0-9/+=]{40})["']?"#,
                )
                .unwrap(),
            ),
            // JWT tokens
            (
                "jwt".to_string(),
                Regex::new(r"eyJ[a-zA-Z0-9_-]{10,}\.[a-zA-Z0-9_-]{10,}\.[a-zA-Z0-9_-]{10,}")
                    .unwrap(),
            ),
            // Database URLs with credentials
            (
                "db_url".to_string(),
                Regex::new(r#"(?i)(postgres|mysql|mongodb|redis)://[^@\s]+@[^\s"']+"#).unwrap(),
            ),
            // Generic secret patterns (password=, secret=)
            (
                "generic_secret".to_string(),
                Regex::new(r#"(?i)(password|passwd|secret)\s*[=:]\s*["']?([^\s"']{8,})["']?"#)
                    .unwrap(),
            ),
            // Private key content (base64 blocks)
            (
                "private_key_content".to_string(),
                Regex::new(r"[A-Za-z0-9+/]{40,}={0,2}").unwrap(),
            ),
        ];

        Self { patterns }
    }

    /// Scrub detected secrets from text, replacing with [REDACTED:<category>]
    pub fn scrub(&self, text: &str) -> String {
        let mut result = text.to_string();

        for (category, regex) in &self.patterns {
            // Skip the generic base64 pattern — too many false positives
            if category == "private_key_content" {
                continue;
            }

            result = regex
                .replace_all(&result, |_: &regex::Captures| {
                    format!("[REDACTED:{category}]")
                })
                .to_string();
        }

        result
    }

    /// Check if text contains any detected secrets (without modifying)
    pub fn has_secrets(&self, text: &str) -> bool {
        self.patterns.iter().any(|(cat, regex)| {
            if cat == "private_key_content" {
                return false;
            }
            regex.is_match(text)
        })
    }
}

impl Default for LeakDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_jwt() {
        let detector = LeakDetector::new();
        let input = "token: eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        assert!(detector.has_secrets(input));
        let scrubbed = detector.scrub(input);
        assert!(scrubbed.contains("[REDACTED:jwt]"));
    }

    #[test]
    fn test_detect_db_url() {
        let detector = LeakDetector::new();
        let input = "DATABASE_URL=postgres://user:password123@localhost:5432/mydb";
        assert!(detector.has_secrets(input));
        let scrubbed = detector.scrub(input);
        assert!(scrubbed.contains("[REDACTED:db_url]"));
    }

    #[test]
    fn test_detect_generic_api_key() {
        let detector = LeakDetector::new();
        let input = "API_KEY=abcdef1234567890abcdef1234567890";
        assert!(detector.has_secrets(input));
    }

    #[test]
    fn test_no_false_positive_normal_text() {
        let detector = LeakDetector::new();
        let input = "Hello world, this is a normal message.";
        assert!(!detector.has_secrets(input));
    }
}
