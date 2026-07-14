//! # Governance — система управления доступом агентов.
//!
//! Обеспечивает контроль доступа для агентов на нескольких уровнях:
//!
//! * **Governance Matrix** — пер-агент матрица Allow/Deny/Ask для категорий ресурсов
//! * **Tool Guard** — контроль доступа к конкретным инструментам (AllowList/BlockList)
//! * **File Guard** — контроль доступа к файловой системе (пути, паттерны)
//!
//! Интегрируется с существующей системой Sandbox/Policy из модуля
//! [`crate::sandbox`]: матрица governance определяет высокоуровневую политику,
//! а sandbox обеспечивает изолированное исполнение.
//!
//! ## Пример
//!
//! ```ignore
//! use praxis_core::governance::{
//!     AgentGovernance, ToolGuard, FileGuard,
//! };
//! use praxis_core::sandbox::policy::AccessPolicy;
//! use praxis_core::agent::tool::ToolCategory;
//!
//! // Создаём матрицу доступа для агента с Deny по умолчанию
//! let mut gov = AgentGovernance::restricted("my-agent");
//! gov.add_rule(ToolCategory::FileRead, AccessPolicy::Allow, "needs to read files");
//!
//! // Ограничиваем инструменты
//! let tool_guard = ToolGuard::allow_list(vec!["calculator", "file_read"]);
//!
//! // Ограничиваем файлы
//! let file_guard = FileGuard::restricted(vec!["/home/user/project"]);
//! ```

mod file_guard;
mod matrix;
mod tool_guard;

pub use file_guard::*;
pub use matrix::*;
pub use tool_guard::*;
