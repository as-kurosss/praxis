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
    ReadFile,
    WriteFile,
    NetworkAccess,
    EnvironmentRead,
}
