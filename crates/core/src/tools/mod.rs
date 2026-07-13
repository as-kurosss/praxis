//! **Built-in Tools** — a collection of ready-to-use [`Tool`](crate::agent::Tool) implementations.
//!
//! # Available tools
//! * [`CalculatorTool`] — safe mathematical expression evaluator
//! * [`TimeTool`] — current system date and time
//! * [`ShellTool`] — execute shell commands
//! * [`CustomTool`] — schema-only tool for user-defined tools
//! * [`WebSearchTool`] — search the web for current information
//! * [`DocumentReadTool`] — read documents from the filesystem

pub mod calculator;
pub mod custom;
pub mod document_read;
pub mod shell_tool;
pub mod time_tool;
pub mod web_search;

pub use calculator::CalculatorTool;
pub use custom::CustomTool;
pub use document_read::{DocumentConfig, DocumentError, DocumentReadTool, DocumentReader};
pub use shell_tool::ShellTool;
pub use time_tool::TimeTool;
pub use web_search::{SearchResult, WebSearchProvider, WebSearchProviderKind, WebSearchTool};
