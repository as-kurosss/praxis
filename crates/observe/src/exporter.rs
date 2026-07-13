//! **Trace exporter** — defines the `TraceExporter` trait and its SQLite
//! implementation.
//!
//! The exporter is responsible for persisting traces, spans, and metrics
//! to a storage backend. The open-source implementation uses SQLite.

use async_trait::async_trait;

use crate::db::Database;
use crate::error::ObserveError;
use crate::trace::{Metric, Span, Trace, TraceFilter};

/// Abstract interface for exporting observability data.
///
/// Implementations can write to SQLite, PostgreSQL, or other backends.
#[async_trait]
pub trait TraceExporter: Send + Sync {
    /// Export a single trace.
    async fn export_trace(&self, trace: &Trace) -> Result<(), ObserveError>;

    /// Export a single span.
    async fn export_span(&self, span: &Span) -> Result<(), ObserveError>;

    /// Export a single metric.
    async fn export_metric(&self, metric: &Metric) -> Result<(), ObserveError>;

    /// Export a batch of traces, spans, and metrics atomically.
    async fn export_batch(
        &self,
        traces: &[Trace],
        spans: &[Span],
        metrics: &[Metric],
    ) -> Result<(), ObserveError>;

    /// Query traces with optional filters.
    async fn query_traces(&self, filter: &TraceFilter) -> Result<Vec<Trace>, ObserveError>;

    /// Query spans for a given trace.
    async fn query_spans(&self, trace_id: &str) -> Result<Vec<Span>, ObserveError>;

    /// Delete traces older than the specified retention period.
    async fn purge_old_traces(&self, retention: chrono::Duration) -> Result<u64, ObserveError>;

    /// Query metrics with optional filters.
    async fn query_metrics(
        &self,
        name: Option<&str>,
        start_after: Option<chrono::DateTime<chrono::Utc>>,
        start_before: Option<chrono::DateTime<chrono::Utc>>,
        limit: Option<u64>,
    ) -> Result<Vec<Metric>, ObserveError>;

    /// Get aggregated dashboard stats.
    async fn dashboard_stats(
        &self,
        since: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<(u64, u64, u64, Option<f64>, u64), ObserveError>;
}

/// SQLite-backed implementation of [`TraceExporter`].
///
/// Operations are offloaded to a blocking thread pool via `spawn_blocking`.
#[derive(Clone)]
pub struct SqliteExporter {
    db: std::sync::Arc<Database>,
}

impl SqliteExporter {
    /// Create a new SQLite exporter that writes to the given database path.
    ///
    /// # Errors
    /// Returns an error if the database cannot be opened or migrations fail.
    pub fn open(path: impl Into<std::path::PathBuf>) -> Result<Self, ObserveError> {
        let db = Database::open(path.into())?;
        Ok(Self {
            db: std::sync::Arc::new(db),
        })
    }

    /// Create a new SQLite exporter backed by an in-memory database (testing).
    ///
    /// # Errors
    /// Returns an error if the database cannot be created.
    pub fn in_memory() -> Result<Self, ObserveError> {
        let db = Database::in_memory()?;
        Ok(Self {
            db: std::sync::Arc::new(db),
        })
    }
}

#[async_trait]
impl TraceExporter for SqliteExporter {
    async fn export_trace(&self, trace: &Trace) -> Result<(), ObserveError> {
        let db = std::sync::Arc::clone(&self.db);
        let trace = trace.clone();
        tokio::task::spawn_blocking(move || db.insert_trace(&trace))
            .await
            .map_err(|e| ObserveError::Internal(format!("Task join failed: {e}")))?
    }

    async fn export_span(&self, span: &Span) -> Result<(), ObserveError> {
        let db = std::sync::Arc::clone(&self.db);
        let span = span.clone();
        tokio::task::spawn_blocking(move || db.insert_span(&span))
            .await
            .map_err(|e| ObserveError::Internal(format!("Task join failed: {e}")))?
    }

    async fn export_metric(&self, metric: &Metric) -> Result<(), ObserveError> {
        let db = std::sync::Arc::clone(&self.db);
        let metric = metric.clone();
        tokio::task::spawn_blocking(move || db.insert_metric(&metric))
            .await
            .map_err(|e| ObserveError::Internal(format!("Task join failed: {e}")))?
    }

    async fn export_batch(
        &self,
        traces: &[Trace],
        spans: &[Span],
        metrics: &[Metric],
    ) -> Result<(), ObserveError> {
        let traces = traces.to_vec();
        let spans = spans.to_vec();
        let metrics = metrics.to_vec();
        let db = std::sync::Arc::clone(&self.db);

        tokio::task::spawn_blocking(move || {
            for trace in &traces {
                db.insert_trace(trace)?;
            }
            for span in &spans {
                db.insert_span(span)?;
            }
            for metric in &metrics {
                db.insert_metric(metric)?;
            }
            Ok(())
        })
        .await
        .map_err(|e| ObserveError::Internal(format!("Task join failed: {e}")))?
    }

    async fn query_traces(&self, filter: &TraceFilter) -> Result<Vec<Trace>, ObserveError> {
        let filter = filter.clone();
        let db = std::sync::Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || db.query_traces(&filter))
            .await
            .map_err(|e| ObserveError::Internal(format!("Task join failed: {e}")))?
    }

    async fn query_spans(&self, trace_id: &str) -> Result<Vec<Span>, ObserveError> {
        let trace_id = trace_id.to_string();
        let db = std::sync::Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || db.query_spans(&trace_id))
            .await
            .map_err(|e| ObserveError::Internal(format!("Task join failed: {e}")))?
    }

    async fn purge_old_traces(&self, retention: chrono::Duration) -> Result<u64, ObserveError> {
        let db = std::sync::Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || db.purge_old_traces(retention))
            .await
            .map_err(|e| ObserveError::Internal(format!("Task join failed: {e}")))?
    }

    async fn query_metrics(
        &self,
        name: Option<&str>,
        start_after: Option<chrono::DateTime<chrono::Utc>>,
        start_before: Option<chrono::DateTime<chrono::Utc>>,
        limit: Option<u64>,
    ) -> Result<Vec<Metric>, ObserveError> {
        let name = name.map(|s| s.to_string());
        let db = std::sync::Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || {
            db.query_metrics(
                name.as_deref(),
                start_after.as_ref(),
                start_before.as_ref(),
                limit,
            )
        })
        .await
        .map_err(|e| ObserveError::Internal(format!("Task join failed: {e}")))?
    }

    async fn dashboard_stats(
        &self,
        since: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<(u64, u64, u64, Option<f64>, u64), ObserveError> {
        let db = std::sync::Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || db.dashboard_stats(since.as_ref()))
            .await
            .map_err(|e| ObserveError::Internal(format!("Task join failed: {e}")))?
    }
}
