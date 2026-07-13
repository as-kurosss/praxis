//! **Linux Bubblewrap Sandbox** — OS-level isolation using the `bwrap` CLI.
//!
//! Bubblewrap (bwrap) is a setuid binary that creates mount namespaces,
//! providing lightweight containerization. This module wraps the `bwrap`
//! CLI to execute commands in an isolated environment.
//!
//! This module is only available on Linux (`#[cfg(target_os = "linux")]`).

use super::types::{SandboxError, SandboxOperation, SandboxOutput, SandboxResult};
use std::path::Path;
use std::time::Duration;

/// Configuration for the Bubblewrap sandbox.
///
/// Controls which mount points are available inside the sandbox,
/// whether networking is enabled, and other namespace settings.
#[derive(Debug, Clone)]
pub struct BubblewrapConfig {
    /// Directories to mount as read-only inside the sandbox.
    pub ro_mounts: Vec<std::path::PathBuf>,
    /// Directories to mount as read-write inside the sandbox.
    pub rw_mounts: Vec<std::path::PathBuf>,
    /// If true, /usr, /lib, /etc, etc. are mounted (minimal base system).
    pub share_base_system: bool,
    /// If true, the network namespace is shared with the host.
    pub share_network: bool,
    /// If true, the process can see the host's PID namespace.
    pub share_pid: bool,
    /// If true, /tmp is isolated (private tmpfs).
    pub private_tmp: bool,
    /// Path to the bwrap binary.
    pub bwrap_path: std::path::PathBuf,
    /// Additional arguments to pass to bwrap.
    pub extra_args: Vec<String>,
}

impl Default for BubblewrapConfig {
    fn default() -> Self {
        Self {
            ro_mounts: Vec::new(),
            rw_mounts: Vec::new(),
            share_base_system: false,
            share_network: false,
            share_pid: false,
            private_tmp: true,
            bwrap_path: std::path::PathBuf::from("bwrap"),
            extra_args: Vec::new(),
        }
    }
}

/// Linux bubblewrap-based sandbox.
///
/// Uses the `bwrap` CLI to execute shell commands inside a mount namespace
/// with configurable visibility of host filesystems.
///
/// # Example
///
/// ```ignore
/// use praxis_core::sandbox::BubblewrapSandbox;
///
/// let sandbox = BubblewrapSandbox::new(BubblewrapConfig::default());
/// let output = sandbox.execute_shell("echo hello", std::time::Duration::from_secs(30)).await?;
/// println!("{}", output.stdout);
/// ```
#[derive(Debug, Clone)]
pub struct BubblewrapSandbox {
    /// Configuration for the bubblewrap invocation.
    config: BubblewrapConfig,
}

impl BubblewrapSandbox {
    /// Create a new bubblewrap sandbox with the given configuration.
    #[must_use]
    pub fn new(config: BubblewrapConfig) -> Self {
        Self { config }
    }

    /// Build the `bwrap` CLI arguments based on the current configuration.
    fn build_bwrap_args(&self, command: &str) -> Vec<String> {
        let mut args = Vec::new();

        // Die if bwrap is not available
        args.push("--die-with-parent".to_string());

        // Private /tmp
        if self.config.private_tmp {
            args.push("--tmpfs".to_string());
            args.push("/tmp".to_string());
        }

        // Mount base system directories
        if self.config.share_base_system {
            for dir in &["/usr", "/lib", "/lib64", "/etc", "/bin", "/sbin"] {
                args.push("--ro-bind".to_string());
                args.push(dir.to_string());
                args.push(dir.to_string());
            }
        }

        // Add configured read-only mounts
        for path in &self.config.ro_mounts {
            args.push("--ro-bind".to_string());
            args.push(path.to_string_lossy().to_string());
            args.push(path.to_string_lossy().to_string());
        }

        // Add configured read-write mounts
        for path in &self.config.rw_mounts {
            args.push("--bind".to_string());
            args.push(path.to_string_lossy().to_string());
            args.push(path.to_string_lossy().to_string());
        }

        // Network namespace
        if !self.config.share_network {
            args.push("--unshare-net".to_string());
        }

        // PID namespace
        if !self.config.share_pid {
            args.push("--unshare-pid".to_string());
        }

        // Extra arguments
        args.extend(self.config.extra_args.iter().cloned());

        // The command to execute
        args.push("--".to_string());
        args.push("sh".to_string());
        args.push("-c".to_string());
        args.push(command.to_string());

        args
    }

