//! **Database layer** — SQLite schema, migrations, and query helpers.
//!
//! Manages the `traces`, `spans`, and `metrics` tables, plus a full-text
//! search (FTS5) index on traces for efficient searching.

use chrono::{DateTime, Utc};
use rusqlite::{Connection, Result as SqlResult, params};
use std::path::Path;
use std::sync::Mutex;

use crate::error::ObserveError;
use crate::trace::{Metric, Span, SpanName, Trace, TraceFilter, TraceStatus};

/// Wraps a SQLite connection behind a `Mutex` for thread-safe access.
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    /// Open (or create) a SQLite database at `path` and run migrations.
    ///
    /// # Errors
    /// Returns an error if the database cannot be opened or migrations fail.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ObserveError> {
        let conn = Connection::open(path.as_ref())?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    /// Create an in-memory SQLite database (useful for testing).
    ///
    /// # Errors
    /// Returns an error if the database cannot be created or migrations fail.
    pub fn in_memory() -> Result<Self, ObserveError> {
        let conn = Connection::open_in_memory()?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    /// Run schema migrations (idempotent — uses `CREATE TABLE IF NOT EXISTS`).
    fn migrate(&self) -> Result<(), ObserveError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| ObserveError::Internal(format!("Failed to lock database: {e}")))?;

        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS traces (
                id          TEXT PRIMARY KEY,
                agent_id    TEXT NOT NULL,
                session_id  TEXT,
                start_time  TEXT NOT NULL,
                end_time    TEXT,
                status      TEXT NOT NULL DEFAULT 'active',
                token_count INTEGER,
                error       TEXT
            );

            CREATE TABLE IF NOT EXISTS spans (
                id              TEXT PRIMARY KEY,
                trace_id        TEXT NOT NULL REFERENCES traces(id) ON DELETE CASCADE,
                parent_span_id  TEXT,
                name            TEXT NOT NULL,
                start_time      TEXT NOT NULL,
                end_time        TEXT,
                metadata        TEXT NOT NULL DEFAULT '{}',
                token_count     INTEGER,
                FOREIGN KEY (trace_id) REFERENCES traces(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS metrics (
                id          TEXT PRIMARY KEY,
                name        TEXT NOT NULL,
                value       REAL NOT NULL,
                tags        TEXT NOT NULL DEFAULT '{}',
                timestamp   TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_spans_trace_id ON spans(trace_id);
            CREATE INDEX IF NOT EXISTS idx_traces_agent_id ON traces(agent_id);
            CREATE INDEX IF NOT EXISTS idx_traces_start_time ON traces(start_time);
            CREATE INDEX IF NOT EXISTS idx_metrics_name ON metrics(name);
            CREATE INDEX IF NOT EXISTS idx_metrics_timestamp ON metrics(timestamp);
            ",
        )?;

        // FTS5 index for full-text search on traces (best-effort; some builds
        // may not include FTS5).
        let _ = conn.execute_batch(
            "
            CREATE VIRTUAL TABLE IF NOT EXISTS traces_fts USING fts5(
                id UNINDEXED,
                agent_id,
                error,
                content='traces',
                content_rowid='rowid'
            );
            ",
        );

        Ok(())
    }

    /// Insert a single trace into the database.
    pub fn insert_trace(&self, trace: &Trace) -> Result<(), ObserveError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| ObserveError::Internal(format!("Failed to lock database: {e}")))?;

        conn.execute(
            "INSERT INTO traces (id, agent_id, session_id, start_time, end_time, status, token_count, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                end_time = COALESCE(?5, end_time),
                status = COALESCE(?6, status),
                token_count = COALESCE(?7, token_count),
                error = COALESCE(?8, error)",
            params![
                trace.id,
                trace.agent_id,
                trace.session_id,
                trace.start_time.to_rfc3339(),
                trace.end_time.map(|t| t.to_rfc3339()),
                trace.status.to_string(),
                trace.token_count.map(|c| c as i64),
                trace.error,
            ],
        )?;

        Ok(())
    }

    /// Insert a single span into the database.
    pub fn insert_span(&self, span: &Span) -> Result<(), ObserveError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| ObserveError::Internal(format!("Failed to lock database: {e}")))?;

        conn.execute(
            "INSERT INTO spans (id, trace_id, parent_span_id, name, start_time, end_time, metadata, token_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                end_time = COALESCE(?6, end_time),
                metadata = COALESCE(?7, metadata),
                token_count = COALESCE(?8, token_count)",
            params![
                span.id,
                span.trace_id,
                span.parent_span_id,
                span.name.as_str(),
                span.start_time.to_rfc3339(),
                span.end_time.map(|t| t.to_rfc3339()),
                span.metadata.to_string(),
                span.token_count.map(|c| c as i64),
            ],
        )?;

        Ok(())
    }

    /// Insert a single metric into the database.
    pub fn insert_metric(&self, metric: &Metric) -> Result<(), ObserveError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| ObserveError::Internal(format!("Failed to lock database: {e}")))?;

        conn.execute(
            "INSERT INTO metrics (id, name, value, tags, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO NOTHING",
            params![
                metric.id,
                metric.name,
                metric.value,
                metric.tags.to_string(),
                metric.timestamp.to_rfc3339(),
            ],
        )?;

        Ok(())
    }

    /// Query traces with optional filters.
    pub fn query_traces(&self, filter: &TraceFilter) -> Result<Vec<Trace>, ObserveError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| ObserveError::Internal(format!("Failed to lock database: {e}")))?;

        let mut sql = String::from(
            "SELECT id, agent_id, session_id, start_time, end_time, status, token_count, error FROM traces WHERE 1=1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ref agent_id) = filter.agent_id {
            param_values.push(Box::new(agent_id.clone()));
            sql.push_str(&format!(" AND agent_id = ?{}", param_values.len()));
        }

        if let Some(ref session_id) = filter.session_id {
            param_values.push(Box::new(session_id.clone()));
            sql.push_str(&format!(" AND session_id = ?{}", param_values.len()));
        }

        if let Some(ref status) = filter.status {
            param_values.push(Box::new(status.to_string()));
            sql.push_str(&format!(" AND status = ?{}", param_values.len()));
        }

        if let Some(ref start_after) = filter.start_after {
            param_values.push(Box::new(start_after.to_rfc3339()));
            sql.push_str(&format!(" AND start_time >= ?{}", param_values.len()));
        }

        if let Some(ref start_before) = filter.start_before {
            param_values.push(Box::new(start_before.to_rfc3339()));
            sql.push_str(&format!(" AND start_time <= ?{}", param_values.len()));
        }

        sql.push_str(" ORDER BY start_time DESC");

        if let Some(limit) = filter.limit {
            param_values.push(Box::new(limit as i64));
            sql.push_str(&format!(" LIMIT ?{}", param_values.len()));
        }

        if let Some(offset) = filter.offset {
            param_values.push(Box::new(offset as i64));
            sql.push_str(&format!(" OFFSET ?{}", param_values.len()));
        }

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;

        let traces = stmt
            .query_map(params_refs.as_slice(), |row| {
                let end_time_str: Option<String> = row.get(4)?;
                let token_count_raw: Option<i64> = row.get(6)?;
                let status_str: String = row.get(5)?;

                Ok(Trace {
                    id: row.get(0)?,
                    agent_id: row.get(1)?,
                    session_id: row.get(2)?,
                    start_time: parse_datetime(&row.get::<_, String>(3)?).unwrap_or_default(),
                    end_time: end_time_str.as_deref().and_then(parse_datetime),
                    status: parse_status(&status_str),
                    token_count: token_count_raw.map(|c| c as u64),
                    error: row.get(7)?,
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;

        Ok(traces)
    }

    /// Query spans for a given trace.
    pub fn query_spans(&self, trace_id: &str) -> Result<Vec<Span>, ObserveError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| ObserveError::Internal(format!("Failed to lock database: {e}")))?;

        let mut stmt = conn.prepare(
            "SELECT id, trace_id, parent_span_id, name, start_time, end_time, metadata, token_count
             FROM spans WHERE trace_id = ?1 ORDER BY start_time ASC",
        )?;

        let spans = stmt
            .query_map(params![trace_id], |row| {
                let end_time_str: Option<String> = row.get(5)?;
                let token_count_raw: Option<i64> = row.get(7)?;
                let name_str: String = row.get(3)?;

                Ok(Span {
                    id: row.get(0)?,
                    trace_id: row.get(1)?,
                    parent_span_id: row.get(2)?,
                    name: parse_span_name(&name_str),
                    start_time: parse_datetime(&row.get::<_, String>(4)?).unwrap_or_default(),
                    end_time: end_time_str.as_deref().and_then(parse_datetime),
                    metadata: serde_json::from_str(&row.get::<_, String>(6)?)
                        .unwrap_or(serde_json::Value::Null),
                    token_count: token_count_raw.map(|c| c as u64),
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;

        Ok(spans)
    }

    /// Delete traces older than the specified retention period.
    ///
    /// Returns the number of deleted traces.
    pub fn purge_old_traces(&self, retention: chrono::Duration) -> Result<u64, ObserveError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| ObserveError::Internal(format!("Failed to lock database: {e}")))?;

        let cutoff = (Utc::now() - retention).to_rfc3339();
        let deleted = conn.execute("DELETE FROM traces WHERE start_time < ?1", params![cutoff])?;

        Ok(deleted as u64)
    }

    /// Get total trace count.
    pub fn trace_count(&self) -> Result<u64, ObserveError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| ObserveError::Internal(format!("Failed to lock database: {e}")))?;

        let count: i64 = conn.query_row("SELECT COUNT(*) FROM traces", [], |row| row.get(0))?;
        Ok(count as u64)
    }

    /// Query metrics with optional name filter and time range.
    pub fn query_metrics(
        &self,
        name: Option<&str>,
        start_after: Option<&DateTime<Utc>>,
        start_before: Option<&DateTime<Utc>>,
        limit: Option<u64>,
    ) -> Result<Vec<Metric>, ObserveError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| ObserveError::Internal(format!("Failed to lock database: {e}")))?;

        let mut sql =
            String::from("SELECT id, name, value, tags, timestamp FROM metrics WHERE 1=1");
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(name) = name {
            param_values.push(Box::new(name.to_string()));
            sql.push_str(&format!(" AND name = ?{}", param_values.len()));
        }

        if let Some(start_after) = start_after {
            param_values.push(Box::new(start_after.to_rfc3339()));
            sql.push_str(&format!(" AND timestamp >= ?{}", param_values.len()));
        }

        if let Some(start_before) = start_before {
            param_values.push(Box::new(start_before.to_rfc3339()));
            sql.push_str(&format!(" AND timestamp <= ?{}", param_values.len()));
        }

        sql.push_str(" ORDER BY timestamp DESC");

        if let Some(limit) = limit {
            param_values.push(Box::new(limit as i64));
            sql.push_str(&format!(" LIMIT ?{}", param_values.len()));
        }

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;

        let metrics = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok(Metric {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    value: row.get(2)?,
                    tags: serde_json::from_str(&row.get::<_, String>(3)?)
                        .unwrap_or(serde_json::Value::Null),
                    timestamp: parse_datetime(&row.get::<_, String>(4)?).unwrap_or_default(),
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;

        Ok(metrics)
    }

    /// Get aggregated dashboard stats.
    /// Returns (total_traces, completed_traces, failed_traces, avg_latency_ms, total_tokens).
    pub fn dashboard_stats(
        &self,
        since: Option<&DateTime<Utc>>,
    ) -> Result<(u64, u64, u64, Option<f64>, u64), ObserveError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| ObserveError::Internal(format!("Failed to lock database: {e}")))?;

        let since_str = since
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| (Utc::now() - chrono::Duration::days(1)).to_rfc3339());

        // Total traces
        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM traces WHERE start_time >= ?1",
            params![since_str],
            |row| row.get(0),
        )?;

        // Completed traces
        let completed: i64 = conn.query_row(
            "SELECT COUNT(*) FROM traces WHERE start_time >= ?1 AND status = 'completed'",
            params![since_str],
            |row| row.get(0),
        )?;

        // Failed traces
        let failed: i64 = conn.query_row(
            "SELECT COUNT(*) FROM traces WHERE start_time >= ?1 AND status = 'failed'",
            params![since_str],
            |row| row.get(0),
        )?;

        // Average latency (from traces that have finished)
        let avg_latency: Option<f64> = conn
            .query_row(
                "SELECT AVG(
                    (julianday(end_time) - julianday(start_time)) * 86400.0 * 1000.0
                 ) FROM traces
                 WHERE start_time >= ?1 AND end_time IS NOT NULL",
                params![since_str],
                |row| row.get(0),
            )
            .ok();

        // Total tokens
        let total_tokens: i64 = conn.query_row(
            "SELECT COALESCE(SUM(token_count), 0) FROM traces WHERE start_time >= ?1",
            params![since_str],
            |row| row.get(0),
        )?;

        Ok((
            total as u64,
            completed as u64,
            failed as u64,
            avg_latency,
            total_tokens as u64,
        ))
    }
}

