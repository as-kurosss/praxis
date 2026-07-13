//! **ResourcePolicy** — permission checks for agent resource access.
//!
//! Policies are checked *before* an operation reaches the sandbox.
//! They are zero-cost when trivial (e.g. `AllowAll`).

use crate::error::Result;
use crate::sandbox::types::RiskLevel;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ── ResourcePolicy trait (unchanged) ──────────────────────────────────────

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

// ── 2.4: Configurable Shell Evasion ───────────────────────────────────────

/// A single configurable shell evasion rule with risk-level tagging.
#[derive(Debug, Clone)]
pub struct EvasionRule {
    /// The pattern to search for (case-insensitive substring match).
    pub pattern: String,
    /// Human-readable description of the threat.
    pub description: &'static str,
    /// Risk level of this pattern.
    pub risk_level: RiskLevel,
    /// Whether this rule is currently enabled.
    pub enabled: bool,
}

impl EvasionRule {
    /// Create a new evasion rule.
    #[must_use]
    pub fn new(
        pattern: impl Into<String>,
        description: &'static str,
        risk_level: RiskLevel,
    ) -> Self {
        Self {
            pattern: pattern.into(),
            description,
            risk_level,
            enabled: true,
        }
    }

    /// Disable this rule without removing it.
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Enable this rule.
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Check whether a command triggers this rule.
    fn matches(&self, command: &str) -> bool {
        if !self.enabled {
            return false;
        }
        command
            .to_lowercase()
            .contains(&self.pattern.to_lowercase())
    }
}

/// Configurable shell command blocklist with evasion rules.
///
/// Extends the original [`ShellBlocklist`] with per-rule enable/disable
/// toggles, risk-level tagging, and expanded pattern coverage.
#[derive(Debug, Clone)]
pub struct ShellBlocklist {
    /// Blocked patterns as simple substrings (legacy API).
    pub blocked_patterns: Vec<String>,
    /// Structured evasion rules with risk levels and toggles.
    pub evasion_rules: Vec<EvasionRule>,
}

impl ShellBlocklist {
    /// Create a new blocklist with the given dangerous patterns (legacy API).
    #[must_use]
    pub fn new(patterns: Vec<impl Into<String>>) -> Self {
        Self {
            blocked_patterns: patterns.into_iter().map(Into::into).collect(),
            evasion_rules: Vec::new(),
        }
    }

    /// The default blocklist matching common dangerous operations.
    #[must_use]
    pub fn default_blocked() -> Self {
        Self {
            blocked_patterns: vec![
                "rm -rf /".into(),
                "rm -rf /*".into(),
                "mkfs".into(),
                "dd if=".into(),
                ":(){ :|:& };:".into(),
                "> /dev/sda".into(),
                "> /dev/sdb".into(),
                "> /dev/nvme".into(),
                "format".into(),
                "fdisk".into(),
                "mkswap".into(),
            ],
            evasion_rules: Self::default_evasion_rules(),
        }
    }

