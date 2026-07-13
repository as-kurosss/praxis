//! **MCP Transport** — manages a child process and communicates via JSON-RPC over stdio.

use crate::types::{JsonRpcMessage, JsonRpcRequest, McpError};
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

/// A JSON-RPC 2.0 transport over a child process's stdio.
///
/// Spawns the MCP server command and communicates by writing JSON-RPC
/// requests to stdin and reading responses from stdout.
pub struct StdioTransport {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    reader: Option<tokio::io::BufReader<ChildStdout>>,
    next_id: AtomicU64,
}

impl StdioTransport {
    /// Spawn the MCP server process.
    ///
    /// `command` should be the path (and args) of the MCP server executable.
    pub async fn spawn(command: &str, args: &[&str]) -> Result<Self, McpError> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| McpError::Transport(format!("failed to spawn MCP server: {e}")))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpError::Transport("no stdin on child process".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpError::Transport("no stdout on child process".into()))?;

        Ok(Self {
            child: Some(child),
            stdin: Some(stdin),
            reader: Some(BufReader::new(stdout)),
            next_id: AtomicU64::new(1),
        })
    }

    /// Send a JSON-RPC request and await the matching response.
    pub async fn request(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, McpError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let request = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id,
            method: method.into(),
            params,
        };

        // Serialize and write
        let line = serde_json::to_string(&request)
            .map_err(|e| McpError::Parse(format!("serialize request: {e}")))?;

        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| McpError::Transport("stdin not available".into()))?;

        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| McpError::Transport(format!("write to stdin: {e}")))?;
        stdin
            .write_all(b"\n")
            .await
            .map_err(|e| McpError::Transport(format!("write newline: {e}")))?;
        stdin
            .flush()
            .await
            .map_err(|e| McpError::Transport(format!("flush stdin: {e}")))?;

        // Read responses until we find the matching id
        let reader = self
            .reader
            .as_mut()
            .ok_or_else(|| McpError::Transport("stdout not available".into()))?;

        let mut line_buf = String::new();
        loop {
            line_buf.clear();
            reader
                .read_line(&mut line_buf)
                .await
                .map_err(|e| McpError::Transport(format!("read from stdout: {e}")))?;

            if line_buf.is_empty() {
                return Err(McpError::Exited("server closed stdout".into()));
            }

            let line = line_buf.trim();
            if line.is_empty() {
                continue;
            }

            match serde_json::from_str::<JsonRpcMessage>(line) {
                Ok(msg) => match msg {
                    JsonRpcMessage::Success(s) if s.id == id => return Ok(s.result),
                    JsonRpcMessage::Error(e) if e.id == id => {
                        return Err(McpError::Server {
                            code: e.error.code,
                            message: e.error.message,
                            data: e.error.data,
                        });
                    }
                    // Ignore notifications or other messages
                    _ => continue,
                },
                Err(_) => {
                    // Try to read more lines; could be a notification
                    // that doesn't match the JSON-RPC format
                    continue;
                }
            }
        }
    }

    /// Gracefully shut down the transport.
    pub async fn shutdown(&mut self) -> Result<(), McpError> {
        if let Some(mut child) = self.child.take() {
            child
                .kill()
                .await
                .map_err(|e| McpError::Transport(format!("kill server: {e}")))?;
            let _ = child.wait().await;
        }
        Ok(())
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        if let Some(child) = self.child.take() {
            if let Some(pid) = child.id() {
                let _ = std::process::Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/F"])
                    .output();
            }
        }
    }
}
