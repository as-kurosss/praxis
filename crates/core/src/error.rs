use thiserror::Error;

/// Unified error type for the Praxis framework.
#[derive(Error, Debug)]
pub enum Error {
    #[error("Invalid parameters: {0}")]
    InvalidParams(String),

    #[error("Operation timed out after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },

    #[error("Verification failed: {0}")]
    VerificationFailed(String),

    #[error("Max iterations ({max}) exceeded")]
    MaxIterationsExceeded { max: u32 },

    #[error("Operation cancelled")]
    Cancelled,

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Access denied to resource '{resource}': {reason}")]
    AccessDenied {
        /// The resource being accessed (e.g. "shell", "read: /etc/passwd").
        resource: String,
        /// Human-readable reason for the denial.
        reason: String,
    },

    #[error("Sandbox error during operation '{operation}': {detail}")]
    SandboxError {
        /// The sandbox operation that failed.
        operation: String,
        /// Error detail.
        detail: String,
    },

    #[error(transparent)]
    Internal(#[from] Box<dyn std::error::Error + Send + Sync>),
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, Error>;
