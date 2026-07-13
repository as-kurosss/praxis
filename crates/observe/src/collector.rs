//! **Trace collector** — in-memory collection of traces, spans, and metrics
//! with periodic flushing to an exporter backend.
//!
//! The [`TraceCollector`] provides a synchronous API for recording
//! observability data. Collected data is buffered in memory and
//! periodically flushed to the configured exporter.

use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::warn;

use crate::error::ObserveError;
use crate::exporter::TraceExporter;
use crate::trace::{Metric, Span, SpanName, Trace, TraceFilter, TraceId, TraceStatus};

/// Identifier returned when a new trace is started.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TraceHandle {
    /// The trace identifier.
    pub trace_id: TraceId,
}

/// Identifier returned when a new span is started.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SpanHandle {
    /// The span identifier.
    pub span_id: String,
}

/// In-memory collector that buffers traces, spans, and metrics before
/// flushing them to an exporter.
///
/// The collector is thread-safe and can be shared across async tasks.
pub struct TraceCollector {
    exporter: Arc<dyn TraceExporter>,
    pending_traces: Mutex<Vec<Trace>>,
    pending_spans: Mutex<Vec<Span>>,
    pending_metrics: Mutex<Vec<Metric>>,
    active_traces: Mutex<HashMap<TraceId, Trace>>,
    active_spans: Mutex<HashMap<String, Span>>,
}

impl TraceCollector {
    /// Create a new collector that writes to the given exporter.
    #[must_use]
    pub fn new(exporter: Arc<dyn TraceExporter>) -> Self {
        Self {
            exporter,
            pending_traces: Mutex::new(Vec::new()),
            pending_spans: Mutex::new(Vec::new()),
            pending_metrics: Mutex::new(Vec::new()),
            active_traces: Mutex::new(HashMap::new()),
            active_spans: Mutex::new(HashMap::new()),
        }
    }

    /// Start a new trace for the given agent and optional session.
    pub async fn start_trace(&self, agent_id: &str, session_id: Option<&str>) -> TraceHandle {
        let trace = Trace {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: agent_id.to_string(),
            session_id: session_id.map(String::from),
            start_time: Utc::now(),
            end_time: None,
            status: TraceStatus::Active,
            token_count: None,
            error: None,
        };

        let handle = TraceHandle {
            trace_id: trace.id.clone(),
        };

        let mut active = self.active_traces.lock().await;
        active.insert(trace.id.clone(), trace);

        handle
    }

    /// Start a new span within a trace.
    ///
    /// `parent_span_id` can be `None` for root spans.
    pub async fn start_span(
        &self,
        trace_id: &str,
        parent_span_id: Option<&str>,
        name: SpanName,
    ) -> SpanHandle {
        let span = Span {
            id: uuid::Uuid::new_v4().to_string(),
            trace_id: trace_id.to_string(),
            parent_span_id: parent_span_id.map(String::from),
            name,
            start_time: Utc::now(),
            end_time: None,
            metadata: serde_json::Value::Null,
            token_count: None,
        };

        let handle = SpanHandle {
            span_id: span.id.clone(),
        };

        let mut active = self.active_spans.lock().await;
        active.insert(span.id.clone(), span);

        handle
    }

    /// End a span, recording its final metadata and token count.
    ///
    /// # Errors
    /// Returns an error if the span is not found in the active set.
    pub async fn end_span(
        &self,
        span_id: &str,
        metadata: serde_json::Value,
        token_count: Option<u64>,
    ) -> Result<(), ObserveError> {
        let mut active = self.active_spans.lock().await;
        let mut span = active
            .remove(span_id)
            .ok_or_else(|| ObserveError::NotFound(format!("Span not found: {span_id}")))?;

        span.end_time = Some(Utc::now());
        span.metadata = metadata;
        span.token_count = token_count;

        // If there's an active trace, aggregate token count.
        {
            let mut traces = self.active_traces.lock().await;
            if let Some(trace) = traces.get_mut(&span.trace_id) {
                trace.token_count = Some(trace.token_count.unwrap_or(0) + token_count.unwrap_or(0));
            }
        }

        let mut pending = self.pending_spans.lock().await;
        pending.push(span);

        Ok(())
    }

    /// End a trace, recording its final status and optional error.
    ///
    /// # Errors
    /// Returns an error if the trace is not found in the active set.
    pub async fn end_trace(
        &self,
        trace_id: &str,
        status: TraceStatus,
        error: Option<String>,
    ) -> Result<(), ObserveError> {
        let mut active = self.active_traces.lock().await;
        let mut trace = active
            .remove(trace_id)
            .ok_or_else(|| ObserveError::NotFound(format!("Trace not found: {trace_id}")))?;

        trace.end_time = Some(Utc::now());
        trace.status = status;
        trace.error = error;

        let mut pending = self.pending_traces.lock().await;
        pending.push(trace);

        Ok(())
    }

    /// Record a metric observation.
    pub async fn record_metric(&self, name: &str, value: f64, tags: serde_json::Value) {
        let metric = Metric {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            value,
            tags,
            timestamp: Utc::now(),
        };

        let mut pending = self.pending_metrics.lock().await;
        pending.push(metric);
    }

