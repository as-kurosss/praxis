//! **MCP Tool Adapter** — wraps a remote MCP tool as a native [`Tool`].

use crate::client::McpClient;
use crate::types::McpError;
use praxis_core::agent::tool::{Tool, ToolCategory, ToolError, ToolSpec};
use serde_json::Value;
use std::sync::Arc;

/// A [`Tool`] implementation that forwards invocations to an MCP server.
///
/// # Example
///
/// ```ignore
/// let client = McpClient::connect("npx", &["@modelcontextprotocol/server-filesystem"]).await?;
/// let adapter = McpToolAdapter::new(&client, "read_file").await?;
/// agent.add_tool(adapter);
/// ```
pub struct McpToolAdapter {
    spec: ToolSpec,
    client: Arc<McpClient>,
    mcp_tool_name: String,
}

impl McpToolAdapter {
    /// Create an adapter for a specific tool on an MCP server.
    ///
    /// The tool's spec is fetched from the server on construction.
    pub async fn new(client: Arc<McpClient>, mcp_tool_name: &str) -> Result<Self, McpError> {
        // Clone Arc to avoid borrow conflict with move into Self
        let client_clone = Arc::clone(&client);
        let mcp_tool = client_clone
            .list_tools()
            .await?
            .iter()
            .find(|t| t.name == mcp_tool_name)
            .cloned()
            .ok_or_else(|| McpError::ToolNotFound(mcp_tool_name.into()))?;

        let spec = ToolSpec {
            name: mcp_tool.name.clone(),
            description: mcp_tool.description.clone(),
            parameters: mcp_tool.input_schema.clone(),
            category: ToolCategory::Generic,
        };

        Ok(Self {
            spec,
            client,
            mcp_tool_name: mcp_tool.name,
        })
    }

    /// Create adapters for ALL tools exposed by an MCP server.
    pub async fn all(client: Arc<McpClient>) -> Result<Vec<Self>, McpError> {
        let tools = client.list_tools().await?;
        let mut adapters = Vec::with_capacity(tools.len());
        for tool in tools {
            let spec = ToolSpec {
                name: tool.name.clone(),
                description: tool.description.clone(),
                parameters: tool.input_schema.clone(),
                category: ToolCategory::Generic,
            };
            adapters.push(Self {
                spec,
                client: Arc::clone(&client),
                mcp_tool_name: tool.name.clone(),
            });
        }
        Ok(adapters)
    }
}

#[async_trait::async_trait]
impl Tool for McpToolAdapter {
    fn spec(&self) -> ToolSpec {
        self.spec.clone()
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let result = self
            .client
            .call_tool(&self.mcp_tool_name, Some(args))
            .await
            .map_err(|e| ToolError::Execution {
                tool: self.mcp_tool_name.clone(),
                message: e.to_string(),
            })?;

        // Combine text content items
        let text: String = result
            .content
            .iter()
            .filter_map(|item| match item {
                crate::types::McpContentItem::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");

        Ok(serde_json::json!({ "content": text, "is_error": result.is_error }))
    }
}
