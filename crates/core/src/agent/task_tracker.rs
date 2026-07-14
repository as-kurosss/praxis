//! **TaskTracker** — track, poll, and cancel background agent tasks.
//!
//! Allows an agent to be spawned in the background and its progress
//! monitored via status polling and cancellation.

use crate::loops::{Context, Loop};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Unique identifier for a background task.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TaskId(pub String);

impl TaskId {
    /// Create a new unique task ID.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Status of a background task.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TaskStatus {
    /// Task is running.
    Running,
    /// Task completed successfully.
    Completed,
    /// Task was cancelled.
    Cancelled,
    /// Task failed with an error.
    Failed(String),
}

/// Summary of a background task's state.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskSummary {
    /// Task identifier.
    pub id: TaskId,
    /// Current status.
    pub status: TaskStatus,
    /// Elapsed time in milliseconds.
    pub elapsed_ms: u64,
    /// Number of iterations executed.
    pub iterations: u32,
    /// Partial output so far (if any).
    pub output: Option<String>,
    /// When the task was created.
    pub created_at: String,
}

/// A handle to a running background task.
///
/// Provides methods to check status, poll for results, and cancel.
#[derive(Debug, Clone)]
pub struct TaskHandle {
    id: TaskId,
    cancelled: Arc<AtomicBool>,
    status: Arc<Mutex<TaskStatus>>,
    output: Arc<Mutex<Option<String>>>,
    iterations: Arc<Mutex<u32>>,
    created_at: String,
    start_time: Instant,
}

impl TaskHandle {
    fn new(id: TaskId) -> Self {
        Self {
            id,
            cancelled: Arc::new(AtomicBool::new(false)),
            status: Arc::new(Mutex::new(TaskStatus::Running)),
            output: Arc::new(Mutex::new(None)),
            iterations: Arc::new(Mutex::new(0)),
            created_at: crate::registry::timestamp(),
            start_time: Instant::now(),
        }
    }

    /// Returns the task ID.
    #[must_use]
    pub fn id(&self) -> &TaskId {
        &self.id
    }

    /// Returns `true` if the task was cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Request cancellation of this task.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Returns the current status without waiting.
    pub async fn status(&self) -> TaskStatus {
        self.status.lock().await.clone()
    }

    /// Returns a summary of the task state.
    pub async fn summary(&self) -> TaskSummary {
        TaskSummary {
            id: self.id.clone(),
            status: self.status.lock().await.clone(),
            elapsed_ms: self.start_time.elapsed().as_millis() as u64,
            iterations: *self.iterations.lock().await,
            output: self.output.lock().await.clone(),
            created_at: self.created_at.clone(),
        }
    }

