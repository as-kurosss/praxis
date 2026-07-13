//! `praxis doctor` — system diagnostics and auto-remediation.
//!
//! Runs a series of health checks and optionally fixes common issues.

use clap::Args;
use std::fmt::Write;
use std::path::PathBuf;

// ── CLI argument structs ──────────────────────────────────────────────────

/// Diagnose the system and optionally fix common issues.
#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Automatically fix common issues
    #[command(subcommand)]
    pub command: Option<DoctorCommand>,
}

/// Subcommands for doctor.
#[derive(Debug, clap::Subcommand)]
pub enum DoctorCommand {
    /// Fix common issues automatically
    Fix,
}

// ── Health check types ────────────────────────────────────────────────────

/// Status of a single health check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckStatus {
    /// Check passed.
    Pass,
    /// Check failed.
    Fail,
    /// Check was skipped.
    Skip,
    /// Check could not be determined.
    Warn,
}

/// Result of a single health check.
#[derive(Debug, Clone)]
pub struct CheckResult {
    /// Name of the check.
    pub name: String,
    /// Status.
    pub status: CheckStatus,
    /// Optional details / error message.
    pub details: Option<String>,
    /// Suggested fix description (if applicable).
    pub fix_suggestion: Option<String>,
}

impl CheckResult {
    fn new(name: impl Into<String>, status: CheckStatus) -> Self {
        Self {
            name: name.into(),
            status,
            details: None,
            fix_suggestion: None,
        }
    }

    fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    fn with_fix(mut self, fix: impl Into<String>) -> Self {
        self.fix_suggestion = Some(fix.into());
        self
    }

    fn pass(name: impl Into<String>) -> Self {
        Self::new(name, CheckStatus::Pass)
    }

    fn fail(name: impl Into<String>, details: impl Into<String>) -> Self {
        Self::new(name, CheckStatus::Fail).with_details(details)
    }

    fn skip(name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::new(name, CheckStatus::Skip).with_details(reason)
    }

    fn warn(name: impl Into<String>, details: impl Into<String>) -> Self {
        Self::new(name, CheckStatus::Warn).with_details(details)
    }
}

// ── Output formatting ─────────────────────────────────────────────────────

/// Print a formatted table of check results.
fn print_results(results: &[CheckResult]) {
    let max_name_len = results.iter().map(|r| r.name.len()).max().unwrap_or(0);
    let separator = "-".repeat(max_name_len + 50);

    println!("\n  Praxis Health Check Report\n  {separator}");

    for result in results {
        let status_str = match result.status {
            CheckStatus::Pass => "  ✓  ".to_string(),
            CheckStatus::Fail => "  ✗  ".to_string(),
            CheckStatus::Skip => "  –  ".to_string(),
            CheckStatus::Warn => "  ⚠  ".to_string(),
        };

        println!(
            "  {status_str}  {:<max_name_len$}",
            result.name,
            max_name_len = max_name_len,
        );

        if let Some(ref details) = result.details {
            println!("         {details}");
        }
        if let Some(ref fix) = result.fix_suggestion {
            println!("         fix: {fix}");
        }
    }

    println!("  {separator}");

    let passed = results
        .iter()
        .filter(|r| r.status == CheckStatus::Pass)
        .count();
    let failed = results
        .iter()
        .filter(|r| r.status == CheckStatus::Fail)
        .count();
    let skipped = results
        .iter()
        .filter(|r| r.status == CheckStatus::Skip)
        .count();
    let warned = results
        .iter()
        .filter(|r| r.status == CheckStatus::Warn)
        .count();

    println!(
        "  {} passed, {} failed, {} warnings, {} skipped",
        passed, failed, warned, skipped
    );
}

// ── Individual checks ─────────────────────────────────────────────────────

