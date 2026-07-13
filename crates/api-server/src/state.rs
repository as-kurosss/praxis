//! **AppState** — shared application state for the Praxis API server.
//!
//! Manages the persistent agent registry and session store.

use praxis_core::registry::{AgentRegistry, SessionStore};
use std::path::PathBuf;
use std::sync::Arc;

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
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        let sessions = SessionStore::open(&data_dir)?;

        let dist_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("web")
            .join("dist");

        Ok(Self {
            registry: Arc::new(registry),
            sessions: Arc::new(sessions),
            data_dir,
            dist_dir,
        })
    }
}
