//! # Scheduling Engine
//!
//! A cron-like scheduler for running [`Loop`]s on a schedule.
//!
//! ## Architecture
//!
//! * [`Schedule`] — when to run (interval, cron, once, recurring)
//! * [`ScheduledTask`] — a [`Loop`] bound to a [`Schedule`]
//! * [`Scheduler`] — runtime engine: tick, next_due, add, remove
//! * [`PersistentScheduler`] — scheduler with JSON-backed task definitions
//!
//! ## Example
//!
//! ```rust
//! use praxis_core::scheduler::*;
//! # use praxis_core::loops::{Context, Loop, LoopResult, LoopId, CycleType, StopCondition};
//! # struct MyLoop;
//! # #[async_trait::async_trait]
//! # impl Loop for MyLoop {
//! #     type Context = String; type State = (); type Output = String;
//! #     async fn execute(&self, _ctx: Context<String>, _state: &mut ()) -> LoopResult<String> {
//! #         LoopResult::success("done".into(), 1, 0)
//! #     }
//! # }
//! use std::sync::Arc;
//! use std::time::{Duration, SystemTime};
//!
//! let mut scheduler = Scheduler::new();
//! let task = ScheduledTask::new(
//!     "my-task",
//!     Schedule::Interval(Duration::from_secs(300)),
//!     MyLoop,
//!     Arc::new(|| Context::new(LoopId::new(), CycleType::Turn,
//!         StopCondition::max_iterations(1), "trigger".into())),
//! );
//! scheduler.add(task);
//!
//! // In your event loop:
//! // loop {
//! //     let events = scheduler.tick(SystemTime::now()).await;
//! //     if let Some(next) = scheduler.next_due(SystemTime::now()) {
//! //         tokio::time::sleep_until(next.into()).await;
//! //     }
//! // }
//! ```

mod persist;
mod schedule;
mod task;

pub use persist::{PersistError, PersistentScheduler, TaskFactory};
pub use schedule::Schedule;
pub use task::{AnyTask, ScheduledTask, Scheduler, SchedulerEvent, TaskId};