    /// Build the default set of evasion rules with risk-level tagging.
    #[must_use]
    pub fn default_evasion_rules() -> Vec<EvasionRule> {
        vec![
            // Original destructive patterns (critical)
            EvasionRule::new(
                "rm -rf /",
                "Recursive root filesystem deletion",
                RiskLevel::Critical,
            ),
            EvasionRule::new("mkfs", "Filesystem creation tool", RiskLevel::Critical),
            EvasionRule::new("dd if=", "Raw disk write operation", RiskLevel::Critical),
            EvasionRule::new(":(){", "Fork bomb denial of service", RiskLevel::Critical),
            EvasionRule::new("> /dev/sd", "Block device raw write", RiskLevel::Critical),
            EvasionRule::new("format", "Drive/partition format", RiskLevel::Critical),
            EvasionRule::new("fdisk", "Partition table manipulation", RiskLevel::Critical),
            EvasionRule::new("mkswap", "Swap partition creation", RiskLevel::Critical),
            // New patterns (2.4)
            EvasionRule::new(
                "find . -delete",
                "Recursive file deletion via find",
                RiskLevel::High,
            ),
            EvasionRule::new(
                "find / -delete",
                "System-wide recursive deletion via find",
                RiskLevel::Critical,
            ),
            EvasionRule::new(
                "find / -exec",
                "Find with exec on system root",
                RiskLevel::Critical,
            ),
            EvasionRule::new(
                "-delete",
                "File deletion flag (find, etc.)",
                RiskLevel::High,
            ),
            EvasionRule::new(
                "base64 -d",
                "Base64 decoded command (possible obfuscation)",
                RiskLevel::High,
            ),
            EvasionRule::new(
                "echo.*|",
                "Piped echo to command (possible obfuscation)",
                RiskLevel::Medium,
            ),
            EvasionRule::new(
                r"eval \(\$",
                "Dynamically evaluated shell expression",
                RiskLevel::Critical,
            ),
            EvasionRule::new("eval ", "Shell eval invocation", RiskLevel::Critical),
            EvasionRule::new("exec ", "Shell exec invocation", RiskLevel::High),
            EvasionRule::new(
                "| bash",
                "Pipe to bash (remote code execution vector)",
                RiskLevel::Critical,
            ),
            EvasionRule::new(
                "| sh",
                "Pipe to sh (remote code execution vector)",
                RiskLevel::Critical,
            ),
            EvasionRule::new("| powershell", "Pipe to PowerShell", RiskLevel::Critical),
            EvasionRule::new(
                "chmod -R 777",
                "World-writable recursive permissions",
                RiskLevel::High,
            ),
            EvasionRule::new(
                "wget.*| bash",
                "Remote script piped to bash",
                RiskLevel::Critical,
            ),
            EvasionRule::new(
                "curl.*| bash",
                "Remote script piped to bash",
                RiskLevel::Critical,
            ),
            EvasionRule::new("sudo rm", "Privileged deletion", RiskLevel::High),
            EvasionRule::new(
                "ptrace",
                "Process tracing (possible sandbox escape)",
                RiskLevel::Critical,
            ),
        ]
    }

    /// Add a custom evasion rule.
    pub fn add_rule(&mut self, rule: EvasionRule) {
        self.evasion_rules.push(rule);
    }

    /// Find a rule by pattern substring.
    #[must_use]
    pub fn find_rule(&self, pattern_substr: &str) -> Option<&EvasionRule> {
        self.evasion_rules.iter().find(|r| {
            r.pattern
                .to_lowercase()
                .contains(&pattern_substr.to_lowercase())
        })
    }

    /// Enable a rule by pattern substring.
    pub fn enable_rule(&mut self, pattern_substr: &str) -> bool {
        if let Some(rule) = self.evasion_rules.iter_mut().find(|r| {
            r.pattern
                .to_lowercase()
                .contains(&pattern_substr.to_lowercase())
        }) {
            rule.enable();
            true
        } else {
            false
        }
    }

    /// Disable a rule by pattern substring.
    pub fn disable_rule(&mut self, pattern_substr: &str) -> bool {
        if let Some(rule) = self.evasion_rules.iter_mut().find(|r| {
            r.pattern
                .to_lowercase()
                .contains(&pattern_substr.to_lowercase())
        }) {
            rule.disable();
            true
        } else {
            false
        }
    }
}

