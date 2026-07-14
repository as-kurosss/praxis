use serde::{Deserialize, Serialize};

/// Reference to a tool binding within an agent definition.
///
/// A tool can be either a built-in tool (known by name) or a custom
/// tool with an inline JSON schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolBinding {
    /// A built-in tool enabled by name (e.g. `"shell"`, `"calculator"`, `"time"`).
    Builtin {
        /// Tool name as understood by the runtime.
        name: String,
        /// Whether this tool is enabled.  `true` by default.
        #[serde(default = "default_enabled")]
        enabled: bool,
    },
    /// A custom tool with inline spec (schema only — no runtime handler).
    Custom {
        /// Tool name sent to the LLM.
        name: String,
        /// Description for the LLM.
        description: String,
        /// JSON Schema of the parameters.
        schema: serde_json::Value,
        /// Whether this tool is enabled.
        #[serde(default = "default_enabled")]
        enabled: bool,
    },
    /// Tools exposed by an MCP server.
    Mcp {
        /// Server name as registered in the MCP server configuration.
        server_name: String,
        /// Specific tool names to include (empty = all tools from this server).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tools: Vec<String>,
        /// Whether this binding is enabled.
        #[serde(default = "default_enabled")]
        enabled: bool,
    },
}

fn default_enabled() -> bool {
    true
}

/// Scroll strategy configuration for agents.
///
/// Mirrors [`ScrollStrategy`](crate::memory::ScrollStrategy) but uses
/// only serializable fields (no closure-based summarizer).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScrollConfig {
    /// Keep system prompt + last N messages.
    Truncate {
        /// Maximum total messages to retain.
        max_messages: usize,
    },
    /// Keep only the last N messages (system may be evicted).
    SlidingWindow {
        /// Window size.
        window_size: usize,
    },
    /// Keep everything.
    NoOp,
}

impl Default for ScrollConfig {
    fn default() -> Self {
        Self::Truncate { max_messages: 50 }
    }
}

/// A fully configured agent definition.
///
/// This struct holds everything needed to instantiate and run an agent,
/// without writing Rust code.  It is stored in the registry as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    /// Unique identifier (e.g. `"my-assistant"`).
    pub id: String,
    /// Human-readable name displayed in UI.
    pub name: String,
    /// Optional description.
    pub description: Option<String>,
    /// Which provider to use (references [`ProviderConfig::id`]).
    pub provider_id: String,
    /// System prompt for the agent.
    pub system_prompt: String,
    /// Per-agent model ID override (e.g. "gpt-4o-mini").
    /// When set, overrides the provider-level model.
    pub model_id: Option<String>,
    /// Whether this agent is enabled (default: `true`).
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Agent language (e.g. "en", "ru", "zh") for language-synced responses.
    pub language: Option<String>,
    /// Number of auto-continue retries on text-only responses (default: `0`).
    /// When an agent returns only text (no tool calls), it can retry up to
    /// this many times to produce a more complete response.
    #[serde(default)]
    pub auto_continue_retry: u32,
    /// Sampling temperature. `None` = provider default.
    pub temperature: Option<f32>,
    /// Maximum tokens. `None` = provider default.
    pub max_tokens: Option<u32>,
    /// Scroll strategy for conversation history.
    #[serde(default)]
    pub scroll_strategy: ScrollConfig,
    /// Tools available to this agent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolBinding>,
    /// Timestamp when this definition was created.
    pub created_at: String,
    /// Timestamp of last modification.
    pub updated_at: String,
}

impl AgentDefinition {
    /// Create a new agent definition with sensible defaults.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        provider_id: impl Into<String>,
        system_prompt: impl Into<String>,
    ) -> Self {
        let now = crate::registry::timestamp();
        Self {
            id: id.into(),
            name: name.into(),
            description: None,
            provider_id: provider_id.into(),
            system_prompt: system_prompt.into(),
            model_id: None,
            enabled: true,
            language: None,
            auto_continue_retry: 0,
            temperature: None,
            max_tokens: None,
            scroll_strategy: ScrollConfig::default(),
            tools: Vec::new(),
            created_at: now.clone(),
            updated_at: now,
        }
    }

    /// Enable a built-in tool by name.
    pub fn with_tool(mut self, name: &str) -> Self {
        self.tools.push(ToolBinding::Builtin {
            name: name.to_string(),
            enabled: true,
        });
        self
    }
}
