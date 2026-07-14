//! **Plugin Host** — среда выполнения WASM-плагинов.
//!
//! Оборачивает `wasmtime` runtime для изолированного выполнения
//! плагинов, скомпилированных в `wasm32-wasi`.
//!
//! > **Note:** Этот модуль опционален и включается через feature `plugin-wasm`.

use super::manifest::PluginManifest;
use std::path::{Path, PathBuf};

/// Политика доступа для WASM-плагина.
#[derive(Debug, Clone)]
pub struct HostAccessPolicy {
    /// Разрешить доступ к файловой системе.
    pub allow_fs: bool,
    /// Разрешить сетевой доступ.
    pub allow_network: bool,
    /// Разрешить доступ к переменным окружения.
    pub allow_env: bool,
    /// Лимит памяти в байтах.
    pub memory_limit: u64,
}

impl Default for HostAccessPolicy {
    fn default() -> Self {
        Self {
            allow_fs: false,
            allow_network: false,
            allow_env: false,
            memory_limit: 10 * 1024 * 1024, // 10 MB
        }
    }
}

/// Результат вызова функции плагина.
#[derive(Debug, Clone)]
pub struct PluginCallResult {
    /// Данные, возвращённые плагином.
    pub output: Vec<u8>,
    /// Время выполнения в миллисекундах.
    pub duration_ms: u64,
}

/// Ошибка выполнения плагина.
#[derive(Debug, thiserror::Error)]
pub enum HostError {
    /// WASM-модуль не скомпилирован.
    #[error("WASM module not compiled: {0}")]
    NotCompiled(String),
    /// Ошибка компиляции WASM.
    #[error("WASM compilation error: {0}")]
    Compilation(String),
    /// Ошибка выполнения WASM.
    #[error("WASM execution error: {0}")]
    Execution(String),
    /// Плагин превысил лимит.
    #[error("Plugin resource limit exceeded: {0}")]
    LimitExceeded(String),
    /// Плагин не найден.
    #[error("Plugin not found: {0}")]
    NotFound(String),
    /// Feature `plugin-wasm` не включена.
    #[error("WASM plugin support not enabled (feature 'plugin-wasm' required)")]
    WasmFeatureDisabled,
}

/// Хост для выполнения WASM-плагинов.
///
/// Управляет компиляцией, инстанцированием и выполнением WASM-модулей
/// через `wasmtime` runtime.
#[derive(Debug, Clone)]
pub struct PluginHost {
    /// Директория для временных файлов плагинов.
    temp_dir: PathBuf,
    /// Политика доступа по умолчанию.
    default_policy: HostAccessPolicy,
    /// Компилировать при загрузке (иначе — ленивая компиляция).
    compile_on_load: bool,
}

impl PluginHost {
    /// Создать новый хост плагинов.
    ///
    /// # Arguments
    /// * `temp_dir` — директория для временных файлов (cache, etc.)
    #[must_use]
    pub fn new(temp_dir: impl Into<PathBuf>) -> Self {
        Self {
            temp_dir: temp_dir.into(),
            default_policy: HostAccessPolicy::default(),
            compile_on_load: true,
        }
    }

    /// Установить политику доступа по умолчанию.
    #[must_use]
    pub fn with_policy(mut self, policy: HostAccessPolicy) -> Self {
        self.default_policy = policy;
        self
    }

    /// Включить/выключить компиляцию при загрузке.
    #[must_use]
    pub fn compile_on_load(mut self, yes: bool) -> Self {
        self.compile_on_load = yes;
        self
    }

    /// Загрузить и скомпилировать WASM-модуль плагина.
    ///
    /// Если feature `plugin-wasm` не включена, возвращает
    /// `HostError::WasmFeatureDisabled`.
    ///
    /// # Arguments
    /// * `manifest` — манифест плагина
    /// * `wasm_path` — путь к WASM-файлу
    #[allow(unused_variables)]
    pub fn compile(
        &self,
        manifest: &PluginManifest,
        wasm_path: &Path,
    ) -> Result<CompiledPlugin, HostError> {
        // WASM-time — опциональная зависимость. Если feature не включена,
        // сообщаем пользователю.
        #[cfg(not(feature = "plugin-wasm"))]
        {
            let _ = manifest;
            let _ = wasm_path;
            return Err(HostError::WasmFeatureDisabled);
        }

        #[cfg(feature = "plugin-wasm")]
        {
            let _ = manifest;
            let _ = wasm_path;
            // TODO: Реальная компиляция через wasmtime:
            //   1. Создать Engine с нужными лимитами
            //   2. Прочитать .wasm файл
            //   3. Скомпилировать в Module
            //   4. Создать linker с WASI
            //   5. Вернуть CompiledPlugin
            Err(HostError::Compilation(
                "wasmtime compilation not yet implemented in this version".into(),
            ))
        }
    }

    /// Выполнить функцию плагина.
    #[allow(unused_variables)]
    pub async fn call_function(
        &self,
        plugin: &CompiledPlugin,
        function: &str,
        args: &[u8],
    ) -> Result<PluginCallResult, HostError> {
        #[cfg(not(feature = "plugin-wasm"))]
        {
            let _ = plugin;
            let _ = function;
            let _ = args;
            return Err(HostError::WasmFeatureDisabled);
        }

        #[cfg(feature = "plugin-wasm")]
        {
            let _ = plugin;
            let _ = function;
            let _ = args;
            // TODO: Выполнение через wasmtime Instance::get_export
            Err(HostError::Execution(
                "wasmtime execution not yet implemented in this version".into(),
            ))
        }
    }

    /// Получить директорию для временных файлов.
    #[must_use]
    pub fn temp_dir(&self) -> &Path {
        &self.temp_dir
    }
}

/// Скомпилированный WASM-модуль плагина.
#[derive(Debug, Clone)]
pub struct CompiledPlugin {
    /// Манифест плагина.
    pub manifest: PluginManifest,
    /// Путь к WASM-файлу.
    pub wasm_path: PathBuf,
    /// Политика доступа для этого экземпляра.
    pub policy: HostAccessPolicy,
}

impl CompiledPlugin {
    /// Создать новый скомпилированный плагин.
    #[must_use]
    pub fn new(manifest: PluginManifest, wasm_path: PathBuf) -> Self {
        Self {
            manifest,
            wasm_path,
            policy: HostAccessPolicy::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_creation() {
        let host = PluginHost::new("/tmp/praxis-plugins");
        let manifest = PluginManifest::from_toml(r#"name = "test-p""#).unwrap();
        let _result = host.compile(&manifest, Path::new("/fake/plugin.wasm"));

        // Без feature 'plugin-wasm' — получаем WasmFeatureDisabled
        #[cfg(not(feature = "plugin-wasm"))]
        assert!(matches!(_result, Err(HostError::WasmFeatureDisabled)));

        // Тестируем, что структуры создаются
        let compiled = CompiledPlugin::new(manifest.clone(), PathBuf::from("/p.wasm"));
        assert_eq!(compiled.manifest.name, "test-p");
    }
}
