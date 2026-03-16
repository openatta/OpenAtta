//! Secret prefix pattern matching

/// Known secret prefixes and their minimum lengths for redaction
const SECRET_PREFIXES: &[(&str, usize)] = &[
    // OpenAI / Anthropic
    ("sk-", 8),
    ("sk_live_", 10),
    ("sk_test_", 10),
    // Slack
    ("xoxb-", 10),
    ("xoxp-", 10),
    ("xoxs-", 10),
    ("xapp-", 10),
    // GitHub
    ("ghp_", 8),
    ("gho_", 8),
    ("ghu_", 8),
    ("ghs_", 8),
    ("ghr_", 8),
    // GitLab
    ("glpat-", 10),
    // AWS
    ("AKIA", 16),
    ("ASIA", 16),
    // Google
    ("AIza", 16),
    // Stripe
    ("pk_live_", 10),
    ("pk_test_", 10),
    ("rk_live_", 10),
    ("rk_test_", 10),
    // Auth headers
    ("Bearer ", 16),
    ("Basic ", 10),
    // PEM
    ("-----BEGIN", 20),
    // npm
    ("npm_", 8),
    // Twilio
    ("SK", 32),
];

const REDACTED: &str = "[REDACTED]";

/// Scrub known secret prefixes from text
///
/// Scans for known secret prefixes and replaces the secret value with `[REDACTED]`.
/// This is a fast O(n*m) scan where n is text length and m is number of prefixes.
pub fn scrub_secret_patterns(text: &str) -> String {
    let mut result = text.to_string();

    for &(prefix, min_len) in SECRET_PREFIXES {
        // Special handling for PEM blocks
        if prefix == "-----BEGIN" {
            result = scrub_pem_blocks(&result);
            continue;
        }

        // Special handling for "Bearer " and "Basic " — look for them in context
        if prefix == "Bearer " || prefix == "Basic " {
            result = scrub_auth_headers(&result, prefix);
            continue;
        }

        // General prefix matching
        let mut search_from = 0;
        while let Some(pos) = result[search_from..].find(prefix) {
            let abs_pos = search_from + pos;
            let after = &result[abs_pos + prefix.len()..];

            // Count contiguous non-whitespace characters after the prefix
            let secret_len: usize = after
                .chars()
                .take_while(|c| {
                    !c.is_whitespace()
                        && *c != '"'
                        && *c != '\''
                        && *c != ','
                        && *c != '}'
                        && *c != ')'
                        && *c != ']'
                })
                .map(|c| c.len_utf8())
                .sum();

            if secret_len >= min_len {
                let end = abs_pos + prefix.len() + secret_len;
                result.replace_range(abs_pos..end, REDACTED);
                search_from = abs_pos + REDACTED.len();
            } else {
                search_from = abs_pos + prefix.len();
            }
        }
    }

    result
}

/// Scrub PEM private key blocks
fn scrub_pem_blocks(text: &str) -> String {
    let mut result = text.to_string();
    let pem_markers = [
        (
            "-----BEGIN RSA PRIVATE KEY-----",
            "-----END RSA PRIVATE KEY-----",
        ),
        ("-----BEGIN PRIVATE KEY-----", "-----END PRIVATE KEY-----"),
        (
            "-----BEGIN EC PRIVATE KEY-----",
            "-----END EC PRIVATE KEY-----",
        ),
        (
            "-----BEGIN DSA PRIVATE KEY-----",
            "-----END DSA PRIVATE KEY-----",
        ),
        (
            "-----BEGIN OPENSSH PRIVATE KEY-----",
            "-----END OPENSSH PRIVATE KEY-----",
        ),
    ];

    for (begin, end) in &pem_markers {
        while let Some(start_pos) = result.find(begin) {
            if let Some(end_offset) = result[start_pos..].find(end) {
                let end_pos = start_pos + end_offset + end.len();
                result.replace_range(start_pos..end_pos, REDACTED);
            } else {
                // No closing marker — redact from begin to end of text
                result.replace_range(start_pos.., REDACTED);
                break;
            }
        }
    }

    result
}

/// Scrub auth headers (Bearer / Basic tokens)
fn scrub_auth_headers(text: &str, prefix: &str) -> String {
    let mut result = text.to_string();
    let mut search_from = 0;

    while let Some(pos) = result[search_from..].find(prefix) {
        let abs_pos = search_from + pos;
        let after = &result[abs_pos + prefix.len()..];

        let token_len: usize = after
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '"' && *c != '\'')
            .map(|c| c.len_utf8())
            .sum();

        if token_len >= 8 {
            let end = abs_pos + prefix.len() + token_len;
            let replacement = format!("{} {}", prefix.trim(), REDACTED);
            result.replace_range(abs_pos..end, &replacement);
            search_from = abs_pos + replacement.len();
        } else {
            search_from = abs_pos + prefix.len();
        }
    }

    result
}

/// Recursively scrub secrets from a JSON value
///
/// Walks all string values in objects, arrays, and top-level strings,
/// applying `scrub_secret_patterns` to each.
pub fn scrub_json_value(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => serde_json::Value::String(scrub_secret_patterns(s)),
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(scrub_json_value).collect())
        }
        serde_json::Value::Object(obj) => serde_json::Value::Object(
            obj.iter()
                .map(|(k, v)| (k.clone(), scrub_json_value(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scrub_openai_key() {
        let input = "My key is sk-1234567890abcdefghijklmnopqrstuvwxyz";
        let result = scrub_secret_patterns(input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("1234567890"));
    }

    #[test]
    fn test_scrub_github_token() {
        let input = "token: ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef";
        let result = scrub_secret_patterns(input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("ABCDEFGH"));
    }

    #[test]
    fn test_scrub_aws_key() {
        let input = "aws_access_key_id = AKIAIOSFODNN7EXAMPLE";
        let result = scrub_secret_patterns(input);
        assert!(result.contains("[REDACTED]"));
    }

    #[test]
    fn test_scrub_bearer_token() {
        let input =
            r#"Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.payload.signature"#;
        let result = scrub_secret_patterns(input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("eyJhbG"));
    }

    #[test]
    fn test_scrub_pem_key() {
        let input = "here is a key:\n-----BEGIN RSA PRIVATE KEY-----\nMIIEow...\n-----END RSA PRIVATE KEY-----\nend";
        let result = scrub_secret_patterns(input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("MIIEow"));
    }

    #[test]
    fn test_no_false_positive_short() {
        let input = "sk-ab"; // Too short to be a real key
        let result = scrub_secret_patterns(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_preserves_normal_text() {
        let input = "Hello world, this is normal text with no secrets.";
        let result = scrub_secret_patterns(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_scrub_json_value_nested() {
        let input = serde_json::json!({
            "output": "key is sk-1234567890abcdefghijklmnopqrstuvwxyz",
            "data": [
                "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef",
                42
            ],
            "clean": "no secrets here"
        });
        let result = scrub_json_value(&input);
        assert!(result["output"].as_str().unwrap().contains("[REDACTED]"));
        assert!(result["data"][0].as_str().unwrap().contains("[REDACTED]"));
        assert_eq!(result["data"][1], 42);
        assert_eq!(result["clean"], "no secrets here");
    }
}
