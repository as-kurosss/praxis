//! **A2A Client** — HTTP-клиент для взаимодействия по протоколу A2A.
//!
//! Позволяет агенту обнаруживать другие агенты через Agent Card
//! и отправлять им задачи.

use super::types::*;

/// HTTP-клиент для A2A-протокола.
#[derive(Debug, Clone)]
pub struct A2AClient {
    /// Внутренний HTTP-клиент reqwest.
    inner: reqwest::Client,
}

impl A2AClient {
    /// Создать новый A2A-клиент.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: reqwest::Client::new(),
        }
    }

    /// Создать клиент с кастомным reqwest-клиентом.
    #[must_use]
    pub fn with_client(inner: reqwest::Client) -> Self {
        Self { inner }
    }

    /// Получить Agent Card агента по URL.
    ///
    /// # Arguments
    /// * `base_url` — базовый URL агента (например, `http://localhost:8088`)
    ///
    /// # Errors
    /// Возвращает `A2AError::Http` при сетевой ошибке или
    /// `A2AError::Deserialization` при некорректном ответе.
    pub async fn fetch_card(&self, base_url: &str) -> A2AResult<AgentCard> {
        let url = format!("{}/.well-known/agent-card", base_url.trim_end_matches('/'));
        let resp = self
            .inner
            .get(&url)
            .send()
            .await
            .map_err(|e| A2AError::Http(format!("{e}")))?;

        resp.json::<AgentCard>()
            .await
            .map_err(|e| A2AError::Deserialization(format!("{e}")))
    }

    /// Создать новую задачу на агенте.
    ///
    /// # Arguments
    /// * `base_url` — базовый URL агента
    /// * `task_id` — идентификатор задачи
    ///
    /// # Errors
    /// Возвращает `A2AError::Http` при сетевой ошибке или
    /// `A2AError::Deserialization` при некорректном ответе.
    pub async fn create_task(&self, base_url: &str, task_id: impl Into<TaskId>) -> A2AResult<Task> {
        let url = format!("{}/tasks", base_url.trim_end_matches('/'));
        let task_id = task_id.into();

        let body = serde_json::json!({ "id": task_id.to_string() });

        let resp = self
            .inner
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| A2AError::Http(format!("{e}")))?;

        resp.json::<Task>()
            .await
            .map_err(|e| A2AError::Deserialization(format!("{e}")))
    }

    /// Получить состояние задачи.
    ///
    /// # Arguments
    /// * `base_url` — базовый URL агента
    /// * `task_id` — идентификатор задачи
    ///
    /// # Errors
    /// Возвращает `A2AError::TaskNotFound` если задача не найдена (404),
    /// или другую ошибку протокола.
    pub async fn get_task(&self, base_url: &str, task_id: &TaskId) -> A2AResult<Task> {
        let url = format!("{}/tasks/{}", base_url.trim_end_matches('/'), task_id);

        let resp = self
            .inner
            .get(&url)
            .send()
            .await
            .map_err(|e| A2AError::Http(format!("{e}")))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(A2AError::TaskNotFound(task_id.clone()));
        }

        resp.json::<Task>()
            .await
            .map_err(|e| A2AError::Deserialization(format!("{e}")))
    }

    /// Отменить задачу.
    ///
    /// # Arguments
    /// * `base_url` — базовый URL агента
    /// * `task_id` — идентификатор задачи
    ///
    /// # Errors
    /// Возвращает `A2AError::TaskNotFound` если задача не найдена.
    pub async fn cancel_task(&self, base_url: &str, task_id: &TaskId) -> A2AResult<Task> {
        let url = format!(
            "{}/tasks/{}/cancel",
            base_url.trim_end_matches('/'),
            task_id
        );

        let resp = self
            .inner
            .post(&url)
            .send()
            .await
            .map_err(|e| A2AError::Http(format!("{e}")))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(A2AError::TaskNotFound(task_id.clone()));
        }

        resp.json::<Task>()
            .await
            .map_err(|e| A2AError::Deserialization(format!("{e}")))
    }
}

impl Default for A2AClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_fetch_card() {
        let mock_server = MockServer::start().await;

        let card = AgentCard {
            name: "test-agent".into(),
            description: "Test".into(),
            url: mock_server.uri(),
            version: "1.0".into(),
            capabilities: vec![],
        };

        Mock::given(method("GET"))
            .and(path("/.well-known/agent-card"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&card))
            .mount(&mock_server)
            .await;

        let client = A2AClient::new();
        let fetched = client.fetch_card(&mock_server.uri()).await.unwrap();

        assert_eq!(fetched.name, "test-agent");
        assert_eq!(fetched.url, mock_server.uri());
    }

    #[tokio::test]
    async fn test_create_and_get_task() {
        let mock_server = MockServer::start().await;

        let task = Task::new("task-1");

        // Mock POST /tasks
        let create_json = serde_json::to_value(&task).unwrap();
        Mock::given(method("POST"))
            .and(path("/tasks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&create_json))
            .mount(&mock_server)
            .await;

        // Mock GET /tasks/task-1
        let get_json = serde_json::to_value(&task).unwrap();
        Mock::given(method("GET"))
            .and(path("/tasks/task-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&get_json))
            .mount(&mock_server)
            .await;

        let client = A2AClient::new();

        let created = client
            .create_task(&mock_server.uri(), TaskId::new("task-1"))
            .await
            .unwrap();
        assert_eq!(created.id.to_string(), "task-1");

        let fetched = client
            .get_task(&mock_server.uri(), &TaskId::new("task-1"))
            .await
            .unwrap();
        assert_eq!(fetched.id.to_string(), "task-1");
    }

    #[tokio::test]
    async fn test_task_not_found() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/tasks/nonexistent"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&mock_server)
            .await;

        let client = A2AClient::new();
        let result = client
            .get_task(&mock_server.uri(), &TaskId::new("nonexistent"))
            .await;

        assert!(matches!(result, Err(A2AError::TaskNotFound(_))));
    }
}
