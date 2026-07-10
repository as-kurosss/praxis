//! Four primitive orchestration cycles.
//!
//! Every feature of the framework is built as a composition of these cycles.
//! No other cycle types should be introduced.

pub mod goal;
pub mod proactive;
pub mod time;
pub mod turn;
