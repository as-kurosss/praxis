//! **ResourcePolicy** — permission checks for agent resource access.
//!
//! Policies are checked *before* an operation reaches the sandbox.
//! They are zero-cost when trivial (e.g. `AllowAll`).

use crate::error::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Permission check for agent resource access.
///
/// Each method returns `Ok(())` if the access is allowed, or
/// `Err(Error::AccessDenied { .. })` if blocked.
pub trait ResourcePolicy: Send + Sync + std::fmt::Debug {
    /// Check whether a shell command is allowed.
    fn check_shell(&self, command: &str) -> Result<()>;

    /// Check whether a file read at `path` is allowed.
    fn check_read(&self, path: &Path) -> Result<()>;

    /// Check whether a file write at `path` is allowed.
    fn check_write(&self, path: &Path) -> Result<()>;

    /// Check whether a network request to `url` is allowed.
    fn check_network(&self, url: &str) -> Result<()>;
}

// ── Built-in policies ────────────────────────────────────────────────────

/// Permits everything (default, zero overhead).
///
/// All methods return `Ok(())` unconditionally. The compiler optimises these
/// trivial bodies away at inline sites.
#[derive(Debug, Clone, Copy)]
pub struct AllowAll;

impl ResourcePolicy for AllowAll {
    fn check_shell(&self, _command: &str) -> Result<()> {
        Ok(())
    }

    fn check_read(&self, _path: &Path) -> Result<()> {
        Ok(())
    }

    fn check_write(&self, _path: &Path) -> Result<()> {
        Ok(())
    }

    fn check_network(&self, _url: &str) -> Result<()> {
        Ok(())
    }
}

/// Blocks everything unconditionally.
#[derive(Debug, Clone, Copy)]
pub struct DenyAll;

impl ResourcePolicy for DenyAll {
    fn check_shell(&self, _command: &str) -> Result<()> {
        Err(crate::error::Error::AccessDenied {
            resource: "shell".into(),
            reason: "all shell commands are denied by policy".into(),
        })
    }

    fn check_read(&self, path: &Path) -> Result<()> {
        Err(crate::error::Error::AccessDenied {
            resource: format!("read: {}", path.display()),
            reason: "all read access is denied by policy".into(),
        })
    }

    fn check_write(&self, path: &Path) -> Result<()> {
        Err(crate::error::Error::AccessDenied {
            resource: format!("write: {}", path.display()),
            reason: "all write access is denied by policy".into(),
        })
    }

    fn check_network(&self, _url: &str) -> Result<()> {
        Err(crate::error::Error::AccessDenied {
            resource: "network".into(),
            reason: "all network access is denied by policy".into(),
        })
    }
}

/// Blocks specific shell commands by substring matching.
///
/// Replaces the hardcoded blacklist that previously lived in `ShellTool`.
#[derive(Debug, Clone)]
pub struct ShellBlocklist {
    /// Command substrings that are blocked (case-insensitive).
    pub blocked_patterns: Vec<String>,
}

impl ShellBlocklist {
    /// Create a new blocklist with the given dangerous patterns.
    #[must_use]
    pub fn new(patterns: Vec<impl Into<String>>) -> Self {
        Self {
            blocked_patterns: patterns.into_iter().map(Into::into).collect(),
        }
    }

    /// The default blocklist matching common dangerous operations.
    #[must_use]
    pub fn default_blocked() -> Self {
        Self::new(vec![
            "rm -rf /",
            "rm -rf /*",
            "mkfs",
            "dd if=",
            ":(){ :|:& };:",
            "> /dev/sda",
            "> /dev/sdb",
            "> /dev/nvme",
            "format",
            "fdisk",
            "mkswap",
        ])
    }
}

impl ResourcePolicy for ShellBlocklist {
    fn check_shell(&self, command: &str) -> Result<()> {
        let lower = command.to_lowercase();
        for pattern in &self.blocked_patterns {
            if lower.contains(&pattern.to_lowercase()) {
                return Err(crate::error::Error::AccessDenied {
                    resource: "shell".into(),
                    reason: format!("command blocked (matched pattern: '{pattern}')"),
                });
            }
        }
        Ok(())
    }

    fn check_read(&self, _path: &Path) -> Result<()> {
        Ok(()) // only restricts shell by default
    }

    fn check_write(&self, _path: &Path) -> Result<()> {
        Ok(())
    }

    fn check_network(&self, _url: &str) -> Result<()> {
        Ok(())
    }
}

/// Restrict file access to allowed directories.
#[derive(Debug, Clone)]
pub struct PathRestrict {
    /// The only directories that may be read/written.
    pub allowed_dirs: Vec<PathBuf>,
    /// If true, also permits subdirectories of allowed dirs.
    pub allow_subdirs: bool,
}

impl PathRestrict {
    /// Create a policy that only allows access within `allowed_dirs`.
    ///
    /// Directories are resolved to canonical forms at construction time.
    #[must_use]
    pub fn new(dirs: Vec<impl Into<PathBuf>>) -> Self {
        let allowed_dirs: Vec<PathBuf> = dirs
            .into_iter()
            .map(Into::into)
            .map(|p| std::fs::canonicalize(&p).unwrap_or(p))
            .collect();
        Self {
            allowed_dirs,
            allow_subdirs: true,
        }
    }

