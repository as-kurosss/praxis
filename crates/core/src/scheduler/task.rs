use std::sync::Arc;
use std::time::SystemTime;

use crate::loops::{Context, Loop, LoopResult};
use crate::scheduler::schedule::Schedule;

/// Unique identifier for a scheduled task.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

impl From<&str> for TaskId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A scheduled task that wraps a [`Loop`] with a [`Schedule`].
pub struct ScheduledTask<L: Loop> {
    /// Unique task identifier.
    pub id: TaskId,
    /// When to run.
    pub schedule: Schedule,
    /// The loop/agent to execute.
    pub agent: L,
    /// Factory function that creates the context for each run.
    pub context_fn: Arc<dyn Fn() -> Context<L::Context> + Send + Sync>,
    /// Maximum consecutive failures before the task is disabled.
    pub max_consecutive_failures: u32,
    /// Last run time (updated after each execution).
    pub last_run: Option<SystemTime>,
    /// Current consecutive failure count.
    pub consecutive_failures: u32,
    /// Whether the task is disabled due to too many failures.
    pub disabled: bool,
}

impl<L: Loop> ScheduledTask<L> {
    /// Create a new scheduled task.
    pub fn new(
        id: impl Into<TaskId>,
        schedule: Schedule,
        agent: L,
        context_fn: Arc<dyn Fn() -> Context<L::Context> + Send + Sync>,
    ) -> Self {
        Self {
            id: id.into(),
            schedule,
            agent,
            context_fn,
            max_consecutive_failures: 5,
            last_run: None,
            consecutive_failures: 0,
            disabled: false,
        }
    }

    /// Execute the task once with a fresh context.
    pub async fn run_once(&mut self) -> LoopResult<L::Output>
    where
        L::State: Default,
    {
        let ctx = (self.context_fn)();
        let mut state = L::State::default();
        let result = self.agent.execute(ctx, &mut state).await;

        self.last_run = Some(SystemTime::now());
        if result.is_success() {
            self.consecutive_failures = 0;
        } else {
            self.consecutive_failures += 1;
            if self.consecutive_failures >= self.max_consecutive_failures {
                self.disabled = true;
            }
        }

        result
    }
}

/// Type-erased task for heterogeneous storage in [`Scheduler`].
#[async_trait::async_trait]
pub trait AnyTask: Send {
    /// Return the next scheduled run time after `now`.
    fn next_run_after(&self, now: SystemTime) -> Option<SystemTime>;
    /// Return the task ID.
    fn task_id(&self) -> &TaskId;
    /// Execute the task if it is due (i.e. `next_run_after` ≤ `now`).
    ///
    /// Returns `Some(duration_ms)` if the task was executed, `None` otherwise.
    async fn execute_if_due(&mut self, now: SystemTime) -> Option<u64>;
    /// Whether the task is disabled.
    fn is_disabled(&self) -> bool;
}

#[async_trait::async_trait]
impl<L: Loop + Send + 'static> AnyTask for ScheduledTask<L>
where
    L::State: Default,
{
    fn next_run_after(&self, now: SystemTime) -> Option<SystemTime> {
        if self.disabled {
            return None;
        }
        if self.last_run.is_none() {
            // First run: interval tasks run immediately; others compute from now
            match &self.schedule {
                Schedule::Interval(_) => return Some(now),
                _ => return self.schedule.next_after(now),
            }
        }
        // Subsequent runs: compute next after the last run time
        let last = self.last_run.unwrap();
        match &self.schedule {
            Schedule::Once(_) => None,
            _ => self.schedule.next_after(last),
        }
    }

    fn task_id(&self) -> &TaskId {
        &self.id
    }

    async fn execute_if_due(&mut self, now: SystemTime) -> Option<u64> {
        if self.disabled {
            return None;
        }
        let next = self.next_run_after(now);
        match next {
            Some(t) if t <= now => {
                let start = std::time::Instant::now();
                self.run_once().await;
                let elapsed = start.elapsed().as_millis() as u64;
                Some(elapsed)
            }
            _ => None,
        }
    }

    fn is_disabled(&self) -> bool {
        self.disabled
    }
}

/// Event produced by [`Scheduler::tick`].
#[derive(Debug, Clone)]
pub enum SchedulerEvent {
    /// A task was executed successfully.
    TaskCompleted { task_id: TaskId, duration_ms: u64 },
    /// A task failed (but will be retried).
    TaskFailed { task_id: TaskId, error: String },
    /// A task was disabled due to too many consecutive failures.
    TaskDisabled {
        task_id: TaskId,
        consecutive_failures: u32,
    },
}

/// Runtime engine that manages and executes scheduled tasks.
pub struct Scheduler {
    tasks: Vec<Box<dyn AnyTask>>,
}

impl Scheduler {
    /// Create an empty scheduler.
    pub fn new() -> Self {
        Self { tasks: Vec::new() }
    }

    /// Add a typed task to the scheduler.
    pub fn add<L: Loop + Send + 'static>(&mut self, task: ScheduledTask<L>)
    where
        L::State: Default,
    {
        self.tasks.push(Box::new(task));
    }

