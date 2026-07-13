//! **Memory** — multi-layer agent memory system.
//!
//! Provides three memory layers that can be used independently or together:
//!
//! * [`WorkingMemory`] — live conversation context with scroll strategy (current behaviour)
//! * [`EpisodicMemory`] — full verbatim history with indexed recall
//! * [`DistilledMemory`] — periodic summarisation of conversation segments
//!
//! # Architecture
//!
//! WorkingMemory holds the active message buffer that fits within the token limit.
//! When old messages are evicted by the scroll strategy, they are recorded in
//! EpisodicMemory. DistilledMemory periodically creates summaries of segments
//! so that long-past context can be injected as concise background.

pub(crate) mod distilled;
pub mod embed_store;
pub(crate) mod episodic;
pub mod scroll;
pub(crate) mod working;

pub use distilled::*;
pub use embed_store::*;
pub use episodic::*;
pub use scroll::record_evicted_turn;
pub use working::*;

use crate::agent::llm::ChatMessage;
use std::sync::Arc;
use std::time::Duration;

// ── Backend configuration ────────────────────────────────────────────

/// Memory backend type configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendConfig {
    /// All memory stored in-memory (no persistence).
    InMemory,
    /// Embedding-based vector storage with the specified model identifier.
    Embedding {
        /// Name of the embedding model (e.g. "text-embedding-3-small").
        model: String,
    },
}

// ── Memory configuration ────────────────────────────────────────────

/// Configuration for the memory system.
///
/// Controls backend selection, retention policy, and optional callbacks.
#[derive(Debug, Clone)]
pub struct MemoryConfig {
    /// Backend type. `None` disables memory entirely.
    pub backend: Option<BackendConfig>,
    /// Number of days to retain memory entries. Older entries are auto-deleted
    /// on the next call to [`cleanup_old_entries`].
    pub retention_days: u64,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            backend: Some(BackendConfig::InMemory),
            retention_days: 30,
        }
    }
}

// ── Turn-end hook ────────────────────────────────────────────────────

/// A callback invoked after each agent turn completes.
///
/// Receives:
/// * `turn_id` — unique turn identifier (e.g. `"turn_17"`)
/// * `input` — the user input that started this turn
/// * `messages` — the full slice of messages produced during this turn
///   (assistant response + optional tool results)
pub type OnTurnEnd = Arc<dyn Fn(&str, &str, &[ChatMessage]) + Send + Sync>;

// ── MemorySystem ─────────────────────────────────────────────────────

/// Unified memory system that wraps all memory layers with configuration.
///
/// Provides convenient access to episodic memory, automatic turn tracking,
/// and retention-based cleanup.
#[derive(Clone)]
pub struct MemorySystem {
    /// Episodic (full verbatim history) memory layer.
    pub episodic: EpisodicMemory,
    /// Memory configuration.
    pub config: MemoryConfig,
    /// Optional hook invoked after each agent turn.
    on_turn_end: Option<OnTurnEnd>,
}

impl MemorySystem {
    /// Create a new memory system with the given configuration.
    #[must_use]
    pub fn new(config: MemoryConfig) -> Self {
        Self {
            episodic: EpisodicMemory::new(),
            config,
            on_turn_end: None,
        }
    }

    /// Attach a callback that fires after each agent turn.
    pub fn with_on_turn_end(mut self, hook: OnTurnEnd) -> Self {
        self.on_turn_end = Some(hook);
        self
    }

    /// Register a callback that fires after each agent turn.
    pub fn set_on_turn_end(&mut self, hook: OnTurnEnd) {
        self.on_turn_end = Some(hook);
    }

    /// Clear the on_turn_end callback.
    pub fn clear_on_turn_end(&mut self) {
        self.on_turn_end = None;
    }

