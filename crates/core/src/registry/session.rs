use crate::agent::llm::ChatMessage;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// A single conversation session with an agent.
///
/// Each session is tied to one agent and contains the full message history.
/// Sessions are stored in individual JSON files under the data directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique session identifier.
    pub id: String,
    /// Which agent this session belongs to.
    pub agent_id: String,
    /// Optional human-readable title.
    pub title: Option<String>,
    /// Full conversation history.
    pub messages: Vec<ChatMessage>,
    /// When the session was created.
    pub created_at: String,
    /// When the session was last updated.
    pub updated_at: String,
}

impl Session {
    /// Create a new empty session.
    pub fn new(agent_id: impl Into<String>) -> Self {
        let now = crate::registry::timestamp();
        let id = format!("sess_{}", uuid::Uuid::new_v4());
        Self {
            id,
            agent_id: agent_id.into(),
            title: None,
            messages: Vec::new(),
            created_at: now.clone(),
            updated_at: now,
        }
    }

    /// Push a message and update the timestamp.
    pub fn push_message(&mut self, msg: ChatMessage) {
        self.messages.push(msg);
        self.updated_at = crate::registry::timestamp();
    }
}

/// Persistent store for sessions.
///
/// Each session is stored as `{data_dir}/sessions/{session_id}.json`.
/// Thread-safe via internal mutability with a cached in-memory index.
#[derive(Debug, Clone)]
pub struct SessionStore {
    data_dir: std::path::PathBuf,
    // In-memory index: session_id -> Session (lazily loaded)
    cache: std::sync::Arc<std::sync::Mutex<HashMap<String, Session>>>,
}

impl SessionStore {
    /// Open (or create) the session store at `{data_dir}/sessions/`.
    pub fn open<P: AsRef<Path>>(data_dir: P) -> std::io::Result<Self> {
        let sessions_dir = data_dir.as_ref().join("sessions");
        std::fs::create_dir_all(&sessions_dir)?;

        // Pre-load all sessions into cache.
        let mut cache = HashMap::new();
        let rd = std::fs::read_dir(&sessions_dir)?;
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json")
                && let Ok(content) = std::fs::read_to_string(&path)
                && let Ok(session) = serde_json::from_str::<Session>(&content)
            {
                cache.insert(session.id.clone(), session);
            }
        }

        Ok(Self {
            data_dir: sessions_dir,
            cache: std::sync::Arc::new(std::sync::Mutex::new(cache)),
        })
    }

    /// Path to a session file.
    fn session_path(&self, id: &str) -> std::path::PathBuf {
        self.data_dir.join(format!("{id}.json"))
    }

    /// Save a single session to disk.
    fn save_session(&self, session: &Session) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(session)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(self.session_path(&session.id), &json)
    }

    /// List all sessions across all agents (summaries).
    pub fn list_all_sessions(&self) -> Vec<SessionSummary> {
        let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let mut summaries: Vec<_> = cache
            .values()
            .map(|s| SessionSummary {
                id: s.id.clone(),
                agent_id: s.agent_id.clone(),
                title: s.title.clone(),
                message_count: s.messages.len(),
                created_at: s.created_at.clone(),
                updated_at: s.updated_at.clone(),
            })
            .collect();
        summaries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        summaries
    }

    /// List all sessions (summaries) for a specific agent.
    pub fn list_sessions(&self, agent_id: &str) -> Vec<SessionSummary> {
        let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let mut summaries: Vec<_> = cache
            .values()
            .filter(|s| s.agent_id == agent_id)
            .map(|s| SessionSummary {
                id: s.id.clone(),
                agent_id: s.agent_id.clone(),
                title: s.title.clone(),
                message_count: s.messages.len(),
                created_at: s.created_at.clone(),
                updated_at: s.updated_at.clone(),
            })
            .collect();
        summaries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        summaries
    }

    /// Get a session by ID.
    pub fn get_session(&self, id: &str) -> Option<Session> {
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
    }

    /// Add or update a session.
    pub fn upsert_session(&self, session: Session) -> std::io::Result<()> {
        let id = session.id.clone();
        self.save_session(&session)?;
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.insert(id, session);
        Ok(())
    }

    /// Delete a session.
    pub fn delete_session(&self, id: &str) -> std::io::Result<bool> {
        let path = self.session_path(id);
        if path.exists() {
            std::fs::remove_file(path)?;
            let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            cache.remove(id);
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

/// Lightweight summary of a session (no messages).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    /// Session ID.
    pub id: String,
    /// Agent ID.
    pub agent_id: String,
    /// Optional title.
    pub title: Option<String>,
    /// Number of messages in the session.
    pub message_count: usize,
    /// Creation timestamp.
    pub created_at: String,
    /// Last update timestamp.
    pub updated_at: String,
}