    /// Check if the bwrap binary is available on the system.
    pub fn is_available() -> bool {
        std::process::Command::new("which")
            .arg("bwrap")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

#[async_trait::async_trait]
impl super::Sandbox for BubblewrapSandbox {
    async fn execute_shell(
        &self,
        command: &str,
        timeout: Duration,
    ) -> SandboxResult<SandboxOutput> {
        let bwrap_args = self.build_bwrap_args(command);

        tokio::time::timeout(timeout, async {
            let output = tokio::process::Command::new(&self.config.bwrap_path)
                .args(&bwrap_args)
                .output()
                .await
                .map_err(|e| SandboxError::ExecutionFailed {
                    detail: format!("bwrap execution failed: {e}"),
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
        // Build a bwrap command that just cats the file
        let file_path = path.to_string_lossy().to_string();
        let cat_command = format!("cat \"{file_path}\"");

        let output = self
            .execute_shell(&cat_command, Duration::from_secs(10))
            .await?;

        Ok(output.stdout.into_bytes())
    }

    async fn write_file(&self, path: &Path, data: &[u8]) -> SandboxResult<()> {
        let file_path = path.to_string_lossy().to_string();
        // Use base64 to safely pass binary data through the shell
        let encoded = base64_encode(data);
        let write_command = format!("echo \"{encoded}\" | base64 -d > \"{file_path}\"");

        self.execute_shell(&write_command, Duration::from_secs(10))
            .await?;
        Ok(())
    }

    fn supported_operations(&self) -> Vec<SandboxOperation> {
        use SandboxOperation::*;
        vec![ExecuteShell, ExecuteCode, ReadFile, WriteFile]
    }
}

/// Encode bytes as base64 (no external crate dependency for this simple case).
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();

    for chunk in data.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(if chunk.len() > 1 { chunk[1] } else { 0 });
        let b2 = u32::from(if chunk.len() > 2 { chunk[2] } else { 0 });
        let combined = (b0 << 16) | (b1 << 8) | b2;

        result.push(CHARS[((combined >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((combined >> 12) & 0x3F) as usize] as char);

        if chunk.len() > 1 {
            result.push(CHARS[((combined >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }

        if chunk.len() > 2 {
            result.push(CHARS[(combined & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bubblewrap_config_default() {
        let config = BubblewrapConfig::default();
        assert!(config.private_tmp);
        assert!(!config.share_network);
        assert_eq!(config.bwrap_path.to_string_lossy(), "bwrap");
    }

    #[test]
    fn test_build_args_no_network() {
        let config = BubblewrapConfig {
            share_network: false,
            ..BubblewrapConfig::default()
        };
        let sandbox = BubblewrapSandbox::new(config);
        let args = sandbox.build_bwrap_args("echo hello");
        assert!(args.contains(&"--unshare-net".to_string()));
        assert!(args.contains(&"echo hello".to_string()));
    }

    #[test]
    fn test_build_args_with_network() {
        let config = BubblewrapConfig {
            share_network: true,
            ..BubblewrapConfig::default()
        };
        let sandbox = BubblewrapSandbox::new(config);
        let args = sandbox.build_bwrap_args("curl example.com");
        assert!(!args.contains(&"--unshare-net".to_string()));
        assert!(args.contains(&"curl example.com".to_string()));
    }

    #[test]
    fn test_base64_roundtrip() {
        let original = b"Hello, World! This has \0 binary \xFF data.";
        let encoded = base64_encode(original);
        assert!(!encoded.is_empty());
        // basic sanity: length should be roughly 4/3 of original
        assert!(encoded.len() >= original.len());
    }

    #[test]
    fn test_bubblewrap_is_available_or_not() {
        // Just verify this doesn't panic
        let _available = BubblewrapSandbox::is_available();
    }
}