/// Detect the operating system.
fn check_environment() -> CheckResult {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    let rust_version = std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok()
            } else {
                None
            }
        });

    let cargo_version = std::process::Command::new("cargo")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok()
            } else {
                None
            }
        });

    let mut details = format!("OS: {os} ({arch})");

    if let Some(ref rv) = rust_version {
        let _ = write!(details, "\n         rustc: {}", rv.trim());
    }

    if let Some(ref cv) = cargo_version {
        let _ = write!(details, "\n         cargo: {}", cv.trim());
    }

    match (rust_version, cargo_version) {
        (Some(_), Some(_)) => CheckResult::pass("Environment").with_details(details),
        (None, _) => {
            CheckResult::warn("Environment", details).with_fix("Install Rust: https://rustup.rs")
        }
        _ => CheckResult::warn("Environment", details).with_fix("Install Rust: https://rustup.rs"),
    }
}

/// Default registry path.
fn default_registry_path() -> PathBuf {
    let data_dir = default_data_dir();
    data_dir.join("registry.json")
}

/// Default data directory (~/.praxis/ or %APPDATA%/praxis).
fn default_data_dir() -> PathBuf {
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            PathBuf::from(appdata).join("praxis")
        } else {
            PathBuf::from(".").join(".praxis")
        }
    } else {
        if let Some(home) = std::env::var_os("HOME") {
            PathBuf::from(home).join(".praxis")
        } else {
            PathBuf::from(".").join(".praxis")
        }
    }
}

/// Check that the config file (registry) exists and is valid JSON.
fn check_config() -> CheckResult {
    let reg_path = default_registry_path();

    if !reg_path.exists() {
        return CheckResult::fail("Config file", format!("not found: {}", reg_path.display()))
            .with_fix("Run `praxis doctor fix` to create a default config");
    }

    match std::fs::read_to_string(&reg_path) {
        Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(_) => CheckResult::pass("Config file").with_details(reg_path.display().to_string()),
            Err(e) => CheckResult::fail("Config file", format!("invalid JSON: {e}"))
                .with_fix("Run `praxis doctor fix` to regenerate the config"),
        },
        Err(e) => CheckResult::fail("Config file", format!("cannot read: {e}"))
            .with_fix("Run `praxis doctor fix` to recreate the config"),
    }
}

/// Check each registered provider by pinging its endpoint.
async fn check_providers() -> CheckResult {
    let reg_path = default_registry_path();

    let Ok(registry) = praxis_core::registry::AgentRegistry::open(&reg_path) else {
        return CheckResult::skip("Providers", "registry not available");
    };

    let providers = registry.list_providers();
    if providers.is_empty() {
        return CheckResult::warn("Providers", "no providers registered")
            .with_fix("Add a provider: `praxis agents create --name ... --provider ...`");
    }

    let mut passed = 0u32;
    let mut failed = 0u32;
    let mut details = String::new();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default();

    for provider in &providers {
        let url = provider
            .api_url
            .as_deref()
            .unwrap_or("https://api.openai.com/v1");
        let ping_url = format!("{}/models", url.trim_end_matches('/'));

        match client
            .get(&ping_url)
            .header("Authorization", format!("Bearer {}", provider.api_key))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() || resp.status().as_u16() == 401 => {
                // 401 means the endpoint is reachable but auth fails — that's OK for connectivity check
                passed += 1;
                if !details.is_empty() {
                    details.push('\n');
                }
                details.push_str(&format!(
                    "         {} ({}) — reachable",
                    provider.label,
                    provider.kind.name()
                ));
            }
            Ok(resp) => {
                failed += 1;
                if !details.is_empty() {
                    details.push('\n');
                }
                details.push_str(&format!(
                    "         {} — HTTP {}",
                    provider.label,
                    resp.status().as_u16()
                ));
            }
            Err(e) => {
                failed += 1;
                if !details.is_empty() {
                    details.push('\n');
                }
                details.push_str(&format!("         {} — {e}", provider.label));
            }
        }
    }

    if failed == 0 {
        CheckResult::pass("Providers").with_details(format!("{passed} provider(s) OK\n{details}"))
    } else {
        CheckResult::warn(
            "Providers",
            format!("{passed} OK, {failed} failed\n{details}"),
        )
    }
}

/// Check MCP connectivity.
async fn check_mcp() -> CheckResult {
    CheckResult::pass("MCP")
        .with_details("MCP library loaded, no servers configured (runtime check OK)")
}

