//! **EpisodicMemory** — full verbatim history with indexed recall.
//!
//! Stores every turn (input / output / tool calls) that has been evicted from
//! working memory. Entries are indexed by extracted keywords so the agent can
//! recall relevant past context even after it has scrolled away.

use crate::agent::llm::ChatMessage;
use std::collections::{HashMap, VecDeque};

/// A single recorded turn in episodic memory.
#[derive(Debug, Clone)]
pub struct EpisodicEntry {
    /// Unique turn identifier (e.g. `"turn_17"`).
    pub turn_id: String,
    /// Wall-clock timestamp when the turn was recorded.
    pub timestamp: std::time::SystemTime,
    /// The user input that started this turn.
    pub input: String,
    /// The assistant's text output (empty if tool-only).
    pub output: String,
    /// Tool calls that were made during this turn.
    pub tool_calls: Vec<StoredToolCall>,
    /// Keywords extracted from the content for search indexing.
    pub keywords: Vec<String>,
}

/// A recorded tool call within an episodic entry.
#[derive(Debug, Clone)]
pub struct StoredToolCall {
    /// Tool name (e.g. `"shell"`, `"calculator"`).
    pub name: String,
    /// JSON-encoded arguments.
    pub arguments: String,
    /// JSON-encoded result or error.
    pub result: String,
}

/// Full verbatim history with keyword-based search.
///
/// # Example
///
/// ```ignore
/// let mut memory = EpisodicMemory::new();
/// memory.record(EpisodicEntry { turn_id: "...", input: "deploy".into(), ... });
///
/// let results = memory.search("deploy", 5);
/// assert_eq!(results.len(), 1);
/// ```
#[derive(Debug, Clone)]
pub struct EpisodicMemory {
    /// All recorded entries, indexed by turn_id.
    store: HashMap<String, EpisodicEntry>,
    /// Keyword → set of turn_ids that contain that keyword.
    index: HashMap<String, Vec<String>>,
    /// Insertion order (for LRU-style eviction when the store grows too large).
    order: VecDeque<String>,
    /// Maximum number of entries before the oldest are evicted.
    max_entries: usize,
}

impl Default for EpisodicMemory {
    fn default() -> Self {
        Self {
            store: HashMap::new(),
            index: HashMap::new(),
            order: VecDeque::new(),
            max_entries: 10_000,
        }
    }
}

impl EpisodicMemory {
    /// Create a new empty episodic memory with default capacity (10 000 entries).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an episodic memory with a custom maximum entry count.
    #[must_use]
    pub fn with_capacity(max_entries: usize) -> Self {
        Self {
            max_entries,
            ..Self::default()
        }
    }

    /// Record a new episodic entry.
    ///
    /// If the store is at capacity the oldest entry is evicted first.
    pub fn record(&mut self, entry: EpisodicEntry) {
        // Evict oldest if at capacity
        if self.store.len() >= self.max_entries {
            if let Some(oldest_id) = self.order.pop_front() {
                self.remove_entry(&oldest_id);
            }
        }

        let turn_id = entry.turn_id.clone();

        // Index keywords
        for kw in &entry.keywords {
            self.index
                .entry(kw.clone())
                .or_default()
                .push(turn_id.clone());
        }

        self.order.push_back(turn_id.clone());
        self.store.insert(turn_id, entry);
    }

    /// Search for entries whose keywords best match a query.
    ///
    /// Returns up to `max_results` entries, ordered by relevance
    /// (number of matching keywords, then recency).
    #[must_use]
    pub fn search(&self, query: &str, max_results: usize) -> Vec<&EpisodicEntry> {
        if max_results == 0 {
            return Vec::new();
        }

        let query_keywords: Vec<String> = query
            .split_whitespace()
            .map(|w| w.to_lowercase())
            .filter(|w| w.len() > 2)
            .collect();

        if query_keywords.is_empty() {
            return Vec::new();
        }

        // Score each matching turn by the number of query keywords found,
        // weighted by inverse document frequency (rare keywords score higher).
        let total_entries = self.store.len();
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for qkw in &query_keywords {
            if let Some(ids) = self.index.get(qkw) {
                // IDF weight: log(total / df). Rare keywords contribute more.
                let df = ids.len();
                let idf = if df >= total_entries || total_entries == 0 {
                    1.0
                } else {
                    (total_entries as f64 / df as f64).ln() + 1.0
                };
                for id in ids {
                    if let Some(pos) = scores.iter().position(|(tid, _)| *tid == id.as_str()) {
                        scores[pos].1 += idf;
                    } else {
                        scores.push((id.as_str(), idf));
                    }
                }
            }
        }

        // Sort: highest score first, then most recent
        scores.sort_by(|a, b| {
            let score_cmp = b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal);
            if score_cmp != std::cmp::Ordering::Equal {
                return score_cmp;
            }
            // More recent = higher position in order
            let pos_a = self.order.iter().position(|id| id == a.0);
            let pos_b = self.order.iter().position(|id| id == b.0);
            pos_b.cmp(&pos_a)
        });