    /// Add a type-erased task directly.
    pub fn add_boxed(&mut self, task: Box<dyn AnyTask>) {
        self.tasks.push(task);
    }

    /// Run all tasks that are due at `now`.
    ///
    /// Returns a list of events describing what happened.
    pub async fn tick(&mut self, now: SystemTime) -> Vec<SchedulerEvent> {
        let mut events = Vec::new();
        for task in &mut self.tasks {
            let id = task.task_id().clone();
            if task.is_disabled() {
                continue;
            }
            if let Some(duration_ms) = task.execute_if_due(now).await {
                events.push(SchedulerEvent::TaskCompleted {
                    task_id: id,
                    duration_ms,
                });
            }
        }
        events
    }

    /// Return the next due time across all tasks.
    ///
    /// Useful for determining how long to `sleep` before the next [`tick`](Scheduler::tick).
    pub fn next_due(&self, now: SystemTime) -> Option<SystemTime> {
        self.tasks
            .iter()
            .filter_map(|t| t.next_run_after(now))
            .min()
    }

    /// Remove a task by ID.
    pub fn remove(&mut self, task_id: &TaskId) -> bool {
        let len_before = self.tasks.len();
        self.tasks.retain(|t| t.task_id() != task_id);
        self.tasks.len() < len_before
    }

    /// Return the number of registered tasks.
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    /// Returns `true` if no tasks are registered.
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loops::{Context, CycleType, LoopId, LoopResult, StopCondition};
    use std::sync::Arc as StdArc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    /// A mock loop that increments a counter each time it runs.
    struct CounterLoop {
        counter: StdArc<AtomicU32>,
    }

    #[async_trait::async_trait]
    impl Loop for CounterLoop {
        type Context = String;
        type State = ();
        type Output = String;

        async fn execute(
            &self,
            _ctx: Context<Self::Context>,
            _state: &mut Self::State,
        ) -> LoopResult<Self::Output> {
            self.counter.fetch_add(1, Ordering::SeqCst);
            LoopResult::success("ok".to_string(), 1, 0)
        }
    }

    fn dummy_context() -> Context<String> {
        Context::new(
            LoopId::new(),
            CycleType::Turn,
            StopCondition::max_iterations(1),
            "test".to_string(),
        )
    }

    #[tokio::test]
    async fn test_scheduler_add_and_tick() {
        let counter = StdArc::new(AtomicU32::new(0));
        let task = ScheduledTask::new(
            TaskId::new(),
            Schedule::Interval(Duration::from_millis(10)),
            CounterLoop {
                counter: StdArc::clone(&counter),
            },
            StdArc::new(|| dummy_context()),
        );

        let mut scheduler = Scheduler::new();
        scheduler.add(task);

        // Tick: should execute all due tasks
        let events = scheduler.tick(SystemTime::now()).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert!(!events.is_empty());

        // Tick again immediately — next run is 10ms later, so nothing should execute
        let _events = scheduler.tick(SystemTime::now()).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        // "now" can be slightly later, but the interval is 10ms so it won't be due
        // unless more than 10ms passed — the test is fast enough
    }

    #[tokio::test]
    async fn test_scheduler_remove() {
        let counter = StdArc::new(AtomicU32::new(0));
        let task = ScheduledTask::new(
            TaskId::new(),
            Schedule::Interval(Duration::from_secs(1)),
            CounterLoop {
                counter: StdArc::clone(&counter),
            },
            StdArc::new(|| dummy_context()),
        );

        let mut scheduler = Scheduler::new();
        let id = task.id.clone();
        scheduler.add(task);

        assert_eq!(scheduler.len(), 1);
        assert!(scheduler.remove(&id));
        assert_eq!(scheduler.len(), 0);
    }

    #[tokio::test]
    async fn test_scheduler_empty_tick() {
        let mut scheduler = Scheduler::new();
        let events = scheduler.tick(SystemTime::now()).await;
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn test_scheduled_task_disabled_after_failures() {
        use crate::loops::{CycleType, StopCondition};

        struct FailingLoop;

        #[async_trait::async_trait]
        impl Loop for FailingLoop {
            type Context = String;
            type State = ();
            type Output = String;

            async fn execute(
                &self,
                _ctx: Context<Self::Context>,
                _state: &mut Self::State,
            ) -> LoopResult<Self::Output> {
                LoopResult::<String>::failure(String::from("always fails"), 1, 0)
            }
        }

        let mut task = ScheduledTask::new(
            TaskId::new(),
            Schedule::Interval(Duration::from_secs(1)),
            FailingLoop,
            StdArc::new(|| {
                Context::new(
                    LoopId::new(),
                    CycleType::Turn,
                    StopCondition::max_iterations(1),
                    "test".to_string(),
                )
            }),
        );
        task.max_consecutive_failures = 3;

        // Run a few times until disabled
        for _ in 0..3 {
            task.run_once().await;
        }

        assert!(task.disabled);
        assert_eq!(task.consecutive_failures, 3);
    }
}
