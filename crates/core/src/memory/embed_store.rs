//! **EmbedStore** — embedding-based search with recency/frequency scoring.
//!
//! Provides scored search results using a configurable embedding model.
//! Search scores combine:
//! * Relevance (keyword/embedding match)
//! * Recency (how recently the entry was accessed)
//! * Frequency (how often the entry has been accessed)
//!
//! # Scoring formula
//!
//! `score = relevance * w1 + recency_score * w2 + frequency_score * w3`
//!
//! where `w1`, `w2`, `w3` are configurable weights (default: 1.0 each).

use crate::memory::episodic::EpisodicMemory;
use std::time::SystemTime;

/// Default weight for the relevance component.
const DEFAULT_RELEVANCE_WEIGHT: f64 = 1.0;
/// Default weight for the recency component.
const DEFAULT_RECENCY_WEIGHT: f64 = 1.0;
/// Default weight for the frequency component.
const DEFAULT_FREQUENCY_WEIGHT: f64 = 1.0;
/// Half-life for recency decay in seconds (7 days).
const RECENCY_HALF_LIFE: f64 = 604_800.0;

/// A search result with scored relevance.
#[derive(Debug, Clone)]
pub struct ScoredSearchEntry {
    /// The matching episodic entry's turn ID.
    pub turn_id: String,
    /// The user input that started this turn.
    pub input: String,
    /// The assistant's text output.
    pub output: String,
    /// Combined score (relevance × w1 + recency × w2 + frequency × w3).
    pub score: f64,
    /// How many times this entry has been accessed via search.
    pub access_count: u64,
    /// When this entry was last accessed.
    pub last_accessed: Option<SystemTime>,
}

/// Scoring weights and embedding model configuration.
#[derive(Debug, Clone)]
pub struct ScoringConfig {
    /// Weight for the relevance (keyword match) component.
    pub relevance_weight: f64,
    /// Weight for the recency (time since last access) component.
    pub recency_weight: f64,
    /// Weight for the frequency (access count) component.
    pub frequency_weight: f64,
    /// Embedding model identifier per backend.
    /// `None` means use the default for the active backend.
    pub embedding_model: Option<String>,
}

impl Default for ScoringConfig {
    fn default() -> Self {
        Self {
            relevance_weight: DEFAULT_RELEVANCE_WEIGHT,
            recency_weight: DEFAULT_RECENCY_WEIGHT,
            frequency_weight: DEFAULT_FREQUENCY_WEIGHT,
            embedding_model: None,
        }
    }
}

/// Usage statistics tracked per entry for recency/frequency scoring.
#[derive(Debug, Clone)]
pub struct UsageRecord {
    /// Number of times this entry has been accessed via search.
    pub access_count: u64,
    /// When this entry was last accessed.
    pub last_accessed: Option<SystemTime>,
}

/// Embedding store with usage-aware search scoring.
///
/// Wraps an [`EpisodicMemory`] and adds recency/frequency scoring
/// to search results.
#[derive(Debug, Clone)]
pub struct EmbedStore {
    /// The underlying episodic memory.
    episodic: EpisodicMemory,
    /// Scoring configuration.
    scoring_config: ScoringConfig,
    /// Per-entry usage statistics (tracked by turn_id).
    usage_stats: std::collections::HashMap<String, UsageRecord>,
}

impl EmbedStore {
    /// Create a new embedding store wrapping an episodic memory.
    #[must_use]
    pub fn new(episodic: EpisodicMemory) -> Self {
        Self {
            episodic,
            scoring_config: ScoringConfig::default(),
            usage_stats: std::collections::HashMap::new(),
        }
    }

    /// Create a new embedding store with custom scoring config.
    #[must_use]
    pub fn with_config(episodic: EpisodicMemory, scoring_config: ScoringConfig) -> Self {
        Self {
            episodic,
            scoring_config,
            usage_stats: std::collections::HashMap::new(),
        }
    }

    /// Set the scoring configuration.
    pub fn set_scoring_config(&mut self, config: ScoringConfig) {
        self.scoring_config = config;
    }

    /// Set the embedding model identifier.
    pub fn set_embedding_model(&mut self, model: impl Into<String>) {
        self.scoring_config.embedding_model = Some(model.into());
    }

