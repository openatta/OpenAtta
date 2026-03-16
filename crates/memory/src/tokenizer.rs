//! Multi-language tokenizer for FTS query preprocessing
//!
//! Handles CJK n-gram segmentation and English stop-word filtering
//! to improve full-text search hit rate across languages.

use std::collections::HashSet;

/// English stop words (common subset)
const ENGLISH_STOP_WORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "in", "on", "at", "to", "for", "of", "with", "by",
    "from", "is", "was", "are", "were", "be", "been", "being", "have", "has", "had", "do", "does",
    "did", "will", "would", "shall", "should", "may", "might", "must", "can", "could", "not", "no",
    "nor", "so", "if", "then", "than", "that", "this", "these", "those", "it", "its", "i", "me",
    "my", "we", "us", "our", "you", "your", "he", "him", "his", "she", "her", "they", "them",
    "their", "what", "which", "who", "whom", "how", "when", "where", "why",
];

/// Chinese stop words (common subset)
const CHINESE_STOP_WORDS: &[&str] = &[
    "的", "了", "在", "是", "我", "有", "和", "就", "不", "人", "都", "一", "一个", "上", "也",
    "很", "到", "说", "要", "去", "你", "会", "着", "没有", "看", "好", "自己", "这",
];

/// Check if a character is CJK (Chinese, Japanese, Korean)
fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}'   |  // CJK Unified Ideographs
        '\u{3400}'..='\u{4DBF}'   |  // CJK Extension A
        '\u{F900}'..='\u{FAFF}'   |  // CJK Compatibility Ideographs
        '\u{3040}'..='\u{309F}'   |  // Hiragana
        '\u{30A0}'..='\u{30FF}'   |  // Katakana
        '\u{AC00}'..='\u{D7AF}'      // Hangul Syllables
    )
}

/// Tokenize text for FTS query, handling both CJK and Latin scripts
///
/// - CJK text: split into 2-gram character sequences
/// - Latin text: split on whitespace, lowercase, filter stop words
pub fn tokenize_for_fts(text: &str) -> Vec<String> {
    let stop_en: HashSet<&str> = ENGLISH_STOP_WORDS.iter().copied().collect();
    let stop_zh: HashSet<&str> = CHINESE_STOP_WORDS.iter().copied().collect();

    let mut tokens = Vec::new();

    // Collect CJK characters for n-gram
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if is_cjk(chars[i]) {
            // CJK: extract 2-grams
            if i + 1 < chars.len() && is_cjk(chars[i + 1]) {
                let bigram: String = chars[i..i + 2].iter().collect();
                if !stop_zh.contains(bigram.as_str()) {
                    tokens.push(bigram);
                }
            }
            // Also add single character as token
            let single: String = chars[i..=i].iter().collect();
            if !stop_zh.contains(single.as_str()) {
                tokens.push(single);
            }
            i += 1;
        } else {
            // Latin: collect word
            let start = i;
            while i < chars.len() && !is_cjk(chars[i]) && !chars[i].is_whitespace() {
                i += 1;
            }
            if start < i {
                let word: String = chars[start..i].iter().collect();
                let lower = word.to_lowercase();
                // Strip punctuation
                let clean: String = lower.chars().filter(|c| c.is_alphanumeric()).collect();
                if !clean.is_empty() && !stop_en.contains(clean.as_str()) {
                    tokens.push(clean);
                }
            } else {
                i += 1;
            }
        }
    }

    tokens
}

/// Build an FTS5-compatible query from tokenized text
///
/// Joins tokens with OR for broad matching.
pub fn build_fts_query(text: &str) -> String {
    let tokens = tokenize_for_fts(text);
    if tokens.is_empty() {
        return text.to_string();
    }
    // Quote each token and join with OR
    tokens
        .into_iter()
        .map(|t| format!("\"{}\"", t.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" OR ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_english() {
        let tokens = tokenize_for_fts("The quick brown fox");
        assert!(tokens.contains(&"quick".to_string()));
        assert!(tokens.contains(&"brown".to_string()));
        assert!(tokens.contains(&"fox".to_string()));
        assert!(!tokens.contains(&"the".to_string())); // stop word
    }

    #[test]
    fn test_tokenize_chinese() {
        let tokens = tokenize_for_fts("机器学习算法");
        // Should contain bigrams
        assert!(tokens.contains(&"机器".to_string()));
        assert!(tokens.contains(&"器学".to_string()));
        assert!(tokens.contains(&"学习".to_string()));
        assert!(tokens.contains(&"习算".to_string()));
    }

    #[test]
    fn test_tokenize_mixed() {
        let tokens = tokenize_for_fts("Rust 是一门系统编程语言");
        assert!(tokens.contains(&"rust".to_string()));
        assert!(tokens.contains(&"系统".to_string()));
        assert!(tokens.contains(&"编程".to_string()));
    }

    #[test]
    fn test_build_fts_query() {
        let query = build_fts_query("machine learning");
        assert!(query.contains("machine"));
        assert!(query.contains("learning"));
        assert!(query.contains(" OR "));
    }

    #[test]
    fn test_is_cjk() {
        assert!(is_cjk('中'));
        assert!(is_cjk('あ'));
        assert!(is_cjk('한'));
        assert!(!is_cjk('A'));
        assert!(!is_cjk('1'));
    }
}
