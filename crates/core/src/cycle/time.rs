//! **Time-based** cycle — triggered by a schedule (wrapper).
//!
//! ⚠️ NEVER contains business logic directly.
//! Delegates to a Goal-based or Turn-based cycle when the time arrives.
