//! `praxis agents` — agent management subcommands.
//!
//! Provides `praxis agents create` to scaffold new agent definitions.

use clap::{Args, Subcommand, ValueEnum};
use praxis_core::registry::{AgentDefinition, AgentRegistry, ProviderConfig, ProviderKind};

// ── CLI argument structs ──────────────────────────────────────────────────

/// Manage agents.
#[derive(Debug, Args)]
pub struct AgentsArgs {
    #[command(subcommand)]
    pub command: AgentsCommand,
}

/// Subcommands for agents.
#[derive(Debug, Subcommand)]
pub enum AgentsCommand {
    /// Create a new agent definition.
    Create(AgentsCreateArgs),
}

/// Arguments for `agents create`.
#[derive(Debug, Args)]
pub struct AgentsCreateArgs {
    /// Name for the new agent.
    #[arg(long)]
    pub name: String,

    /// Provider ID to associate with this agent
    /// (e.g. \"openai-gpt4\" or a provider already registered).
    #[arg(long)]
    pub provider: String,

    /// Template to use for the agent configuration.
    #[arg(long, default_value = "default")]
    pub template: TemplateKind,

    /// System prompt override.
    #[arg(long)]
    pub system_prompt: Option<String>,

    /// Description for the agent.
    #[arg(long)]
    pub description: Option<String>,

    /// Directory to initialize the workspace in.
    #[arg(long)]
    pub workspace: Option<String>,
}

/// Available agent templates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TemplateKind {
    /// General-purpose assistant with default tools.
    Default,
    /// Lightweight local-model agent (no external API calls).
    Local,
    /// QA/testing agent with strict controls.
    Qa,
}

impl TemplateKind {
    fn system_prompt(self) -> &'static str {
        match self {
            Self::Default => {
                "You are a helpful AI assistant with access to various tools. \
                 Respond accurately and concisely. Use your tools when appropriate \
                 to help the user accomplish their tasks."
            }
            Self::Local => {
                "You are a local AI assistant running on the user's machine. \
                 You have limited capabilities and no internet access. \
                 Help with local file operations, calculations, and simple queries."
            }
            Self::Qa => {
                "You are a QA testing agent. You run automated tests, \
                 verify outputs, and report results. Be precise and thorough. \
                 Do NOT modify production data or configurations without explicit \
                 approval."
            }
        }
    }

    fn tools(self) -> Vec<&'static str> {
        match self {
            Self::Default => vec!["shell", "calculator", "time", "web_search"],
            Self::Local | Self::Qa => vec!["shell", "calculator", "time"],
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Default => "General-purpose agent with full tool access",
            Self::Local => "Lightweight agent for local-only operations",
            Self::Qa => "QA/testing agent with restricted access",
        }
    }
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

/// Execute the `agents create` subcommand.
pub fn execute_create(args: &AgentsCreateArgs) {
    let reg_path = default_registry_path();

    // Ensure parent dir exists
    if let Some(parent) = reg_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("Error: cannot create registry directory: {e}");
            return;
        }
    }

    let registry = match AgentRegistry::open(&reg_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: cannot open registry: {e}");
            return;
        }
    };

    // Check if provider exists, or try to create one
    let provider_id = resolve_provider_id(&registry, &args.provider, &reg_path);
    let provider_id = match provider_id {
        Some(id) => id,
        None => return,
    };

    let template = args.template;

    // Create a unique ID from the name
    let agent_id = args
        .name
        .to_lowercase()
        .replace(' ', "_")
        .replace(|c: char| !c.is_alphanumeric() && c != '_' && c != '-', "");

    let system_prompt = args
        .system_prompt
        .clone()
        .unwrap_or_else(|| template.system_prompt().to_string());

    let description = args
        .description
        .clone()
        .or_else(|| Some(template.description().to_string()))
        .unwrap_or_default();

    let mut def = AgentDefinition::new(&agent_id, &args.name, &provider_id, &system_prompt);
    def.description = if description.is_empty() {
        None
    } else {
        Some(description.clone())
    };

    // Add template tools
    for tool_name in template.tools() {
        def = def.with_tool(tool_name);
    }

    if let Err(e) = registry.upsert_agent(def) {
        eprintln!("Error: failed to create agent: {e}");
        return;
    }

    println!();
    println!("  ✓ Agent '{}' created successfully.", args.name);
    println!("    ID:             {agent_id}");
    println!("    Provider:       {provider_id}");
    println!("    Template:       {:?}", args.template);
    println!(
        "    Description:    {}",
        if description.is_empty() {
            "(none)"
        } else {
            &description
        },
    );
    println!("    Tools:          {}", template.tools().join(", "));
    println!("  Stored in: {}", reg_path.display());

    // Initialize workspace if requested
    if let Some(ref ws_path) = args.workspace {
        initialize_workspace(ws_path, &agent_id, &args.name);
    }
}

