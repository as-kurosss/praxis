//! **MCP Client** — high-level client that manages the full lifecycle of an
//! MCP server connection: initialize → tool discovery → tool invocation.

use crate::transport::StdioTransport;
use crate::types::{
    InitializeResult, McpCallToolResult, McpError, McpToolSpec, ServerInfo,
};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

/// A connected MCP client.
///
/// Wraps a [`StdioTransport`] and provides typed access to MCP methods.
/// After construction the client is already initialized — tools are
/// discovered lazily on first access.
pub struct McpClient {
    transport: Arc<Mutex<StdioTransport>>,
    server_info: ServerInfo,
    /// Cached tool list (populated on first call to [`list_tools`]).
    tools: tokio::sync::OnceCell<Vec<McpToolSpec>>,
}

impl McpClient {
    /// Spawn an MCP server and run the `initialize` handshake.
    ///
    /// # Arguments
    /// * `command` — executable path or name
    /// * `args` — command-line arguments
    pub async fn connect(command: &str, args: &[&str]) -> Result<Self, McpError> {
        let mut transport = StdioTransport::spawn(command, args).await?;

        // Initialize
        let init_params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "praxis",
                "version": "0.1.0"
            }
        });

        let result: Value = transport
            .request("initialize", Some(init_params))
            .await?;

        let init: InitializeResult = serde_json::from_value(result)
            .map_err(|e| McpError::Parse(format!("parse initialize result: {e}")))?;

        Ok(Self {
            transport: Arc::new(Mutex::new(transport)),
            server_info: init.server_info,
            tools: tokio::sync::OnceCell::new(),
        })
    }

    /// Server info returned during initialization.
    pub fn server_info(&self) -> &ServerInfo {
        &self.server_info
    }

    /// List tools provided by this MCP server.
    ///
    /// Results are cached after the first call.
    pub async fn list_tools(&self) -> Result<&[McpToolSpec], McpError> {
        self.tools
            .get_or_try_init(|| async {
                let mut transport = self.transport.lock().await;
                let result: Value = transport.request("tools/list", None).await?;
                let list = result
                    .get("tools")
                    .ok_or_else(|| McpError::Parse("missing 'tools' in list result".into()))?;
                let tools: Vec<McpToolSpec> = serde_json::from_value(list.clone())
                    .map_err(|e| McpError::Parse(format!("parse tool list: {e}")))?;
                Ok(tools)
            })
            .await
            .map(Vec::as_slice)
    }

    /// Call a tool on the MCP server.
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Option<Value>,
    ) -> Result<McpCallToolResult, McpError> {
        let params = serde_json::json!({
            "name": name,
            "arguments": arguments.unwrap_or(serde_json::json!({})),
        });

        let mut transport = self.transport.lock().await;
        let result: Value = transport.request("tools/call", Some(params)).await?;

        let call_result: McpCallToolResult = serde_json::from_value(result)
            .map_err(|e| McpError::Parse(format!("parse tool call result: {e}")))?;
        Ok(call_result)
    }

    /// Shut down the MCP server gracefully.
    pub async fn shutdown(&self) -> Result<(), McpError> {
        let mut transport = self.transport.lock().await;
        transport.shutdown().await
    }
}