impl ResourcePolicy for ShellBlocklist {
    fn check_shell(&self, command: &str) -> Result<()> {
        let lower = command.to_lowercase();

        // Legacy pattern check
        for pattern in &self.blocked_patterns {
            if lower.contains(&pattern.to_lowercase()) {
                return Err(crate::error::Error::AccessDenied {
                    resource: "shell".into(),
                    reason: format!("command blocked (matched pattern: '{pattern}')"),
                });
            }
        }

        // Evasion rule check
        for rule in &self.evasion_rules {
            if rule.matches(command) {
                return Err(crate::error::Error::AccessDenied {
                    resource: "shell".into(),
                    reason: format!(
                        "command blocked by evasion rule '{}' (risk: {})",
                        rule.description, rule.risk_level
                    ),
                });
            }
        }

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

// ── Restrict file access ──────────────────────────────────────────────────

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
        Ok(())
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

// ── 2.3: Access Policy Gateway ────────────────────────────────────────────

/// Decision for a specific capability or resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AccessPolicy {
    /// Access is always granted.
    Allow,
    /// Access is always denied.
    Deny,
    /// User should be prompted for a decision each time.
    Ask,
}

impl Default for AccessPolicy {
    fn default() -> Self {
        Self::Allow
    }
}

/// A per-session override for a specific capability.
#[derive(Debug, Clone)]
pub struct SessionOverride {
    /// The tool category this override applies to.
    pub category: crate::agent::tool::ToolCategory,
    /// The override decision.
    pub policy: AccessPolicy,
    /// Optional reason for the override.
    pub reason: String,
    /// Session ID this override is scoped to.
    pub session_id: String,
}

/// Evaluates access policies for tool categories.
///
/// Supports default policies, per-category overrides, and per-session overrides.
/// Integrates with the persistence crate for policy storage.
#[derive(Debug, Clone)]
pub struct AccessPolicyEvaluator {
    /// Default policy applied to all categories.
    default_policy: AccessPolicy,
    /// Per-category policies.
    category_policies: HashMap<String, AccessPolicy>,
    /// Per-session overrides (keyed by session_id + category).
    session_overrides: HashMap<String, AccessPolicy>,
}

impl AccessPolicyEvaluator {
    /// Create a new evaluator with the given default policy.
    #[must_use]
    pub fn new(default_policy: AccessPolicy) -> Self {
        Self {
            default_policy,
            category_policies: HashMap::new(),
            session_overrides: HashMap::new(),
        }
    }

    /// Set a policy for a specific tool category.
    pub fn set_category_policy(
        &mut self,
        category: &crate::agent::tool::ToolCategory,
        policy: AccessPolicy,
    ) {
        let key = Self::category_key(category);
        self.category_policies.insert(key, policy);
    }

    /// Get the policy for a specific tool category.
    #[must_use]
    pub fn get_category_policy(&self, category: &crate::agent::tool::ToolCategory) -> AccessPolicy {
        let key = Self::category_key(category);
        self.category_policies
            .get(&key)
            .copied()
            .unwrap_or(self.default_policy)
    }

    /// Add a per-session override.
    pub fn add_session_override(&mut self, override_: SessionOverride) {
        let key = format!("{}/{:?}", override_.session_id, override_.category);
        self.session_overrides.insert(key, override_.policy);
    }

    /// Remove a per-session override.
    pub fn remove_session_override(
        &mut self,
        session_id: &str,
        category: &crate::agent::tool::ToolCategory,
    ) {
        let key = format!("{session_id}/{category:?}");
        self.session_overrides.remove(&key);
    }

    /// Evaluate whether access is allowed for the given category and session.
    ///
    /// Returns `Some(AccessPolicy::Ask)` if the user should be prompted.
    #[must_use]
    pub fn evaluate(
        &self,
        category: &crate::agent::tool::ToolCategory,
        session_id: Option<&str>,
    ) -> AccessPolicy {
        // 1. Check per-session override first (highest priority)
        if let Some(session) = session_id {
            let key = format!("{session}/{category:?}");
            if let Some(policy) = self.session_overrides.get(&key) {
                return *policy;
            }
        }

        // 2. Check per-category policy
        let category_key = Self::category_key(category);
        if let Some(policy) = self.category_policies.get(&category_key) {
            return *policy;
        }

        // 3. Fall back to default
        self.default_policy
    }

    /// Save the current policy configuration to a JSON file.
    ///
    /// # Errors
    /// Returns an error if serialization or file I/O fails.
    pub fn save_to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        crate::persistence::save_json(path, self)
    }

    /// Load a policy configuration from a JSON file.
    ///
    /// # Errors
    /// Returns an error if file I/O or deserialization fails.
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        crate::persistence::load_json(path)
    }

    fn category_key(category: &crate::agent::tool::ToolCategory) -> String {
        format!("{category:?}")
    }
}

impl Default for AccessPolicyEvaluator {
    fn default() -> Self {
        Self {
            default_policy: AccessPolicy::Allow,
            category_policies: HashMap::new(),
            session_overrides: HashMap::new(),
        }
    }
}

impl serde::Serialize for AccessPolicyEvaluator {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("AccessPolicyEvaluator", 3)?;
        state.serialize_field("default_policy", &self.default_policy)?;