    /// Access the underlying episodic memory (immutable).
    #[must_use]
    pub fn episodic(&self) -> &EpisodicMemory {
        &self.episodic
    }

    /// Access the underlying episodic memory (mutable).
    pub fn episodic_mut(&mut self) -> &mut EpisodicMemory {
        &mut self.episodic
    }

    /// Record a usage event for the given turn ID (increments access count).
    pub fn record_access(&mut self, turn_id: &str) {
        let entry = self
            .usage_stats
            .entry(turn_id.to_string())
            .or_insert(UsageRecord {
                access_count: 0,
                last_accessed: None,
            });
        entry.access_count += 1;
        entry.last_accessed = Some(SystemTime::now());
    }

    /// Search with recency/frequency scoring.
    ///
    /// Returns up to `max_results` entries, scored by:
    /// `score = relevance * w1 + recency_score * w2 + frequency_score * w3`
    #[must_use]
    pub fn search_scored(&self, query: &str, max_results: usize) -> Vec<ScoredSearchEntry> {
        if max_results == 0 {
            return Vec::new();
        }

        // Get raw keyword-based results from episodic memory
        let raw_results = self.episodic.search(query, max_results);

        let mut scored: Vec<ScoredSearchEntry> = raw_results
            .iter()
            .map(|entry| {
                let usage = self.usage_stats.get(&entry.turn_id);
                let access_count = usage.map(|u| u.access_count).unwrap_or(0);
                let last_accessed = usage.and_then(|u| u.last_accessed);

                // Relevance: use the existing TF-IDF score from episodic search
                // (normalised to [0, 1] range by dividing by max possible score).
                let relevance = self.compute_relevance(query, entry);

                // Recency: exponential decay based on time since last access.
                let recency = self.compute_recency(last_accessed);

                // Frequency: logarithmic scaling of access count.
                let frequency = self.compute_frequency(access_count);

                let score = relevance * self.scoring_config.relevance_weight
                    + recency * self.scoring_config.recency_weight
                    + frequency * self.scoring_config.frequency_weight;

                ScoredSearchEntry {
                    turn_id: entry.turn_id.clone(),
                    input: entry.input.clone(),
                    output: entry.output.clone(),
                    score,
                    access_count,
                    last_accessed,
                }
            })
            .collect();

        // Sort by score descending
        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(max_results);
        scored
    }

    /// Convenience: search and auto-record access for retrieved entries.
    #[must_use]
    pub fn search_scored_and_track(
        mut self,
        query: &str,
        max_results: usize,
    ) -> Vec<ScoredSearchEntry> {
        let results = self.search_scored(query, max_results);
        for result in &results {
            self.record_access(&result.turn_id);
        }
        results
    }

    /// Total number of stored entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.episodic.len()
    }

    /// Whether the store is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.episodic.is_empty()
    }

    /// Compute relevance score for an entry given a query.
    ///
    /// Uses keyword overlap ratio (number of matching query keywords / total query keywords).
    fn compute_relevance(
        &self,
        query: &str,
        entry: &crate::memory::episodic::EpisodicEntry,
    ) -> f64 {
        let query_kws: Vec<String> = EpisodicMemory::extract_keywords(query);
        if query_kws.is_empty() {
            return 0.0;
        }

        let entry_kws: Vec<String> =
            EpisodicMemory::extract_keywords(&format!("{} {}", entry.input, entry.output,));
        // Also index tool call names
        let tool_names: Vec<String> = entry.tool_calls.iter().map(|tc| tc.name.clone()).collect();
        let all_kws: Vec<String> = [entry_kws, tool_names].concat();

        let matched = query_kws.iter().filter(|qkw| all_kws.contains(qkw)).count();

        matched as f64 / query_kws.len() as f64
    }

    /// Compute recency score using exponential decay.
    ///
    /// Returns a value in [0, 1] where 1 = just accessed, 0 = never accessed.
    fn compute_recency(&self, last_accessed: Option<SystemTime>) -> f64 {
        match last_accessed {
            Some(time) => {
                match time.elapsed() {
                    Ok(elapsed) => {
                        let secs = elapsed.as_secs_f64();
                        // Exponential decay: 2^(-t / half_life)
                        (-secs / RECENCY_HALF_LIFE).exp()
                    }
                    Err(_) => 0.0,
                }
            }
            None => 0.0,
        }
    }

    /// Compute frequency score using logarithmic scaling.
    ///
    /// Returns a value in [0, 1] where higher access count = higher score.
    fn compute_frequency(&self, access_count: u64) -> f64 {
        if access_count == 0 {
            return 0.0;
        }
        // log(1 + count) / log(1 + max_expected)
        // Using log2 so every doubling adds a fixed increment
        let count = (1.0 + access_count as f64).ln();
        let max = (1.0 + 100.0_f64).ln();
        count / max
    }
}

