//! **Built-in Tools** — a collection of ready-to-use [`Tool`](crate::agent::Tool) implementations.
//!
//! # Available tools
//! * [`CalculatorTool`] — safe mathematical expression evaluator
//! * [`TimeTool`] — current system date and time
//! * [`ShellTool`] — execute shell commands

pub mod calculator;
pub mod shell_tool;
pub mod time_tool;

pub use calculator::CalculatorTool;
pub use shell_tool::ShellTool;
pub use time_tool::TimeTool;
