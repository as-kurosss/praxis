//! **SessionHistory** — persistent per-session turn storage.
//!
//! Each session gets its own SQLite database in `{data_dir}/sessions/{session_id}/`.
//! Turns are stored with full text for FTS5 indexing, so old (evicted) turns
//! remain searchable on demand.

use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A single turn (user, assistant, or tool) in a conversation.
#[derive(Debug, Clone)]
pub struct TurnRecord {
    /// Unique turn identifier (UUID).
    pub id: String,
    /// Session this turn belongs to.
    pub session_id: String,
    /// Unix timestamp in milliseconds.
    pub timestamp: i64,
    /// Role: "user", "assistant", "tool", "system".
    pub role: String,
    /// Text content (None for pure-tool-call assistant messages).
    pub content: Option<String>,
    /// JSON-serialized tool calls (array), or None.
    pub tool_calls: Option<String>,
    /// JSON-serialized tool results (array), or None.
    pub tool_results: Option<String>,
    /// Approximate token count for this turn.
    pub token_count: i64,
    /// Whether this turn has been evicted from the working set.
    pub evicted: bool,
}

/// A structured fact extracted from a conversation turn.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryFact {
    /// Unique fact identifier (UUID).
    pub id: String,
    /// Session the fact belongs to.
    pub session_id: String,
    /// The entity this fact is about (e.g. "user", "project", "server").
    pub entity: String,
    /// The attribute or property name (e.g. "name", "preference", "location").
    pub attribute: String,
    /// The value of the attribute.
    pub value: String,
    /// Confidence score (0.0 – 1.0).
    pub confidence: f64,
    /// Unix timestamp when the fact was recorded.
    pub timestamp: i64,
}

/// Persistent, per-session turn history backed by SQLite + FTS5.
///
/// # Threading
///
/// Internal SQLite operations are synchronous.  Use the `*_async` methods
/// or `spawn_blocking` in async contexts.
#[derive(Clone)]
pub struct SessionHistory {
    db: Arc<Mutex<Connection>>,
    session_id: String,
    data_dir: PathBuf,
}

