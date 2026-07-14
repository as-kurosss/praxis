use crate::agent::llm::{LlmClient, LlmError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Supported LLM provider kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// OpenAI-compatible API (also works with Ollama, vLLM, OpenRouter, etc.)
    Openai,
    /// Anthropic Claude API.
    Anthropic,
    /// Google Gemini API.
    Gemini,
    /// Ollama local models (OpenAI-compatible endpoint).
    Ollama,
    /// Generic OpenAI-compatible custom API.
    Custom,
    /// LM Studio local models (OpenAI-compatible endpoint).
    LmStudio,
}

impl ProviderKind {
    /// Human-readable name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Openai => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::Gemini => "Gemini",
            Self::Ollama => "Ollama",
            Self::Custom => "Custom",
            Self::LmStudio => "LM Studio",
        }
    }

    /// Whether this provider needs an explicit `api_url`.
    /// OpenAI-compatible providers can customize the URL; Anthropic and Gemini
    /// use fixed defaults.
    pub fn supports_custom_url(&self) -> bool {
        matches!(
            self,
            Self::Openai | Self::Ollama | Self::Custom | Self::LmStudio
        )
    }
}

/// A saved LLM provider configuration.
///
/// Each provider entry in the registry represents one set of credentials and
/// a default model. You can have multiple entries for the same provider kind
/// (e.g. one for GPT‑4o, one for GPT‑4o‑mini).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Unique identifier (e.g. `"my-openai"`).
    pub id: String,
    /// Provider kind.
    pub kind: ProviderKind,
    /// Human-readable label (e.g. `"My OpenAI key"`).
    pub label: String,
    /// API base URL.  `None` = provider default.
    pub api_url: Option<String>,
    /// API key.
    pub api_key: String,
    /// Default model name (e.g. `"gpt-4o"`, `"claude-3-5-sonnet"`).
    pub model: String,
    /// Optional notes.
    pub notes: Option<String>,
}

impl ProviderConfig {
    /// Create a new provider config with the required fields.
    pub fn new(
        id: impl Into<String>,
        kind: ProviderKind,
        label: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            label: label.into(),
            api_url: None,
            api_key: api_key.into(),
            model: model.into(),
            notes: None,
        }
    }

    /// Set a custom API URL.
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.api_url = Some(url.into());
        self
    }

    /// Set notes.
    pub fn with_notes(mut self, notes: impl Into<String>) -> Self {
        self.notes = Some(notes.into());
        self
    }
}

// ── ProviderFactory trait ───────────────────────────────────────────────

/// Dynamically creates an [`LlmClient`] from a [`ProviderConfig`].
///
/// Each provider kind registers a factory that knows how to build
/// the correct client implementation.
#[async_trait::async_trait]
pub trait ProviderFactory: Send + Sync {
    /// Create an LLM client from the given configuration.
    fn create(&self, config: &ProviderConfig) -> Result<Arc<dyn LlmClient>, LlmError>;
}

/// Registry of provider factories.
///
/// Maps [`ProviderKind`] to a factory that can create the appropriate
/// [`LlmClient`].  Factories are registered at startup.
#[derive(Default)]
pub struct ProviderFactoryRegistry {
    factories: HashMap<ProviderKind, Box<dyn ProviderFactory>>,
}

impl ProviderFactoryRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    /// Register a factory for a provider kind.
    pub fn register(&mut self, kind: ProviderKind, factory: Box<dyn ProviderFactory>) {
        self.factories.insert(kind, factory);
    }

    /// Create an LLM client for the given provider configuration.
    ///
    /// Returns an error if no factory is registered for the provider kind.
    pub fn create(&self, config: &ProviderConfig) -> Result<Arc<dyn LlmClient>, LlmError> {
        let factory = self.factories.get(&config.kind).ok_or_else(|| {
            LlmError::Request(format!(
                "no factory registered for provider kind {:?}",
                config.kind
            ))
        })?;
        factory.create(config)
    }

    /// Returns true if a factory is registered for the given kind.
    pub fn supports(&self, kind: &ProviderKind) -> bool {
        self.factories.contains_key(kind)
    }

    /// Number of registered factories.
    pub fn len(&self) -> usize {
        self.factories.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.factories.is_empty()
    }
}
