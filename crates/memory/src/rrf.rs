//! Reciprocal Rank Fusion (RRF) algorithm
//!
//! Merges results from multiple ranked lists into a single combined ranking.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A scored item from a single ranking source
#[derive(Debug, Clone)]
pub struct RankedItem {
    pub id: Uuid,
    pub rank: usize,
}

/// RRF fusion parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RrfParams {
    /// Smoothing constant (default 60)
    pub k: f32,
    /// Weight for vector search results
    pub vector_weight: f32,
    /// Weight for FTS results
    pub fts_weight: f32,
}

impl Default for RrfParams {
    fn default() -> Self {
        Self {
            k: 60.0,
            vector_weight: 0.7,
            fts_weight: 0.3,
        }
    }
}

/// Compute RRF scores for items appearing in vector and/or FTS results.
///
/// Returns a list of (id, score) sorted by score descending.
pub fn rrf_merge(
    vector_results: &[RankedItem],
    fts_results: &[RankedItem],
    params: &RrfParams,
) -> Vec<(Uuid, f32)> {
    use std::collections::HashMap;

    let mut scores: HashMap<Uuid, f32> = HashMap::new();

    for item in vector_results {
        let rrf_score = params.vector_weight / (params.k + item.rank as f32 + 1.0);
        *scores.entry(item.id).or_default() += rrf_score;
    }

    for item in fts_results {
        let rrf_score = params.fts_weight / (params.k + item.rank as f32 + 1.0);
        *scores.entry(item.id).or_default() += rrf_score;
    }

    let mut results: Vec<(Uuid, f32)> = scores.into_iter().collect();
    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rrf_merge_basic() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();

        let vector = vec![
            RankedItem { id: id1, rank: 0 },
            RankedItem { id: id2, rank: 1 },
        ];
        let fts = vec![
            RankedItem { id: id2, rank: 0 },
            RankedItem { id: id3, rank: 1 },
        ];

        let results = rrf_merge(&vector, &fts, &RrfParams::default());

        // id2 should be ranked highest (appears in both lists)
        assert_eq!(results[0].0, id2);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_rrf_merge_empty() {
        let results = rrf_merge(&[], &[], &RrfParams::default());
        assert!(results.is_empty());
    }

    #[test]
    fn test_rrf_scores_decrease_with_rank() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        let vector = vec![
            RankedItem { id: id1, rank: 0 },
            RankedItem { id: id2, rank: 5 },
        ];

        let results = rrf_merge(&vector, &[], &RrfParams::default());
        assert!(results[0].1 > results[1].1);
    }
}
