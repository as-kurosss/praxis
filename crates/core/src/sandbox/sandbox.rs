//! **Sandbox** — isolated execution environment for agent operations.

use super::types::{SandboxError, SandboxOperation, SandboxOutput, SandboxResult};
use std::path::Path;
use std::time::Duration;

/// Isolated execution environment for agent operations.
///
/// A sandbox provides methods to execute shell commands, read files, and
/// write files in a controlled environment. Different implementations
/// provide varying levels of isolation:
///
/// * [`DirectSandbox`] — no isolation, direct host execution (default)
/// * [`NullSandbox`] — blocks everything, for testing
/// * `WasmSandbox` — wasmtime-based isolation (future)
/// * `ProcessSandbox` — OS-level process isolation (future)
#[async_trait::async_trait]
pub trait Sandbox: Send + Sync + std::fmt::Debug {
    /// Execute a shell command and return its output.
    async fn execute_shell(&self, command: &str, timeout: Duration) -> SandboxResult<SandboxOutput>;

    /// Read a file's contents as bytes.
    async fn read_file(&self, path: &Path) -> SandboxResult<Vec<u8>>;

    /// Write bytes to a file.
    async fn write_file(&self, path: &Path, data: &[u8]) -> SandboxResult<()>;

    /// List the operations this sandbox supports.
    fn supported_operations(&self) -> Vec<SandboxOperation>;
}

// ── Built-in implementations ─────────────────────────────────────────────

/// Direct host execution with no isolation.
///
/// This is the default sandbox — uses `tokio::process::Command` for shell
/// and `tokio::fs` for file I/O. Has zero overhead beyond the operation
/// itself.
#[derive(Debug, Clone)]
pub struct DirectSandbox;

impl DirectSandbox {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for DirectSandbox {
    fn default() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Sandbox for DirectSandbox {
    async fn execute_shell(&self, command: &str, timeout: Duration) -> SandboxResult<SandboxOutput> {
        tokio::time::timeout(timeout, async {
            let output = tokio::process::Command::new(if cfg!(windows) {
                "cmd"
            } else {
                "sh"
            })
            .arg(if cfg!(windows) { "/C" } else { "-c" })
            .arg(command)
            .output()
            .await
            .map_err(|e| SandboxError::ExecutionFailed {
                detail: format!("failed to execute command: {e}"),
            })?;

            Ok(SandboxOutput {
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                exit_code: output.status.code().unwrap_or(-1),
            }) as SandboxResult<SandboxOutput>
        })
        .await
        .map_err(|_| SandboxError::Timeout { duration: timeout })?
    }

    async fn read_file(&self, path: &Path) -> SandboxResult<Vec<u8>> {
        tokio::fs::read(path)
            .await
            .map_err(|e| SandboxError::ExecutionFailed {
                detail: format!("failed to read file: {e}"),
            })
    }

    async fn write_file(&self, path: &Path, data: &[u8]) -> SandboxResult<()> {
        tokio::fs::write(path, data)
            .await
            .map_err(|e| SandboxError::ExecutionFailed {
                detail: format!("failed to write file: {e}"),
            })
    }

    fn supported_operations(&self) -> Vec<SandboxOperation> {
        vec![
            SandboxOperation::ExecuteShell,
            SandboxOperation::ReadFile,
            SandboxOperation::WriteFile,
        ]
    }
}

/// Blocks all operations — always returns `Unsupported`.
///
/// Useful for testing rejection paths or for locked-down agents
/// that should not have any sandbox capability.
#[derive(Debug, Clone, Copy)]
pub struct NullSandbox;

#[async_trait::async_trait]
impl Sandbox for NullSandbox {
    async fn execute_shell(&self, _command: &str, _timeout: Duration) -> SandboxResult<SandboxOutput> {
        Err(SandboxError::Unsupported {
            operation: "shell".into(),
        })
    }

    async fn read_file(&self, _path: &Path) -> SandboxResult<Vec<u8>> {
        Err(SandboxError::Unsupported {
            operation: "read_file".into(),
        })
    }

    async fn write_file(&self, _path: &Path, _data: &[u8]) -> SandboxResult<()> {
        Err(SandboxError::Unsupported {
            operation: "write_file".into(),
        })
    }

    fn supported_operations(&self) -> Vec<SandboxOperation> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_null_sandbox_rejects_all() {
        let s = NullSandbox;
        let result = s.execute_shell("echo hi", Duration::from_secs(5)).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SandboxError::Unsupported { .. }));

        assert!(s.read_file(Path::new("/tmp/x")).await.is_err());
        assert!(s.write_file(Path::new("/tmp/x"), b"data").await.is_err());
    }

    #[test]
    fn test_null_sandbox_no_operations() {
        let s = NullSandbox;
        assert!(s.supported_operations().is_empty());
    }

    #[test]
    fn test_direct_sandbox_supported_ops() {
        let s = DirectSandbox::new();
        let ops = s.supported_operations();
        assert!(ops.contains(&SandboxOperation::ExecuteShell));
        assert!(ops.contains(&SandboxOperation::ReadFile));
        assert!(ops.contains(&SandboxOperation::WriteFile));
    }
}
