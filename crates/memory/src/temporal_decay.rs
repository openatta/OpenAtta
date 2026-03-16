//! Temporal decay scoring
//!
//! Applies exponential decay to search result scores based on memory age.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::traits::SearchResult;

/// Temporal decay parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecayParams {
    /// Whether temporal decay is enabled
    pub enabled: bool,
    /// Half-life in days: after this many days, score is halved
    pub half_life_days: f32,
}

impl Default for DecayParams {
    fn default() -> Self {
        Self {
            enabled: false,
            half_life_days: 30.0,
        }
    }
}

/// Apply exponential temporal decay to search results
///
/// Uses the formula: `score *= exp(-lambda * age_days)`
/// where `lambda = ln(2) / half_life_days`.
pub fn apply_temporal_decay(
    results: &mut [SearchResult],
    params: &DecayParams,
    now: DateTime<Utc>,
) {
    if !params.enabled || params.half_life_days <= 0.0 {
        return;
    }

    let lambda = (2.0_f32).ln() / params.half_life_days;

    for r in results.iter_mut() {
        let age_days = (now - r.entry.created_at).num_seconds() as f32 / 86400.0;
        let decay = (-lambda * age_days.max(0.0)).exp();
        r.score *= decay;
    }

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{MatchSource, MemoryEntry, MemoryMetadata};
    use atta_types::MemoryType;
    use uuid::Uuid;

    fn make_result(age_days: i64, score: f32) -> SearchResult {
        SearchResult {
            entry: MemoryEntry {
                id: Uuid::new_v4(),
                memory_type: MemoryType::Knowledge,
                content: "test".to_string(),
                embedding: None,
                metadata: MemoryMetadata::default(),
                created_at: Utc::now() - chrono::Duration::days(age_days),
                last_accessed: Utc::now(),
                access_count: 0,
            },
            score,
            match_source: MatchSource::Hybrid,
        }
    }

    #[test]
    fn test_no_decay_when_disabled() {
        let params = DecayParams {
            enabled: false,
            half_life_days: 30.0,
        };
        let mut results = vec![make_result(60, 1.0)];
        apply_temporal_decay(&mut results, &params, Utc::now());
        assert!((results[0].score - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_half_life_decay() {
        let params = DecayParams {
            enabled: true,
            half_life_days: 30.0,
        };
        let mut results = vec![make_result(30, 1.0)];
        apply_temporal_decay(&mut results, &params, Utc::now());
        // After one half-life, score should be ~0.5
        assert!((results[0].score - 0.5).abs() < 0.05);
    }

    #[test]
    fn test_recent_items_score_higher() {
        let params = DecayParams {
            enabled: true,
            half_life_days: 7.0,
        };
        let mut results = vec![
            make_result(30, 0.8), // old
            make_result(1, 0.7),  // recent
        ];
        apply_temporal_decay(&mut results, &params, Utc::now());
        // Recent item should now rank first due to less decay
        assert!(results[0].entry.created_at > results[1].entry.created_at);
    }

    #[test]
    fn test_zero_age_no_decay() {
        let params = DecayParams {
            enabled: true,
            half_life_days: 30.0,
        };
        let mut results = vec![make_result(0, 1.0)];
        apply_temporal_decay(&mut results, &params, Utc::now());
        assert!((results[0].score - 1.0).abs() < 0.01);
    }
}
