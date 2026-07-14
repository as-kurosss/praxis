//! **EpisodicMemory** — full verbatim history with indexed recall.
//!
//! Stores every turn (input / output / tool calls) that has been evicted from
//! working memory. Entries are indexed by extracted keywords so the agent can
//! recall relevant past context even after it has scrolled away.

use crate::agent::llm::ChatMessage;
use rusqlite::{Connection, params};
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::time::SystemTime;

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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
/// Supports two backends:
/// * **In-memory** — fast, does not persist (default with [`new`](EpisodicMemory::new)).
/// * **SQLite + FTS5** — persists to disk, full-text search via FTS5 (via [`open`](EpisodicMemory::open)).
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
#[derive(Debug)]
pub struct EpisodicMemory {
    backend: EpisodicBackend,
    /// Maximum number of entries before the oldest are evicted (in-memory only).
    max_entries: usize,
}

/// Internal backend enumeration.
#[derive(Debug)]
enum EpisodicBackend {
    /// Pure in-memory store (HashMap + keyword index).
    Memory {
        store: HashMap<String, EpisodicEntry>,
        index: HashMap<String, Vec<String>>,
        order: VecDeque<String>,
    },
    /// SQLite-backed persistent store with FTS5 full-text search.
    Sqlite {
        conn: Connection,
        /// Cache of recently accessed entries to support reference returns.
        cache: HashMap<String, EpisodicEntry>,
    },
}

// ── SQLite schema ────────────────────────────────────────────────────────────

const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS episodic_entries (
    turn_id      TEXT PRIMARY KEY,
    session_id   TEXT NOT NULL DEFAULT '',
    agent_id     TEXT NOT NULL DEFAULT '',
    input        TEXT NOT NULL,
    output       TEXT NOT NULL DEFAULT '',
    tool_calls   TEXT NOT NULL DEFAULT '[]',
    keywords     TEXT NOT NULL DEFAULT '[]',
    timestamp    INTEGER NOT NULL
);

CREATE VIRTUAL TABLE IF NOT EXISTS episodic_fts USING fts5(
    turn_id UNINDEXED,
    input, output, keywords,
    tokenize='porter unicode61'
);

CREATE INDEX IF NOT EXISTS idx_episodic_session ON episodic_entries(session_id);

CREATE TABLE IF NOT EXISTS capped_tool_results (
    tool_call_id TEXT PRIMARY KEY,
    tool_name    TEXT NOT NULL,
    arguments    TEXT NOT NULL DEFAULT '{}',
    result       TEXT NOT NULL
);
";

impl Default for EpisodicMemory {
    fn default() -> Self {
        Self {
            backend: EpisodicBackend::Memory {
                store: HashMap::new(),
                index: HashMap::new(),
                order: VecDeque::new(),
            },
            max_entries: 10_000,
        }
    }
}

impl EpisodicMemory {
    /// Create a new empty in-memory episodic memory with default capacity (10 000 entries).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an episodic memory with a custom maximum entry count (in-memory only).
    #[must_use]
    pub fn with_capacity(max_entries: usize) -> Self {
        Self {
            max_entries,
            ..Self::default()
        }
    }

