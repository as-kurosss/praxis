//! **A2A Server** — Axum-based HTTP сервер для A2A-протокола.
//!
//! Предоставляет REST API для взаимодействия с агентом по протоколу A2A:
//! * `GET  /.well-known/agent-card` — карточка агента
//! * `POST /tasks` — создание задачи
//! * `GET  /tasks/:id` — получение состояния задачи
//! * `POST /tasks/:id/cancel` — отмена задачи
//! * `GET  /tasks/:id/stream` — SSE-стрим изменений состояния

use super::types::*;
use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Внутреннее хранилище задач A2A-сервера.
#[derive(Debug, Clone)]
pub struct TaskStore {
    tasks: Arc<Mutex<HashMap<TaskId, Task>>>,
}

impl TaskStore {
    /// Создать новое пустое хранилище задач.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Сохранить задачу.
    pub async fn store(&self, task: Task) {
        let mut tasks = self.tasks.lock().await;
        tasks.insert(task.id.clone(), task);
    }

    /// Получить задачу по ID.
    pub async fn get(&self, id: &TaskId) -> Option<Task> {
        let tasks = self.tasks.lock().await;
        tasks.get(id).cloned()
    }

    /// Обновить состояние задачи.
    pub async fn update_state(&self, id: &TaskId, state: TaskState) -> A2AResult<()> {
        let mut tasks = self.tasks.lock().await;
        let task = tasks
            .get_mut(id)
            .ok_or_else(|| A2AError::TaskNotFound(id.clone()))?;
        task.state = state;
        Ok(())
    }
}

impl Default for TaskStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Внутреннее состояние A2A-сервера для передачи в обработчики.
#[derive(Debug, Clone)]
pub struct ServerState {
    /// Карточка агента.
    pub agent_card: AgentCard,
    /// Хранилище задач.
    pub task_store: TaskStore,
}

/// A2A-сервер с Axum-роутером.
#[derive(Debug, Clone)]
pub struct A2AServer {
    /// Внутреннее состояние.
    state: Arc<ServerState>,
}

impl A2AServer {
    /// Создать новый A2A-сервер с указанной карточкой агента.
    #[must_use]
    pub fn new(agent_card: AgentCard) -> Self {
        Self {
            state: Arc::new(ServerState {
                agent_card,
                task_store: TaskStore::new(),
            }),
        }
    }

    /// Получить хранилище задач (для внешних манипуляций).
    #[must_use]
    pub fn task_store(&self) -> &TaskStore {
        &self.state.task_store
    }

    /// Построить Axum-роутер с A2A-эндпоинтами.
    ///
    /// SSE-стрим (`GET /tasks/:id/stream`) требует включения `axum` feature `ss`
    /// и отключён по умолчанию.
    pub fn into_router(self) -> Router<Arc<ServerState>> {
        Router::new()
            .route("/.well-known/agent-card", get(handle_agent_card))
            .route("/tasks", post(handle_create_task))
            .route("/tasks/{id}", get(handle_get_task))
            .route("/tasks/{id}/cancel", post(handle_cancel_task))
            .with_state(self.state)
    }
}

// ── Обработчики (standalone-функции) ──────────────────────────────────────

/// GET /.well-known/agent-card
async fn handle_agent_card(State(state): State<Arc<ServerState>>) -> Json<AgentCard> {
    Json(state.agent_card.clone())
}

/// POST /tasks — создание новой задачи.
async fn handle_create_task(
    State(state): State<Arc<ServerState>>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<Task>, (StatusCode, Json<A2AError>)> {
    let task_id = payload
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("default");

    let task = Task::new(TaskId::new(task_id));
    state.task_store.store(task.clone()).await;

    Ok(Json(task))
}

/// GET /tasks/:id — получение задачи.
async fn handle_get_task(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<Task>, (StatusCode, Json<A2AError>)> {
    let task_id = TaskId::new(&id);
    state
        .task_store
        .get(&task_id)
        .await
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, Json(A2AError::TaskNotFound(task_id))))
}

/// POST /tasks/:id/cancel — отмена задачи.
async fn handle_cancel_task(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<Task>, (StatusCode, Json<A2AError>)> {
    let task_id = TaskId::new(&id);

    let task = state.task_store.get(&task_id).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(A2AError::TaskNotFound(task_id.clone())),
        )
    })?;

    if task.state.is_terminal() {
        return Ok(Json(task));
    }

    state
        .task_store
        .update_state(&task_id, TaskState::Canceled)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e)))?;

    let updated = state.task_store.get(&task_id).await.unwrap();
    Ok(Json(updated))
}
