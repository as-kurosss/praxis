//! **AppState** — shared application state for the Praxis API server.
//!
//! Manages in-memory storage for graph executions, approval gates, and agents.
//! All state is behind `Arc<RwLock<...>>` for concurrent access.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Runtime status of a managed resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceStatus {
    /// Resource created but not yet active.
    Idle,
    /// Currently executing.
    Running,
    /// Execution completed successfully.
    Completed,
    /// Execution failed.
    Failed,
    /// Awaiting human approval.
    Pending,
    /// Approved by a human.
    Approved,
    /// Rejected by a human.
    Rejected,
}

/// A tracked graph execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphHandle {
    /// Human-readable label.
    pub label: String,
    /// Current execution status.
    pub status: ResourceStatus,
    /// Optional result payload (JSON).
    pub result: Option<Value>,
    /// ISO-8601 creation timestamp.
    pub created_at: String,
}

/// A tracked approval gate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalHandle {
    /// Description shown to the human reviewer.
    pub prompt: String,
    /// Current approval status.
    pub status: ResourceStatus,
    /// ISO-8601 creation timestamp.
    pub created_at: String,
}

/// A tracked agent instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentHandle {
    /// Agent configuration (JSON).
    pub config: Value,
    /// Current status.
    pub status: ResourceStatus,
    /// ISO-8601 creation timestamp.
    pub created_at: String,
}

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    /// Registered graph executions, keyed by ID.
    pub graphs: Arc<RwLock<HashMap<String, GraphHandle>>>,
    /// Pending/processed approval gates, keyed by ID.
    pub approvals: Arc<RwLock<HashMap<String, ApprovalHandle>>>,
    /// Registered agents, keyed by ID.
    pub agents: Arc<RwLock<HashMap<String, AgentHandle>>>,
}

impl AppState {
    /// Create a new empty application state.
    pub fn new() -> Self {
        Self {
            graphs: Arc::new(RwLock::new(HashMap::new())),
            approvals: Arc::new(RwLock::new(HashMap::new())),
            agents: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
