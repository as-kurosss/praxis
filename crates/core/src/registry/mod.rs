//! # Registry — persistent configuration for agents, providers, and sessions.
//!
//! This module provides serializable types for managing agents and LLM
//! providers without recompiling Rust code.  Everything is stored in
//! a JSON file on disk (the "registry").
//!
//! ## Key types
//!
//! * [`ProviderConfig`] — an LLM provider configuration (API key, URL, model, …)
//! * [`AgentDefinition`] — a named agent definition referencing a provider + tools
//! * [`AgentRegistry`] — persistent store for providers and agent definitions
//! * [`Session`] — a conversation history with an agent
//! * [`SessionStore`] — persistent store for sessions

mod provider;
pub use provider::*;

mod agent_def;
pub use agent_def::*;

mod store;
pub use store::*;

mod session;
pub use session::*;

/// Return an ISO-8601 UTC timestamp string (no external crate needed).
pub fn timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();

    // Days since epoch → (year, month, day)
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    let (y, m, d) = days_to_date(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m, d, hours, minutes, seconds
    )
}

fn days_to_date(days: u64) -> (u64, u64, u64) {
    let mut y = 1970i64;
    let mut d = days as i64;
    loop {
        let yd = if is_leap(y) { 366 } else { 365 };
        if d < yd {
            break;
        }
        d -= yd;
        y += 1;
    }
    let leap = is_leap(y);
    let months: [i64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 1u64;
    for &md in &months {
        if d < md {
            break;
        }
        d -= md;
        m += 1;
    }
    (y as u64, m, (d + 1) as u64)
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}
