//! # Plugin System — динамическая загрузка и управление плагинами.
//!
//! Плагины — это динамически загружаемые модули, которые расширяют
//! возможности Praxis: добавляют инструменты, точки расширения (hooks)
//! и кастомную логику.
//!
//! ## Архитектура
//!
//! * [`PluginManifest`] — манифест плагина (TOML/JSON)
//! * [`PluginRegistry`] — реестр загруженных плагинов
//! * [`PluginLoader`] — сканирование директорий и загрузка
//! * [`PluginHost`] — среда выполнения WASM-плагинов (feature `plugin-wasm`)
//!
//! ## Пример
//!
//! ```ignore
//! use praxis_core::plugin::{PluginRegistry, PluginLoader, LoaderConfig};
//!
//! let mut registry = PluginRegistry::new();
//! let loader = PluginLoader::default();
//! loader.load_all(&mut registry).unwrap();
//!
//! for plugin in registry.list() {
//!     println!("Loaded: {} v{}", plugin.manifest.name, plugin.manifest.version);
//! }
//! ```

mod host;
mod loader;
mod manifest;
mod registry;

pub use host::*;
pub use loader::*;
pub use manifest::*;
pub use registry::*;
