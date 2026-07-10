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

    #[error(transparent)]
    Internal(#[from] Box<dyn std::error::Error + Send + Sync>),
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, Error>;
