//! # Praxis — Agent Orchestration Framework
//!
//! A state-graph orchestrator for agent systems built on four primitive cycles:
//! **Turn-based**, **Goal-based**, **Time-based**, and **Proactive**.

pub mod agent;
pub mod context;
pub mod cycle;
pub mod error;
pub mod governance;
pub mod loops;
pub mod memory;
pub mod orchestration;
pub mod persistence;
pub mod plugin;
pub mod registry;
pub mod sandbox;
pub mod scheduler;
pub mod tools;

pub use error::Error;
pub use error::Result;
