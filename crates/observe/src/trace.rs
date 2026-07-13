//! **Trace data model** — core types for distributed tracing.
//!
//! These types model execution traces, spans (individual operations),
//! and metrics for the Praxis Observe observability subsystem.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique identifier for a trace.
pub type TraceId = String;

/// Unique identifier for a span.
pub type SpanId = String;

/// The status of a completed trace.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TraceStatus {
    /// Trace is still being collected.
    Active,
    /// Trace completed successfully.
    Completed,
    /// Trace failed with an error.
    Failed,
}

impl fmt::Display for TraceStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

/// A trace represents a complete execution flow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trace {
    /// Unique trace identifier.
    pub id: TraceId,
    /// The agent that executed this trace.
    pub agent_id: String,
    /// Optional session identifier for grouping traces.
    pub session_id: Option<String>,
    /// When the trace started.
    pub start_time: DateTime<Utc>,
    /// When the trace ended (None if still active).
    pub end_time: Option<DateTime<Utc>>,
    /// Current status of the trace.
    pub status: TraceStatus,
    /// Total token usage across all spans.
    pub token_count: Option<u64>,
    /// Error message if the trace failed.
    pub error: Option<String>,
}

/// The type of operation a span represents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SpanName {
    /// A call to an LLM provider.
    #[serde(rename = "llm_call")]
    LlmCall,
    /// A tool invocation.
    #[serde(rename = "tool_call")]
    ToolCall,
    /// A single agent turn in a conversation loop.
    #[serde(rename = "agent_turn")]
    AgentTurn,
}

impl fmt::Display for SpanName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LlmCall => write!(f, "llm_call"),
            Self::ToolCall => write!(f, "tool_call"),
            Self::AgentTurn => write!(f, "agent_turn"),
        }
    }
}

impl SpanName {
    /// Returns the serialised form of this span name.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LlmCall => "llm_call",
            Self::ToolCall => "tool_call",
            Self::AgentTurn => "agent_turn",
        }
    }
}

/// A span represents a single operation within a trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    /// Unique span identifier.
    pub id: SpanId,
    /// The trace this span belongs to.
    pub trace_id: TraceId,
    /// Parent span identifier (None for root spans).
    pub parent_span_id: Option<SpanId>,
    /// Name/type of this span.
    pub name: SpanName,
    /// When the span started.
    pub start_time: DateTime<Utc>,
    /// When the span ended (None if still active).
    pub end_time: Option<DateTime<Utc>>,
    /// Arbitrary metadata associated with this span.
    pub metadata: serde_json::Value,
    /// Token usage for this span.
    pub token_count: Option<u64>,
}

/// A metric represents a numerical observation at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metric {
    /// Unique metric identifier.
    pub id: String,
    /// Metric name (e.g. "latency_ms", "token_usage").
    pub name: String,
    /// The numeric value.
    pub value: f64,
    /// Tags for filtering and grouping metrics.
    pub tags: serde_json::Value,
    /// When the metric was recorded.
    pub timestamp: DateTime<Utc>,
}

/// Filter parameters for querying traces.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TraceFilter {
    /// Filter by agent identifier.
    pub agent_id: Option<String>,
    /// Filter by session identifier.
    pub session_id: Option<String>,
    /// Filter by status.
    pub status: Option<TraceStatus>,
    /// Only return traces started after this time.
    pub start_after: Option<DateTime<Utc>>,
    /// Only return traces started before this time.
    pub start_before: Option<DateTime<Utc>>,
    /// Maximum number of results.
    pub limit: Option<u64>,
    /// Offset for pagination.
    pub offset: Option<u64>,
    /// Full-text search query.
    pub search: Option<String>,
}

/// Filter parameters for querying spans.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SpanFilter {
    /// Filter by trace identifier.
    pub trace_id: Option<String>,
    /// Filter by span name/type.
    pub name: Option<SpanName>,
    /// Maximum number of results.
    pub limit: Option<u64>,
}
