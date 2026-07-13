use serde::{Deserialize, Serialize};

/// Supported LLM provider kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
}

impl ProviderKind {
    /// Human-readable name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Openai => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::Gemini => "Gemini",
            Self::Ollama => "Ollama",
        }
    }

    /// Whether this provider needs an explicit `api_url`.
    /// OpenAI-compatible providers can customize the URL; Anthropic and Gemini
    /// use fixed defaults.
    pub fn supports_custom_url(&self) -> bool {
        matches!(self, Self::Openai | Self::Ollama)
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
