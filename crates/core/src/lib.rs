//! # Praxis — Agent Orchestration Framework
//!
//! A state-graph orchestrator for agent systems built on four primitive cycles:
//! **Turn-based**, **Goal-based**, **Time-based**, and **Proactive**.

pub mod agent;
pub mod cycle;
pub mod error;
pub mod loops;

pub use error::Error;
pub use error::Result;
