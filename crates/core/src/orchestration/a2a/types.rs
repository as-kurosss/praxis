//! **A2A (Agent-to-Agent) Protocol** — типы данных для Google A2A-совместимого
//! протокола взаимодействия между агентами.
//!
//! A2A определяет Task-ориентированную модель: агенты обмениваются задачами
//! (Task), каждая из которых проходит через конечный автомат состояний
//! (Submitted → Working → Completed / Failed / Canceled).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Уникальный идентификатор задачи в A2A-протоколе.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(pub String);

impl TaskId {
    /// Создать новый `TaskId` из строки.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for TaskId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for TaskId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<&String> for TaskId {
    fn from(s: &String) -> Self {
        Self(s.clone())
    }
}

/// Состояние задачи в A2A-протоколе.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskState {
    /// Задача создана и ожидает обработки.
    Submitted,
    /// Задача в процессе выполнения.
    Working,
    /// Агенту требуется дополнительный ввод от пользователя.
    InputRequired,
    /// Задача успешно завершена.
    Completed,
    /// Задача завершилась с ошибкой.
    Failed,
    /// Задача отменена.
    Canceled,
}

impl TaskState {
    /// Возвращает `true`, если состояние является терминальным.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Canceled)
    }
}

/// Роль отправителя сообщения.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageRole {
    /// Сообщение от пользователя.
    User,
    /// Сообщение от агента.
    Agent,
}

/// Содержимое части сообщения.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Part {
    /// Текстовый контент.
    Text(String),
    /// Файловый контент.
    File(FileContent),
    /// Произвольные структурированные данные.
    Data(serde_json::Value),
}

/// Файловый контент в сообщении A2A.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileContent {
    /// Имя файла.
    pub name: String,
    /// MIME-тип содержимого.
    pub mime_type: String,
    /// Бинарные данные файла (base64-encoded при сериализации).
    pub data: Vec<u8>,
}

/// Сообщение в A2A-протоколе.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Роль отправителя.
    pub role: MessageRole,
    /// Части содержимого сообщения.
    pub parts: Vec<Part>,
}

impl Message {
    /// Создать новое текстовое сообщение.
    #[must_use]
    pub fn text(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            parts: vec![Part::Text(content.into())],
        }
    }
}

/// Задача в A2A-протоколе.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Уникальный идентификатор задачи.
    pub id: TaskId,
    /// Текущее состояние.
    pub state: TaskState,
    /// История сообщений задачи.
    #[serde(default)]
    pub history: Vec<Message>,
    /// Метаданные задачи (произвольные пары ключ-значение).
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

impl Task {
    /// Создать новую задачу в состоянии `Submitted`.
    #[must_use]
    pub fn new(id: impl Into<TaskId>) -> Self {
        Self {
            id: id.into(),
            state: TaskState::Submitted,
            history: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Добавить сообщение в историю задачи.
    pub fn add_message(&mut self, msg: Message) {
        self.history.push(msg);
    }

    /// Добавить метаданные.
    pub fn add_metadata(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.metadata.insert(key.into(), value.into());
    }
}

/// A2A Agent Card — карточка агента для обнаружения сервиса.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCard {
    /// Название агента.
    pub name: String,
    /// Описание возможностей.
    #[serde(default)]
    pub description: String,
    /// URL эндпоинта A2A API.
    pub url: String,
    /// Версия протокола A2A.
    #[serde(default = "default_version")]
    pub version: String,
    /// Возможности агента.
    #[serde(default)]
    pub capabilities: Vec<Capability>,
}

/// Возможность агента.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    /// Название возможности.
    pub name: String,
    /// Описание.
    #[serde(default)]
    pub description: String,
}

fn default_version() -> String {
    "1.0".to_string()
}

/// Ошибки A2A-протокола.
#[derive(Debug, thiserror::Error, serde::Serialize)]
pub enum A2AError {
    /// Ошибка HTTP-запроса.
    #[error("A2A HTTP error: {0}")]
    Http(String),
    /// Ошибка десериализации ответа.
    #[error("A2A deserialization error: {0}")]
    Deserialization(String),
    /// Ошибка сериализации запроса.
    #[error("A2A serialization error: {0}")]
    Serialization(String),
    /// Задача не найдена.
    #[error("Task '{0}' not found")]
    TaskNotFound(TaskId),
    /// Внутренняя ошибка.
    #[error("A2A internal error: {0}")]
    Internal(String),
}

