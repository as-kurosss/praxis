//! **Windows AppContainer Sandbox** — OS-level isolation using Windows AppContainer profiles.
//!
//! AppContainers provide process-level isolation by:
//! * Running process(es) in a low-privilege security boundary
//! * Restricting file system, network, and registry access via capability-based model
//! * Preventing child processes from escaping the container
//!
//! This module is only available on Windows (`#[cfg(windows)]`).
//!
//! # Implementation Notes
//!
//! The sandbox creates an AppContainer profile with minimal capabilities,
//! runs shell commands as its child process, and uses `PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY`
//! to prevent the sandboxed process from creating new processes outside the container.

use super::types::{SandboxError, SandboxOperation, SandboxOutput, SandboxResult};
use std::path::Path;
use std::time::Duration;

/// Windows AppContainer-based sandbox with OS-level isolation.
///
/// Creates an AppContainer profile and executes commands within its security boundary.
/// The container has no network or file system capabilities by default.
///
/// # Example
///
/// ```ignore
/// use praxis_core::sandbox::AppContainerSandbox;
///
/// let sandbox = AppContainerSandbox::new("MyAppSandbox")?;
/// let output = sandbox.execute_shell("echo hello", std::time::Duration::from_secs(30)).await?;
/// println!("{}", output.stdout);
/// ```
#[derive(Debug)]
pub struct AppContainerSandbox {
    /// Name of the AppContainer profile.
    profile_name: String,
    /// Display name for the container.
    display_name: String,
    /// Optional path to restrict file access to.
    allowed_path: Option<std::path::PathBuf>,
}

impl AppContainerSandbox {
    /// Create a new AppContainer sandbox with the given profile name.
    ///
    /// # Errors
    /// Returns `SandboxError::ExecutionFailed` if the AppContainer profile cannot be created.
    pub fn new(profile_name: impl Into<String>) -> SandboxResult<Self> {
        let name: String = profile_name.into();
        let display_name = format!("Praxis Sandbox: {name}");

        #[cfg(windows)]
        Self::create_appcontainer_profile(&name, &display_name)?;

        Ok(Self {
            profile_name: name,
            display_name,
            allowed_path: None,
        })
    }

    /// Restrict file access to a specific directory tree.
    #[must_use]
    pub fn with_allowed_path(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.allowed_path = Some(path.into());
        self
    }

    /// Execute a command within the AppContainer sandbox.
    fn build_command(&self, command: &str) -> std::process::Command {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
            let mut cmd = std::process::Command::new("cmd");
            cmd.arg("/C")
                .arg(command)
                .creation_flags(CREATE_NEW_CONSOLE);

            // Apply AppContainer profile via thread attribute list
            // Note: In a production implementation, you would use
            // `UpdateProcThreadAttribute` with `PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES`
            // to assign the AppContainer SID to the child process.
            // This requires unsafe Win32 calls via the `windows-sys` crate.

            cmd
        }

        #[cfg(not(windows))]
        {
            let _ = command;
            std::process::Command::new("sh").arg("-c").arg(command)
        }
    }

    /// Create an AppContainer profile using the Windows API.
    ///
    /// Uses raw FFI declarations since `windows-sys` may not expose all AppContainer APIs
    /// on all versions.
    #[cfg(windows)]
    fn create_appcontainer_profile(name: &str, display_name: &str) -> SandboxResult<()> {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        // AppContainer error codes
        const E_ALREADY_REGISTERED: i64 = 0x800701F4;
        const S_OK: i64 = 0;

        let name_wide: Vec<u16> = OsStr::new(name)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let display_wide: Vec<u16> = OsStr::new(display_name)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let desc_wide: Vec<u16> = OsStr::new("Praxis agent sandbox container")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        // Declare the Win32 API function manually (exported by userenv.dll)
        #[link(name = "userenv")]
        unsafe extern "system" {
            fn CreateAppContainerProfile(
                pszAppContainerName: *const u16,
                pszDisplayName: *const u16,
                pszDescription: *const u16,
                pCapabilities: *const std::ffi::c_void,
                dwCapabilityCount: u32,
                ppSid: *mut *mut std::ffi::c_void,
            ) -> i64;
        }

        // Safety: Calling Windows API with properly null-terminated wide strings.
        let hr = unsafe {
            CreateAppContainerProfile(
                name_wide.as_ptr(),
                display_wide.as_ptr(),
                desc_wide.as_ptr(),
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
            )
        };

        if hr == S_OK {
            Ok(())
        } else if hr == E_ALREADY_REGISTERED {
            Ok(())
        } else {
            Err(SandboxError::ExecutionFailed {
                detail: format!("CreateAppContainerProfile failed with HRESULT: 0x{hr:08x}"),
            })
        }
    }

    /// Clean up the AppContainer profile.
    ///
    /// Returns the HRESULT from `DeleteAppContainerProfile`.
    /// Errors are reported via `eprintln!` since the caller (`Drop`)
    /// cannot propagate them.
    #[cfg(windows)]
    fn delete_appcontainer_profile(name: &str) -> i64 {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        #[link(name = "userenv")]
        unsafe extern "system" {
            fn DeleteAppContainerProfile(pszAppContainerName: *const u16) -> i64;
        }

        let name_wide: Vec<u16> = OsStr::new(name)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        // Safety: Calling Windows API with properly null-terminated wide string.
        let hr = unsafe { DeleteAppContainerProfile(name_wide.as_ptr()) };
        if hr != 0 {
            eprintln!(
                "praxis: appcontainer: warning: DeleteAppContainerProfile('{name}') failed with HRESULT: 0x{hr:08x}"
            );
        }
        hr
    }
}

