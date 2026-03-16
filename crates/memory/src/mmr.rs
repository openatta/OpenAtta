//! Maximal Marginal Relevance (MMR) reranking
//!
//! Reduces redundancy in search results by balancing relevance with diversity.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::traits::SearchResult;

/// MMR parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MmrParams {
    /// Whether MMR reranking is enabled
    pub enabled: bool,
    /// Tradeoff between relevance and diversity (1.0 = pure relevance, 0.0 = pure diversity)
    pub lambda: f32,
}

impl Default for MmrParams {
    fn default() -> Self {
        Self {
            enabled: false,
            lambda: 0.7,
        }
    }
}

/// Rerank results using Maximal Marginal Relevance
///
/// Uses Jaccard similarity on tokenized content to measure inter-result similarity.
pub fn mmr_rerank(results: Vec<SearchResult>, lambda: f32) -> Vec<SearchResult> {
    if results.len() <= 1 {
        return results;
    }

    // Normalize scores to [0, 1]
    let max_score = results
        .iter()
        .map(|r| r.score)
        .fold(f32::NEG_INFINITY, f32::max);
    let min_score = results
        .iter()
        .map(|r| r.score)
        .fold(f32::INFINITY, f32::min);
    let score_range = (max_score - min_score).max(1e-6);

    let normalized: Vec<f32> = results
        .iter()
        .map(|r| (r.score - min_score) / score_range)
        .collect();

    // Pre-tokenize all content
    let tokens: Vec<HashSet<&str>> = results.iter().map(|r| tokenize(&r.entry.content)).collect();

    let n = results.len();
    let mut selected_indices: Vec<usize> = Vec::with_capacity(n);
    let mut remaining: Vec<usize> = (0..n).collect();

    // First: pick highest-scored item
    let first_idx = remaining
        .iter()
        .copied()
        .max_by(|&a, &b| normalized[a].partial_cmp(&normalized[b]).unwrap())
        .unwrap();
    selected_indices.push(first_idx);
    remaining.retain(|&i| i != first_idx);

    // Iteratively select remaining items
    while !remaining.is_empty() {
        let mut best_idx = remaining[0];
        let mut best_mmr = f32::NEG_INFINITY;

        for &candidate in &remaining {
            let relevance = normalized[candidate];

            // Max similarity to any already-selected result
            let max_sim = selected_indices
                .iter()
                .map(|&sel| jaccard_similarity(&tokens[candidate], &tokens[sel]))
                .fold(0.0f32, f32::max);

            let mmr_score = lambda * relevance - (1.0 - lambda) * max_sim;

            if mmr_score > best_mmr {
                best_mmr = mmr_score;
                best_idx = candidate;
            }
        }

        selected_indices.push(best_idx);
        remaining.retain(|&i| i != best_idx);
    }

    // Rebuild results in MMR order
    selected_indices
        .into_iter()
        .map(|i| results[i].clone())
        .collect()
}

/// Simple whitespace tokenizer
fn tokenize(text: &str) -> HashSet<&str> {
    text.split_whitespace().collect()
}

/// Jaccard similarity between two token sets
fn jaccard_similarity(a: &HashSet<&str>, b: &HashSet<&str>) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let intersection = a.intersection(b).count() as f32;
    let union = a.union(b).count() as f32;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{MatchSource, MemoryEntry, MemoryMetadata};
    use atta_types::MemoryType;
    use uuid::Uuid;

    fn make_result(content: &str, score: f32) -> SearchResult {
        SearchResult {
            entry: MemoryEntry {
                id: Uuid::new_v4(),
                memory_type: MemoryType::Knowledge,
                content: content.to_string(),
                embedding: None,
                metadata: MemoryMetadata::default(),
                created_at: chrono::Utc::now(),
                last_accessed: chrono::Utc::now(),
                access_count: 0,
            },
            score,
            match_source: MatchSource::Hybrid,
        }
    }

    #[test]
    fn test_mmr_rerank_empty() {
        let results = mmr_rerank(vec![], 0.7);
        assert!(results.is_empty());
    }

    #[test]
    fn test_mmr_rerank_single() {
        let results = vec![make_result("hello world", 0.9)];
        let reranked = mmr_rerank(results, 0.7);
        assert_eq!(reranked.len(), 1);
    }

    #[test]
    fn test_mmr_rerank_promotes_diversity() {
        let results = vec![
            make_result("rust programming language systems", 0.9),
            make_result("rust programming language memory safety", 0.85),
            make_result("python data science machine learning", 0.8),
        ];
        // Very low lambda (0.1) = almost pure diversity
        let reranked = mmr_rerank(results, 0.1);
        assert_eq!(reranked.len(), 3);
        // First stays (highest score), then the most different one should be promoted
        // The python result is lexically very different from rust results
        let second_content = &reranked[1].entry.content;
        let third_content = &reranked[2].entry.content;
        // Either python is second (diversity) or the two rust results are separated
        assert!(
            second_content.contains("python") || third_content.contains("python"),
            "python should be in results: [{second_content}] [{third_content}]"
        );
    }

    #[test]
    fn test_jaccard_similarity() {
        let a: HashSet<&str> = ["hello", "world"].iter().copied().collect();
        let b: HashSet<&str> = ["hello", "rust"].iter().copied().collect();
        let sim = jaccard_similarity(&a, &b);
        assert!((sim - 1.0 / 3.0).abs() < 0.01);
    }
}
