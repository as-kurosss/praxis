//! **Plugin Loader** — обнаружение и загрузка плагинов из файловой системы.
//!
//! Сканирует указанные директории, парсит манифесты (.toml/.json) и
//! регистрирует плагины в реестре.

use super::manifest::{ManifestError, PluginManifest};
use super::registry::{PluginInstance, PluginRegistry};
use std::path::{Path, PathBuf};

/// Настройки загрузчика плагинов.
#[derive(Debug, Clone)]
pub struct LoaderConfig {
    /// Директории для поиска плагинов.
    pub search_dirs: Vec<PathBuf>,
    /// Имя файла манифеста (по умолчанию `plugin.toml`).
    pub manifest_name: String,
    /// Рекурсивный поиск.
    pub recursive: bool,
}

impl Default for LoaderConfig {
    fn default() -> Self {
        Self {
            search_dirs: vec![PathBuf::from(".agents/plugins")],
            manifest_name: "plugin.toml".into(),
            recursive: false,
        }
    }
}

/// Ошибка загрузки плагина.
#[derive(Debug, thiserror::Error)]
pub enum LoaderError {
    /// Ошибка файловой системы.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// Ошибка парсинга манифеста.
    #[error("Manifest error: {0}")]
    Manifest(#[from] ManifestError),
    /// Директория не найдена.
    #[error("Search directory not found: {0}")]
    DirNotFound(PathBuf),
}

/// Загрузчик плагинов — сканирует директории и загружает плагины.
#[derive(Debug, Clone)]
pub struct PluginLoader {
    /// Конфигурация загрузчика.
    config: LoaderConfig,
}

impl PluginLoader {
    /// Создать новый загрузчик плагинов с указанной конфигурацией.
    #[must_use]
    pub fn new(config: LoaderConfig) -> Self {
        Self { config }
    }

    /// Создать загрузчик с конфигурацией по умолчанию.
    #[must_use]
    pub fn with_default_config() -> Self {
        Self {
            config: LoaderConfig::default(),
        }
    }

    /// Создать загрузчик для кастомной директории.
    #[must_use]
    pub fn with_dir(dir: impl Into<PathBuf>) -> Self {
        Self {
            config: LoaderConfig {
                search_dirs: vec![dir.into()],
                ..Default::default()
            },
        }
    }

    /// Загрузить все плагины из настроенных директорий в реестр.
    ///
    /// # Errors
    /// Возвращает ошибку, если директория не существует или нечитаема.
    pub fn load_all(&self, registry: &mut PluginRegistry) -> Result<(), LoaderError> {
        for dir in &self.config.search_dirs {
            if !dir.exists() {
                // Пропускаем несуществующие директории с предупреждением
                eprintln!(
                    "praxis: plugin: warning: search directory '{}' not found",
                    dir.display()
                );
                continue;
            }
            self.load_from_dir(dir, registry)?;
        }
        Ok(())
    }

    /// Загрузить плагины из конкретной директории.
    fn load_from_dir(&self, dir: &Path, registry: &mut PluginRegistry) -> Result<(), LoaderError> {
        let entries = std::fs::read_dir(dir)?;

        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_dir() && self.config.recursive {
                self.load_from_dir(&path, registry)?;
                continue;
            }

            // Ищем файл манифеста
            if path.is_file() && self.is_manifest(&path) {
                match PluginManifest::from_file(&path) {
                    Ok(manifest) => {
                        let plugin_dir = path.parent().unwrap_or(dir).to_path_buf();
                        let instance = PluginInstance::new(manifest, plugin_dir);
                        registry.register(instance);
                    }
                    Err(e) => {
                        eprintln!(
                            "praxis: plugin: warning: failed to load manifest '{}': {e}",
                            path.display()
                        );
                    }
                }
            }
        }

        Ok(())
    }

    /// Проверить, является ли файл манифестом плагина.
    fn is_manifest(&self, path: &Path) -> bool {
        path.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == self.config.manifest_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_loader_discovers_plugins() {
        let dir = tempfile::tempdir().unwrap();

        // Создаём манифест плагина прямо в корневой директории
        // (recursive поиск выключен по умолчанию)
        let manifest_path = dir.path().join("plugin.toml");
        let mut file = std::fs::File::create(&manifest_path).unwrap();
        writeln!(
            file,
            r#"name = "my-plugin"
version = "1.0.0"
author = "test"
description = "Test plugin"
[[tools]]
name = "hello"
description = "Says hello"
"#
        )
        .unwrap();

        let loader = PluginLoader::with_dir(dir.path());
        let mut registry = PluginRegistry::new();
        loader.load_all(&mut registry).unwrap();

        assert_eq!(registry.count(), 1);
        let plugin = registry.get("my-plugin").unwrap();
        assert_eq!(plugin.manifest.tools.len(), 1);
        assert_eq!(plugin.manifest.tools[0].name, "hello");
    }

    #[test]
    fn test_loader_skips_missing_dir() {
        let loader = PluginLoader::with_dir("/nonexistent/path/for/plugins");
        let mut registry = PluginRegistry::new();
        // Не должно быть ошибки — только предупреждение
        loader.load_all(&mut registry).unwrap();
        assert_eq!(registry.count(), 0);
    }
}