        let cats: Vec<(&str, &AccessPolicy)> = self
            .category_policies
            .iter()
            .map(|(k, v)| (k.as_str(), v))
            .collect();
        state.serialize_field("category_policies", &cats)?;

        let sessions: Vec<(&str, &AccessPolicy)> = self
            .session_overrides
            .iter()
            .map(|(k, v)| (k.as_str(), v))
            .collect();
        state.serialize_field("session_overrides", &sessions)?;
        state.end()
    }
}

impl<'de> serde::Deserialize<'de> for AccessPolicyEvaluator {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Helper {
            default_policy: AccessPolicy,
            category_policies: Vec<(String, AccessPolicy)>,
            session_overrides: Vec<(String, AccessPolicy)>,
        }

        let helper = Helper::deserialize(deserializer)?;
        let mut evaluator = Self::new(helper.default_policy);
        for (key, policy) in helper.category_policies {
            evaluator.category_policies.insert(key, policy);
        }
        for (key, policy) in helper.session_overrides {
            evaluator.session_overrides.insert(key, policy);
        }
        Ok(evaluator)
    }
}

// ── PolicyChain (unchanged) ────────────────────────────────────────────────

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

// ── 2.5: Auth-bypass Host Whitelist ───────────────────────────────────────

/// Result of checking a host against the whitelist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostCheckResult {
    /// The host is explicitly whitelisted.
    Allowed,
    /// The host is not whitelisted. A security warning was generated.
    NotWhitelisted { warning: String },
    /// The host address is invalid.
    InvalidAddress(String),
}

/// Configuration for hosts that are allowed to bypass authentication.
///
/// When an agent connects to a host that is not on the whitelist,
/// a security warning is generated. This is useful for local development
/// or trusted internal services.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AllowNoAuthHosts {
    /// Hostnames and IPs that are allowed without authentication.
    pub allowed_hosts: Vec<String>,
    /// If true, allow all private (RFC 1918) addresses without auth.
    pub allow_private_range: bool,
    /// If true, allow localhost (127.0.0.1, ::1) without auth.
    pub allow_localhost: bool,
}

impl Default for AllowNoAuthHosts {
    fn default() -> Self {
        Self {
            allowed_hosts: Vec::new(),
            allow_private_range: true,
            allow_localhost: true,
        }
    }
}

impl AllowNoAuthHosts {
    /// Check whether a host is allowed without authentication.
    ///
    /// Returns a [`HostCheckResult`] indicating whether access is allowed
    /// and any associated security warning.
    #[must_use]
    pub fn check_host(&self, host: &str) -> HostCheckResult {
        // Check if the host is explicitly in the whitelist
        if self.allowed_hosts.iter().any(|h| h == host) {
            return HostCheckResult::Allowed;
        }

        // Check if it's localhost
        if self.allow_localhost && is_localhost(host) {
            return HostCheckResult::Allowed;
        }

        // Check if it's a private IP
        if self.allow_private_range && is_private_ip(host) {
            return HostCheckResult::Allowed;
        }

        // Validate IP format if it looks like an IP
        if looks_like_ip(host) && !is_valid_ip(host) {
            return HostCheckResult::InvalidAddress(format!("'{host}' is not a valid IP address"));
        }

        HostCheckResult::NotWhitelisted {
            warning: format!(
                "Host '{host}' is not on the auth-bypass whitelist. Consider adding it to `allowed_hosts` if this is a trusted service."
            ),
        }
    }

    /// Add a host to the whitelist.
    pub fn add_host(&mut self, host: impl Into<String>) {
        let host = host.into();
        if !self.allowed_hosts.contains(&host) {
            self.allowed_hosts.push(host);
        }
    }

    /// Remove a host from the whitelist.
    pub fn remove_host(&mut self, host: &str) {
        self.allowed_hosts.retain(|h| h != host);
    }
}

/// Check if a host string represents a localhost address.
fn is_localhost(host: &str) -> bool {
    let host = host
        .trim_start_matches("http://")
        .trim_start_matches("https://");

    // Try exact match first (handles plain localhost and IPv6 ::1)
    if host == "localhost"
        || host == "127.0.0.1"
        || host == "::1"
        || host == "[::1]"
        || host == "0.0.0.0"
    {
        return true;
    }

    // For URLs with paths, split on / or ? to get just the host part
    // But don't split on : (to preserve IPv6 addresses)
    for separator in &['/', '?'] {
        if let Some(pos) = host.find(*separator) {
            let host_only = &host[..pos];
            return host_only == "localhost" || host_only == "127.0.0.1" || host_only == "0.0.0.0";
        }
    }

    false
}

