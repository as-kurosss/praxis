//! **MemoryExtractor** — extracts structured facts from conversation turns.
//!
//! After each user ↔ assistant exchange, the extractor can be called to
//! identify entities, preferences, and facts that should be persisted
//! for long-term recall.
//!
//! # Two modes
//!
//! 1. **LLM-driven** (recommended): provide an `extract_fn` callback that
//!    sends a prompt to any LLM and returns a list of facts.  The callback
//!    receives the user input and the assistant response.
//! 2. **Keyword-based fallback**: when no callback is set, the built-in
//!    heuristic extracts noun-like tokens (words > 3 chars) as simple
//!    `(entity=keyword, attribute="mentioned", value=keyword)` facts.

use super::session_history::{MemoryFact, SessionHistory, SessionHistoryError};
use std::sync::Arc;

/// Callback signature for LLM-based fact extraction.
///
/// # Arguments
/// * `user_input` — the user's message.
/// * `assistant_response` — the assistant's reply text (empty if tool-only).
///
/// # Returns
/// A vector of extracted facts.  The callback is responsible for generating
/// unique IDs and timestamps.
pub type FactExtractorFn = Arc<dyn Fn(&str, &str) -> Vec<MemoryFact> + Send + Sync>;

/// Extracts structured facts from conversation turns and persists them
/// into the session history.
pub struct MemoryExtractor {
    history: SessionHistory,
    extractor: Option<FactExtractorFn>,
}

impl MemoryExtractor {
    /// Create a new extractor that stores facts in `history`.
    ///
    /// No extraction callback is set — use [`with_extractor`](Self::with_extractor)
    /// or the built-in keyword-based fallback will be used.
    pub fn new(history: SessionHistory) -> Self {
        Self {
            history,
            extractor: None,
        }
    }

    /// Attach an LLM-based extraction callback.
    pub fn with_extractor(mut self, extractor: FactExtractorFn) -> Self {
        self.extractor = Some(extractor);
        self
    }

    /// Borrow the inner session history (for inspection, search, etc.).
    pub fn history(&self) -> &SessionHistory {
        &self.history
    }

    /// Extract facts from a single turn and persist them.
    ///
    /// * `user_input` — the user's message.
    /// * `assistant_response` — the assistant's reply (empty for tool-only turns).
    ///
    /// Returns the number of facts stored.
    pub fn extract_from_turn(
        &self,
        user_input: &str,
        assistant_response: &str,
    ) -> Result<usize, SessionHistoryError> {
        let facts = if let Some(ref extractor) = self.extractor {
            extractor(user_input, assistant_response)
        } else {
            self.keyword_extract(user_input, assistant_response)
        };

        for fact in &facts {
            self.history.store_fact(fact)?;
        }

        Ok(facts.len())
    }

    /// Search facts by FTS5 query.
    pub fn search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MemoryFact>, SessionHistoryError> {
        self.history.search_facts(query, limit)
    }

    /// Load the most recent facts for the current session.
    pub fn recent_facts(&self, limit: usize) -> Result<Vec<MemoryFact>, SessionHistoryError> {
        self.history
            .get_facts_for_session(self.history.session_id(), limit)
    }

    // ── Built-in keyword-based fallback ─────────────────────────────

    /// Simple keyword-based fact extraction (fallback when no LLM callback
    /// is configured).
    ///
    /// Extracts words longer than 3 characters as `(entity, "mentioned", word)`
    /// facts with confidence 0.3.
    fn keyword_extract(&self, user_input: &str, assistant_response: &str) -> Vec<MemoryFact> {
        let mut facts = Vec::new();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let session_id = self.history.session_id().to_string();

        // Extract from user input
        for word in Self::keyphrases(user_input) {
            facts.push(MemoryFact {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: session_id.clone(),
                entity: word.clone(),
                attribute: "mentioned".into(),
                value: word,
                confidence: 0.3,
                timestamp: now,
            });
        }

        // Extract from assistant response
        if !assistant_response.is_empty() {
            for word in Self::keyphrases(assistant_response) {
                facts.push(MemoryFact {
                    id: uuid::Uuid::new_v4().to_string(),
                    session_id: session_id.clone(),
                    entity: word.clone(),
                    attribute: "mentioned".into(),
                    value: word,
                    confidence: 0.3,
                    timestamp: now,
                });
            }
        }

        facts
    }

    fn keyphrases(text: &str) -> Vec<String> {
        text.split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-')
            .filter(|w| w.len() > 3 && !w.chars().all(|c| c.is_ascii_digit()))
            .map(|w| w.to_lowercase())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_extractor() -> MemoryExtractor {
        let hist = SessionHistory::open_in_memory("mem-test").unwrap();
        MemoryExtractor::new(hist)
    }

    #[test]
    fn test_keyword_fallback_extracts_words() {
        let ext = make_extractor();
        let n = ext
            .extract_from_turn(
                "I love Rust programming",
                "Rust is great for systems programming",
            )
            .unwrap();
        assert!(n > 0, "should extract at least one keyword fact");
    }

    #[test]
    fn test_search_facts() {
        let ext = make_extractor();
        ext.extract_from_turn("My name is Alexey", "Hello Alexey!")
            .unwrap();

        let results = ext.search("alexey", 10).unwrap();
        assert!(!results.is_empty(), "should find fact about alexey");
    }

    #[test]
    fn test_custom_extractor() {
        let hist = SessionHistory::open_in_memory("mem-test-custom").unwrap();
        let extractor: FactExtractorFn = Arc::new(|_input, _response| {
            vec![MemoryFact {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: "custom-session".into(),
                entity: "test".into(),
                attribute: "type".into(),
                value: "custom".into(),
                confidence: 1.0,
                timestamp: 1_000_000,
            }]
        });

        let ext = MemoryExtractor::new(hist).with_extractor(extractor);
        let n = ext.extract_from_turn("hello", "world").unwrap();
        assert_eq!(n, 1);
    }
}