    /// Returns `true` if memory is enabled (a backend is configured).
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.config.backend.is_some()
    }

    /// Record a turn to episodic memory and invoke the `on_turn_end` hook.
    ///
    /// This is called automatically by the agent runtime after each turn.
    /// Can also be called manually.
    pub fn record_turn(&mut self, turn_id: &str, input: &str, messages: &[ChatMessage]) {
        if !self.is_enabled() {
            return;
        }

        let entry = EpisodicEntry::from_turn(turn_id, input, messages);
        self.episodic.record(entry);

        if let Some(ref hook) = self.on_turn_end {
            hook(turn_id, input, messages);
        }
    }

    /// Remove entries older than `retention_days`.
    ///
    /// Returns the number of deleted entries.
    pub fn cleanup_old_entries(&mut self) -> usize {
        if !self.is_enabled() {
            return 0;
        }
        let max_age = Duration::from_secs(self.config.retention_days * 86_400);

        let old_ids: Vec<String> = self
            .episodic
            .iter()
            .filter(|entry| entry.timestamp.elapsed().is_ok_and(|age| age > max_age))
            .map(|entry| entry.turn_id.clone())
            .collect();

        let count = old_ids.len();
        for id in old_ids {
            self.episodic.remove_entry(&id);
        }
        count
    }
}

impl Default for MemorySystem {
    fn default() -> Self {
        Self::new(MemoryConfig::default())
    }
}

/// Convenience function to clean up old entries in an episodic memory.
///
/// Deletes entries whose age exceeds `retention_days`.
pub fn cleanup_old_entries(memory: &mut EpisodicMemory, retention_days: u64) -> usize {
    let max_age = Duration::from_secs(retention_days * 86_400);

    let old_ids: Vec<String> = memory
        .iter()
        .filter(|entry| entry.timestamp.elapsed().is_ok_and(|age| age > max_age))
        .map(|entry| entry.turn_id.clone())
        .collect();

    let count = old_ids.len();
    for id in old_ids {
        memory.remove_entry(&id);
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_config_default() {
        let config = MemoryConfig::default();
        assert_eq!(config.backend, Some(BackendConfig::InMemory));
        assert_eq!(config.retention_days, 30);
    }

    #[test]
    fn test_memory_config_none_backend() {
        let config = MemoryConfig {
            backend: None,
            retention_days: 0,
        };
        assert!(config.backend.is_none());
    }

    #[test]
    fn test_memory_system_default_enabled() {
        let system = MemorySystem::default();
        assert!(system.is_enabled());
    }

    #[test]
    fn test_memory_system_disabled() {
        let config = MemoryConfig {
            backend: None,
            retention_days: 0,
        };
        let mut system = MemorySystem::new(config);
        assert!(!system.is_enabled());
        // record_turn should be a no-op
        system.record_turn("t1", "hello", &[]);
        assert_eq!(system.episodic.len(), 0);
    }

    #[test]
    fn test_record_turn_creates_entry() {
        let mut system = MemorySystem::default();
        let msgs = vec![ChatMessage::assistant("response")];
        system.record_turn("t1", "hello", &msgs);
        assert_eq!(system.episodic.len(), 1);
    }

    #[test]
    fn test_on_turn_end_callback_fires() {
        let fired = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let fired_clone = Arc::clone(&fired);

        let mut system =
            MemorySystem::default().with_on_turn_end(Arc::new(move |_turn_id, _input, _msgs| {
                fired_clone.store(true, std::sync::atomic::Ordering::SeqCst);
            }));

        let msgs = vec![ChatMessage::assistant("resp")];
        system.record_turn("t1", "hello", &msgs);
        assert!(fired.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn test_cleanup_old_entries_retains_recent() {
        let mut system = MemorySystem::new(MemoryConfig {
            backend: Some(BackendConfig::InMemory),
            retention_days: 1,
        });
        let msgs = vec![ChatMessage::assistant("resp")];
        system.record_turn("t1", "hello", &msgs);
        system.record_turn("t2", "world", &msgs);

        // Both entries are recent, cleanup should remove nothing
        let removed = system.cleanup_old_entries();
        assert_eq!(removed, 0);
        assert_eq!(system.episodic.len(), 2);
    }

    #[test]
    fn test_cleanup_old_entries_disabled() {
        let config = MemoryConfig {
            backend: None,
            retention_days: 0,
        };
        let mut system = MemorySystem::new(config);
        assert_eq!(system.cleanup_old_entries(), 0);
    }
}
