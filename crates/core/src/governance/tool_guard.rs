//! **Tool Guard** — контроль доступа к инструментам для каждого агента.
//!
//! Определяет, какие инструменты (tools) разрешены агенту.
//! Может работать в трёх режимах:
//! * `AllowList` — разрешены только перечисленные инструменты
//! * `BlockList` — запрещены только перечисленные инструменты
//! * `AllowAll` — разрешены все инструменты

use crate::agent::tool::ToolCategory;
use std::collections::HashSet;

/// Режим фильтрации инструментов.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardMode {
    /// Разрешены только перечисленные инструменты (deny by default).
    AllowList,
    /// Запрещены только перечисленные инструменты (allow by default).
    BlockList,
    /// Разрешены все инструменты.
    AllowAll,
}

/// Охранник инструментов — проверяет, разрешён ли агенту конкретный инструмент.
#[derive(Debug, Clone)]
pub struct ToolGuard {
    /// Режим фильтрации.
    mode: GuardMode,
    /// Список имён инструментов для AllowList/BlockList.
    tool_names: HashSet<String>,
    /// Категории, полностью запрещённые для агента.
    blocked_categories: HashSet<ToolCategory>,
}

impl ToolGuard {
    /// Создать охранника в режиме AllowAll.
    #[must_use]
    pub fn allow_all() -> Self {
        Self {
            mode: GuardMode::AllowAll,
            tool_names: HashSet::new(),
            blocked_categories: HashSet::new(),
        }
    }

    /// Создать охранника с AllowList.
    #[must_use]
    pub fn allow_list(tools: Vec<impl Into<String>>) -> Self {
        Self {
            mode: GuardMode::AllowList,
            tool_names: tools.into_iter().map(Into::into).collect(),
            blocked_categories: HashSet::new(),
        }
    }

    /// Создать охранника с BlockList.
    #[must_use]
    pub fn block_list(tools: Vec<impl Into<String>>) -> Self {
        Self {
            mode: GuardMode::BlockList,
            tool_names: tools.into_iter().map(Into::into).collect(),
            blocked_categories: HashSet::new(),
        }
    }

    /// Добавить инструмент в список.
    pub fn add_tool(&mut self, name: impl Into<String>) {
        let name = name.into();
        match self.mode {
            GuardMode::AllowList => {
                self.tool_names.insert(name);
            }
            GuardMode::BlockList => {
                self.tool_names.insert(name);
            }
            GuardMode::AllowAll => {}
        }
    }

    /// Заблокировать целую категорию инструментов.
    pub fn block_category(&mut self, category: ToolCategory) {
        self.blocked_categories.insert(category);
    }

    /// Проверить, разрешён ли инструмент.
    ///
    /// # Arguments
    /// * `tool_name` — имя инструмента
    /// * `category` — категория инструмента
    ///
    /// Возвращает `true`, если инструмент разрешён.
    #[must_use]
    pub fn is_allowed(&self, tool_name: &str, category: &ToolCategory) -> bool {
        // Сначала проверяем категорию
        if self.blocked_categories.contains(category) {
            return false;
        }

        match self.mode {
            GuardMode::AllowAll => true,
            GuardMode::AllowList => self.tool_names.contains(tool_name),
            GuardMode::BlockList => !self.tool_names.contains(tool_name),
        }
    }

    /// Получить текущий режим.
    #[must_use]
    pub fn mode(&self) -> GuardMode {
        self.mode
    }

    /// Получить список инструментов (для AllowList/BlockList).
    #[must_use]
    pub fn tool_names(&self) -> &HashSet<String> {
        &self.tool_names
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::tool::ToolCategory;
    use proptest::prelude::*;

    proptest! {
        /// AllowAll разрешает любой инструмент.
        #[test]
        fn tool_guard_allow_all_allows_everything(tool_name: String, category: ToolCategory) {
            let guard = ToolGuard::allow_all();
            assert!(guard.is_allowed(&tool_name, &category));
        }

        /// AllowList с пустым списком запрещает всё.
        #[test]
        fn empty_allow_list_denies_all(tool_name: String, category: ToolCategory) {
            let guard = ToolGuard::allow_list(Vec::<String>::new());
            assert!(!guard.is_allowed(&tool_name, &category));
        }

        /// BlockList разрешает всё, кроме перечисленного.
        #[test]
        fn block_list_allows_unknown_tools(tool_name: String, category: ToolCategory) {
            let guard = ToolGuard::block_list(vec!["blocked_tool"]);
            // Неизвестный инструмент должен быть разрешён
            if tool_name != "blocked_tool" {
                assert!(guard.is_allowed(&tool_name, &category));
            }
        }
    }

    #[test]
    fn test_tool_guard_allow_all() {
        let guard = ToolGuard::allow_all();
        assert!(guard.is_allowed("shell", &ToolCategory::Shell));
        assert!(guard.is_allowed("network", &ToolCategory::Network));
    }

    #[test]
    fn test_tool_guard_allow_list() {
        let guard = ToolGuard::allow_list(vec!["calculator", "time"]);
        assert!(guard.is_allowed("calculator", &ToolCategory::Generic));
        assert!(guard.is_allowed("time", &ToolCategory::Generic));
        assert!(!guard.is_allowed("shell", &ToolCategory::Shell));
    }

    #[test]
    fn test_tool_guard_block_list() {
        let guard = ToolGuard::block_list(vec!["shell", "web_search"]);
        assert!(!guard.is_allowed("shell", &ToolCategory::Shell));
        assert!(!guard.is_allowed("web_search", &ToolCategory::Network));
        assert!(guard.is_allowed("calculator", &ToolCategory::Generic));
        assert!(guard.is_allowed("time", &ToolCategory::Generic));
    }

    #[test]
    fn test_tool_guard_block_category() {
        let mut guard = ToolGuard::allow_all();
        guard.block_category(ToolCategory::Network);
        assert!(!guard.is_allowed("web_search", &ToolCategory::Network));
        assert!(!guard.is_allowed("http", &ToolCategory::Network));
        assert!(guard.is_allowed("shell", &ToolCategory::Shell));
    }
}
