use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::loops::Loop;
use crate::scheduler::schedule::Schedule;
use crate::scheduler::task::{AnyTask, ScheduledTask, Scheduler, TaskId};

/// Error type for persistent scheduler operations.
#[derive(Debug, thiserror::Error)]
pub enum PersistError {
    /// I/O error during save/load.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON serialization error.
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    /// Task factory not found for a stored task.
    #[error("no factory registered for task type: {0}")]
    UnknownTaskType(String),
}

/// Serializable snapshot of a schedule definition.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct StoredTaskDef {
    id: String,
    schedule: StoredSchedule,
    /// User-defined type discriminator for factory recreation.
    task_type: String,
    max_consecutive_failures: u32,
}

/// Serializable version of [`Schedule`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum StoredSchedule {
    IntervalMs(u64),
    Cron(String),
    OnceUnixSecs(u64),
    Recurring {
        start_unix_secs: u64,
        interval_ms: u64,
    },
}

impl From<&Schedule> for StoredSchedule {
    fn from(s: &Schedule) -> Self {
        match s {
            Schedule::Interval(d) => Self::IntervalMs(d.as_millis() as u64),
            Schedule::Cron(expr) => Self::Cron(expr.clone()),
            Schedule::Once(t) => Self::OnceUnixSecs(
                t.duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            ),
            Schedule::Recurring { start, interval } => Self::Recurring {
                start_unix_secs: start
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                interval_ms: interval.as_millis() as u64,
            },
        }
    }
}

/// Factory type for recreating tasks on load.
///
/// A `TaskFactory` is registered for each `task_type` string used in
/// [`PersistentScheduler::register_task_type`]. When loading from disk,
/// the factory is called to produce a [`ScheduledTask`] for the given
/// schedule definition.
pub struct TaskFactory<L: Loop + Send + 'static> {
    inner: Arc<dyn Fn(Schedule) -> ScheduledTask<L> + Send + Sync>,
}

impl<L: Loop + Send + 'static> TaskFactory<L> {
    /// Create a new factory.
    ///
    /// The closure receives the deserialized [`Schedule`] and must
    /// return a fully configured [`ScheduledTask`].
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(Schedule) -> ScheduledTask<L> + Send + Sync + 'static,
    {
        Self {
            inner: Arc::new(f),
        }
    }
}

/// A scheduler with persistent task definitions saved to a JSON file.
///
/// Tasks are defined by type name + schedule. On load, registered
/// [`TaskFactory`]s recreate the tasks from their stored definitions.
pub struct PersistentScheduler {
    /// The underlying runtime scheduler.
    pub inner: Scheduler,
    /// Path to the JSON file storing task definitions.
    store: PathBuf,
    /// Registered factories keyed by task type name.
    factories: Vec<(
        String,
        Arc<dyn Fn(StoredTaskDef) -> Result<Box<dyn AnyTask>, PersistError> + Send + Sync>,
    )>,
    /// In-memory task definitions, synced to disk on [`save`](PersistentScheduler::save).
    stored_defs: Vec<StoredTaskDef>,
}

impl PersistentScheduler {
    /// Create or load a persistent scheduler.
    ///
    /// If `path` exists, task definitions are loaded (but factories must
    /// be registered separately via [`register_task_type`](PersistentScheduler::register_task_type)).
    /// If it does not exist, an empty scheduler is created.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, PersistError> {
        let path = path.into();
        let inner = Scheduler::new();
        let factories = Vec::new();
        let stored_defs = Vec::new();