    /// Open a persistent SQLite-backed episodic memory at the given path.
    ///
    /// Creates the database file and schema if they do not exist.
    /// Uses WAL mode and FTS5 for full-text search.
    ///
    /// # Errors
    /// Returns an error if the database cannot be opened or the schema cannot be created.
    pub fn open(path: impl AsRef<Path>) -> crate::error::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(SCHEMA_SQL)?;
        Ok(Self {
            backend: EpisodicBackend::Sqlite {
                conn,
                cache: HashMap::new(),
            },
            max_entries: 10_000,
        })
    }

    /// Open an in-memory SQLite-backed episodic memory (for testing).
    ///
    /// # Errors
    /// Returns an error if the database cannot be created.
    pub fn open_in_memory() -> crate::error::Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(SCHEMA_SQL)?;
        Ok(Self {
            backend: EpisodicBackend::Sqlite {
                conn,
                cache: HashMap::new(),
            },
            max_entries: 10_000,
        })
    }

    /// Returns `true` if this store is backed by SQLite.
    #[must_use]
    pub fn is_persistent(&self) -> bool {
        matches!(self.backend, EpisodicBackend::Sqlite { .. })
    }

    /// Record a new episodic entry.
    ///
    /// For the in-memory backend, if the store is at capacity the oldest entry
    /// is evicted first. For the SQLite backend, the entry is written directly
    /// to the database (capacity limits are not enforced — the DB can grow).
    pub fn record(&mut self, entry: EpisodicEntry) {
        match &mut self.backend {
            EpisodicBackend::Memory {
                store,
                index,
                order,
            } => {
                // Evict oldest if at capacity
                if store.len() >= self.max_entries
                    && let Some(oldest_id) = order.pop_front()
                {
                    Self::remove_entry_inner(oldest_id, store, index, order);
                }

                let turn_id = entry.turn_id.clone();

                // Index keywords
                for kw in &entry.keywords {
                    index.entry(kw.clone()).or_default().push(turn_id.clone());
                }

                order.push_back(turn_id.clone());
                store.insert(turn_id, entry);
            }
            EpisodicBackend::Sqlite { conn, cache } => {
                if let Err(e) = Self::sqlite_insert(conn, &entry) {
                    tracing::warn!("episodic: failed to insert entry: {e}");
                    return;
                }
                // Keep cache in sync — remove stale entry if present
                cache.remove(&entry.turn_id);
            }
        }
    }

    /// Search for entries whose keywords best match a query.
    ///
    /// For the in-memory backend, uses IDF-weighted keyword scoring.
    /// For the SQLite backend, uses FTS5 BM25 full-text search.
    ///
    /// Returns up to `max_results` entries, ordered by relevance.
    #[must_use]
    pub fn search(&mut self, query: &str, max_results: usize) -> Vec<&EpisodicEntry> {
        match &mut self.backend {
            EpisodicBackend::Memory {
                store,
                index,
                order,
            } => Self::search_memory(store, index, order, query, max_results),
            EpisodicBackend::Sqlite { conn, cache } => {
                Self::search_sqlite(conn, cache, query, max_results)
            }
        }
    }

    /// Recall a specific entry by its turn ID.
    #[must_use]
    pub fn recall(&mut self, turn_id: &str) -> Option<&EpisodicEntry> {
        match &mut self.backend {
            EpisodicBackend::Memory { store, .. } => store.get(turn_id),
            EpisodicBackend::Sqlite { conn, cache } => Self::recall_sqlite(conn, cache, turn_id),
        }
    }

    /// Total number of stored entries.
    #[must_use]
    pub fn len(&self) -> usize {
        match &self.backend {
            EpisodicBackend::Memory { store, .. } => store.len(),
            EpisodicBackend::Sqlite { conn, .. } => conn
                .query_row("SELECT COUNT(*) FROM episodic_entries", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap_or(0) as usize,
        }
    }

    /// Whether the store is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Iterate over all entries (oldest first).
    ///
    /// For the SQLite backend this loads all entries into the cache.
    #[must_use]
    pub fn iter(&mut self) -> Vec<&EpisodicEntry> {
        match &mut self.backend {
            EpisodicBackend::Memory { store, order, .. } => {
                order.iter().filter_map(|id| store.get(id)).collect()
            }
            EpisodicBackend::Sqlite { conn, cache } => {
                let mut stmt = match conn.prepare(
                    "SELECT turn_id, session_id, agent_id, input, output, \
                     tool_calls, keywords, timestamp \
                     FROM episodic_entries ORDER BY timestamp ASC",
                ) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!("episodic: iter prepare failed: {e}");
                        return Vec::new();
                    }
                };

                let rows = match stmt.query_map([], Self::row_to_entry) {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!("episodic: iter query failed: {e}");
                        return Vec::new();
                    }
                };

                // Collect into a Vec first to release the statement borrow,
                // then populate the cache.
                let loaded: Vec<EpisodicEntry> = rows.filter_map(|r| r.ok()).collect();
                let ordered_ids: Vec<String> = loaded.iter().map(|e| e.turn_id.clone()).collect();

                for entry in loaded {
                    cache.entry(entry.turn_id.clone()).or_insert(entry);
                }

                ordered_ids
                    .into_iter()
                    .filter_map(|id| cache.get(&id))
                    .collect()
            }
        }
    }

    /// Remove all entries (both backends).
    pub fn clear(&mut self) {
        match &mut self.backend {
            EpisodicBackend::Memory {
                store,
                index,
                order,
            } => {
                store.clear();
                index.clear();
                order.clear();
            }
            EpisodicBackend::Sqlite { conn, cache } => {
                let _ = conn.execute_batch(
                    "DELETE FROM episodic_entries; \
                     DELETE FROM episodic_fts;",
                );
                cache.clear();
            }
        }
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

    // ── Internal: remove_entry for Memory backend ──────────────────────────

    fn remove_entry_inner(
        turn_id: String,
        store: &mut HashMap<String, EpisodicEntry>,
        index: &mut HashMap<String, Vec<String>>,
        order: &mut VecDeque<String>,
    ) {
        if let Some(entry) = store.remove(&turn_id) {
            for kw in &entry.keywords {
                if let Some(ids) = index.get_mut(kw) {
                    ids.retain(|id| id != &turn_id);
                    if ids.is_empty() {
                        index.remove(kw);
                    }
                }
            }
        }
        order.retain(|id| id != &turn_id);
    }

    // ── Internal: in-memory search ────────────────────────────────────────

    fn search_memory<'a>(
        store: &'a HashMap<String, EpisodicEntry>,
        index: &HashMap<String, Vec<String>>,
        order: &VecDeque<String>,
        query: &str,
        max_results: usize,
    ) -> Vec<&'a EpisodicEntry> {
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

        let total_entries = store.len();
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for qkw in &query_keywords {
            if let Some(ids) = index.get(qkw) {
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

        scores.sort_by(|a, b| {
            let score_cmp = b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal);
            if score_cmp != std::cmp::Ordering::Equal {
                return score_cmp;
            }
            let pos_a = order.iter().position(|id| id == a.0);
            let pos_b = order.iter().position(|id| id == b.0);
            pos_b.cmp(&pos_a)
        });

        scores
            .into_iter()
            .take(max_results)
            .filter_map(|(id, _)| store.get(id))
            .collect()
    }

    // ── Internal: SQLite helpers ──────────────────────────────────────────

    fn sqlite_insert(conn: &Connection, entry: &EpisodicEntry) -> rusqlite::Result<()> {
        let tool_calls_json =
            serde_json::to_string(&entry.tool_calls).unwrap_or_else(|_| "[]".to_string());
        let keywords_json =
            serde_json::to_string(&entry.keywords).unwrap_or_else(|_| "[]".to_string());
        let timestamp_secs = entry
            .timestamp
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        conn.execute(
            "INSERT OR REPLACE INTO episodic_entries \
             (turn_id, session_id, agent_id, input, output, tool_calls, keywords, timestamp) \
             VALUES (?1, '', '', ?2, ?3, ?4, ?5, ?6)",
            params![
                entry.turn_id,
                entry.input,
                entry.output,
                tool_calls_json,
                keywords_json,
                timestamp_secs,
            ],
        )?;

        // Also insert into the FTS index
        let fts_keywords = entry.keywords.join(" ");
        conn.execute(
            "INSERT OR REPLACE INTO episodic_fts (turn_id, input, output, keywords) \
             VALUES (?1, ?2, ?3, ?4)",
            params![entry.turn_id, entry.input, entry.output, fts_keywords],
        )?;

        Ok(())
    }

    fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<EpisodicEntry> {
        let turn_id: String = row.get("turn_id")?;
        let input: String = row.get("input")?;
        let output: String = row.get("output")?;
        let tool_calls_json: String = row.get("tool_calls")?;
        let keywords_json: String = row.get("keywords")?;
        let timestamp_secs: i64 = row.get("timestamp")?;

        let tool_calls: Vec<StoredToolCall> = serde_json::from_str(&tool_calls_json)
            .unwrap_or_else(|e| {
                tracing::warn!("episodic: failed to parse tool_calls JSON: {e}, using default");
                Vec::new()
            });
        let keywords: Vec<String> = serde_json::from_str(&keywords_json).unwrap_or_else(|e| {
            tracing::warn!("episodic: failed to parse keywords JSON: {e}, using default");
            Vec::new()
        });

        let timestamp = SystemTime::UNIX_EPOCH
            .checked_add(std::time::Duration::from_secs(timestamp_secs.max(0) as u64))
            .unwrap_or(SystemTime::UNIX_EPOCH);

        Ok(EpisodicEntry {
            turn_id,
            timestamp,
            input,
            output,
            tool_calls,
            keywords,
        })
    }

    fn search_sqlite<'a>(
        conn: &Connection,
        cache: &'a mut HashMap<String, EpisodicEntry>,
        query: &str,
        max_results: usize,
    ) -> Vec<&'a EpisodicEntry> {
        if max_results == 0 || query.trim().is_empty() {
            return Vec::new();
        }

        // Build FTS5 prefix query from non-trivial words
        let fts_query: String = query
            .split_whitespace()
            .filter(|w| w.len() > 2)
            .map(|w| format!("{}*", w))
            .collect::<Vec<_>>()
            .join(" AND ");

        if fts_query.is_empty() {
            return Vec::new();
        }

        // Use a single query: join episodic_entries with FTS5 on turn_id
        let sql = "
            SELECT e.turn_id, e.session_id, e.agent_id, e.input, e.output,
                   e.tool_calls, e.keywords, e.timestamp
            FROM episodic_fts fts
            JOIN episodic_entries e ON fts.turn_id = e.turn_id
            WHERE fts MATCH ?1
            ORDER BY rank
            LIMIT ?2
        ";

        let mut stmt = match conn.prepare(sql) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("episodic: FTS5 search prepare failed: {e}");
                return Vec::new();
            }
        };

        let rows = match stmt.query_map(params![fts_query, max_results as i64], Self::row_to_entry)
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("episodic: FTS5 search query failed: {e}");
                return Vec::new();
            }
        };

        let mut ordered_ids = Vec::new();
        for row in rows.flatten() {
            ordered_ids.push(row.turn_id.clone());
            cache.entry(row.turn_id.clone()).or_insert(row);
        }

        ordered_ids
            .into_iter()
            .filter_map(|id| cache.get(&id))
            .collect()
    }

    fn recall_sqlite<'a>(
        conn: &Connection,
        cache: &'a mut HashMap<String, EpisodicEntry>,
        turn_id: &str,
    ) -> Option<&'a EpisodicEntry> {
        // Fast path: already in cache
        if cache.contains_key(turn_id) {
            return cache.get(turn_id);
        }

        let entry = conn
            .query_row(
                "SELECT turn_id, session_id, agent_id, input, output, \
                 tool_calls, keywords, timestamp \
                 FROM episodic_entries WHERE turn_id = ?1",
                params![turn_id],
                Self::row_to_entry,
            )
            .ok()?;

        let tid = entry.turn_id.clone();
        cache.insert(tid, entry);
        cache.get(turn_id)
    }

    /// Store a capped tool result in the SQLite backend.
    ///
    /// Returns `true` if the result was stored.  No-op for the in-memory
    /// backend (returns `false`).
    pub fn store_capped_tool_result(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        arguments: &str,
        result: &str,
    ) -> bool {
        match &self.backend {
            EpisodicBackend::Memory { .. } => false,
            EpisodicBackend::Sqlite { conn, .. } => conn
                .execute(
                    "INSERT OR REPLACE INTO capped_tool_results \
                     (tool_call_id, tool_name, arguments, result) \
                     VALUES (?1, ?2, ?3, ?4)",
                    params![tool_call_id, tool_name, arguments, result],
                )
                .is_ok(),
        }
    }

    /// Recall a previously capped tool result by its `tool_call_id`.
    ///
    /// Returns `(tool_name, arguments_json, result_json)` on success, or
    /// `None` if the ID is not found or the backend is in-memory.
    #[must_use]
    pub fn recall_tool(&self, tool_call_id: &str) -> Option<(String, String, String)> {
        match &self.backend {
            EpisodicBackend::Memory { .. } => None,
            EpisodicBackend::Sqlite { conn, .. } => conn
                .query_row(
                    "SELECT tool_name, arguments, result \
                     FROM capped_tool_results WHERE tool_call_id = ?1",
                    params![tool_call_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .ok(),
        }
    }

    /// Remove an entry by its turn ID.
    ///
    /// Removes from both the internal store/FTS index and the cache.
    /// Returns `true` if the entry existed (in-memory backend) — for the
    /// SQLite backend it always returns `true` (best-effort).
    pub fn remove(&mut self, turn_id: &str) -> bool {
        match &mut self.backend {
            EpisodicBackend::Memory {
                store,
                index,
                order,
            } => {
                if let Some(entry) = store.remove(turn_id) {
                    for kw in &entry.keywords {
                        if let Some(ids) = index.get_mut(kw) {
                            ids.retain(|id| id != turn_id);
                            if ids.is_empty() {
                                index.remove(kw);
                            }
                        }
                    }
                    order.retain(|id| id != turn_id);
                    true
                } else {
                    false
                }
            }
            EpisodicBackend::Sqlite { conn, cache } => {
                cache.remove(turn_id);
                let _ = conn.execute(
                    "DELETE FROM episodic_entries WHERE turn_id = ?1",
                    params![turn_id],
                );
                let _ = conn.execute(
                    "DELETE FROM episodic_fts WHERE turn_id = ?1",
                    params![turn_id],
                );
                true
            }
        }
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
            if let Some(content) = &msg.content
                && msg.role == crate::agent::llm::Role::Assistant
            {
                if !output.is_empty() {
                    output.push('\n');
                }
                output.push_str(content);
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

    fn make_entry(
        turn_id: &str,
        input: &str,
        output: &str,
        extra_keywords: &[&str],
    ) -> EpisodicEntry {
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
        mem.record(make_entry(
            "turn_1",
            "deploy the app",
            "deployment complete",
            &[],
        ));
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
        let mut mem = EpisodicMemory::new();
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

        let entries = mem.iter();
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
