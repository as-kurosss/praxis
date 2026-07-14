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

mod distilled;
mod episodic;
pub mod scroll;
mod working;

pub use distilled::*;
pub use episodic::*;
pub use scroll::{
    apply_with_active_turn_protection, build_evicted_entry, find_active_tail_boundary,
    record_evicted_turn, record_evicted_turn_async,
};
pub use working::*;
