//! `praxis skills` — skill management subcommands.
//!
//! Provides `praxis skills info <name>` to display skill details.

use clap::{Args, Subcommand};
use praxis_core::registry::AgentRegistry;

// ── CLI argument structs ──────────────────────────────────────────────────

/// Manage skills (agent definitions).
#[derive(Debug, Args)]
pub struct SkillsArgs {
    #[command(subcommand)]
    pub command: SkillsCommand,
}

/// Subcommands for skills.
#[derive(Debug, Subcommand)]
pub enum SkillsCommand {
    /// Show detailed information about a skill (agent definition).
    Info(SkillsInfoArgs),
}

/// Arguments for `skills info`.
#[derive(Debug, Args)]
pub struct SkillsInfoArgs {
    /// Name or ID of the skill (agent definition) to inspect.
    pub name: String,
}

// ── Default paths ────────────────────────────────────────────────────────

/// Default data directory.
fn default_data_dir() -> std::path::PathBuf {
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            std::path::PathBuf::from(appdata).join("praxis")
        } else {
            std::path::PathBuf::from(".").join(".praxis")
        }
    } else {
        if let Some(home) = std::env::var_os("HOME") {
            std::path::PathBuf::from(home).join(".praxis")
        } else {
            std::path::PathBuf::from(".").join(".praxis")
        }
    }
}

/// Default registry path.
fn default_registry_path() -> std::path::PathBuf {
    default_data_dir().join("registry.json")
}

// ── Execution ─────────────────────────────────────────────────────────────

/// Execute the `skills info` subcommand.
pub fn execute_info(args: &SkillsInfoArgs) {
    let reg_path = default_registry_path();
    let registry = match AgentRegistry::open(&reg_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: cannot open registry at {}: {e}", reg_path.display());
            return;
        }
    };

    // Search by name or ID
    let name_or_id = &args.name;
    let agent = registry.get_agent(name_or_id).or_else(|| {
        registry
            .list_agents()
            .into_iter()
            .find(|a| a.name == *name_or_id || a.id == *name_or_id)
    });

    match agent {
        Some(def) => {
            println!();
            println!("  Skill: {}", def.name);
            println!("  {}", "─".repeat(60));
            println!("  ID:             {}", def.id);
            println!("  Name:           {}", def.name);
            println!(
                "  Description:    {}",
                def.description.as_deref().unwrap_or("(none)")
            );
            println!("  Provider ID:    {}", def.provider_id);
            println!("  System Prompt:  {}", def.system_prompt);
            println!(
                "  Temperature:    {}",
                def.temperature
                    .map_or("(default)".into(), |t| t.to_string())
            );
            println!(
                "  Max Tokens:     {}",
                def.max_tokens.map_or("(default)".into(), |t| t.to_string())
            );
            println!(
                "  Scroll Strategy: {}",
                match def.scroll_strategy {
                    praxis_core::registry::ScrollConfig::Truncate { max_messages } =>
                        format!("Truncate ({max_messages} messages)"),
                    praxis_core::registry::ScrollConfig::SlidingWindow { window_size } =>
                        format!("Sliding Window ({window_size} messages)"),
                    praxis_core::registry::ScrollConfig::NoOp => "NoOp (keep all)".into(),
                }
            );

            // Show tools
            if def.tools.is_empty() {
                println!("  Tools:          (none)");
            } else {
                println!("  Tools:");
                for tool in &def.tools {
                    match tool {
                        praxis_core::registry::ToolBinding::Builtin { name, enabled } => {
                            println!(
                                "    - {name} {}",
                                if *enabled { "(enabled)" } else { "(disabled)" }
                            );
                        }
                        praxis_core::registry::ToolBinding::Custom {
                            name,
                            description,
                            enabled,
                            ..
                        } => {
                            println!(
                                "    - {name}: {description} {}",
                                if *enabled { "(enabled)" } else { "(disabled)" }
                            );
                        }
                        praxis_core::registry::ToolBinding::Mcp {
                            server_name,
                            tools,
                            enabled,
                            ..
                        } => {
                            println!(
                                "    - MCP:{server_name} {}",
                                if *enabled { "(enabled)" } else { "(disabled)" }
                            );
                            if !tools.is_empty() {
                                println!("      Tools: {}", tools.join(", "));
                            }
                        }
                    }
                }
            }

            println!("  Channel Bindings: (not yet implemented)");

            println!("  Created:        {}", def.created_at);
            println!("  Updated:        {}", def.updated_at);

            // Show provider details if available
            if let Some(provider) = registry.get_provider(&def.provider_id) {
                println!();
                println!("  Provider: {}", provider.label);
                println!("    Kind:    {}", provider.kind.name());
                println!(
                    "    API URL: {}",
                    provider.api_url.as_deref().unwrap_or("(default)")
                );
                println!("    Model:   {}", provider.model);
            }
        }
        None => {
            eprintln!("Error: no skill found matching '{}'", name_or_id);
            let all: Vec<_> = registry
                .list_agents()
                .into_iter()
                .map(|a| format!("  - {} ({})", a.name, a.id))
                .collect();
            if !all.is_empty() {
                eprintln!("Available skills:");
                for line in all {
                    eprintln!("{line}");
                }
            } else {
                eprintln!("No skills (agent definitions) registered yet.");
                eprintln!("Tip: create one with `praxis agents create --name ... --provider ...`");
            }
        }
    }
}

/// Execute the skills subcommand.
pub fn execute(args: &SkillsArgs) {
    match &args.command {
        SkillsCommand::Info(info_args) => execute_info(info_args),
    }
}