    /// Wait for the task to complete with a timeout.
    ///
    /// Polls every 100ms until the task completes or the timeout expires.
    pub async fn wait_for_completion(&self, timeout: Duration) -> TaskSummary {
        let start = Instant::now();
        loop {
            if start.elapsed() >= timeout {
                return self.summary().await;
            }
            let status = self.status.lock().await.clone();
            if status != TaskStatus::Running {
                return self.summary().await;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

/// A tracker for managing background agent tasks.
///
/// Provides a registry of running tasks and methods to spawn, list,
/// poll, and cancel them.
#[derive(Default, Clone)]
pub struct TaskTracker {
    tasks: Arc<Mutex<Vec<TaskHandle>>>,
}

impl TaskTracker {
    /// Create a new empty task tracker.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Register a new task handle.
    pub async fn register(&self, handle: TaskHandle) {
        self.tasks.lock().await.push(handle);
    }

    /// List all tracked tasks with their summaries.
    pub async fn list_tasks(&self) -> Vec<TaskSummary> {
        let tasks = self.tasks.lock().await;
        let mut summaries = Vec::with_capacity(tasks.len());
        for handle in tasks.iter() {
            summaries.push(handle.summary().await);
        }
        summaries
    }

    /// Get a task handle by ID.
    pub async fn get_task(&self, id: &TaskId) -> Option<TaskHandle> {
        let tasks = self.tasks.lock().await;
        tasks.iter().find(|h| h.id() == id).cloned()
    }

    /// Cancel a task by ID.
    pub async fn cancel_task(&self, id: &TaskId) -> Result<(), String> {
        let tasks = self.tasks.lock().await;
        let handle = tasks
            .iter()
            .find(|h| h.id() == id)
            .ok_or_else(|| format!("task '{id}' not found"))?;
        handle.cancel();
        Ok(())
    }

    /// Remove completed tasks older than the given duration.
    pub async fn clean_old_tasks(&self, max_age: Duration) {
        let mut tasks = self.tasks.lock().await;
        let now = Instant::now();
        tasks.retain(|h| {
            // Remove cancelled tasks immediately.
            // Remove completed/stuck tasks older than max_age.
            if h.is_cancelled() {
                return false;
            }
            now.duration_since(h.start_time) < max_age
        });
    }

    /// Spawn an agent in the background and return a [`TaskHandle`].
    ///
    /// The agent executes with isolated state. The task handle can be used
    /// to poll status, get partial output, or cancel execution.
    pub async fn spawn_background<S>(
        &self,
        task_id: TaskId,
        agent: impl Loop<Context = String, State = S, Output = String> + 'static,
        ctx: Context<String>,
        state: S,
    ) -> TaskHandle
    where
        S: Send + 'static,
    {
        let handle = TaskHandle::new(task_id);
        let handle_clone = handle.clone();
        let handle_for_task = handle.clone();

        tokio::spawn(async move {
            let mut state = state;
            let result = agent.execute(ctx, &mut state).await;

            let mut status = handle_for_task.status.lock().await;
            let mut output = handle_for_task.output.lock().await;
            let mut iterations = handle_for_task.iterations.lock().await;

            *iterations = result.iterations;

            if handle_for_task.is_cancelled() {
                *status = TaskStatus::Cancelled;
            } else if result.is_success() {
                *status = TaskStatus::Completed;
                *output = result.output;
            } else {
                let err = match &result.status {
                    crate::loops::LoopStatus::Failed(msg) => msg.clone(),
                    _ => "task failed".into(),
                };
                *status = TaskStatus::Failed(err);
            }
        });

        self.register(handle_clone).await;
        handle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_task_id_new_is_unique() {
        let id1 = TaskId::new();
        let id2 = TaskId::new();
        assert_ne!(id1, id2);
    }

    #[tokio::test]
    async fn test_task_handle_initial_state() {
        let handle = TaskHandle::new(TaskId::new());
        assert_eq!(handle.status().await, TaskStatus::Running);
        assert!(!handle.is_cancelled());
    }

    #[tokio::test]
    async fn test_task_cancellation() {
        let handle = TaskHandle::new(TaskId::new());
        handle.cancel();
        assert!(handle.is_cancelled());
    }

    #[tokio::test]
    async fn test_task_summary() {
        let handle = TaskHandle::new(TaskId::new());
        let summary = handle.summary().await;
        assert_eq!(summary.status, TaskStatus::Running);
        assert_eq!(summary.iterations, 0);
        assert!(summary.output.is_none());
    }

    #[tokio::test]
    async fn test_task_tracker_register_and_list() {
        let tracker = TaskTracker::new();
        let handle = TaskHandle::new(TaskId::new());
        tracker.register(handle).await;
        let tasks = tracker.list_tasks().await;
        assert_eq!(tasks.len(), 1);
    }

    #[tokio::test]
    async fn test_task_tracker_cancel() {
        let tracker = TaskTracker::new();
        let id = TaskId::new();
        let handle = TaskHandle::new(id.clone());
        tracker.register(handle).await;
        let result = tracker.cancel_task(&id).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_task_tracker_get_nonexistent() {
        let tracker = TaskTracker::new();
        let result = tracker.get_task(&TaskId::new()).await;
        assert!(result.is_none());
    }
}
