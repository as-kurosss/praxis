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

mod working;
mod episodic;
mod distilled;
pub mod scroll;

pub use working::*;
pub use episodic::*;
pub use distilled::*;
pub use scroll::record_evicted_turn;
