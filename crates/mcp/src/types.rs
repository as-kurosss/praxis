//! **MCP Protocol Types** — JSON-RPC 2.0 messages for the Model Context Protocol.
//!
//! Implements only the subset needed for tool discovery and invocation:
//! * `initialize` — capability negotiation
//! * `tools/list` — discover available tools
//! * `tools/call` — invoke a tool

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── JSON-RPC 2.0 envelope ───────────────────────────────────────────────

/// A JSON-RPC 2.0 request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// A JSON-RPC 2.0 response (success).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcSuccess {
    pub jsonrpc: String,
    pub id: u64,
    pub result: Value,
}

/// A JSON-RPC 2.0 error response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub jsonrpc: String,
    pub id: u64,
    pub error: JsonRpcErrorDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcErrorDetail {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Any JSON-RPC 2.0 message received from the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    Success(JsonRpcSuccess),
    Error(JsonRpcError),
}

impl JsonRpcMessage {
    /// Extract the result value from a success response, or convert error.
    pub fn into_result(self) -> Result<Value, McpError> {
        match self {
            Self::Success(s) => Ok(s.result),
            Self::Error(e) => Err(McpError::Server {
                code: e.error.code,
                message: e.error.message,
                data: e.error.data,
            }),
        }
    }
}

// ── MCP Server Capabilities ─────────────────────────────────────────────

/// Information about the MCP server implementation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCapabilities {
    /// Whether the server supports the `tools` capability.
    #[serde(default)]
    pub tools: Option<ToolCapabilities>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCapabilities {
    /// Whether the server supports listing available tools.
    #[serde(default)]
    pub list_changed: Option<bool>,
}

/// Information about the MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

/// Result of an `initialize` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    pub server_info: ServerInfo,
}

// ── MCP Tool definitions ────────────────────────────────────────────────

/// A tool exposed by an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolSpec {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub input_schema: Value,
}

/// Result of `tools/call`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpCallToolResult {
    #[serde(default)]
    pub content: Vec<McpContentItem>,
    #[serde(default)]
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum McpContentItem {
    #[serde(rename = "text")]
    Text {
        #[serde(default)]
        text: String,
    },
    #[serde(rename = "resource")]
    Resource {
        #[serde(default)]
        resource: Value,
    },
}

// ── Errors ──────────────────────────────────────────────────────────────

/// Errors that can occur during MCP communication.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    /// Transport-level I/O error.
    #[error("MCP transport error: {0}")]
    Transport(String),

    /// Server returned a JSON-RPC error.
    #[error("MCP server error (code={code}): {message}")]
    Server {
        code: i64,
        message: String,
        data: Option<Value>,
    },

    /// Failed to parse a message.
    #[error("MCP parse error: {0}")]
    Parse(String),

    /// The server did not respond within the timeout.
    #[error("MCP timeout")]
    Timeout,

    /// The server process exited unexpectedly.
    #[error("MCP server exited: {0}")]
    Exited(String),

    /// Tool not found on the server.
    #[error("MCP tool not found: {0}")]
    ToolNotFound(String),
}
