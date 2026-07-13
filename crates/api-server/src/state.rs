//! **AppState** — shared application state for the Praxis API server.
//!
//! Manages the persistent agent registry, session store, and observability
//! collector.

use praxis_core::registry::{AgentRegistry, SessionStore};
use praxis_observe::collector::TraceCollector;
use praxis_observe::exporter::SqliteExporter;
use std::path::PathBuf;
use std::sync::Arc;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    /// Persistent agent & provider registry.
    pub registry: Arc<AgentRegistry>,
    /// Persistent session store.
    pub sessions: Arc<SessionStore>,
    /// Observability trace collector.
    pub observer: Arc<TraceCollector>,
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

        // Initialise observe with SQLite backend
        let db_path = data_dir.join("observe.db");
        let exporter = SqliteExporter::open(db_path)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        let observer = Arc::new(TraceCollector::new(Arc::new(exporter)));

        Ok(Self {
            registry: Arc::new(registry),
            sessions: Arc::new(sessions),
            observer,
            data_dir,
            dist_dir,
        })
    }
}
