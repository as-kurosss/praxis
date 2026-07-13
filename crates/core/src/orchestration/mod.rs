//! **Multi-Agent Orchestration Patterns** — compose agents into higher-level
//! coordination structures.
//!
//! Each pattern implements [`Loop`] so it can be used as a graph node.
//!
//! # Available patterns
//! * [`Broadcast`] — send input to all agents, collect all outputs
//! * [`RoundRobin`] — cycle agents, each receives previous output
//! * [`Supervisor`] — a leader agent delegates to worker agents
//! * [`Router`] — route input to an agent based on a routing function

pub mod acp;
mod broadcast;
mod round_robin;
mod router;
mod supervisor;

pub use acp::*;
pub use broadcast::Broadcast;
pub use round_robin::RoundRobin;
pub use router::Router;
pub use supervisor::Supervisor;