        scores
            .into_iter()
            .take(max_results)
            .filter_map(|(id, _)| self.store.get(id))
            .collect()
    }

    /// Recall a specific entry by its turn ID.
    #[must_use]
    pub fn recall(&self, turn_id: &str) -> Option<&EpisodicEntry> {
        self.store.get(turn_id)
    }

    /// Total number of stored entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.store.len()
    }

    /// Whether the store is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.store.is_empty()
    }

    /// Iterate over all entries (oldest first).
    #[must_use]
    pub fn iter(&self) -> impl Iterator<Item = &EpisodicEntry> {
        self.order
            .iter()
            .filter_map(move |id| self.store.get(id))
    }

    /// Remove a single entry and its index entries.
    fn remove_entry(&mut self, turn_id: &str) {
        if let Some(entry) = self.store.remove(turn_id) {
            for kw in &entry.keywords {
                if let Some(ids) = self.index.get_mut(kw) {
                    ids.retain(|id| id != turn_id);
                    if ids.is_empty() {
                        self.index.remove(kw);
                    }
                }
            }
        }
        self.order.retain(|id| id != turn_id);
    }

    /// Extract simple keywords from a text for indexing.
    ///
    /// Splits on whitespace/punctuation, lowercases, discards very short tokens.
    pub fn extract_keywords(text: &str) -> Vec<String> {
        text.split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|w| w.len() > 2)
            .map(|w| w.to_lowercase())
            .collect()
    }
}

// ── Helpers to build EpisodicEntry from conversation data ────────────────

impl EpisodicEntry {
    /// Create a new episodic entry from a user input and the messages produced.
    ///
    /// `all_messages` is a slice of the full conversation for this turn
    /// (assistant response + optional tool results). The last assistant
    /// message's text is used as the output.
    pub fn from_turn(
        turn_id: impl Into<String>,
        input: impl Into<String>,
        all_messages: &[ChatMessage],
    ) -> Self {
        let input = input.into();
        let (output, tool_calls) = Self::extract_output_and_tools(all_messages);
        let combined_keywords = Self::build_keywords(&input, &output, &tool_calls);

        Self {
            turn_id: turn_id.into(),
            timestamp: std::time::SystemTime::now(),
            input,
            output,
            tool_calls,
            keywords: combined_keywords,
        }
    }

    fn extract_output_and_tools(messages: &[ChatMessage]) -> (String, Vec<StoredToolCall>) {
        let mut output = String::new();
        let mut tool_calls = Vec::new();

        for msg in messages {
            if let Some(content) = &msg.content {
                if msg.role == crate::agent::llm::Role::Assistant {
                    if !output.is_empty() {
                        output.push('\n');
                    }
                    output.push_str(content);
                }
            }
            if let Some(ref calls) = msg.tool_calls {
                for tc in calls {
                    tool_calls.push(StoredToolCall {
                        name: tc.name.clone(),
                        arguments: tc.arguments.to_string(),
                        result: String::new(), // filled later from tool_results
                    });
                }
            }
        }

        (output, tool_calls)
    }

