//! **CustomTool** — a schema-only tool for user-defined tool definitions.
//!
//! The tool's spec is provided at runtime (from an `AgentDefinition`),
//! but there is no Rust handler behind it.  When invoked, it returns a
//! note that no runtime handler was registered.

use crate::agent::tool::{Tool, ToolError, ToolSpec};
use serde_json::Value;

/// A tool whose spec is provided at runtime from a configuration file.
///
/// The LLM will see the schema and may call the tool, but the call will
/// return a message explaining that no runtime handler was registered.
pub struct CustomTool {
    spec: ToolSpec,
}

impl CustomTool {
    /// Create a new custom tool with the given name, description, and JSON schema.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        schema: Value,
    ) -> Self {
        Self {
            spec: ToolSpec {
                name: name.into(),
                description: description.into(),
                parameters: schema,
                category: crate::agent::tool::ToolCategory::Generic,
            },
        }
    }
}

#[async_trait::async_trait]
impl Tool for CustomTool {
    fn spec(&self) -> ToolSpec {
        self.spec.clone()
    }

    async fn call(&self, _args: Value) -> Result<Value, ToolError> {
        Ok(serde_json::json!({
            "note": "tool spec registered, no runtime handler",
            "tool": self.spec.name,
        }))
    }
}
