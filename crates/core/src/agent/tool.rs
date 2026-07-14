//! **Tool** abstraction — capabilities that an agent can invoke.
//!
//! Provides the [`Tool`] trait, [`ToolSpec`] for LLM function declarations,
//! [`ToolError`], and [`ToolSet`] for managing a collection of tools.

use serde_json::Value;
use std::sync::Arc;

/// Category of a tool for policy and sandbox routing.
///
/// Used by [`GovernedTool`](crate::sandbox::GovernedTool) to decide which
/// policy checks and sandbox routing apply.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, Default,
)]
pub enum ToolCategory {
    /// Generic tool with no resource access.
    #[default]
    Generic,
    /// Shell command execution.
    Shell,
    /// File read operations.
    FileRead,
    /// File write operations.
    FileWrite,
    /// Network access.
    Network,
}

/// Error type for tool execution.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// Tool with the given name was not found in the set.
    #[error("Tool '{tool}' not found")]
    NotFound { tool: String },
    /// Arguments provided to the tool were invalid.
    #[error("Invalid arguments for tool '{tool}': {message}")]
    InvalidArgs { tool: String, message: String },
    /// Tool execution failed at runtime.
    #[error("Tool '{tool}' execution failed: {message}")]
    Execution { tool: String, message: String },
    /// Access denied by policy.
    #[error("Access denied for tool '{tool}': {reason}")]
    AccessDenied { tool: String, reason: String },
    /// Internal error (wraps any other error).
    #[error(transparent)]
    Internal(#[from] Box<dyn std::error::Error + Send + Sync>),
}

/// Specification of a tool for LLM function calling.
///
/// Contains metadata that the LLM uses to decide when and how to call the tool.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolSpec {
    /// Tool name sent to the LLM.
    pub name: String,
    /// Description for the LLM to understand when to use this tool.
    pub description: String,
    /// JSON Schema object describing the expected parameters.
    pub parameters: Value,
    /// Resource category for policy and sandbox routing.
    #[serde(default)]
    pub category: ToolCategory,
}

/// A capability that an agent can invoke.
///
/// Implement this trait for any tool (API call, database query, file operation, etc.).
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    /// Returns the specification used for LLM function calling.
    fn spec(&self) -> ToolSpec;

    /// Execute the tool with the given JSON arguments.
    async fn call(&self, args: Value) -> Result<Value, ToolError>;
}

/// A managed collection of tools, indexed by name.
///
/// Provides lookup, batch specification, and execution by name.
#[derive(Default, Clone)]
pub struct ToolSet {
    tools: Vec<Arc<dyn Tool>>,
}

impl ToolSet {
    /// Create an empty tool set.
    #[must_use]
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    /// Create a tool set from a pre-built vector of tools.
    #[must_use]
    pub fn from_tools(tools: Vec<Arc<dyn Tool>>) -> Self {
        Self { tools }
    }

    /// Add a tool to the set.
    pub fn add<T: Tool + 'static>(&mut self, tool: T) {
        self.tools.push(Arc::new(tool));
    }

    /// Returns the specification of every registered tool.
    #[must_use]
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.iter().map(|t| t.spec()).collect()
    }

    /// Find a tool by name and execute it with the given arguments.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError::NotFound`] if no tool with the given name is registered.
    /// Returns [`ToolError::Execution`] if the underlying tool fails at runtime.
    #[must_use = "tool execution returns a Result which should be handled"]
    pub async fn execute(&self, name: &str, args: Value) -> Result<Value, ToolError> {
        let tool = self
            .tools
            .iter()
            .find(|t| t.spec().name == name)
            .ok_or_else(|| ToolError::NotFound {
                tool: name.to_string(),
            })?;
        tool.call(args).await
    }
}