/// Check if a host string is a private (RFC 1918) IP address.
fn is_private_ip(host: &str) -> bool {
    let host = host
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/');

    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v4) => v4.is_private(),
            std::net::IpAddr::V6(v6) => v6.is_loopback(),
        };
    }
    false
}

/// Check if a string looks like an IP address.
fn looks_like_ip(host: &str) -> bool {
    host.contains(|c: char| c.is_ascii_digit()) && (host.contains('.') || host.contains(':'))
}

/// Validate that a string is a valid IP address.
fn is_valid_ip(host: &str) -> bool {
    host.parse::<std::net::IpAddr>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    // ── Original tests ────────────────────────────────────────────────

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
        assert!(p.check_shell("rm file.txt").is_ok());
        assert!(p.check_shell("ls -la").is_ok());
    }

    #[test]
    fn test_path_restrict_allows_subdir() {
        let p = PathRestrict::new(vec!["/tmp/workdir"]);
        assert!(p.check_read("/tmp/workdir/file.txt".as_ref()).is_ok());
        assert!(p.check_read("/tmp/workdir/sub/file.txt".as_ref()).is_ok());
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
            Arc::new(AllowAll),
        ]);
        assert!(chain.check_shell("anything").is_err());
    }

    // ── 2.3: Access Policy Gateway tests ───────────────────────────────

    #[test]
    fn test_access_policy_default_allow() {
        let eval = AccessPolicyEvaluator::default();
        assert_eq!(
            eval.evaluate(&crate::agent::tool::ToolCategory::Shell, None),
            AccessPolicy::Allow
        );
    }

    #[test]
    fn test_access_policy_category_override() {
        let mut eval = AccessPolicyEvaluator::new(AccessPolicy::Allow);
        eval.set_category_policy(&crate::agent::tool::ToolCategory::Shell, AccessPolicy::Deny);
        assert_eq!(
            eval.evaluate(&crate::agent::tool::ToolCategory::Shell, None),
            AccessPolicy::Deny
        );
        assert_eq!(
            eval.evaluate(&crate::agent::tool::ToolCategory::FileRead, None),
            AccessPolicy::Allow
        );
    }

    #[test]
    fn test_access_policy_session_override() {
        let mut eval = AccessPolicyEvaluator::new(AccessPolicy::Deny);
        eval.add_session_override(SessionOverride {
            category: crate::agent::tool::ToolCategory::Shell,
            policy: AccessPolicy::Allow,
            reason: "Debug session".into(),
            session_id: "session-1".into(),
        });
        assert_eq!(
            eval.evaluate(&crate::agent::tool::ToolCategory::Shell, Some("session-1")),
            AccessPolicy::Allow
        );
        // Different session should not be affected
        assert_eq!(
            eval.evaluate(&crate::agent::tool::ToolCategory::Shell, Some("session-2")),
            AccessPolicy::Deny
        );
    }

    #[test]
    fn test_access_policy_serde_roundtrip() {
        let mut eval = AccessPolicyEvaluator::new(AccessPolicy::Ask);
        eval.set_category_policy(
            &crate::agent::tool::ToolCategory::Network,
            AccessPolicy::Deny,
        );
        eval.add_session_override(SessionOverride {
            category: crate::agent::tool::ToolCategory::Shell,
            policy: AccessPolicy::Allow,
            reason: "test".into(),
            session_id: "s1".into(),
        });

        let json = serde_json::to_string(&eval).unwrap();
        let loaded: AccessPolicyEvaluator = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.default_policy, AccessPolicy::Ask);
        assert_eq!(
            loaded.get_category_policy(&crate::agent::tool::ToolCategory::Network),
            AccessPolicy::Deny
        );
    }

    // ── 2.4: Shell Evasion Rule tests ──────────────────────────────────

    #[test]
    fn test_evasion_rule_matches() {
        let rule = EvasionRule::new("find / -delete", "Test rule", RiskLevel::Critical);
        assert!(rule.matches("find / -delete -name '*.log'"));
        assert!(!rule.matches("find . -name '*.txt'"));
    }

    #[test]
    fn test_evasion_rule_disable() {
        let mut rule = EvasionRule::new("base64 -d", "Test rule", RiskLevel::High);
        assert!(rule.matches("echo 'aGVsbG8=' | base64 -d"));
        rule.disable();
        assert!(!rule.matches("echo 'aGVsbG8=' | base64 -d"));
    }

    #[test]
    fn test_shell_blocklist_with_evasion_rules() {
        let p = ShellBlocklist::default_blocked();

        // Should block base64-obfuscated commands
        assert!(p.check_shell("echo 'Y2xlYXI=' | base64 -d | bash").is_err());

        // Should block find -delete
        assert!(p.check_shell("find /tmp -delete").is_err());
    }

    #[test]
    fn test_disable_specific_rule() {
        let mut p = ShellBlocklist::default_blocked();
        assert!(p.disable_rule("base64 -d"));
        // The rule should no longer trigger
        assert!(p.check_shell("echo 'aGVsbG8=' | base64 -d").is_ok());
    }

    #[test]
    fn test_find_rule_by_pattern() {
        let p = ShellBlocklist::default_blocked();
        let rule = p.find_rule("find / -delete");
        assert!(rule.is_some());
        assert_eq!(rule.unwrap().risk_level, RiskLevel::Critical);
    }

    // ── 2.5: Auth-bypass Host Whitelist tests ───────────────────────────

    #[test]
    fn test_allow_no_auth_localhost() {
        let config = AllowNoAuthHosts::default();
        assert_eq!(config.check_host("localhost"), HostCheckResult::Allowed);
        assert_eq!(config.check_host("127.0.0.1"), HostCheckResult::Allowed);
        assert_eq!(
            config.check_host("http://localhost"),
            HostCheckResult::Allowed
        );
    }

    #[test]
    fn test_allow_no_auth_private_ip() {
        let config = AllowNoAuthHosts::default();
        assert_eq!(config.check_host("192.168.1.1"), HostCheckResult::Allowed);
        assert_eq!(config.check_host("10.0.0.1"), HostCheckResult::Allowed);
    }

    #[test]
    fn test_allow_no_auth_explicit_host() {
        let mut config = AllowNoAuthHosts::default();
        config.add_host("api.internal.corp.com");
        assert_eq!(
            config.check_host("api.internal.corp.com"),
            HostCheckResult::Allowed
        );
    }

    #[test]
    fn test_allow_no_auth_not_whitelisted() {
        let config = AllowNoAuthHosts {
            allow_private_range: false,
            allow_localhost: false,
            allowed_hosts: vec![],
        };
        let result = config.check_host("evil.com");
        assert_eq!(result, HostCheckResult::NotWhitelisted {
            warning: "Host 'evil.com' is not on the auth-bypass whitelist. Consider adding it to `allowed_hosts` if this is a trusted service.".into()
        });
    }

    #[test]
    fn test_allow_no_auth_invalid_ip() {
        let config = AllowNoAuthHosts::default();
        let result = config.check_host("999.999.999.999");
        assert!(matches!(result, HostCheckResult::InvalidAddress(_)));
    }

    #[test]
    fn test_allow_no_auth_remove_host() {
        let mut config = AllowNoAuthHosts::default();
        config.add_host("internal.dev");
        assert_eq!(config.check_host("internal.dev"), HostCheckResult::Allowed);
        config.remove_host("internal.dev");
        let result = config.check_host("internal.dev");
        assert_ne!(result, HostCheckResult::Allowed);
    }

    #[test]
    fn test_is_localhost_variants() {
        assert!(is_localhost("localhost"));
        assert!(is_localhost("127.0.0.1"));
        assert!(is_localhost("::1"));
        assert!(is_localhost("[::1]"));
        assert!(is_localhost("http://localhost/api"));
        assert!(!is_localhost("example.com"));
    }

    #[test]
    fn test_is_valid_ip() {
        assert!(is_valid_ip("192.168.1.1"));
        assert!(is_valid_ip("::1"));
        assert!(!is_valid_ip("999.999.999.999"));
    }
}