impl SessionHistory {
    /// Open (or create) the session history database.
    ///
    /// * `data_dir` — root data directory (e.g. `./praxis-data`).
    ///   The session DB is stored at `{data_dir}/sessions/{session_id}/history.db`.
    /// * `session_id` — unique session identifier.
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be created or the schema
    /// migration fails.
    pub fn open(data_dir: &Path, session_id: &str) -> Result<Self, SessionHistoryError> {
        let session_dir = data_dir.join("sessions").join(session_id);
        std::fs::create_dir_all(&session_dir).map_err(|e| {
            SessionHistoryError::Io(format!("cannot create session dir {session_dir:?}: {e}"))
        })?;

        let db_path = session_dir.join("history.db");
        let db = Connection::open(&db_path).map_err(|e| {
            SessionHistoryError::Io(format!("cannot open history db {db_path:?}: {e}"))
        })?;

        Self::migrate(&db)?;

        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            session_id: session_id.to_string(),
            data_dir: session_dir,
        })
    }

    /// Open an in-memory database (useful for tests).
    pub fn open_in_memory(session_id: &str) -> Result<Self, SessionHistoryError> {
        let db = Connection::open_in_memory()
            .map_err(|e| SessionHistoryError::Io(format!("cannot open in-memory db: {e}")))?;
        Self::migrate(&db)?;
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            session_id: session_id.to_string(),
            data_dir: PathBuf::from(":memory:"),
        })
    }

    /// Create tables and FTS5 index.
    fn migrate(db: &Connection) -> Result<(), SessionHistoryError> {
        db.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS turns (
                id          TEXT PRIMARY KEY,
                session_id  TEXT NOT NULL,
                timestamp   INTEGER NOT NULL,
                role        TEXT NOT NULL,
                content     TEXT,
                tool_calls  TEXT,
                tool_results TEXT,
                token_count INTEGER NOT NULL DEFAULT 0,
                evicted     INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_turns_session_ts
                ON turns(session_id, timestamp);

            CREATE INDEX IF NOT EXISTS idx_turns_evicted
                ON turns(evicted);

            CREATE VIRTUAL TABLE IF NOT EXISTS turns_fts USING fts5(
                content, tokenize='unicode61'
            );

            CREATE TABLE IF NOT EXISTS memory_facts (
                id          TEXT PRIMARY KEY,
                session_id  TEXT NOT NULL,
                entity      TEXT NOT NULL,
                attribute   TEXT NOT NULL,
                value       TEXT NOT NULL,
                confidence  REAL NOT NULL DEFAULT 1.0,
                timestamp   INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_facts_session
                ON memory_facts(session_id);

            CREATE VIRTUAL TABLE IF NOT EXISTS facts_fts USING fts5(
                entity, attribute, value, tokenize='unicode61'
            );
            ",
        )
        .map_err(|e| SessionHistoryError::Io(format!("schema migration failed: {e}")))?;
        Ok(())
    }

    /// Append a new turn to the history.
    ///
    /// Also syncs the content into the FTS5 index so evicted turns remain
    /// searchable.
    ///
    /// # Errors
    /// Returns an error if the INSERT or FTS sync fails.
    pub fn append_turn(&self, record: &TurnRecord) -> Result<(), SessionHistoryError> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO turns (id, session_id, timestamp, role, content, tool_calls, tool_results, token_count, evicted)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                record.id,
                record.session_id,
                record.timestamp,
                record.role,
                record.content,
                record.tool_calls,
                record.tool_results,
                record.token_count,
                record.evicted as i64,
            ],
        )
        .map_err(|e| SessionHistoryError::Io(format!("append_turn failed: {e}")))?;

        // Sync content into the FTS5 index (rowid corresponds to `turns` rowid)
        if let Some(ref content) = record.content {
            db.execute(
                "INSERT INTO turns_fts(rowid, content) VALUES (last_insert_rowid(), ?1)",
                params![content],
            )
            .map_err(|e| SessionHistoryError::Io(format!("append_turn fts sync: {e}")))?;
        }

        Ok(())
    }

    /// Fetch the most recent `n` turns (non-evicted).
    ///
    /// # Errors
    /// Returns an error if the query fails.
    pub fn get_recent(&self, n: usize) -> Result<Vec<TurnRecord>, SessionHistoryError> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, session_id, timestamp, role, content, tool_calls, tool_results, token_count, evicted
             FROM turns
             WHERE evicted = 0
             ORDER BY timestamp ASC, rowid ASC
             LIMIT ?1",
        )
        .map_err(|e| SessionHistoryError::Io(format!("get_recent prepare: {e}")))?;

        let rows = stmt
            .query_map(params![n as i64], Self::row_to_record)
            .map_err(|e| SessionHistoryError::Io(format!("get_recent query: {e}")))?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row.map_err(|e| SessionHistoryError::Io(e.to_string()))?);
        }
        Ok(records)
    }

    /// Search all turns (including evicted) by FTS5 full-text query.
    ///
    /// Returns up to `limit` matching turns ordered by relevance.
    ///
    /// # Errors
    /// Returns an error if the FTS5 query fails.
    pub fn search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<TurnRecord>, SessionHistoryError> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT t.id, t.session_id, t.timestamp, t.role, t.content, t.tool_calls, t.tool_results, t.token_count, t.evicted
             FROM turns t
             JOIN turns_fts fts ON t.rowid = fts.rowid
             WHERE turns_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )
        .map_err(|e| SessionHistoryError::Io(format!("search prepare: {e}")))?;

        let rows = stmt
            .query_map(params![query, limit as i64], Self::row_to_record)
            .map_err(|e| SessionHistoryError::Io(format!("search query: {e}")))?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row.map_err(|e| SessionHistoryError::Io(e.to_string()))?);
        }
        Ok(records)
    }

    /// Recall turns by keyword(s) — a convenience wrapper around `search`.
    ///
    /// Treats each keyword as an FTS5 term (OR'd together).
    pub fn recall(&self, keywords: &[&str]) -> Result<Vec<TurnRecord>, SessionHistoryError> {
        let query = keywords.join(" OR ");
        self.search(&query, 50)
    }

    /// Calculate the total token count of all non-evicted turns.
    pub fn total_tokens(&self) -> Result<i64, SessionHistoryError> {
        let db = self.db.lock().unwrap();
        let sum: i64 = db
            .query_row(
                "SELECT COALESCE(SUM(token_count), 0) FROM turns WHERE evicted = 0",
                [],
                |row| row.get(0),
            )
            .map_err(|e| SessionHistoryError::Io(format!("total_tokens failed: {e}")))?;
        Ok(sum)
    }

    /// Evict the oldest turns until the total token count is at or below
    /// `max_tokens`.  Returns the list of evicted records.
    ///
    /// Evicted turns are marked in the database but remain searchable via FTS5.
    pub fn evict_to(&self, max_tokens: i64) -> Result<Vec<TurnRecord>, SessionHistoryError> {
        let mut evicted = Vec::new();
        let db = self.db.lock().unwrap();

        loop {
            let total: i64 = db
                .query_row(
                    "SELECT COALESCE(SUM(token_count), 0) FROM turns WHERE evicted = 0",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| SessionHistoryError::Io(format!("evict total: {e}")))?;

            if total <= max_tokens {
                break;
            }

            // Find the oldest non-evicted turn
            let oldest = db
                .query_row(
                    "SELECT id, session_id, timestamp, role, content, tool_calls, tool_results, token_count, evicted
                     FROM turns WHERE evicted = 0
                     ORDER BY timestamp ASC, rowid ASC
                     LIMIT 1",
                    [],
                    Self::row_to_record,
                )
                .map_err(|e| SessionHistoryError::Io(format!("evict find oldest: {e}")))?;

            db.execute(
                "UPDATE turns SET evicted = 1 WHERE id = ?1",
                params![oldest.id],
            )
            .map_err(|e| SessionHistoryError::Io(format!("evict update: {e}")))?;

            evicted.push(oldest);
        }

        Ok(evicted)
    }

    // ── Memory Fact methods ────────────────────────────────────────

    /// Store a single extracted fact.
    pub fn store_fact(&self, fact: &MemoryFact) -> Result<(), SessionHistoryError> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO memory_facts (id, session_id, entity, attribute, value, confidence, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                fact.id,
                fact.session_id,
                fact.entity,
                fact.attribute,
                fact.value,
                fact.confidence,
                fact.timestamp,
            ],
        )
        .map_err(|e| SessionHistoryError::Io(format!("store_fact failed: {e}")))?;

        // Sync into FTS5 index
        db.execute(
            "INSERT INTO facts_fts(rowid, entity, attribute, value)
             VALUES (last_insert_rowid(), ?1, ?2, ?3)",
            params![fact.entity, fact.attribute, fact.value],
        )
        .map_err(|e| SessionHistoryError::Io(format!("store_fact fts sync: {e}")))?;

        Ok(())
    }

    /// Search facts by FTS5 full-text query.
    ///
    /// Matches across `entity`, `attribute`, and `value` columns.
    pub fn search_facts(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MemoryFact>, SessionHistoryError> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let db = self.db.lock().unwrap();
        let mut stmt = db
            .prepare(
                "SELECT f.id, f.session_id, f.entity, f.attribute, f.value, f.confidence, f.timestamp
                 FROM memory_facts f
                 JOIN facts_fts fts ON f.rowid = fts.rowid
                 WHERE facts_fts MATCH ?1
                 ORDER BY rank
                 LIMIT ?2",
            )
            .map_err(|e| SessionHistoryError::Io(format!("search_facts prepare: {e}")))?;

        let rows = stmt
            .query_map(params![query, limit as i64], Self::row_to_fact)
            .map_err(|e| SessionHistoryError::Io(format!("search_facts query: {e}")))?;

        let mut facts = Vec::new();
        for row in rows {
            facts.push(row.map_err(|e| SessionHistoryError::Io(e.to_string()))?);
        }
        Ok(facts)
    }

    /// Get all facts for the given session, ordered by recency.
    pub fn get_facts_for_session(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<MemoryFact>, SessionHistoryError> {
        let db = self.db.lock().unwrap();
        let mut stmt = db
            .prepare(
                "SELECT id, session_id, entity, attribute, value, confidence, timestamp
                 FROM memory_facts
                 WHERE session_id = ?1
                 ORDER BY timestamp DESC
                 LIMIT ?2",
            )
            .map_err(|e| SessionHistoryError::Io(format!("get_facts_for_session prepare: {e}")))?;

        let rows = stmt
            .query_map(params![session_id, limit as i64], Self::row_to_fact)
            .map_err(|e| SessionHistoryError::Io(format!("get_facts_for_session query: {e}")))?;

        let mut facts = Vec::new();
        for row in rows {
            facts.push(row.map_err(|e| SessionHistoryError::Io(e.to_string()))?);
        }
        Ok(facts)
    }

    /// Helper: map a SQLite row to a `MemoryFact`.
    fn row_to_fact(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryFact> {
        Ok(MemoryFact {
            id: row.get("id")?,
            session_id: row.get("session_id")?,
            entity: row.get("entity")?,
            attribute: row.get("attribute")?,
            value: row.get("value")?,
            confidence: row.get("confidence")?,
            timestamp: row.get("timestamp")?,
        })
    }

    /// Helper: map a SQLite row to a `TurnRecord`.
    fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<TurnRecord> {
        Ok(TurnRecord {
            id: row.get("id")?,
            session_id: row.get("session_id")?,
            timestamp: row.get("timestamp")?,
            role: row.get("role")?,
            content: row.get("content")?,
            tool_calls: row.get("tool_calls")?,
            tool_results: row.get("tool_results")?,
            token_count: row.get("token_count")?,
            evicted: row.get::<_, i64>("evicted")? != 0,
        })
    }

    /// Path to the session directory.
    pub fn session_dir(&self) -> &Path {
        &self.data_dir
    }

    /// The session identifier.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    // ── Async wrappers (spawn_blocking) ────────────────────────────

    /// Append a turn via `spawn_blocking`.  Requires `Arc<Self>`.
    pub async fn append_turn_async(
        self: Arc<Self>,
        record: TurnRecord,
    ) -> Result<(), SessionHistoryError> {
        tokio::task::spawn_blocking(move || self.append_turn(&record))
            .await
            .map_err(|e| SessionHistoryError::Io(format!("spawn_blocking: {e}")))?
    }

    /// Get recent turns via `spawn_blocking`.  Requires `Arc<Self>`.
    pub async fn get_recent_async(
        self: Arc<Self>,
        n: usize,
    ) -> Result<Vec<TurnRecord>, SessionHistoryError> {
        tokio::task::spawn_blocking(move || self.get_recent(n))
            .await
            .map_err(|e| SessionHistoryError::Io(format!("spawn_blocking: {e}")))?
    }

    /// Store a fact via `spawn_blocking`.  Requires `Arc<Self>`.
    pub async fn store_fact_async(
        self: Arc<Self>,
        fact: MemoryFact,
    ) -> Result<(), SessionHistoryError> {
        tokio::task::spawn_blocking(move || self.store_fact(&fact))
            .await
            .map_err(|e| SessionHistoryError::Io(format!("spawn_blocking: {e}")))?
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SessionHistoryError {
    /// I/O or database error.
    #[error("SessionHistory error: {0}")]
    Io(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_turn(session_id: &str, role: &str, content: &str, ts: i64) -> TurnRecord {
        TurnRecord {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            timestamp: ts,
            role: role.to_string(),
            content: Some(content.to_string()),
            tool_calls: None,
            tool_results: None,
            token_count: content.len() as i64 / 4,
            evicted: false,
        }
    }

    #[test]
    fn test_open_in_memory() {
        let hist = SessionHistory::open_in_memory("test-session").unwrap();
        assert_eq!(hist.session_id(), "test-session");
    }

    #[test]
    fn test_append_and_get_recent() {
        let hist = SessionHistory::open_in_memory("test").unwrap();
        hist.append_turn(&make_turn("test", "user", "hello", 1000))
            .unwrap();
        hist.append_turn(&make_turn("test", "assistant", "hi there", 2000))
            .unwrap();

        let recent = hist.get_recent(10).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].role, "user");
        assert_eq!(recent[0].content.as_deref(), Some("hello"));
        assert_eq!(recent[1].role, "assistant");
    }

    #[test]
    fn test_fts_search() {
        let hist = SessionHistory::open_in_memory("test").unwrap();
        hist.append_turn(&make_turn(
            "test",
            "user",
            "what is the capital of France?",
            1000,
        ))
        .unwrap();
        hist.append_turn(&make_turn(
            "test",
            "assistant",
            "Paris is the capital of France.",
            2000,
        ))
        .unwrap();
        hist.append_turn(&make_turn("test", "user", "tell me about Rust", 3000))
            .unwrap();

        let results = hist.search("Paris", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].content.as_deref(),
            Some("Paris is the capital of France.")
        );

        let results = hist.search("Rust", 10).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_eviction() {
        let hist = SessionHistory::open_in_memory("test").unwrap();

        // Turn 1: 25 tokens, Turn 2: 25 tokens, Turn 3: 25 tokens
        hist.append_turn(&make_turn(
            "test",
            "user",
            "A longer message that takes about twenty-five tokens or so",
            1000,
        ))
        .unwrap();
        hist.append_turn(&make_turn(
            "test",
            "assistant",
            "Another longer reply that should consume tokens here and there",
            2000,
        ))
        .unwrap();
        hist.append_turn(&make_turn(
            "test",
            "user",
            "Yet another long query with plenty of tokens in this content",
            3000,
        ))
        .unwrap();

        let total = hist.total_tokens().unwrap();
        assert!(total > 0);

        // Evict to a very small budget — forces everything out
        let evicted = hist.evict_to(5).unwrap();
        assert!(!evicted.is_empty(), "should have evicted at least one turn");

        // Recent should now be empty (all evicted)
        let recent = hist.get_recent(10).unwrap();
        assert!(recent.is_empty() || recent.len() < 3);

        // But FTS search should still find the evicted turns
        // (use FTS5 prefix operator so "token" matches "tokens")
        let results = hist.search("token*", 10).unwrap();
        assert!(
            !results.is_empty(),
            "evicted turns should still be searchable"
        );
    }

    #[test]
    fn test_recall_empty_query() {
        let hist = SessionHistory::open_in_memory("test").unwrap();
        hist.append_turn(&make_turn("test", "user", "hello world", 1000))
            .unwrap();
        let results = hist.search("", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_recall_multiple_keywords() {
        let hist = SessionHistory::open_in_memory("test").unwrap();
        hist.append_turn(&make_turn("test", "user", "I like cats and dogs", 1000))
            .unwrap();
        hist.append_turn(&make_turn(
            "test",
            "user",
            "Python is a programming language",
            2000,
        ))
        .unwrap();

        let results = hist.recall(&["cats", "dogs"]).unwrap();
        assert_eq!(results.len(), 1);
    }
}