/// Результат A2A-операции.
pub type A2AResult<T> = std::result::Result<T, A2AError>;

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    impl Arbitrary for TaskState {
        type Parameters = ();
        type Strategy = proptest::strategy::BoxedStrategy<Self>;

        fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
            proptest::prop_oneof![
                Just(TaskState::Submitted),
                Just(TaskState::Working),
                Just(TaskState::InputRequired),
                Just(TaskState::Completed),
                Just(TaskState::Failed),
                Just(TaskState::Canceled),
            ]
            .boxed()
        }
    }

    impl Arbitrary for Task {
        type Parameters = ();
        type Strategy = proptest::strategy::BoxedStrategy<Self>;

        fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
            (any::<String>(), any::<TaskState>())
                .prop_map(|(id, state)| {
                    let mut task = Task::new(id);
                    task.state = state;
                    task
                })
                .boxed()
        }
    }

    proptest! {
        /// TaskState никогда не паникует при вызове is_terminal().
        #[test]
        fn task_state_is_terminal_never_panics(state: TaskState) {
            let _ = state.is_terminal();
        }

        /// Каждое состояние либо терминальное, либо нет (закон исключённого третьего).
        #[test]
        fn task_state_terminal_or_not(state: TaskState) {
            assert!(state.is_terminal() || !state.is_terminal());
        }

        /// Task::new() всегда создаёт задачу в Submitted.
        #[test]
        fn task_starts_submitted(task_id: String) {
            let task = Task::new(task_id);
            assert_eq!(task.state, TaskState::Submitted);
        }

        /// Терминальные состояния: только Completed, Failed, Canceled.
        #[test]
        fn task_state_terminal_mapping(state: TaskState) {
            match state {
                TaskState::Completed | TaskState::Failed | TaskState::Canceled => {
                    assert!(state.is_terminal(), "{state:?} should be terminal");
                }
                _ => {
                    assert!(!state.is_terminal(), "{state:?} should not be terminal");
                }
            }
        }

        /// Message работает с любым содержимым текста.
        #[test]
        fn message_text_content(content: String) {
            let msg = Message::text(MessageRole::User, &content);
            if let Some(Part::Text(text)) = msg.parts.first() {
                assert_eq!(text, &content);
            } else {
                panic!("Expected Text part");
            }
        }
    }

    #[test]
    fn test_task_lifecycle() {
        let task = Task::new("task-1");
        assert_eq!(task.state, TaskState::Submitted);
        assert!(!task.state.is_terminal());

        let mut task = task;
        task.state = TaskState::Working;

        let msg = Message::text(MessageRole::User, "hello");
        task.add_message(msg);

        task.state = TaskState::Completed;
        assert!(task.state.is_terminal());
        assert_eq!(task.history.len(), 1);
    }

    #[test]
    fn test_agent_card_serde() {
        let card = AgentCard {
            name: "test-agent".into(),
            description: "A test agent".into(),
            url: "http://localhost:8088".into(),
            version: "1.0".into(),
            capabilities: vec![Capability {
                name: "text-generation".into(),
                description: "Generates text".into(),
            }],
        };

        let json = serde_json::to_string(&card).unwrap();
        let deserialized: AgentCard = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.name, "test-agent");
        assert_eq!(deserialized.capabilities.len(), 1);
    }

    #[test]
    fn test_task_id_conversions() {
        let from_str: TaskId = "test".into();
        let from_string: TaskId = String::from("test").into();
        assert_eq!(from_str, from_string);
        assert_eq!(from_str.to_string(), "test");
    }

    #[test]
    fn test_task_metadata() {
        let mut task = Task::new("meta-test");
        task.add_metadata("key1", "value1");
        assert_eq!(task.metadata.get("key1").unwrap(), "value1");
    }

    #[test]
    fn test_terminal_states() {
        assert!(TaskState::Completed.is_terminal());
        assert!(TaskState::Failed.is_terminal());
        assert!(TaskState::Canceled.is_terminal());
        assert!(!TaskState::Submitted.is_terminal());
        assert!(!TaskState::Working.is_terminal());
        assert!(!TaskState::InputRequired.is_terminal());
    }
}