    fn is_allowed(&self, path: &Path) -> bool {
        for allowed in &self.allowed_dirs {
            if self.allow_subdirs {
                if path.starts_with(allowed) {
                    return true;
                }
            } else if path == allowed {
                return true;
            }
        }
        false
    }
}

impl ResourcePolicy for PathRestrict {
    fn check_shell(&self, _command: &str) -> Result<()> {
        Ok(()) // only restricts file access
    }

    fn check_read(&self, path: &Path) -> Result<()> {
        if self.is_allowed(path) {
            Ok(())
        } else {
            Err(crate::error::Error::AccessDenied {
                resource: format!("read: {}", path.display()),
                reason: format!("path not in allowed directories: {:?}", self.allowed_dirs),
            })
        }
    }

    fn check_write(&self, path: &Path) -> Result<()> {
        if self.is_allowed(path) {
            Ok(())
        } else {
            Err(crate::error::Error::AccessDenied {
                resource: format!("write: {}", path.display()),
                reason: format!("path not in allowed directories: {:?}", self.allowed_dirs),
            })
        }
    }

    fn check_network(&self, _url: &str) -> Result<()> {
        Ok(())
    }
}

/// Chains multiple policies — all must pass for access to be granted.
#[derive(Debug, Clone)]
pub struct PolicyChain {
    policies: Vec<Arc<dyn ResourcePolicy>>,
}

impl PolicyChain {
    /// Create a chain from multiple policies.
    #[must_use]
    pub fn new(policies: Vec<Arc<dyn ResourcePolicy>>) -> Self {
        Self { policies }
    }
}

impl ResourcePolicy for PolicyChain {
    fn check_shell(&self, command: &str) -> Result<()> {
        for p in &self.policies {
            p.check_shell(command)?;
        }
        Ok(())
    }

    fn check_read(&self, path: &Path) -> Result<()> {
        for p in &self.policies {
            p.check_read(path)?;
        }
        Ok(())
    }

    fn check_write(&self, path: &Path) -> Result<()> {
        for p in &self.policies {
            p.check_write(path)?;
        }
        Ok(())
    }

    fn check_network(&self, url: &str) -> Result<()> {
        for p in &self.policies {
            p.check_network(url)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    #[test]
    fn test_allow_all() {
        let p = AllowAll;
        assert!(p.check_shell("rm -rf /").is_ok());
        assert!(p.check_read("/etc/passwd".as_ref()).is_ok());
        assert!(p.check_write("/".as_ref()).is_ok());
        assert!(p.check_network("http://evil.com").is_ok());
    }

    #[test]
    fn test_deny_all() {
        let p = DenyAll;
        assert!(p.check_shell("echo hi").is_err());
        assert!(p.check_read("/tmp".as_ref()).is_err());
    }

    #[test]
    fn test_shell_blocklist() {
        let p = ShellBlocklist::default_blocked();
        assert!(p.check_shell("echo hello").is_ok());
        assert!(p.check_shell("rm -rf /").is_err());
        assert!(p.check_shell("rm -rf /*").is_err());
        assert!(p.check_shell("mkfs.ext4 /dev/sda1").is_err());
    }

    #[test]
    fn test_shell_blocklist_allows_safe() {
        let p = ShellBlocklist::default_blocked();
        assert!(p.check_shell("rm file.txt").is_ok()); // no -rf /
        assert!(p.check_shell("ls -la").is_ok());
    }

    #[test]
    fn test_path_restrict_allows_subdir() {
        let p = PathRestrict::new(vec!["/tmp/workdir"]);
        assert!(p.check_read("/tmp/workdir/file.txt".as_ref()).is_ok());
        assert!(p.check_read("/tmp/workdir/sub/file.txt".as_ref()).is_ok()); // subdir allowed
    }

    #[test]
    fn test_path_restrict_blocks_outside() {
        let p = PathRestrict::new(vec!["/tmp/workdir"]);
        let result = p.check_read("/etc/passwd".as_ref());
        assert!(result.is_err());
        if let Err(Error::AccessDenied {
            resource: _,
            reason: _,
        }) = result
        {
            // expected
        } else {
            panic!("expected AccessDenied");
        }
    }

    #[test]
    fn test_policy_chain_all_pass() {
        let chain = PolicyChain::new(vec![
            Arc::new(AllowAll) as Arc<dyn ResourcePolicy>,
            Arc::new(ShellBlocklist::new(vec!["rm"])),
        ]);
        assert!(chain.check_shell("echo hi").is_ok());
        assert!(chain.check_shell("rm -rf /").is_err());
    }

    #[test]
    fn test_policy_chain_first_fail_shortcircuits() {
        let chain = PolicyChain::new(vec![
            Arc::new(DenyAll) as Arc<dyn ResourcePolicy>,
            Arc::new(AllowAll), // never reached
        ]);
        assert!(chain.check_shell("anything").is_err());
    }
}
