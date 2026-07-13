//! # Agent system — LLM-powered agent runtime over the Loop Engine
//!
//! This module provides the abstractions needed to build AI agents:
//! * [`Tool`] trait — define capabilities (APIs, functions, tools)
//! * [`ToolSet`] — manage a collection of tools
//! * [`LlmClient`] trait — abstract LLM interface for any provider
//! * [`Agent`] — an LLM-powered agent that implements [`Loop`]
//! * [`AgentConfig`] — configuration for the agent
//!
//! An [`Agent`] wraps an [`LlmClient`] and a [`ToolSet`] into a [`Loop`] that
//! runs a tool-calling loop: call LLM → execute tools → repeat until final answer.

pub mod tool;
pub use tool::*;

pub mod llm;
pub use llm::*;

pub mod runtime;
pub use runtime::*;

pub mod sub_agent;
pub use sub_agent::*;

pub mod memory;
pub use memory::*;

pub mod task_tracker;
pub use task_tracker::*;
