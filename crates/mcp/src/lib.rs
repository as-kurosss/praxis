//! # Praxis MCP — Model Context Protocol Integration
//!
//! Allows Praxis agents to discover and invoke tools from any MCP server
//! via JSON-RPC 2.0 over stdio.
//!
//! # Quick Start
//!
//! ```ignore
//! use praxis_mcp::McpRegistry;
//!
//! let mut registry = McpRegistry::new();
//! registry.connect("fs", "npx", &["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]).await?;
//!
//! let tools = registry.collect_tools().await?;
//! // tools can now be added to an agent
//! ```

pub mod adapter;
pub mod client;
pub mod registry;
pub mod transport;
pub mod types;

pub use adapter::McpToolAdapter;
pub use client::McpClient;
pub use registry::McpRegistry;
pub use types::*;
