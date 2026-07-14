//! **AppState** — shared application state for the Praxis API server.
//!
//! Manages the persistent agent registry and session store.

use praxis_core::registry::{AgentRegistry, ProviderFactoryRegistry, SessionStore};
use praxis_core::sandbox::PendingApprovalStore;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Configuration for an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Unique name for this server configuration.
    pub name: String,
    /// The MCP server command (e.g. "npx", "node", "python").
    pub command: String,
    /// Command-line arguments.
    #[serde(default)]
    pub args: Vec<String>,
}

/// A notification from background tasks.
#[derive(Debug, Clone, Serialize)]
pub struct Notification {
    pub kind: String,
    pub message: String,
    pub timestamp: String,
}

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    /// Persistent agent & provider registry.
    pub registry: Arc<AgentRegistry>,
    /// Persistent session store.
    pub sessions: Arc<SessionStore>,
    /// Data directory path.
    pub data_dir: PathBuf,
    /// Web dist directory for static files.
    pub dist_dir: PathBuf,
    /// Request timeout in seconds for LLM calls.
    pub request_timeout_seconds: u64,
    /// Owner identifier (empty = single-user mode).
    pub owner_id: String,
    /// Pending notifications for the frontend.
    pub notifications: Arc<Mutex<Vec<Notification>>>,
    /// Provider factory registry — creates LlmClient from ProviderConfig.
    pub provider_registry: Arc<ProviderFactoryRegistry>,
    /// MCP server configurations (name → command/args).
    pub mcp_servers: Arc<Mutex<Vec<McpServerConfig>>>,
    /// Pending approval requests for interactive Ask-mode policy.
    pub approvals: PendingApprovalStore,
}

impl AppState {
    /// Create a new application state with registry + sessions in `data_dir`.
    ///
    /// # Errors
    /// Returns an I/O error if the data directory cannot be created,
    /// or the registry / session store files cannot be read.
    pub fn new(data_dir: PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(&data_dir)?;

        let registry_path = data_dir.join("registry.json");
        let registry = AgentRegistry::open(&registry_path)
            .map_err(|e| std::io::Error::other(e.to_string()))?;

        let sessions = SessionStore::open(&data_dir)?;

        let dist_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("web")
            .join("dist");

        let provider_registry = Arc::new(praxis_runtime::register_default_factories());

        let request_timeout_seconds = std::env::var("PRAXIS_TIMEOUT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);

        let owner_id = std::env::var("PRAXIS_OWNER").unwrap_or_default();

        let approvals = PendingApprovalStore::new();

        let state = Self {
            registry: Arc::new(registry),
            sessions: Arc::new(sessions),
            data_dir,
            dist_dir,
            request_timeout_seconds,
            owner_id,
            provider_registry,
            mcp_servers: Arc::new(Mutex::new(Vec::new())),
            approvals,
            notifications: Arc::new(Mutex::new(Vec::new())),
        };

        // Wire up approval notifications: every time a tool creates a pending
        // approval request, push a notification so the frontend can pick it up.
        let notifier = state.clone();
        state.approvals.set_on_pending(Box::new(move |req| {
            notifier.notify(
                "approval_created",
                format!(
                    "Tool '{}' requires approval — {}",
                    req.tool_name, req.reason
                ),
            );
        }));

        Ok(state)
    }

    /// Push a notification for the frontend.
    pub fn notify(&self, kind: impl Into<String>, message: impl Into<String>) {
        if let Ok(mut notes) = self.notifications.lock() {
            notes.push(Notification {
                kind: kind.into(),
                message: message.into(),
                timestamp: chrono::Utc::now().to_rfc3339(),
            });
        }
    }

    /// Drain all pending notifications.
    pub fn drain_notifications(&self) -> Vec<Notification> {
        self.notifications
            .lock()
            .map_or_else(|_| Vec::new(), |mut notes| std::mem::take(&mut *notes))
    }

    /// Create a minimal state for integration testing (uses temp directory).
    #[cfg(test)]
    pub fn test() -> Self {
        let tmp = std::env::temp_dir().join(format!("praxis-api-test-{}", uuid::Uuid::new_v4()));
        Self::new(tmp).expect("failed to create test AppState")
    }
}
