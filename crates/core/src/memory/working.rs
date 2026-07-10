//! **WorkingMemory** — live conversation context.
//!
//! The current [`ScrollStrategy`] and [`ScrollContext`] live in
//! [`crate::agent::memory`]. This module re-exports them so they are also
//! available from the top-level `crate::memory` path.
//!
//! In a future refactor the implementations will move here directly.

pub use crate::agent::memory::{ScrollContext, ScrollStrategy};