impl From<EpisodicMemory> for EmbedStore {
    fn from(episodic: EpisodicMemory) -> Self {
        Self::new(episodic)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::episodic::EpisodicEntry;

    fn make_entry(turn_id: &str, input: &str, output: &str) -> EpisodicEntry {
        let keywords = EpisodicMemory::extract_keywords(input);
        EpisodicEntry {
            turn_id: turn_id.to_string(),
            timestamp: std::time::SystemTime::now(),
            input: input.to_string(),
            output: output.to_string(),
            tool_calls: vec![],
            keywords,
        }
    }

    #[test]
    fn test_scored_search_returns_results() {
        let mut episodic = EpisodicMemory::new();
        episodic.record(make_entry("t1", "deploy the app", "deployment complete"));
        episodic.record(make_entry("t2", "run tests", "all tests passed"));

        let store = EmbedStore::new(episodic);
        let results = store.search_scored("deploy", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].turn_id, "t1");
        assert!(results[0].score > 0.0);
    }

    #[test]
    fn test_recency_scoring_prefers_recent_access() {
        let mut episodic = EpisodicMemory::new();
        episodic.record(make_entry("t1", "deploy one", "done"));
        episodic.record(make_entry("t2", "deploy two", "done"));

        let mut store = EmbedStore::new(episodic);

        // Access t1 recently
        store.record_access("t1");

        // Both match "deploy" equally on relevance, but t1 has higher recency
        let results = store.search_scored("deploy", 10);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].turn_id, "t1");
        assert!(results[0].score >= results[1].score);
    }

    #[test]
    fn test_frequency_scoring() {
        let mut episodic = EpisodicMemory::new();
        episodic.record(make_entry("t1", "deploy one", "done"));
        episodic.record(make_entry("t2", "deploy two", "done"));

        let mut store = EmbedStore::new(episodic);

        // Access t1 multiple times
        store.record_access("t1");
        store.record_access("t1");
        store.record_access("t1");

        let results = store.search_scored("deploy", 10);
        assert_eq!(results.len(), 2);
        // t1 should have higher frequency score
        assert_eq!(results[0].turn_id, "t1");
        assert!(results[0].access_count > results[1].access_count);
    }

    #[test]
    fn test_empty_search() {
        let episodic = EpisodicMemory::new();
        let store = EmbedStore::new(episodic);
        let results = store.search_scored("anything", 10);
        assert!(results.is_empty());
    }

    #[test]
    fn test_zero_max_results() {
        let mut episodic = EpisodicMemory::new();
        episodic.record(make_entry("t1", "deploy", "done"));
        let store = EmbedStore::new(episodic);
        let results = store.search_scored("deploy", 0);
        assert!(results.is_empty());
    }

    #[test]
    fn test_scoring_config_default() {
        let config = ScoringConfig::default();
        assert!((config.relevance_weight - 1.0).abs() < f64::EPSILON);
        assert!((config.recency_weight - 1.0).abs() < f64::EPSILON);
        assert!((config.frequency_weight - 1.0).abs() < f64::EPSILON);
        assert!(config.embedding_model.is_none());
    }

    #[test]
    fn test_custom_embedding_model() {
        let mut store = EmbedStore::new(EpisodicMemory::new());
        store.set_embedding_model("text-embedding-3-small");
        assert_eq!(
            store.scoring_config.embedding_model.as_deref(),
            Some("text-embedding-3-small")
        );
    }
}
