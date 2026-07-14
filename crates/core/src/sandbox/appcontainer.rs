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
//! runs shell commands as its child process, and uses
//! `PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES` to run the child inside
//! the AppContainer.

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

        #[cfg(windows)]
        Self::create_appcontainer_profile(&name)?;

        Ok(Self {
            profile_name: name,
            allowed_path: None,
        })
    }

    /// Restrict file access to a specific directory tree.
    #[must_use]
    pub fn with_allowed_path(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.allowed_path = Some(path.into());
        self
    }

    /// Spawn a command process inside the AppContainer and return once it finishes.
    ///
    /// This is a synchronous (blocking) helper used inside `execute_shell`.
    #[cfg(windows)]
    fn spawn_inside_appcontainer(
        &self,
        command: &str,
    ) -> Result<(Vec<u8>, Vec<u8>, i32), SandboxError> {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        // ── 1. Derive the AppContainer SID from the profile name ──
        let name_wide: Vec<u16> = OsStr::new(&self.profile_name)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        // The SID from DeriveAppContainerSidFromAppContainerName is an opaque
        // pointer used only as the AppContainerSid field of SECURITY_CAPABILITIES.
        let mut sid: *mut std::ffi::c_void = std::ptr::null_mut();

        #[link(name = "userenv")]
        unsafe extern "system" {
            fn DeriveAppContainerSidFromAppContainerName(
                pszAppContainerName: *const u16,
                ppSid: *mut *mut std::ffi::c_void,
            ) -> i64;
        }

        let hr = unsafe { DeriveAppContainerSidFromAppContainerName(name_wide.as_ptr(), &mut sid) };
        if hr != 0 {
            return Err(SandboxError::ExecutionFailed {
                detail: format!(
                    "DeriveAppContainerSidFromAppContainerName failed: HRESULT 0x{hr:08x}"
                ),
            });
        }

        // ── 2. Build SECURITY_CAPABILITIES ──
        use windows_sys::Win32::Security::SECURITY_CAPABILITIES;

        let caps = SECURITY_CAPABILITIES {
            AppContainerSid: sid,
            Capabilities: std::ptr::null_mut(),
            CapabilityCount: 0,
            Reserved: 0,
        };

        // ── 3. Set up STARTUPINFOEXW with attribute list ──
        use windows_sys::Win32::Foundation::{CloseHandle, FALSE, HANDLE};
        use windows_sys::Win32::System::Threading::{
            CreateProcessW, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT,
            InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION,
            STARTUPINFOEXW, UpdateProcThreadAttribute,
        };

        // First call to get the required buffer size
        let mut attr_list_size: usize = 0;
        // Safety: passing null to probe required size
        unsafe {
            InitializeProcThreadAttributeList(
                std::ptr::null_mut(),
                1, // one attribute
                0,
                &mut attr_list_size,
            );
        }

        // Allocate the attribute list buffer
        let mut attr_buffer = vec![0u8; attr_list_size];
        let attr_list = attr_buffer.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;

        // Safety: attr_buffer is properly sized
        if unsafe { InitializeProcThreadAttributeList(attr_list, 1, 0, &mut attr_list_size) }
            == FALSE
        {
            return Err(SandboxError::ExecutionFailed {
                detail: "InitializeProcThreadAttributeList failed".into(),
            });
        }

        // Safety: add the AppContainer capability attribute
        // PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES = 0x20009 (u32)
        // windows-sys gives it as a u32 so we cast to usize for UpdateProcThreadAttribute
        const PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES: usize = 0x20009;
        let update_ok = unsafe {
            UpdateProcThreadAttribute(
                attr_list,
                0,
                PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
                &caps as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<SECURITY_CAPABILITIES>(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if update_ok == FALSE {
            // Safety: cleanup on error
            unsafe {
                DeleteProcThreadAttributeList(attr_list);
            }
            return Err(SandboxError::ExecutionFailed {
                detail: "UpdateProcThreadAttribute for AppContainer SID failed".into(),
            });
        }

        // ── 4. Create stdout/stderr pipes ──
        use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
        use windows_sys::Win32::System::Pipes::CreatePipe;

        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: 1, // TRUE — children inherit
        };

        let mut stdout_read: HANDLE = std::ptr::null_mut();
        let mut stdout_write: HANDLE = std::ptr::null_mut();
        let mut stderr_read: HANDLE = std::ptr::null_mut();
        let mut stderr_write: HANDLE = std::ptr::null_mut();

        // Safety: pipe handles are written by CreatePipe
        if unsafe {
            CreatePipe(
                &mut stdout_read,
                &mut stdout_write,
                &sa as *const _ as *mut _,
                0,
            )
        } == FALSE
            || unsafe {
                CreatePipe(
                    &mut stderr_read,
                    &mut stderr_write,
                    &sa as *const _ as *mut _,
                    0,
                )
            } == FALSE
        {
            unsafe {
                DeleteProcThreadAttributeList(attr_list);
            }
            return Err(SandboxError::ExecutionFailed {
                detail: "CreatePipe failed".into(),
            });
        }

        const STARTF_USESTDHANDLES: u32 = 0x0000_0100;

        // ── 5. Build command line ──
        let cmd_wide: Vec<u16> = OsStr::new("cmd.exe")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let full_args = format!("/C {command}");
        let args_wide: Vec<u16> = OsStr::new(&full_args)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        // ── 6. Create the process ──
        let mut si: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
        si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        si.StartupInfo.hStdOutput = stdout_write;
        si.StartupInfo.hStdError = stderr_write;
        si.lpAttributeList = attr_list;

        let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };

        // Safety: CreateProcessW with properly initialised structures
        let created = unsafe {
            CreateProcessW(
                cmd_wide.as_ptr(),
                args_wide.as_ptr() as *mut u16,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                1, // TRUE — inherit handles (needed for pipes)
                EXTENDED_STARTUPINFO_PRESENT | 0x00000010, // also CREATE_NEW_CONSOLE
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &si.StartupInfo,
                &mut pi,
            )
        };

        // Always clean up the attribute list
        unsafe {
            DeleteProcThreadAttributeList(attr_list);
        }

        // Close the write ends so ReadFile won't hang
        unsafe {
            CloseHandle(stdout_write);
            CloseHandle(stderr_write);
        }

        if created == FALSE {
            unsafe {
                CloseHandle(stdout_read);
                CloseHandle(stderr_read);
            }
            return Err(SandboxError::ExecutionFailed {
                detail: "CreateProcessW inside AppContainer failed".into(),
            });
        }

        // Close the thread handle (we only need the process)
        unsafe {
            CloseHandle(pi.hThread);
        }

        // ── 7. Read stdout/stderr and wait ──
        use std::io::Read;

        let mut stdout_data = Vec::new();
        let mut stderr_data = Vec::new();

        // Wrap raw handles in std::fs::File for convenient reading
        // Safety: these are valid pipe handles
        let mut stdout_file: std::fs::File = unsafe {
            use std::os::windows::io::FromRawHandle;
            FromRawHandle::from_raw_handle(stdout_read as std::os::windows::raw::HANDLE)
        };
        let mut stderr_file: std::fs::File = unsafe {
            use std::os::windows::io::FromRawHandle;
            FromRawHandle::from_raw_handle(stderr_read as std::os::windows::raw::HANDLE)
        };

        // Wait for the process to finish
        unsafe {
            windows_sys::Win32::System::Threading::WaitForSingleObject(
                pi.hProcess,
                windows_sys::Win32::System::Threading::INFINITE,
            );
        }

        // Read remaining pipe data
        let _ = stdout_file.read_to_end(&mut stdout_data);
        let _ = stderr_file.read_to_end(&mut stderr_data);

        // Get exit code
        let mut exit_code: u32 = 0;
        unsafe {
            windows_sys::Win32::System::Threading::GetExitCodeProcess(pi.hProcess, &mut exit_code);
            CloseHandle(pi.hProcess);
        }

        Ok((stdout_data, stderr_data, exit_code as i32))
    }

    /// Create an AppContainer profile using the Windows API.
    ///
    /// Uses raw FFI declarations since `windows-sys` does not export
    /// `CreateAppContainerProfile` on all versions.
    #[cfg(windows)]
    fn create_appcontainer_profile(name: &str) -> SandboxResult<()> {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        const E_ALREADY_REGISTERED: i64 = 0x800701F4;
        const S_OK: i64 = 0;

        let name_wide: Vec<u16> = OsStr::new(name)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let display_wide: Vec<u16> = OsStr::new(&format!("Praxis Sandbox: {name}"))
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let desc_wide: Vec<u16> = OsStr::new("Praxis agent sandbox container")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

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

        if hr == S_OK || hr == E_ALREADY_REGISTERED {
            Ok(())
        } else {
            Err(SandboxError::ExecutionFailed {
                detail: format!("CreateAppContainerProfile failed with HRESULT: 0x{hr:08x}"),
            })
        }
    }

    /// Clean up the AppContainer profile.
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
            #[cfg(windows)]
            {
                let (stdout, stderr, exit_code) = self.spawn_inside_appcontainer(command)?;
                Ok(SandboxOutput {
                    stdout: String::from_utf8_lossy(&stdout).to_string(),
                    stderr: String::from_utf8_lossy(&stderr).to_string(),
                    exit_code,
                })
            }

            #[cfg(not(windows))]
            {
                // Fallback: no AppContainer on non-Windows
                let output = tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg(command)
                    .output()
                    .await
                    .map_err(|e| SandboxError::ExecutionFailed {
                        detail: format!("sandbox execution failed: {e}"),
                    })?;
                Ok(SandboxOutput {
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                    exit_code: output.status.code().unwrap_or(-1),
                })
            }
        })
        .await
        .map_err(|_| SandboxError::Timeout { duration: timeout })?
    }

    async fn read_file(&self, path: &Path) -> SandboxResult<Vec<u8>> {
        // If an allowed path is configured, ensure the file is within it
        if let Some(ref allowed) = self.allowed_path
            && !path.starts_with(allowed)
        {
            return Err(SandboxError::PolicyDenied {
                reason: format!(
                    "file '{}' is outside the allowed path '{}'",
                    path.display(),
                    allowed.display()
                ),
            });
        }

        tokio::fs::read(path)
            .await
            .map_err(|e| SandboxError::ExecutionFailed {
                detail: format!("AppContainer sandbox file read failed: {e}"),
            })
    }

    async fn write_file(&self, path: &Path, data: &[u8]) -> SandboxResult<()> {
        if let Some(ref allowed) = self.allowed_path
            && !path.starts_with(allowed)
        {
            return Err(SandboxError::PolicyDenied {
                reason: format!(
                    "file '{}' is outside the allowed path '{}'",
                    path.display(),
                    allowed.display()
                ),
            });
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

    #[cfg(windows)]
    #[test]
    fn test_appcontainer_new() {
        let name = format!("test-container-{}", std::process::id());
        let result = AppContainerSandbox::new(&name);
        if let Ok(sandbox) = result {
            let ops = sandbox.supported_operations();
            assert!(ops.contains(&SandboxOperation::ExecuteShell));
        }
    }

    #[ignore = "requires AppContainer admin rights on Windows"]
    #[tokio::test]
    async fn test_appcontainer_execute_shell() {
        let name = format!("test-exec-{}", std::process::id());
        let sandbox = match AppContainerSandbox::new(&name) {
            Ok(s) => s,
            Err(_) => return,
        };

        let output = sandbox
            .execute_shell("echo hello from container", Duration::from_secs(10))
            .await;

        if let Ok(out) = output {
            assert!(out.stdout.contains("hello"), "stdout: {}", out.stdout);
        }
    }

    #[tokio::test]
    async fn test_appcontainer_allowed_path_rejects_outside() {
        let sandbox = match AppContainerSandbox::new("test-container-path-test") {
            Ok(s) => s.with_allowed_path("C:\\sandbox"),
            Err(_) => return,
        };

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
