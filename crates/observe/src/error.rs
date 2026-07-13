//! **Error types** — unified error handling for the Observe subsystem.

use thiserror::Error;

/// Unified error type for all observe operations.
#[derive(Error, Debug)]
pub enum ObserveError {
    /// A database operation failed.
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// A requested resource was not found.
    #[error("Not found: {0}")]
    NotFound(String),

    /// An internal error occurred.
    #[error("Internal error: {0}")]
    Internal(String),
}
