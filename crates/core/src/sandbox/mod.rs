//! **Sandbox / Governance** — policy enforcement and isolated execution for agents.
//!
//! Provides two layers of protection:
//!
//! * **Policy** ([`ResourcePolicy`]) — permission checks before any resource access.
//!   Zero-cost when using [`AllowAll`] (the default).
//! * **Sandbox** ([`Sandbox`]) — isolated execution environment. [`DirectSandbox`]
//!   has no overhead, while future backends (WASM, process) add real isolation.
//!
//! [`GovernedTool`] wraps any [`Tool`](crate::agent::Tool) with a policy and sandbox.

mod policy;
mod sandbox;
mod types;

pub use policy::*;
pub use sandbox::*;
pub use types::*;

use crate::agent::tool::{Tool, ToolCategory, ToolError, ToolSpec};
use std::sync::Arc;

/// A [`Tool`] wrapper that applies policy and sandbox restrictions.
///
/// Intercepts [`Tool::call`] to:
/// 1. Run policy checks (shell, file, network)
/// 2. Route the operation through the sandbox
/// 3. Fall back to the inner tool's implementation for safe operations
///
/// # Example
///
/// ```ignore
/// use praxis_core::sandbox::{GovernedTool, AllowAll, DirectSandbox, ShellBlocklist};
/// use praxis_core::tools::ShellTool;
/// use std::sync::Arc;
///
/// let tool = GovernedTool::new(
///     ShellTool::default(),
///     Arc::new(ShellBlocklist::default_blocked()),
///     Arc::new(DirectSandbox::new()),
/// );
/// ```
pub struct GovernedTool<T: Tool> {
    inner: T,
    policy: Arc<dyn ResourcePolicy>,
    sandbox: Arc<dyn Sandbox>,
}

impl<T: Tool> GovernedTool<T> {
    /// Wrap a tool with policy and sandbox restrictions.
    pub fn new(
        inner: T,
        policy: Arc<dyn ResourcePolicy>,
        sandbox: Arc<dyn Sandbox>,
    ) -> Self {
        Self {
            inner,
            policy,
            sandbox,
        }
    }

    /// Run policy checks and route through sandbox if applicable.
    fn check_and_sandbox(&self, category: ToolCategory, args: &serde_json::Value) -> std::result::Result<bool, crate::error::Error> {
        match category {
            ToolCategory::Shell => {
                if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
                    self.policy.check_shell(cmd)?;
                    return Ok(true);
                }
            }
            ToolCategory::FileRead => {
                if let Some(path_str) = args.get("path").and_then(|v| v.as_str()) {
                    self.policy.check_read(std::path::Path::new(path_str))?;
                    return Ok(true);
                }
            }
            ToolCategory::FileWrite => {
                if let Some(path_str) = args.get("path").and_then(|v| v.as_str()) {
                    self.policy.check_write(std::path::Path::new(path_str))?;
                    return Ok(true);
                }
            }
            ToolCategory::Network => {
                if let Some(url) = args.get("url").and_then(|v| v.as_str()) {
                    self.policy.check_network(url)?;
                    return Ok(true);
                }
            }
            ToolCategory::Generic => {}
        }
        Ok(false)
    }
}

#[async_trait::async_trait]
impl<T: Tool + Send + Sync> Tool for GovernedTool<T> {
    fn spec(&self) -> ToolSpec {
        self.inner.spec()
    }

    async fn call(&self, args: serde_json::Value) -> std::result::Result<serde_json::Value, ToolError> {
        let spec = self.inner.spec();
        let name = spec.name;
        let category = spec.category;

        // Phase 1: Policy check + sandbox routing decision
        let use_sandbox = self.check_and_sandbox(category, &args).map_err(|e| {
            ToolError::AccessDenied {
                tool: name.clone(),
                reason: format!("{e}"),
            }
        })?;

        // Phase 2: Execute via sandbox or forward to inner tool
        if use_sandbox && category == ToolCategory::Shell {
            let command = args
                .get("command")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::InvalidArgs {
                    tool: name.clone(),
                    message: "missing 'command' string".into(),
                })?;

            let output = self.sandbox.execute_shell(command, std::time::Duration::from_secs(30)).await.map_err(|e| {
                ToolError::AccessDenied {
                    tool: name.clone(),
                    reason: format!("sandbox: {e}"),
                }
            })?;

            return Ok(serde_json::json!({
                "stdout": output.stdout,
                "stderr": output.stderr,
                "exit_code": output.exit_code,
            }));
        }

        // Default: pass through to inner tool
        self.inner.call(args).await
    }
}
