//! **Plugin Manifest** — метаданные плагина.
//!
//! Каждый плагин описывается манифестом (TOML/JSON), который определяет
//! его имя, версию, автора, инструменты и точки расширения (hooks).

use serde::{Deserialize, Serialize};

/// Манифест плагина — описывает метаданные и возможности.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Имя плагина (уникальный идентификатор).
    pub name: String,
    /// Версия плагина (semver).
    #[serde(default = "default_version")]
    pub version: String,
    /// Автор плагина.
    #[serde(default)]
    pub author: String,
    /// Описание плагина.
    #[serde(default)]
    pub description: String,
    /// Путь к WASM-бинарнику плагина (относительно манифеста).
    #[serde(default)]
    pub wasm_path: Option<String>,
    /// Инструменты, предоставляемые плагином.
    #[serde(default)]
    pub tools: Vec<ToolManifest>,
    /// Точки расширения (hooks), которые поддерживает плагин.
    #[serde(default)]
    pub hooks: Vec<String>,
}

/// Описание инструмента в манифесте плагина.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolManifest {
    /// Имя инструмента (для LLM function calling).
    pub name: String,
    /// Описание для LLM.
    #[serde(default)]
    pub description: String,
    /// JSON Schema параметров.
    #[serde(default = "default_params")]
    pub parameters: serde_json::Value,
}

fn default_version() -> String {
    "0.1.0".to_string()
}

fn default_params() -> serde_json::Value {
    serde_json::json!({"type": "object", "properties": {}})
}

/// Ошибки парсинга манифеста.
#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    /// Ошибка чтения файла.
    #[error("Failed to read manifest file: {0}")]
    Io(#[from] std::io::Error),
    /// Ошибка парсинга TOML.
    #[error("Failed to parse TOML manifest: {0}")]
    Toml(String),
    /// Ошибка парсинга JSON.
    #[error("Failed to parse JSON manifest: {0}")]
    Json(String),
    /// Неверный формат манифеста.
    #[error("Invalid manifest: {0}")]
    Invalid(String),
}

impl PluginManifest {
    /// Загрузить манифест из TOML-строки.
    ///
    /// # Errors
    /// Возвращает `ManifestError::Toml` при ошибке парсинга.
    pub fn from_toml(input: &str) -> Result<Self, ManifestError> {
        toml::from_str(input).map_err(|e| ManifestError::Toml(e.to_string()))
    }

    /// Загрузить манифест из JSON-строки.
    ///
    /// # Errors
    /// Возвращает `ManifestError::Json` при ошибке парсинга.
    pub fn from_json(input: &str) -> Result<Self, ManifestError> {
        serde_json::from_str(input).map_err(|e| ManifestError::Json(e.to_string()))
    }

    /// Загрузить манифест из файла (автоопределение TOML/JSON по расширению).
    ///
    /// # Errors
    /// Возвращает `ManifestError::Io` при ошибке чтения или
    /// `ManifestError::Invalid` при неизвестном расширении.
    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self, ManifestError> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path)?;

        match path.extension().and_then(|e| e.to_str()) {
            Some("toml") => Self::from_toml(&content),
            Some("json") => Self::from_json(&content),
            other => Err(ManifestError::Invalid(format!(
                "unsupported manifest extension: {other:?}. Use .toml or .json"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_from_toml() {
        let toml = r#"
name = "test-plugin"
version = "1.0.0"
author = "test"
description = "A test plugin"

[[tools]]
name = "greet"
description = "Greets the user"
"#;
        let manifest = PluginManifest::from_toml(toml).unwrap();
        assert_eq!(manifest.name, "test-plugin");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.tools.len(), 1);
        assert_eq!(manifest.tools[0].name, "greet");
    }

    #[test]
    fn test_manifest_from_json() {
        let json = r#"{
            "name": "json-plugin",
            "version": "0.2.0",
            "author": "json-author",
            "description": "JSON test",
            "tools": [
                {"name": "tool1", "description": "First tool"}
            ]
        }"#;
        let manifest = PluginManifest::from_json(json).unwrap();
        assert_eq!(manifest.name, "json-plugin");
    }

    #[test]
    fn test_manifest_defaults() {
        let toml = r#"name = "minimal""#;
        let manifest = PluginManifest::from_toml(toml).unwrap();
        assert_eq!(manifest.version, "0.1.0");
        assert_eq!(manifest.tools.len(), 0);
        assert!(manifest.hooks.is_empty());
    }

    #[test]
    fn test_manifest_invalid_toml() {
        let result = PluginManifest::from_toml("not valid toml {{{");
        assert!(result.is_err());
    }
}
