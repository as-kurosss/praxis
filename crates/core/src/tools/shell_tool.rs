//! **ShellTool** — executes shell commands with built-in safety restrictions.

use crate::agent::tool::{Tool, ToolError, ToolSpec};
use serde_json::{Value, json};
use std::time::Duration;

/// A tool that executes shell commands.
///
/// # Restrictions
/// * Command timeout: [`timeout_secs`] (default: 30 seconds)
/// * Only a single command when `allow_chaining` is `false` (no pipes/chains)
///
/// # Arguments
/// * `command` — shell command to execute
///
/// # Returns
/// stdout, stderr, and exit code.
pub struct ShellTool {
    /// Maximum execution time in seconds (default: 30).
    pub timeout_secs: u64,
    /// Whether to allow shell pipe/chaining operators (`|`, `&&`, `||`, `;`).
    pub allow_chaining: bool,
}

impl Default for ShellTool {
    fn default() -> Self {
        Self {
            timeout_secs: 30,
            allow_chaining: false,
        }
    }
}

#[async_trait::async_trait]
impl Tool for ShellTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "shell".into(),
            description: "Executes a shell command and returns the output. Use for file operations, git commands, and other system tasks. Timeout: 30s.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    }
                },
                "required": ["command"]
            }),
            category: crate::agent::tool::ToolCategory::Shell,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let command =
            args.get("command")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::InvalidArgs {
                    tool: "shell".into(),
                    message: "missing 'command' string".into(),
                })?;

        // Prevent potentially dangerous commands
        let lower = command.to_lowercase();
        let dangerous = [
            "rm -rf /",
            "rm -rf /*",
            "mkfs",
            "dd if=",
            ":(){ :|:& };:",
            "> /dev/sda",
        ];
        if dangerous.iter().any(|d| lower.contains(d)) {
            return Err(ToolError::InvalidArgs {
                tool: "shell".into(),
                message: "command blocked for safety".into(),
            });
        }

        // Block chaining operators when not allowed
        if !self.allow_chaining {
            let chain_chars = ['|', ';', '&'];
            if command.contains(chain_chars) {
                return Err(ToolError::InvalidArgs {
                    tool: "shell".into(),
                    message: "chaining operators (|, ;, &) are disabled — set allow_chaining=true to enable".into(),
                });
            }
        }

        // Use cmd /c on Windows
        let timeout = Duration::from_secs(self.timeout_secs);
        let output = tokio::time::timeout(timeout, async {
            tokio::process::Command::new(if cfg!(windows) { "cmd" } else { "sh" })
                .arg(if cfg!(windows) { "/C" } else { "-c" })
                .arg(command)
                .output()
                .await
        })
        .await
        .map_err(|_| ToolError::Execution {
            tool: "shell".into(),
            message: format!("command timed out after {}s", self.timeout_secs),
        })?
        .map_err(|e| ToolError::Execution {
            tool: "shell".into(),
            message: format!("failed to execute command: {e}"),
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        Ok(json!({
            "stdout": stdout,
            "stderr": stderr,
            "exit_code": output.status.code().unwrap_or(-1),
        }))
    }
}
