//! **Context** — persistent conversation history with scroll/eviction and recall.
//!
//! Provides [`SessionHistory`] — a SQLite-backed store for conversation turns
//! with FTS5 full-text search.  Designed to be used inside [`Loop::execute`]
//! so the agent can persist, evict, and recall past turns across requests.

pub mod memory_extractor;
pub mod scroll;
pub mod session_history;
pub mod vector_memory;

pub use memory_extractor::{FactExtractorFn, MemoryExtractor};
pub use scroll::SessionScroll;
pub use session_history::*;
pub use vector_memory::*;
