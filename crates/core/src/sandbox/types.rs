//! **Sandbox types** — shared types for the sandbox/governance system.

use std::time::Duration;

/// Result type for sandbox operations.
pub type SandboxResult<T> = Result<T, SandboxError>;

/// Error type for sandbox operations.
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("Operation not supported: {operation}")]
    Unsupported { operation: String },

    #[error("Operation denied by policy: {reason}")]
    PolicyDenied { reason: String },

    #[error("Sandbox execution failed: {detail}")]
    ExecutionFailed { detail: String },

    #[error("Timed out after {duration:?}")]
    Timeout { duration: Duration },
}

/// The output of a sandboxed shell command.
#[derive(Debug, Clone)]
pub struct SandboxOutput {
    /// Stdout content.
    pub stdout: String,
    /// Stderr content.
    pub stderr: String,
    /// Exit code.
    pub exit_code: i32,
}

/// An operation that a sandbox implementation supports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxOperation {
    ExecuteShell,
    ExecuteCode,
    ReadFile,
    WriteFile,
    NetworkAccess,
    EnvironmentRead,
}

/// Risk level for a shell evasion pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskLevel {
    /// Low-risk pattern (informational).
    Low,
    /// Medium-risk pattern (potentially dangerous).
    Medium,
    /// High-risk pattern (likely dangerous).
    High,
    /// Critical-risk pattern (extremely dangerous, guaranteed destructive).
    Critical,
}

impl RiskLevel {
    /// Returns a human-readable label for this risk level.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            RiskLevel::Low => "low",
            RiskLevel::Medium => "medium",
            RiskLevel::High => "high",
            RiskLevel::Critical => "critical",
        }
    }
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}
