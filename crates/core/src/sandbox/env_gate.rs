//! **EnvGate** — environment-variable switch for gating operations.
//!
//! Used by [`GovernedTool`] to allow or deny operations based on an env var.
//! The gate is checked before policy and sandbox routing.
//!
//! # Example
//!
//! ```ignore
//! use praxis_core::sandbox::EnvGate;
//!
//! let gate = EnvGate::unsandboxed_recall();
//! assert!(gate.check().is_err()); // unless PRAXIS_ALLOW_UNSANDBOXED_RECALL=1
//! ```

use crate::agent::tool::ToolError;

/// An environment-variable-based gate that controls access to an operation.
///
/// When `enabled` is `true`, the gate checks the environment variable on every
/// [`check`](EnvGate::check) call.  When `enabled` is `false` it always passes.
#[derive(Debug, Clone)]
pub struct EnvGate {
    /// Name of the environment variable to check (e.g. `"PRAXIS_ALLOW_UNSANDBOXED_RECALL"`).
    var_name: String,
    /// Whether this gate is active.  `false` = always pass.
    enabled: bool,
}

impl EnvGate {
    /// Create a new env gate that checks `var_name`.
    ///
    /// The gate is enabled by default.  Call [`disabled`](EnvGate::disabled) to
    /// create a permanently-passing gate.
    #[must_use]
    pub fn new(var_name: &str) -> Self {
        Self {
            var_name: var_name.to_string(),
            enabled: true,
        }
    }

    /// Disable this gate — it will always pass.
    #[must_use]
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Returns the env-var name.
    #[must_use]
    pub fn var_name(&self) -> &str {
        &self.var_name
    }

    /// Check whether the gate passes.
    ///
    /// Returns `Ok(())` when:
    /// * the gate is disabled, or
    /// * the environment variable is set to `"1"` or `"true"` (case-insensitive).
    ///
    /// Returns a [`ToolError::AccessDenied`] otherwise.
    ///
    /// # Errors
    ///
    /// Returns `ToolError::AccessDenied` when the gate is enabled and the env
    /// var is not set (or set to an unrecognised value).
    pub fn check(&self, tool: &str) -> Result<(), ToolError> {
        if !self.enabled {
            return Ok(());
        }

        match std::env::var(&self.var_name) {
            Ok(val) if val == "1" || val.eq_ignore_ascii_case("true") => Ok(()),
            _ => Err(ToolError::AccessDenied {
                tool: tool.to_string(),
                reason: format!("env var '{}' is not set to '1' or 'true'", self.var_name),
            }),
        }
    }

    // ── Pre-built gates ──────────────────────────────────────────────

    /// Gate for `PRAXIS_ALLOW_UNSANDBOXED_RECALL` — allows unsandboxed
    /// recall history access.
    #[must_use]
    pub fn unsandboxed_recall() -> Self {
        Self::new("PRAXIS_ALLOW_UNSANDBOXED_RECALL")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Restores an env var to its previous state on drop.
    struct EnvGuard {
        key: String,
        old_value: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &str, val: &str) -> Self {
            let old_value = std::env::var(key).ok();
            // SAFETY: set_var is not thread-safe, but tests are sequential
            // within a single test binary and use unique keys.
            unsafe { std::env::set_var(key, val) };
            Self {
                key: key.to_string(),
                old_value,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: same reasoning as above — unique keys, no concurrent access.
            match &self.old_value {
                Some(v) => unsafe { std::env::set_var(&self.key, v) },
                None => unsafe { std::env::remove_var(&self.key) },
            }
        }
    }

    #[test]
    fn test_disabled_always_passes() {
        let gate = EnvGate::new("SOME_VAR").disabled();
        assert!(gate.check("test_tool").is_ok());
    }

    #[test]
    fn test_env_not_set_denies() {
        let gate = EnvGate::new("PRAXIS_TEST_VAR_THAT_DOES_NOT_EXIST");
        let err = gate.check("test_tool").unwrap_err();
        assert!(matches!(err, ToolError::AccessDenied { .. }));
    }

    #[test]
    fn test_env_set_to_1_allows() {
        let _guard = EnvGuard::set("PRAXIS_TEST_GATE", "1");
        let gate = EnvGate::new("PRAXIS_TEST_GATE");
        assert!(gate.check("test_tool").is_ok());
    }

    #[test]
    fn test_env_set_to_true_allows() {
        let _guard = EnvGuard::set("PRAXIS_TEST_GATE", "true");
        let gate = EnvGate::new("PRAXIS_TEST_GATE");
        assert!(gate.check("test_tool").is_ok());
    }

    #[test]
    fn test_unsandboxed_recall_prebuilt() {
        let gate = EnvGate::unsandboxed_recall();
        // Var is not set
        assert!(gate.check("test").is_err());
    }
}