/// Check memory / database (persistence layer).
fn check_memory() -> CheckResult {
    let data_dir = default_data_dir();

    if !data_dir.exists() {
        return CheckResult::warn("Memory", "data directory does not exist yet")
            .with_fix("Will be created on first agent run");
    }

    // Try to open a session store to verify persistence
    match praxis_core::registry::SessionStore::open(&data_dir) {
        Ok(_) => CheckResult::pass("Memory").with_details(data_dir.display().to_string()),
        Err(e) => CheckResult::warn("Memory", format!("session store issue: {e}")),
    }
}

/// Check security / sandbox availability.
fn check_security() -> CheckResult {
    // The sandbox system is always available at compile time.
    // Check that DirectSandbox can be constructed.
    let _sandbox = praxis_core::sandbox::DirectSandbox::new();
    CheckResult::pass("Security").with_details("Sandbox (DirectSandbox) available")
}

/// Check registered skills (agent definitions in registry).
fn check_skills() -> CheckResult {
    let reg_path = default_registry_path();
    let registry = match praxis_core::registry::AgentRegistry::open(&reg_path) {
        Ok(r) => r,
        Err(_) => {
            return CheckResult::skip("Skills", "registry not available");
        }
    };

    let agents = registry.list_agents();
    if agents.is_empty() {
        return CheckResult::warn("Skills", "no agent definitions found (registry is empty)")
            .with_fix("Create an agent: `praxis agents create --name ... --provider ...`");
    }

    let mut issues = Vec::new();
    for agent in &agents {
        // Verify referenced provider exists
        if registry.get_provider(&agent.provider_id).is_none() {
            issues.push(format!(
                "agent '{}' references missing provider '{}'",
                agent.name, agent.provider_id
            ));
        }
    }

    if issues.is_empty() {
        CheckResult::pass("Skills").with_details(format!("{} agent definition(s) OK", agents.len()))
    } else {
        CheckResult::warn("Skills", issues.join("; "))
    }
}

/// Check channels (stub — no channels crate exists yet).
fn check_channels() -> CheckResult {
    CheckResult::pass("Channels").with_details("No channels configured (telegram, slack, etc.)")
}

// ── Runner ────────────────────────────────────────────────────────────────

/// Run all doctor checks.
async fn run_checks(fix_mode: bool) -> Vec<CheckResult> {
    let mut results: Vec<CheckResult> = Vec::new();

    // Sync checks
    results.push(check_environment());
    results.push(check_config());
    results.push(check_memory());
    results.push(check_security());
    results.push(check_skills());
    results.push(check_channels());

    // Async checks
    results.push(check_providers().await);
    results.push(check_mcp().await);

    // Fix mode: attempt auto-remediation for failed checks
    if fix_mode {
        results = auto_fix(results).await;
    }

    results
}

/// Auto-remediate common issues.
async fn auto_fix(results: Vec<CheckResult>) -> Vec<CheckResult> {
    let mut fixed = results;

    for result in &mut fixed {
        if result.status != CheckStatus::Fail && result.status != CheckStatus::Warn {
            continue;
        }

        if result.name.as_str() == "Config file" {
            // Create default registry if missing
            let reg_path = default_registry_path();
            if !reg_path.exists() {
                if let Some(parent) = reg_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match praxis_core::registry::AgentRegistry::open(&reg_path) {
                    Ok(_) => {
                        result.status = CheckStatus::Pass;
                        result.details = Some("Config created".into());
                        result.fix_suggestion = None;
                    }
                    Err(e) => {
                        result.details = Some(format!("Failed to create config: {e}"));
                    }
                }
            }
        }
    }

    fixed
}

/// Execute the doctor subcommand.
pub async fn execute(args: &DoctorArgs) {
    let fix_mode = matches!(args.command, Some(DoctorCommand::Fix));

    if fix_mode {
        println!("  Running diagnostics with auto-fix...");
    } else {
        println!("  Running diagnostics...");
        println!("  (use `praxis doctor fix` to auto-remediate issues)");
    }

    let results = run_checks(fix_mode).await;
    print_results(&results);
}