    /// Flush all pending data to the exporter.
    ///
    /// This drains the in-memory buffers and writes everything to the
    /// configured storage backend.
    ///
    /// # Errors
    /// Returns an error if the export fails.
    pub async fn flush(&self) -> Result<(), ObserveError> {
        let traces = {
            let mut pending = self.pending_traces.lock().await;
            std::mem::take(&mut *pending)
        };

        let spans = {
            let mut pending = self.pending_spans.lock().await;
            std::mem::take(&mut *pending)
        };

        let metrics = {
            let mut pending = self.pending_metrics.lock().await;
            std::mem::take(&mut *pending)
        };

        if traces.is_empty() && spans.is_empty() && metrics.is_empty() {
            return Ok(());
        }

        self.exporter.export_batch(&traces, &spans, &metrics).await
    }

    /// Query traces from the backend.
    ///
    /// # Errors
    /// Returns an error if the query fails.
    pub async fn query_traces(&self, filter: &TraceFilter) -> Result<Vec<Trace>, ObserveError> {
        self.exporter.query_traces(filter).await
    }

    /// Query spans for a given trace.
    ///
    /// # Errors
    /// Returns an error if the query fails.
    pub async fn query_spans(&self, trace_id: &str) -> Result<Vec<Span>, ObserveError> {
        self.exporter.query_spans(trace_id).await
    }

    /// Query metrics with optional filters.
    ///
    /// # Errors
    /// Returns an error if the query fails.
    pub async fn query_metrics(
        &self,
        name: Option<&str>,
        start_after: Option<chrono::DateTime<chrono::Utc>>,
        start_before: Option<chrono::DateTime<chrono::Utc>>,
        limit: Option<u64>,
    ) -> Result<Vec<Metric>, ObserveError> {
        self.exporter
            .query_metrics(name, start_after, start_before, limit)
            .await
    }

    /// Get aggregated dashboard stats.
    ///
    /// # Errors
    /// Returns an error if the query fails.
    pub async fn dashboard_stats(
        &self,
        since: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<(u64, u64, u64, Option<f64>, u64), ObserveError> {
        self.exporter.dashboard_stats(since).await
    }

    /// Spawn a background task that periodically flushes pending data.
    ///
    /// The task runs until the returned [`tokio::sync::watch::Receiver`]
    /// is dropped or `shutdown` is signalled. When dropped, a final flush
    /// is attempted.
    ///
    /// # Panics
    /// Never panics in normal operation; errors during flush are logged via
    /// `tracing::warn!`.
    pub fn spawn_flush_task(
        self: &Arc<Self>,
        interval: std::time::Duration,
    ) -> tokio::sync::watch::Sender<bool> {
        let (tx, mut rx) = tokio::sync::watch::channel(false);
        let collector = Arc::clone(self);

        let flush_duration = interval;
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(flush_duration) => {
                        if let Err(e) = collector.flush().await {
                            warn!(error = %e, "Observe flush failed");
                        }
                    }
                    _ = rx.changed() => {
                        // Shutdown signal received
                        if *rx.borrow() {
                            let _ = collector.flush().await;
                            break;
                        }
                    }
                }
            }
        });

        tx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exporter::SqliteExporter;

    fn create_collector() -> (Arc<TraceCollector>, Arc<SqliteExporter>) {
        let exporter = Arc::new(SqliteExporter::in_memory().unwrap());
        let collector = Arc::new(TraceCollector::new(
            Arc::clone(&exporter) as Arc<dyn TraceExporter>
        ));
        (collector, exporter)
    }

    #[tokio::test]
    async fn test_start_and_end_trace() {
        let (collector, _exporter) = create_collector();

        let handle = collector.start_trace("agent-1", Some("session-1")).await;
        collector
            .end_trace(&handle.trace_id, TraceStatus::Completed, None)
            .await
            .unwrap();

        collector.flush().await.unwrap();

        let traces = collector
            .query_traces(&TraceFilter::default())
            .await
            .unwrap();
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].agent_id, "agent-1");
        assert_eq!(traces[0].status, TraceStatus::Completed);
    }

    #[tokio::test]
    async fn test_start_and_end_span() {
        let (collector, _exporter) = create_collector();

        let trace = collector.start_trace("agent-1", None).await;
        let span = collector
            .start_span(&trace.trace_id, None, SpanName::LlmCall)
            .await;

        collector
            .end_span(
                &span.span_id,
                serde_json::json!({"model": "gpt-4"}),
                Some(150),
            )
            .await
            .unwrap();

        collector
            .end_trace(&trace.trace_id, TraceStatus::Completed, None)
            .await
            .unwrap();

        collector.flush().await.unwrap();

        let spans = collector.query_spans(&trace.trace_id).await.unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].name, SpanName::LlmCall);
    }

    #[tokio::test]
    async fn test_record_metric() {
        let (collector, _exporter) = create_collector();

        collector
            .record_metric("latency_ms", 123.45, serde_json::json!({"agent": "test"}))
            .await;

        collector.flush().await.unwrap();
    }

    #[tokio::test]
    async fn test_end_trace_error_not_found() {
        let (collector, _exporter) = create_collector();

        let result = collector
            .end_trace(
                "non-existent",
                TraceStatus::Failed,
                Some("error".to_string()),
            )
            .await;
        assert!(result.is_err());
    }
}
