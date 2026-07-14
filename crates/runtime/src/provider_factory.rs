//! **ProviderFactory implementations** — bridge between [`ProviderConfig`] and
//! concrete [`LlmClient`] instances.
//!
//! [`register_defaults`] registers factories for all three backends
//! (OpenAI-compatible, Anthropic, Gemini) into a [`ProviderFactoryRegistry`].

use praxis_core::agent::llm::{LlmClient, LlmError};
use praxis_core::registry::{
    ProviderConfig, ProviderFactory, ProviderFactoryRegistry, ProviderKind,
};
use std::sync::Arc;

// ── OpenAI / Ollama / Custom / LM Studio ───────────────────────────────────

struct OpenAiFactory;

#[async_trait::async_trait]
impl ProviderFactory for OpenAiFactory {
    fn create(&self, config: &ProviderConfig) -> Result<Arc<dyn LlmClient>, LlmError> {
        let default_url = match config.kind {
            ProviderKind::Ollama => "http://localhost:11434/v1",
            ProviderKind::LmStudio => "http://localhost:1234/v1",
            _ => "https://api.openai.com/v1",
        };
        let url = config.api_url.as_deref().unwrap_or(default_url);
        Ok(Arc::new(super::OpenAiClient::new(
            url,
            &config.api_key,
            &config.model,
        )))
    }
}

// ── Anthropic ─────────────────────────────────────────────────────────────—

struct AnthropicFactory;

#[async_trait::async_trait]
impl ProviderFactory for AnthropicFactory {
    fn create(&self, config: &ProviderConfig) -> Result<Arc<dyn LlmClient>, LlmError> {
        let url = config
            .api_url
            .as_deref()
            .unwrap_or("https://api.anthropic.com/v1");
        Ok(Arc::new(super::AnthropicClient::new(
            url,
            &config.api_key,
            &config.model,
        )))
    }
}

// ── Gemini ────────────────────────────────────────────────────────────────

struct GeminiFactory;

#[async_trait::async_trait]
impl ProviderFactory for GeminiFactory {
    fn create(&self, config: &ProviderConfig) -> Result<Arc<dyn LlmClient>, LlmError> {
        Ok(Arc::new(super::GeminiClient::new(
            &config.api_key,
            &config.model,
        )))
    }
}

/// Register all built-in provider factories into a registry and return it.
///
/// The returned registry can be extended with custom factories via
/// [`ProviderFactoryRegistry::register`].
pub fn register_default_factories() -> ProviderFactoryRegistry {
    let mut registry = ProviderFactoryRegistry::new();
    registry.register(ProviderKind::Openai, Box::new(OpenAiFactory));
    registry.register(ProviderKind::Anthropic, Box::new(AnthropicFactory));
    registry.register(ProviderKind::Gemini, Box::new(GeminiFactory));
    registry.register(ProviderKind::Ollama, Box::new(OpenAiFactory));
    registry.register(ProviderKind::LmStudio, Box::new(OpenAiFactory));
    registry.register(ProviderKind::Custom, Box::new(OpenAiFactory));
    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_default_all_kinds() {
        let registry = register_default_factories();
        assert_eq!(registry.len(), 6);
        assert!(registry.supports(&ProviderKind::Openai));
        assert!(registry.supports(&ProviderKind::Anthropic));
        assert!(registry.supports(&ProviderKind::Gemini));
        assert!(registry.supports(&ProviderKind::Ollama));
        assert!(registry.supports(&ProviderKind::LmStudio));
        assert!(registry.supports(&ProviderKind::Custom));
    }

    #[test]
    fn test_create_openai_client() {
        let registry = register_default_factories();
        let config = ProviderConfig::new("test", ProviderKind::Openai, "test", "sk-test", "gpt-4o");
        let client = registry.create(&config);
        assert!(
            client.is_ok(),
            "should create OpenAI client: {:?}",
            client.err()
        );
    }

    #[test]
    fn test_create_anthropic_client() {
        let registry = register_default_factories();
        let config = ProviderConfig::new(
            "test",
            ProviderKind::Anthropic,
            "test",
            "sk-ant-test",
            "claude-sonnet-4",
        );
        let client = registry.create(&config);
        assert!(
            client.is_ok(),
            "should create Anthropic client: {:?}",
            client.err()
        );
    }

    #[test]
    fn test_create_gemini_client() {
        let registry = register_default_factories();
        let config = ProviderConfig::new(
            "test",
            ProviderKind::Gemini,
            "test",
            "gemini-test",
            "gemini-2.0-flash",
        );
        let client = registry.create(&config);
        assert!(
            client.is_ok(),
            "should create Gemini client: {:?}",
            client.err()
        );
    }

    #[test]
    fn test_create_ollama_client() {
        let registry = register_default_factories();
        let config = ProviderConfig::new("test", ProviderKind::Ollama, "test", "", "llama3");
        let client = registry.create(&config);
        assert!(
            client.is_ok(),
            "should create Ollama client: {:?}",
            client.err()
        );
    }

    #[test]
    fn test_create_unknown_kind_returns_error() {
        let mut registry = ProviderFactoryRegistry::new();
        // Only register a subset, not all
        registry.register(ProviderKind::Openai, Box::new(OpenAiFactory));
        let config = ProviderConfig::new("test", ProviderKind::Anthropic, "test", "key", "model");
        let result = registry.create(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_registry() {
        let registry = ProviderFactoryRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }
}
