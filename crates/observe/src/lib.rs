//! **Praxis Observe** — built-in observability subsystem inspired by
//! LangFuse / LangSmith.
//!
//! Provides tracing, span-based instrumentation, and metrics collection
//! for debugging and monitoring agent behaviour. The open-source version
//! ships with a SQLite backend and 7-day configurable retention.
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use praxis_observe::collector::TraceCollector;
//! use praxis_observe::exporter::SqliteExporter;
//! use praxis_observe::trace::{SpanName, TraceFilter, TraceStatus};
//!
//! # async fn example() {
//! let exporter = Arc::new(SqliteExporter::open("/tmp/observe.db").unwrap());
//! let collector = Arc::new(TraceCollector::new(exporter));
//!
//! // Start a trace
//! let trace = collector.start_trace("my-agent", None).await;
//!
//! // Record a span
//! let span = collector.start_span(&trace.trace_id, None, SpanName::LlmCall).await;
//! collector.end_span(&span.span_id, serde_json::json!({"model": "gpt-4"}), Some(150)).await.unwrap();
//!
//! // Complete the trace
//! collector.end_trace(&trace.trace_id, TraceStatus::Completed, None).await.unwrap();
//!
//! // Flush to database
//! collector.flush().await.unwrap();
//! # }
//! ```
//!
//! # Monetization Tiers
//!
//! | Tier | Backend | Retention | Features |
//! |------|---------|-----------|----------|
//! | **Open Source** | SQLite | 7 days (configurable) | Basic metrics, trace/spans, FTS |
//! | **Pro** | PostgreSQL | Unlimited | Advanced metrics, alerts, multi-user |
//! | **Enterprise** | Clustered | Unlimited | RBAC, audit log, clustering |
//!
//! See the [`collector`], [`exporter`], and [`trace`] modules for details.

pub mod collector;
pub mod db;
pub mod error;
pub mod exporter;
pub mod trace;

pub use error::ObserveError;