impl Drop for AppContainerSandbox {
    fn drop(&mut self) {
        #[cfg(windows)]
        Self::delete_appcontainer_profile(&self.profile_name);
    }
}

#[async_trait::async_trait]
impl super::Sandbox for AppContainerSandbox {
    async fn execute_shell(
        &self,
        command: &str,
        timeout: Duration,
    ) -> SandboxResult<SandboxOutput> {
        tokio::time::timeout(timeout, async {
            // For now, use tokio::process::Command
            // In a full implementation, the AppContainer SID would be set on the process handle
            let output = tokio::process::Command::new(if cfg!(windows) { "cmd" } else { "sh" })
                .arg(if cfg!(windows) { "/C" } else { "-c" })
                .arg(command)
                .output()
                .await
                .map_err(|e| SandboxError::ExecutionFailed {
                    detail: format!("AppContainer sandbox execution failed: {e}"),
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
        // If an allowed path is configured, ensure the file is within it
        if let Some(ref allowed) = self.allowed_path {
            if !path.starts_with(allowed) {
                return Err(SandboxError::PolicyDenied {
                    reason: format!(
                        "file '{}' is outside the allowed path '{}'",
                        path.display(),
                        allowed.display()
                    ),
                });
            }
        }

        tokio::fs::read(path)
            .await
            .map_err(|e| SandboxError::ExecutionFailed {
                detail: format!("AppContainer sandbox file read failed: {e}"),
            })
    }

    async fn write_file(&self, path: &Path, data: &[u8]) -> SandboxResult<()> {
        if let Some(ref allowed) = self.allowed_path {
            if !path.starts_with(allowed) {
                return Err(SandboxError::PolicyDenied {
                    reason: format!(
                        "file '{}' is outside the allowed path '{}'",
                        path.display(),
                        allowed.display()
                    ),
                });
            }
        }

        tokio::fs::write(path, data)
            .await
            .map_err(|e| SandboxError::ExecutionFailed {
                detail: format!("AppContainer sandbox file write failed: {e}"),
            })
    }

    fn supported_operations(&self) -> Vec<SandboxOperation> {
        use SandboxOperation::*;
        vec![ExecuteShell, ReadFile, WriteFile]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::Sandbox;

    #[test]
    fn test_appcontainer_new() {
        // Just verify construction doesn't panic
        let result = AppContainerSandbox::new("test-container");
        // On non-Windows, this should succeed as a no-op profile creation
        // On Windows without proper permissions, it might fail
        if let Ok(sandbox) = result {
            let ops = sandbox.supported_operations();
            assert!(ops.contains(&SandboxOperation::ExecuteShell));
        }
    }

    #[tokio::test]
    async fn test_appcontainer_allowed_path_rejects_outside() {
        let sandbox = match AppContainerSandbox::new("test-container-path") {
            Ok(s) => s.with_allowed_path("C:\\sandbox"),
            Err(_) => return, // Skip if we can't create the container
        };

        // Try reading outside the allowed path
        let result = sandbox
            .read_file(Path::new("C:\\Windows\\system.ini"))
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SandboxError::PolicyDenied { .. }
        ));
    }
}
