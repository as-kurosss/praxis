//! **Plugin Registry** — реестр загруженных плагинов.
//!
//! Управляет коллекцией плагинов: загрузка, выгрузка, поиск по имени,
//! итерирование. Каждый плагин представлен загруженным WASM-инстансом
//! или нативным объектом, реализующим трейт [`PluginInstance`].

use super::manifest::PluginManifest;
use std::collections::HashMap;
use std::path::PathBuf;

/// Экземпляр загруженного плагина.
#[derive(Debug, Clone)]
pub struct PluginInstance {
    /// Манифест плагина.
    pub manifest: PluginManifest,
    /// Путь к директории плагина.
    pub path: PathBuf,
    /// WASM-модуль скомпилирован (если плагин WASM-based).
    pub compiled: bool,
    /// Плагин активен.
    pub active: bool,
}

impl PluginInstance {
    /// Создать новый экземпляр плагина.
    #[must_use]
    pub fn new(manifest: PluginManifest, path: PathBuf) -> Self {
        Self {
            manifest,
            path,
            compiled: false,
            active: true,
        }
    }
}

/// Статус загрузки плагина.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginStatus {
    /// Плагин загружен и активен.
    Loaded,
    /// Плагин загружен, но деактивирован.
    Inactive,
    /// Ошибка загрузки плагина.
    Error(String),
}

/// Реестр плагинов — центральное хранилище всех загруженных плагинов.
#[derive(Debug, Clone)]
pub struct PluginRegistry {
    /// Карта плагинов: имя → экземпляр.
    plugins: HashMap<String, PluginInstance>,
    /// Статусы плагинов.
    statuses: HashMap<String, PluginStatus>,
}

impl PluginRegistry {
    /// Создать пустой реестр плагинов.
    #[must_use]
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            statuses: HashMap::new(),
        }
    }

    /// Зарегистрировать плагин в реестре.
    pub fn register(&mut self, instance: PluginInstance) {
        let name = instance.manifest.name.clone();
        self.statuses.insert(name.clone(), PluginStatus::Loaded);
        self.plugins.insert(name, instance);
    }

    /// Удалить плагин из реестра.
    pub fn unregister(&mut self, name: &str) {
        self.plugins.remove(name);
        self.statuses.remove(name);
    }

    /// Получить плагин по имени.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&PluginInstance> {
        self.plugins.get(name)
    }

    /// Проверить, зарегистрирован ли плагин.
    #[must_use]
    pub fn has(&self, name: &str) -> bool {
        self.plugins.contains_key(name)
    }

    /// Получить статус плагина.
    #[must_use]
    pub fn status(&self, name: &str) -> Option<&PluginStatus> {
        self.statuses.get(name)
    }

    /// Деактивировать плагин (без удаления).
    pub fn deactivate(&mut self, name: &str) {
        if let Some(status) = self.statuses.get_mut(name) {
            *status = PluginStatus::Inactive;
        }
        if let Some(plugin) = self.plugins.get_mut(name) {
            plugin.active = false;
        }
    }

    /// Активировать плагин.
    pub fn activate(&mut self, name: &str) {
        if let Some(status) = self.statuses.get_mut(name) {
            *status = PluginStatus::Loaded;
        }
        if let Some(plugin) = self.plugins.get_mut(name) {
            plugin.active = true;
        }
    }

    /// Получить список всех зарегистрированных плагинов.
    #[must_use]
    pub fn list(&self) -> Vec<&PluginInstance> {
        self.plugins.values().collect()
    }

    /// Получить список активных плагинов.
    #[must_use]
    pub fn list_active(&self) -> Vec<&PluginInstance> {
        self.plugins.values().filter(|p| p.active).collect()
    }

    /// Количество зарегистрированных плагинов.
    #[must_use]
    pub fn count(&self) -> usize {
        self.plugins.len()
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_lifecycle() {
        let mut registry = PluginRegistry::new();

        let manifest = PluginManifest::from_toml(r#"name = "test-p""#).unwrap();
        let instance = PluginInstance::new(manifest, PathBuf::from("/plugins/test-p"));

        registry.register(instance);
        assert_eq!(registry.count(), 1);
        assert!(registry.has("test-p"));
        assert_eq!(registry.status("test-p"), Some(&PluginStatus::Loaded));

        registry.deactivate("test-p");
        assert!(!registry.get("test-p").unwrap().active);

        registry.activate("test-p");
        assert!(registry.get("test-p").unwrap().active);

        registry.unregister("test-p");
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn test_registry_list_active() {
        let mut registry = PluginRegistry::new();

        let m1 = PluginManifest::from_toml(r#"name = "p1""#).unwrap();
        let m2 = PluginManifest::from_toml(r#"name = "p2""#).unwrap();

        registry.register(PluginInstance::new(m1, PathBuf::from("/p1")));
        registry.register(PluginInstance::new(m2, PathBuf::from("/p2")));
        registry.deactivate("p2");

        assert_eq!(registry.list_active().len(), 1);
        assert_eq!(registry.list_active()[0].manifest.name, "p1");
    }
}