/// Resolve a provider ID from the registry or auto-create one.
/// Returns `None` if the provider cannot be found or created (error already printed).
fn resolve_provider_id(
    registry: &AgentRegistry,
    provider_arg: &str,
    reg_path: &std::path::Path,
) -> Option<String> {
    if registry.get_provider(provider_arg).is_some() {
        return Some(provider_arg.to_string());
    }

    // Try to auto-create a provider config from the provider string
    if let Some(config) = auto_create_provider(provider_arg) {
        let id = config.id.clone();
        if let Err(e) = registry.upsert_provider(config) {
            eprintln!(
                "Error: provider '{}' not found and auto-creation failed: {e}",
                provider_arg
            );
            return None;
        }
        println!("  Created provider '{}' automatically.", id);
        return Some(id);
    }

    eprintln!(
        "Error: provider '{}' is not registered. \
         Register a provider first or use a known provider ID.",
        provider_arg
    );
    println!();
    println!("  To register a provider manually, add it to the registry at:");
    println!("    {}", reg_path.display());
    println!();
    println!("  Known provider patterns:");
    println!("    openai:gpt-4o      — OpenAI with GPT-4o");
    println!("    anthropic:sonnet   — Anthropic Claude Sonnet");
    println!("    ollama:llama3      — Ollama local model");
    println!("    gemini:gemini-pro  — Google Gemini Pro");
    None
}

/// Try to auto-create a provider config from a provider identifier string.
///
/// Supported patterns:
/// - `openai:model` or `openai` → OpenAI API
/// - `anthropic:model` or `anthropic` → Anthropic API
/// - `gemini:model` or `gemini` → Google Gemini API
/// - `ollama:model` or `ollama` → Ollama local
fn auto_create_provider(provider_str: &str) -> Option<ProviderConfig> {
    let (kind_str, model) = if let Some((kind, model)) = provider_str.split_once(':') {
        (kind.to_lowercase(), Some(model.to_string()))
    } else {
        (provider_str.to_lowercase(), None)
    };

    let (kind, default_model, url) = match kind_str.as_str() {
        "openai" => (ProviderKind::Openai, "gpt-4o", None::<String>),
        "anthropic" => (
            ProviderKind::Anthropic,
            "claude-3-5-sonnet-20241022",
            None::<String>,
        ),
        "gemini" => (ProviderKind::Gemini, "gemini-1.5-pro", None::<String>),
        "ollama" => (
            ProviderKind::Ollama,
            "llama3",
            Some("http://localhost:11434/v1".into()),
        ),
        _ => return None,
    };

    let model = model.unwrap_or_else(|| default_model.to_string());
    let label = format!("{} ({model})", kind.name());
    let id = format!("{}-{}", kind_str, model.replace(['.', ':'], "-"));

    let mut config = ProviderConfig::new(&id, kind, &label, "", &model);
    if let Some(url) = url {
        config = config.with_url(url);
    }
    config.notes = Some(format!("Auto-created by `praxis agents create`"));

    Some(config)
}

/// Initialize a workspace directory for the agent.
fn initialize_workspace(workspace_path: &str, agent_id: &str, _agent_name: &str) {
    let path = std::path::Path::new(workspace_path);

    if path.exists() {
        println!(
            "  Workspace '{}' already exists, skipping initialization.",
            workspace_path
        );
        return;
    }

    match std::fs::create_dir_all(path) {
        Ok(_) => {
            // Create a minimal workspace structure
            let dirs = [path.join("data"), path.join("logs"), path.join("config")];

            for dir in &dirs {
                let _ = std::fs::create_dir_all(dir);
            }

            // Create a .praxis-workspace metadata file
            let meta = serde_json::json!({
                "version": 1,
                "agent_id": agent_id,
                "created_at": praxis_core::registry::timestamp(),
            });

            if let Ok(json) = serde_json::to_string_pretty(&meta) {
                let _ = std::fs::write(path.join(".praxis-workspace"), &json);
            }

            println!("  Workspace initialized at: {workspace_path}");
            println!("    data/   — runtime data");
            println!("    logs/   — execution logs");
            println!("    config/ — agent-specific configuration");
        }
        Err(e) => {
            eprintln!("  Warning: workspace initialization failed: {e}");
        }
    }
}

/// Execute the agents subcommand.
pub fn execute(args: &AgentsArgs) {
    match &args.command {
        AgentsCommand::Create(create_args) => execute_create(create_args),
    }
}