/// Parse an RFC 3339 datetime string.
fn parse_datetime(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Parse a trace status string back into `TraceStatus`.
fn parse_status(s: &str) -> TraceStatus {
    match s {
        "active" => TraceStatus::Active,
        "completed" => TraceStatus::Completed,
        "failed" => TraceStatus::Failed,
        _ => TraceStatus::Active,
    }
}

/// Parse a span name string back into `SpanName`.
fn parse_span_name(s: &str) -> SpanName {
    match s {
        "llm_call" => SpanName::LlmCall,
        "tool_call" => SpanName::ToolCall,
        "agent_turn" => SpanName::AgentTurn,
        _ => SpanName::AgentTurn,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::{Metric, Span, SpanName, Trace, TraceFilter, TraceStatus};
    use chrono::Utc;

    fn create_test_trace(id: &str) -> Trace {
        Trace {
            id: id.to_string(),
            agent_id: "test-agent".to_string(),
            session_id: None,
            start_time: Utc::now(),
            end_time: None,
            status: TraceStatus::Active,
            token_count: None,
            error: None,
        }
    }

    #[test]
    fn test_insert_and_query_trace() {
        let db = Database::in_memory().unwrap();
        let trace = create_test_trace("test-1");
        db.insert_trace(&trace).unwrap();

        let filter = TraceFilter::default();
        let results = db.query_traces(&filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "test-1");
        assert_eq!(results[0].agent_id, "test-agent");
    }

    #[test]
    fn test_purge_old_traces() {
        let db = Database::in_memory().unwrap();

        // Insert a trace with a very old timestamp
        let mut old_trace = create_test_trace("old");
        old_trace.start_time = Utc::now() - chrono::Duration::days(30);
        db.insert_trace(&old_trace).unwrap();

        // Insert a recent trace
        let recent_trace = create_test_trace("recent");
        db.insert_trace(&recent_trace).unwrap();

        // Purge with 7-day retention
        let deleted = db.purge_old_traces(chrono::Duration::days(7)).unwrap();
        assert_eq!(deleted, 1);

        let remaining = db.trace_count().unwrap();
        assert_eq!(remaining, 1);
    }

    #[test]
    fn test_insert_and_query_span() {
        let db = Database::in_memory().unwrap();
        let trace = create_test_trace("trace-1");
        db.insert_trace(&trace).unwrap();

        let span = Span {
            id: "span-1".to_string(),
            trace_id: "trace-1".to_string(),
            parent_span_id: None,
            name: SpanName::LlmCall,
            start_time: Utc::now(),
            end_time: None,
            metadata: serde_json::json!({"model": "gpt-4"}),
            token_count: Some(150),
        };
        db.insert_span(&span).unwrap();

        let spans = db.query_spans("trace-1").unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].name, SpanName::LlmCall);
        assert_eq!(spans[0].token_count, Some(150));
    }

    #[test]
    fn test_insert_metric() {
        let db = Database::in_memory().unwrap();
        let metric = Metric {
            id: "metric-1".to_string(),
            name: "latency_ms".to_string(),
            value: 1234.5,
            tags: serde_json::json!({"agent": "test"}),
            timestamp: Utc::now(),
        };
        db.insert_metric(&metric).unwrap();
    }
}
