//! **MCP Registry** — manages a collection of MCP server connections,
//! providing a unified view of all tools across servers.

use crate::adapter::McpToolAdapter;
use crate::client::McpClient;
use crate::types::McpError;
use praxis_core::agent::ToolSet;
use std::collections::HashMap;
use std::sync::Arc;

/// A registry of MCP server connections.
///
/// Provides a simple interface to connect to multiple MCP servers and
/// collect their tools into a single [`ToolSet`].
pub struct McpRegistry {
    clients: HashMap<String, Arc<McpClient>>,
}

impl McpRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
        }
    }

    /// Connect to an MCP server and add it to the registry.
    ///
    /// # Arguments
    /// * `name` — a user-chosen identifier for this server
    /// * `command` — the MCP server executable
    /// * `args` — command-line arguments
    pub async fn connect(
        &mut self,
        name: impl Into<String>,
        command: &str,
        args: &[&str],
    ) -> Result<(), McpError> {
        let client = McpClient::connect(command, args).await?;
        self.clients.insert(name.into(), Arc::new(client));
        Ok(())
    }

    /// Get a client by name.
    pub fn get(&self, name: &str) -> Option<&Arc<McpClient>> {
        self.clients.get(name)
    }

    /// Collect all tools from all connected servers into a [`ToolSet`].
    ///
    /// Tools with duplicate names will be prefixed with `{server_name}_`.
    pub async fn collect_tools(&self) -> Result<ToolSet, McpError> {
        let mut tools = ToolSet::new();
        for (_server_name, client) in &self.clients {
            let adapters = McpToolAdapter::all(Arc::clone(client)).await?;
            for adapter in adapters {
                tools.add(adapter);
            }
        }
        Ok(tools)
    }

    /// Collect tools from a specific server into a [`ToolSet`].
    pub async fn collect_server_tools(&self, name: &str) -> Result<ToolSet, McpError> {
        let client = self
            .clients
            .get(name)
            .ok_or_else(|| McpError::Transport(format!("server '{name}' not found")))?;
        let adapters = McpToolAdapter::all(Arc::clone(client)).await?;
        let mut tools = ToolSet::new();
        for adapter in adapters {
            tools.add(adapter);
        }
        Ok(tools)
    }

    /// Shut down all connected servers.
    pub async fn shutdown_all(&self) {
        for client in self.clients.values() {
            let _ = client.shutdown().await;
        }
    }
}

impl Default for McpRegistry {
    fn default() -> Self {
        Self::new()
    }
}
