use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use super::{AgentDefinition, ProviderConfig};

/// The on-disk registry file schema.
///
/// ```json
/// {
///   "version": 1,
///   "providers": { ... },
///   "agents": { ... }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistryFile {
    version: u32,
    providers: HashMap<String, ProviderConfig>,
    agents: HashMap<String, AgentDefinition>,
}

/// Persistent store for providers and agent definitions.
///
/// Reads and writes a single JSON file.  Thread-safe via internal mutability.
#[derive(Debug, Clone)]
pub struct AgentRegistry {
    path: std::path::PathBuf,
    // We use a std::sync::Mutex for simplicity (file I/O is not async-hot).
    inner: std::sync::Arc<std::sync::Mutex<RegistryFile>>,
}

impl AgentRegistry {
    /// Open (or create) the registry at the given path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        let file = if path.exists() {
            let content = std::fs::read_to_string(&path)
                .map_err(|e| crate::error::Error::Io(e))?;
            serde_json::from_str(&content)
                .map_err(|e| crate::error::Error::Json(e))?
        } else {
            // Create parent directories if needed.
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| crate::error::Error::Io(e))?;
            }
            let file = RegistryFile {
                version: 1,
                providers: HashMap::new(),
                agents: HashMap::new(),
            };
            // Write the empty file.
            let json = serde_json::to_string_pretty(&file)
                .map_err(|e| crate::error::Error::Json(e))?;
            std::fs::write(&path, &json)
                .map_err(|e| crate::error::Error::Io(e))?;
            file
        };

        Ok(Self {
            path,
            inner: std::sync::Arc::new(std::sync::Mutex::new(file)),
        })
    }

    /// Persist the current state to disk.
    fn save(&self) -> Result<()> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let json = serde_json::to_string_pretty(&*guard)
            .map_err(|e| crate::error::Error::Json(e))?;
        std::fs::write(&self.path, &json)
            .map_err(|e| crate::error::Error::Io(e))?;
        Ok(())
    }

    // ── Providers ──────────────────────────────────────────────────

    /// List all registered providers.
    pub fn list_providers(&self) -> Vec<ProviderConfig> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut list: Vec<_> = guard.providers.values().cloned().collect();
        list.sort_by(|a, b| a.label.cmp(&b.label));
        list
    }

    /// Get a provider by ID.
    pub fn get_provider(&self, id: &str) -> Option<ProviderConfig> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).providers.get(id).cloned()
    }

    /// Add or update a provider.
    pub fn upsert_provider(&self, config: ProviderConfig) -> Result<()> {
        let id = config.id.clone();
        {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.providers.insert(id, config);
        }
        self.save()
    }

    /// Delete a provider.
    pub fn delete_provider(&self, id: &str) -> Result<bool> {
        let removed = {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.providers.remove(id)
        };
        if removed.is_some() {
            self.save()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    // ── Agent Definitions ──────────────────────────────────────────

    /// List all agent definitions.
    pub fn list_agents(&self) -> Vec<AgentDefinition> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut list: Vec<_> = guard.agents.values().cloned().collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    }

    /// Get an agent definition by ID.
    pub fn get_agent(&self, id: &str) -> Option<AgentDefinition> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).agents.get(id).cloned()
    }

    /// Add or update an agent definition.
    pub fn upsert_agent(&self, def: AgentDefinition) -> Result<()> {
        let id = def.id.clone();
        {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.agents.insert(id, def);
        }
        self.save()
    }

    /// Delete an agent definition.
    pub fn delete_agent(&self, id: &str) -> Result<bool> {
        let removed = {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.agents.remove(id)
        };
        if removed.is_some() {
            self.save()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
