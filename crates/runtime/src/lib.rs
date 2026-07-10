//! # Praxis Runtime — concrete implementations for the Agent system.
//!
//! This crate provides:
//! * [`OpenAiClient`] — an OpenAI-compatible [`LlmClient`](praxis_core::agent::LlmClient)
//!   implementation that works with any OpenAI-compatible API.

pub mod openai;

pub use openai::*;