        Ok(Self {
            inner,
            store: path,
            factories,
            stored_defs,
        })
    }

    /// Register a factory for a named task type.
    ///
    /// The `type_name` must match the `task_type` that was set when
    /// [`add`](PersistentScheduler::add) was called.
    pub fn register_task_type<L: Loop + Send + 'static>(
        &mut self,
        type_name: impl Into<String>,
        factory: TaskFactory<L>,
    ) where
        L::State: Default,
    {
        let type_name = type_name.into();
        let factory = factory.inner;
        self.factories.push((
            type_name,
            Arc::new(move |def: StoredTaskDef| -> Result<Box<dyn AnyTask>, PersistError> {
                let schedule = StoredSchedule::to_schedule(&def.schedule);
                let mut task = (factory)(schedule);
                task.id = TaskId(def.id);
                task.max_consecutive_failures = def.max_consecutive_failures;
                Ok(Box::new(task))
            }),
        ));
    }

    /// Add a typed task and persist immediately.
    pub fn add<L: Loop + Send + 'static>(
        &mut self,
        task_type: impl Into<String>,
        task: ScheduledTask<L>,
    ) where
        L::State: Default,
    {
        let task_type = task_type.into();
        let def = StoredTaskDef {
            id: task.id.to_string(),
            schedule: (&task.schedule).into(),
            task_type,
            max_consecutive_failures: task.max_consecutive_failures,
        };
        self.stored_defs.push(def);
        self.inner.add(task);
        self.save().ok();
    }

    /// Save all task definitions to the JSON file.
    pub fn save(&self) -> Result<(), PersistError> {
        let json = serde_json::to_string_pretty(&self.stored_defs)?;
        std::fs::write(&self.store, json)?;
        Ok(())
    }

    /// Load task definitions from the JSON file.
    ///
    /// Requires that factories for all stored task types have been
    /// registered via [`register_task_type`](PersistentScheduler::register_task_type).
    pub fn load(&mut self) -> Result<(), PersistError> {
        if !self.store.exists() {
            return Ok(());
        }
        let json = std::fs::read_to_string(&self.store)?;
        let defs: Vec<StoredTaskDef> = serde_json::from_str(&json)?;

        // Reset in-memory state before loading
        self.stored_defs.clear();
        self.inner = Scheduler::new();

        for def in &defs {
            let mut found = false;
            for (type_name, factory) in &self.factories {
                if *type_name == def.task_type {
                    let task = factory(def.clone())?;
                    self.inner.add_boxed(task);
                    self.stored_defs.push(def.clone());
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(PersistError::UnknownTaskType(def.task_type.clone()));
            }
        }

        Ok(())
    }

    /// Return the path to the store file.
    pub fn store_path(&self) -> &Path {
        &self.store
    }
}

impl StoredSchedule {
    fn to_schedule(s: &StoredSchedule) -> Schedule {
        match s {
            StoredSchedule::IntervalMs(ms) => {
                Schedule::Interval(Duration::from_millis(*ms))
            }
            StoredSchedule::Cron(e) => Schedule::Cron(e.clone()),
            StoredSchedule::OnceUnixSecs(t) => {
                Schedule::Once(SystemTime::UNIX_EPOCH + Duration::from_secs(*t))
            }
            StoredSchedule::Recurring {
                start_unix_secs,
                interval_ms,
            } => Schedule::Recurring {
                start: SystemTime::UNIX_EPOCH + Duration::from_secs(*start_unix_secs),
                interval: Duration::from_millis(*interval_ms),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc as StdArc;
    use std::time::Duration;
    use tempfile::TempDir;
    use crate::loops::{Context, LoopId, LoopResult, StopCondition};

    struct TestLoop;

    #[async_trait::async_trait]
    impl Loop for TestLoop {
        type Context = String;
        type State = ();
        type Output = String;

        async fn execute(
            &self,
            _ctx: Context<Self::Context>,
            _state: &mut Self::State,
        ) -> LoopResult<Self::Output> {
            LoopResult::success("ok".to_string(), 1, 0)
        }
    }

    fn dummy_ctx() -> Context<String> {
        Context::new(
            LoopId::new(),
            crate::loops::CycleType::Turn,
            StopCondition::max_iterations(1),
            "test".to_string(),
        )
    }

    #[test]
    fn test_persistent_scheduler_create() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("scheduler.json");
        let ps = PersistentScheduler::new(path).unwrap();
        assert!(ps.inner.is_empty());
    }

    #[test]
    fn test_persistent_scheduler_save_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("scheduler.json");

        let mut ps = PersistentScheduler::new(path.clone()).unwrap();

        let counter = StdArc::new(AtomicU32::new(0));
        let c = StdArc::clone(&counter);
        let factory = TaskFactory::new(move |schedule| {
            let c = StdArc::clone(&c);
            let counter = c;
            ScheduledTask::new(
                TaskId::new(),
                schedule,
                TestLoop,
                StdArc::new(move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                    dummy_ctx()
                }),
            )
        });

        ps.register_task_type("test", factory);
        let task = ScheduledTask::new(
            TaskId::new(),
            Schedule::Interval(Duration::from_secs(60)),
            TestLoop,
            StdArc::new(dummy_ctx),
        );
        ps.add("test", task);
        ps.save().unwrap();

        // Load into a fresh scheduler
        let mut ps2 = PersistentScheduler::new(path).unwrap();
        let counter2 = StdArc::new(AtomicU32::new(0));
        let c2 = StdArc::clone(&counter2);
        let factory2 = TaskFactory::new(move |schedule| {
            let c2 = StdArc::clone(&c2);
            let counter = c2;
            ScheduledTask::new(
                TaskId::new(),
                schedule,
                TestLoop,
                StdArc::new(move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                    dummy_ctx()
                }),
            )
        });
        ps2.register_task_type("test", factory2);
        ps2.load().unwrap();
        assert_eq!(ps2.inner.len(), 1, "should load 1 task from disk");
    }
}
