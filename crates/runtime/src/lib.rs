//! # Praxis Runtime — concrete implementations for the Agent system.
//!
//! This crate provides:
//! * [`OpenAiClient`] — an OpenAI-compatible [`LlmClient`](praxis_core::agent::LlmClient)
//!   implementation that works with any OpenAI-compatible API.
//! * [`AnthropicClient`] — an [`LlmClient`](praxis_core::agent::LlmClient)
//!   implementation for Anthropic's Messages API.

pub mod anthropic;
pub mod gemini;
pub mod openai;

pub use anthropic::*;
pub use gemini::*;
pub use openai::*;
