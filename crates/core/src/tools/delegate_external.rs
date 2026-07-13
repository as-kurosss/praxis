//! **DelegateExternalAgentTool** — delegates work to external agent runners
//! (Claude Code, Codex, OpenCode, Qwen Code) via their CLI interfaces.
//!
//! This tool uses the ACP [`StdioTransport`] to communicate with the external
//! agent process over JSON-delimited stdin/stdout messages.
//!
//! The tool is **disabled by default** — it must be explicitly enabled via
//! the `enabled` configuration flag.

use crate::agent::tool::{Tool, ToolError, ToolSpec};
use crate::orchestration::acp::{AcpTransport, AgentId, AgentMessage, StdioTransport};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

/// Supported external agent runner types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExternalAgentRunner {
    /// Claude Code CLI (`claude`).
    Claude,
    /// Codex CLI (`codex`).
    Codex,
    /// OpenCode CLI (`opencode`).
    OpenCode,
    /// Qwen Code CLI (`qwen`).
    Qwen,
    /// Custom runner binary path.
    Custom(String),
}

impl ExternalAgentRunner {
    fn binary_name(&self) -> &str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::Qwen => "qwen",
            Self::Custom(path) => path,
        }
    }
}

/// Configuration for the delegate external agent tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegateExternalConfig {
    /// Which external agent runner to use.
    pub runner: ExternalAgentRunner,
    /// Whether this tool is enabled (default: `false`).
    #[serde(default)]
    pub enabled: bool,
    /// Additional CLI arguments passed to the runner.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Maximum time to wait for a response (in seconds).
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Local agent ID for the ACP transport.
    #[serde(default = "default_agent_id")]
    pub agent_id: String,
}

fn default_timeout() -> u64 {
    120
}

fn default_agent_id() -> String {
    "praxis".into()
}

impl Default for DelegateExternalConfig {
    fn default() -> Self {
        Self {
            runner: ExternalAgentRunner::Claude,
            enabled: false,
            args: Vec::new(),
            timeout_secs: default_timeout(),
            agent_id: default_agent_id(),
        }
    }
}

/// A tool that delegates work to an external agent runner via ACP stdio transport.
///
/// # Arguments
/// * `task` — the task description to delegate (required)
/// * `config` — optional overrides (runner, timeout, args)
///
/// # Returns
/// A JSON object with `output`, `duration_ms`, and `runner`.
///
/// # Notes
/// * Disabled by default — set `enabled: true` in config to use.
/// * The external binary must be installed and on `$PATH`.
pub struct DelegateExternalAgentTool {
    config: DelegateExternalConfig,
}

impl DelegateExternalAgentTool {
    /// Create a new delegation tool.
    #[must_use]
    pub fn new(config: DelegateExternalConfig) -> Self {
        Self { config }
    }

    /// Returns `true` if this tool is enabled.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}

#[async_trait::async_trait]
impl Tool for DelegateExternalAgentTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "delegate_external_agent".into(),
            description: "Delegates a task to an external agent runner (Claude Code, Codex, \
                          OpenCode, or Qwen Code). The external agent executes the task \
                          independently and returns its output. Disabled by default — enable \
                          in configuration."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "The task to delegate to the external agent"
                    },
                    "runner": {
                        "type": "string",
                        "description": "Override runner: claude, codex, opencode, qwen (optional)",
                        "enum": ["claude", "codex", "opencode", "qwen"],
                        "default": null
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Maximum wait time in seconds (optional)",
                        "default": 120
                    }
                },
                "required": ["task"]
            }),
            category: crate::agent::tool::ToolCategory::Network,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        if !self.config.enabled {
            return Ok(json!({
                "note": "delegate_external_agent is disabled. Enable it in configuration to use.",
                "enabled": false,
            }));
        }

        let task =
            args.get("task")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::InvalidArgs {
                    tool: "delegate_external_agent".into(),
                    message: "missing required 'task' string".into(),
                })?;

        let runner = args
            .get("runner")
            .and_then(Value::as_str)
            .map(|r| match r {
                "claude" => ExternalAgentRunner::Claude,
                "codex" => ExternalAgentRunner::Codex,
                "opencode" => ExternalAgentRunner::OpenCode,
                "qwen" => ExternalAgentRunner::Qwen,
                _ => ExternalAgentRunner::Custom(r.to_string()),
            })
            .unwrap_or_else(|| self.config.runner.clone());

        let timeout_secs = args
            .get("timeout_secs")
            .and_then(Value::as_u64)
            .unwrap_or(self.config.timeout_secs);

        let timeout = Duration::from_secs(timeout_secs);
        let start = std::time::Instant::now();

        // Build CLI args
        let mut cli_args: Vec<&str> = Vec::new();
        for arg in &self.config.args {
            cli_args.push(arg);
        }

        let agent_id = format!("{}-ext", self.config.agent_id);

        // Create stdio transport and spawn the external process
        let transport = StdioTransport::spawn(AgentId(agent_id), runner.binary_name(), &cli_args)
            .map_err(|e| ToolError::Execution {
            tool: "delegate_external_agent".into(),
            message: format!("failed to spawn external agent: {e}"),
        })?;

        // Send the task as an ACP message
        let msg = AgentMessage::new(
            transport.local_id(),
            AgentId("external".into()),
            format!("task_{}", uuid::Uuid::new_v4()),
            task.as_bytes().to_vec(),
        );

        transport
            .send(msg)
            .await
            .map_err(|e| ToolError::Execution {
                tool: "delegate_external_agent".into(),
                message: format!("failed to send task: {e}"),
            })?;

        // Wait for response
        let response = transport
            .receive(timeout)
            .await
            .map_err(|e| ToolError::Execution {
                tool: "delegate_external_agent".into(),
                message: format!("external agent error: {e}"),
            })?;

        let elapsed = start.elapsed().as_millis() as u64;

        match response {
            Some(msg) => {
                let output = String::from_utf8_lossy(&msg.payload).to_string();
                Ok(json!({
                    "output": output,
                    "runner": runner.binary_name(),
                    "duration_ms": elapsed,
                }))
            }
            None => Ok(json!({
                "note": "external agent did not respond within timeout",
                "runner": runner.binary_name(),
                "timeout_secs": timeout_secs,
                "duration_ms": elapsed,
            })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delegate_external_disabled_by_default() {
        let config = DelegateExternalConfig::default();
        let tool = DelegateExternalAgentTool::new(config);
        assert!(!tool.is_enabled());
        assert_eq!(tool.spec().name, "delegate_external_agent");
    }

    #[test]
    fn test_delegate_external_spec() {
        let config = DelegateExternalConfig {
            enabled: true,
            ..Default::default()
        };
        let tool = DelegateExternalAgentTool::new(config);
        let spec = tool.spec();
        assert!(spec.description.contains("Claude Code"));
        assert!(
            spec.parameters["required"]
                .as_array()
                .unwrap()
                .contains(&json!("task"))
        );
    }

    #[tokio::test]
    async fn test_disabled_returns_note() {
        let tool = DelegateExternalAgentTool::new(DelegateExternalConfig::default());
        let result = tool.call(json!({"task": "do something"})).await.unwrap();
        assert_eq!(result["enabled"], false);
        assert!(result["note"].as_str().unwrap().contains("disabled"));
    }

    #[tokio::test]
    async fn test_missing_task_returns_error() {
        let tool = DelegateExternalAgentTool::new(DelegateExternalConfig {
            enabled: true,
            ..Default::default()
        });
        let result = tool.call(json!({})).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing required"));
    }
}