    fn build_keywords(input: &str, output: &str, calls: &[StoredToolCall]) -> Vec<String> {
        let mut words = Vec::new();
        words.extend(EpisodicMemory::extract_keywords(input));
        words.extend(EpisodicMemory::extract_keywords(output));
        for tc in calls {
            words.extend(EpisodicMemory::extract_keywords(&tc.name));
            words.extend(EpisodicMemory::extract_keywords(&tc.arguments));
        }
        words.sort();
        words.dedup();
        words
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(turn_id: &str, input: &str, output: &str, extra_keywords: &[&str]) -> EpisodicEntry {
        let mut keywords = EpisodicMemory::extract_keywords(input);
        keywords.extend(EpisodicMemory::extract_keywords(output));
        for kw in extra_keywords {
            keywords.push(kw.to_string());
        }
        keywords.sort();
        keywords.dedup();

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
    fn test_record_and_recall() {
        let mut mem = EpisodicMemory::new();
        let entry = make_entry("turn_1", "deploy the app", "deployment complete", &[]);
        mem.record(entry);

        let recalled = mem.recall("turn_1");
        assert!(recalled.is_some());
        assert_eq!(recalled.unwrap().input, "deploy the app");
    }

    #[test]
    fn test_search_by_keyword() {
        let mut mem = EpisodicMemory::new();
        mem.record(make_entry("turn_1", "deploy the app", "deployment complete", &[]));
        mem.record(make_entry("turn_2", "run tests", "all tests passed", &[]));

        let results = mem.search("deploy", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].turn_id, "turn_1");
    }

    #[test]
    fn test_search_multiple_matches() {
        let mut mem = EpisodicMemory::new();
        mem.record(make_entry("t1", "deploy backend service", "deployed", &[]));
        mem.record(make_entry("t2", "deploy frontend app", "done", &[]));
        mem.record(make_entry("t3", "run tests", "passed", &[]));

        let results = mem.search("deploy backend", 10);
        assert_eq!(results.len(), 2);
        // t1 matches both "deploy" and "backend", t2 matches only "deploy"
        assert_eq!(results[0].turn_id, "t1");
    }

    #[test]
    fn test_capacity_eviction() {
        let mut mem = EpisodicMemory::with_capacity(2);
        mem.record(make_entry("t1", "first", "done", &[]));
        mem.record(make_entry("t2", "second", "done", &[]));
        mem.record(make_entry("t3", "third", "done", &[]));

        assert_eq!(mem.len(), 2);
        assert!(mem.recall("t1").is_none()); // oldest evicted
        assert!(mem.recall("t2").is_some());
        assert!(mem.recall("t3").is_some());
    }

    #[test]
    fn test_empty_search() {
        let mem = EpisodicMemory::new();
        let results = mem.search("anything", 10);
        assert!(results.is_empty());
    }

    #[test]
    fn test_short_query_no_search() {
        let mut mem = EpisodicMemory::new();
        mem.record(make_entry("t1", "deploy app", "done", &[]));
        let results = mem.search("a", 10); // too short, filtered out
        assert!(results.is_empty());
    }

    #[test]
    fn test_iter_order() {
        let mut mem = EpisodicMemory::new();
        mem.record(make_entry("t1", "first", "done", &[]));
        mem.record(make_entry("t2", "second", "done", &[]));

        let entries: Vec<&EpisodicEntry> = mem.iter().collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].turn_id, "t1");
        assert_eq!(entries[1].turn_id, "t2");
    }

    #[test]
    fn test_extract_keywords() {
        let words = EpisodicMemory::extract_keywords("Deploy the APP to production");
        assert!(words.contains(&"deploy".to_string()));
        assert!(words.contains(&"the".to_string())); // "the" is >2 chars
        assert!(words.contains(&"app".to_string()));
        assert!(words.contains(&"production".to_string()));
    }

    #[test]
    fn test_extract_keywords_short_words_filtered() {
        let words = EpisodicMemory::extract_keywords("a an the of");
        assert!(!words.contains(&"a".to_string()));
        assert!(!words.contains(&"an".to_string()));
    }

    #[test]
    fn test_from_turn_builds_entry() {
        let msgs = vec![
            ChatMessage::assistant("I will deploy the app"),
            ChatMessage::tool_result("call_1", &serde_json::json!({"status": "ok"})),
        ];
        let entry = EpisodicEntry::from_turn("turn_1", "deploy now", &msgs);
        assert_eq!(entry.turn_id, "turn_1");
        assert_eq!(entry.input, "deploy now");
        assert_eq!(entry.output, "I will deploy the app");
    }
}
