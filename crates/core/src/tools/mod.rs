//! **Built-in Tools** — a collection of ready-to-use [`Tool`](crate::agent::Tool) implementations.
//!
//! # Available tools
//! * [`CalculatorTool`] — safe mathematical expression evaluator
//! * [`TimeTool`] — current system date and time
//! * [`ShellTool`] — execute shell commands
//! * [`CustomTool`] — schema-only tool for user-defined tools

pub mod calculator;
pub mod custom;
pub mod shell_tool;
pub mod time_tool;

pub use calculator::CalculatorTool;
pub use custom::CustomTool;
pub use shell_tool::ShellTool;
pub use time_tool::TimeTool;
