//! **File Guard** — контроль доступа к файловой системе для каждого агента.
//!
//! Определяет, какие пути доступны агенту для чтения/записи.
//! Интегрируется с существующим `PathRestrict` из модуля `sandbox`.

use std::path::{Path, PathBuf};

/// Режим файлового guard'а.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileGuardMode {
    /// Доступны только указанные пути.
    Restricted,
    /// Доступны все пути.
    Unrestricted,
}

/// Охранник файлов — проверяет, разрешён ли агенту доступ к пути.
#[derive(Debug, Clone)]
pub struct FileGuard {
    /// Режим.
    mode: FileGuardMode,
    /// Список разрешённых директорий.
    allowed_dirs: Vec<PathBuf>,
    /// Разрешить поддиректории.
    allow_subdirs: bool,
    /// Паттерны, запрещённые в любом режиме.
    blocked_patterns: Vec<String>,
}

impl FileGuard {
    /// Создать unrestricted-охранника (все пути разрешены).
    #[must_use]
    pub fn unrestricted() -> Self {
        Self {
            mode: FileGuardMode::Unrestricted,
            allowed_dirs: Vec::new(),
            allow_subdirs: true,
            blocked_patterns: Vec::new(),
        }
    }

    /// Создать restricted-охранника с указанными директориями.
    #[must_use]
    pub fn restricted(dirs: Vec<impl Into<PathBuf>>) -> Self {
        Self {
            mode: FileGuardMode::Restricted,
            allowed_dirs: dirs.into_iter().map(Into::into).collect(),
            allow_subdirs: true,
            blocked_patterns: vec![
                "etc/passwd".into(),
                "etc/shadow".into(),
                ".ssh".into(),
                ".git".into(),
            ],
        }
    }

    /// Добавить разрешённую директорию.
    pub fn add_dir(&mut self, dir: impl Into<PathBuf>) {
        self.allowed_dirs.push(dir.into());
    }

    /// Добавить заблокированный паттерн (substring match).
    pub fn add_blocked_pattern(&mut self, pattern: impl Into<String>) {
        self.blocked_patterns.push(pattern.into());
    }

    /// Проверить, разрешено ли чтение пути.
    ///
    /// # Arguments
    /// * `path` — путь для проверки
    ///
    /// Возвращает `true`, если чтение разрешено.
    #[must_use]
    pub fn can_read(&self, path: &Path) -> bool {
        self.is_allowed(path)
    }

    /// Проверить, разрешена ли запись в путь.
    ///
    /// # Arguments
    /// * `path` — путь для проверки
    ///
    /// Возвращает `true`, если запись разрешена.
    #[must_use]
    pub fn can_write(&self, path: &Path) -> bool {
        self.is_allowed(path)
    }

    /// Базовая проверка: проходит ли путь через все фильтры.
    fn is_allowed(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy().to_lowercase();

        // Проверка заблокированных паттернов
        for pattern in &self.blocked_patterns {
            if path_str.contains(&pattern.to_lowercase()) {
                return false;
            }
        }

        match self.mode {
            FileGuardMode::Unrestricted => true,
            FileGuardMode::Restricted => {
                if self.allowed_dirs.is_empty() {
                    return false;
                }
                self.allowed_dirs.iter().any(|dir| {
                    if self.allow_subdirs {
                        path.starts_with(dir)
                    } else {
                        path == dir.as_path()
                    }
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unrestricted() {
        let guard = FileGuard::unrestricted();
        assert!(guard.can_read(Path::new("/etc/passwd")));
    }

    #[test]
    fn test_restricted_allows_subdir() {
        let guard = FileGuard::restricted(vec!["/home/user/project"]);
        assert!(guard.can_read(Path::new("/home/user/project/src/main.rs")));
        assert!(!guard.can_read(Path::new("/etc/passwd")));
    }

    #[test]
    fn test_blocks_sensitive_patterns() {
        let guard = FileGuard::restricted(vec!["/home/user"]);
        assert!(!guard.can_read(Path::new("/home/user/.ssh/id_rsa")));
        assert!(!guard.can_read(Path::new("/home/user/.git/config")));
    }

    #[test]
    fn test_add_dir_and_pattern() {
        let mut guard = FileGuard::restricted(vec!["/tmp"]);
        guard.add_dir("/var/log");
        guard.add_blocked_pattern("secret");

        assert!(guard.can_read(Path::new("/tmp/file.txt")));
        assert!(guard.can_read(Path::new("/var/log/app.log")));
        assert!(!guard.can_read(Path::new("/tmp/secret.txt")));
    }
}
