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

mod approval;
mod env_gate;
mod exec;
mod policy;
pub mod scanner;
mod types;

#[cfg(windows)]
pub mod appcontainer;

#[cfg(target_os = "linux")]
pub mod bubblewrap;

pub use approval::*;
pub use env_gate::*;
pub use exec::*;
pub use policy::*;
pub use scanner::{Finding, FindingCategory, ScanError, ScanReport, ScannerConfig, SkillScanner};
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
    approval_gate: Option<ApprovalGate>,
    env_gates: Vec<EnvGate>,
}

impl<T: Tool> GovernedTool<T> {
    /// Wrap a tool with policy and sandbox restrictions.
    pub fn new(inner: T, policy: Arc<dyn ResourcePolicy>, sandbox: Arc<dyn Sandbox>) -> Self {
        Self {
            inner,
            policy,
            sandbox,
            approval_gate: None,
            env_gates: Vec::new(),
        }
    }

    /// Attach an approval gate for interactive Ask-mode policy.
    pub fn with_approval_gate(mut self, gate: ApprovalGate) -> Self {
        self.approval_gate = Some(gate);
        self
    }

    /// Add an environment-variable gate that must pass before execution.
    pub fn with_env_gate(mut self, gate: EnvGate) -> Self {
        self.env_gates.push(gate);
        self
    }

    /// Run policy checks and route through sandbox if applicable.
    fn check_and_sandbox(
        &self,
        category: ToolCategory,
        args: &serde_json::Value,
    ) -> std::result::Result<bool, crate::error::Error> {
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

    async fn call(
        &self,
        args: serde_json::Value,
    ) -> std::result::Result<serde_json::Value, ToolError> {
        let spec = self.inner.spec();
        let name = spec.name;
        let category = spec.category;

        // Phase 0: Approval gate check (Allow/Deny/Ask)
        if let Some(ref gate) = self.approval_gate {
            gate.check(&category, &name, &args, None)?;
        }

        // Phase 0.5: Env gate checks
        for gate in &self.env_gates {
            gate.check(&name)?;
        }

        // Phase 1: Policy check + sandbox routing decision
        let use_sandbox =
            self.check_and_sandbox(category, &args)
                .map_err(|e| ToolError::AccessDenied {
                    tool: name.clone(),
                    reason: format!("{e}"),
                })?;

        // Phase 2: Execute via sandbox or forward to inner tool
        match (use_sandbox, category) {
            (true, ToolCategory::Shell) => {
                let command = args
                    .get("command")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::InvalidArgs {
                        tool: name.clone(),
                        message: "missing 'command' string".into(),
                    })?;

                let output = self
                    .sandbox
                    .execute_shell(command, std::time::Duration::from_secs(30))
                    .await
                    .map_err(|e| ToolError::AccessDenied {
                        tool: name.clone(),
                        reason: format!("sandbox: {e}"),
                    })?;

                Ok(serde_json::json!({
                    "stdout": output.stdout,
                    "stderr": output.stderr,
                    "exit_code": output.exit_code,
                }))
            }
            (true, ToolCategory::FileRead) => {
                let path_str = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    ToolError::InvalidArgs {
                        tool: name.clone(),
                        message: "missing 'path' string".into(),
                    }
                })?;

                let data = self
                    .sandbox
                    .read_file(std::path::Path::new(path_str))
                    .await
                    .map_err(|e| ToolError::AccessDenied {
                        tool: name.clone(),
                        reason: format!("sandbox: {e}"),
                    })?;

                Ok(serde_json::json!({
                    "data": String::from_utf8_lossy(&data),
                    "path": path_str,
                }))
            }
            (true, ToolCategory::FileWrite) => {
                let path_str = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    ToolError::InvalidArgs {
                        tool: name.clone(),
                        message: "missing 'path' string".into(),
                    }
                })?;
                let data = args
                    .get("data")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();

                self.sandbox
                    .write_file(std::path::Path::new(path_str), data.as_bytes())
                    .await
                    .map_err(|e| ToolError::AccessDenied {
                        tool: name.clone(),
                        reason: format!("sandbox: {e}"),
                    })?;

                Ok(serde_json::json!({"path": path_str, "written": true}))
            }
            _ => {
                // Default: pass through to inner tool for non-sandboxed categories
                self.inner.call(args).await
            }
        }
    }
}
