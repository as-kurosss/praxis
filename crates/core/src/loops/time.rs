//! **Time-based** loop — triggered by a schedule (wrapper).
//!
//! Delegates `execute()` to an inner Turn/Goal loop.
//! The schedule is used by an external scheduler to determine when to trigger.

use super::loop_trait::{Context, Loop, LoopResult};
use std::time::Duration;

/// How a time-based loop is triggered.
#[derive(Debug, Clone)]
pub enum Schedule {
    /// Fixed interval between executions.
    Interval(Duration),
    /// Cron expression for scheduled times.
    Cron(String),
}

/// A **Time-based** loop — wrapper that adds schedule metadata.
///
/// Delegates `execute()` to the inner loop. The schedule is used by an
/// external scheduler to determine when to trigger execution.
pub struct TimeLoop<I> {
    inner: I,
    schedule: Schedule,
}

impl<I> TimeLoop<I> {
    /// Create a new time-based loop wrapping the given inner loop.
    pub fn new(inner: I, schedule: Schedule) -> Self {
        Self { inner, schedule }
    }

    /// Schedule that controls when this loop is triggered.
    pub fn schedule(&self) -> &Schedule {
        &self.schedule
    }
}

#[async_trait::async_trait]
impl<I: Loop + Send + Sync> Loop for TimeLoop<I> {
    type Context = I::Context;
    type State = I::State;
    type Output = I::Output;

    async fn execute(
        &self,
        ctx: Context<Self::Context>,
        state: &mut Self::State,
    ) -> LoopResult<Self::Output> {
        self.inner.execute(ctx, state).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loops::{CycleType, LoopId, StopCondition, TurnLoop};

    fn echo_inner() -> TurnLoop<String, String> {
        TurnLoop::new(Box::new(Ok))
    }

    #[tokio::test]
    async fn test_time_loop_delegates_to_inner() {
        let inner = echo_inner();
        let time_loop = TimeLoop::new(inner, Schedule::Interval(Duration::from_secs(60)));
        let ctx = Context::new(
            LoopId::new(),
            CycleType::Time,
            StopCondition::timeout(Duration::from_secs(10)),
            "hello".to_string(),
        );
        let mut state = ();

        let result = time_loop.execute(ctx, &mut state).await;

        assert!(result.is_success());
        assert_eq!(result.output, Some("hello".to_string()));
        assert_eq!(result.iterations, 1);
    }

    #[tokio::test]
    async fn test_time_loop_with_goal_inner() {
        use crate::loops::GoalLoop;
        use crate::loops::verifier::AlwaysMet;

        let inner = GoalLoop::<u32, String>::new(
            Box::new(|s: &mut u32| {
                *s += 1;
                Ok(())
            }),
            Box::new(AlwaysMet),
        );
        let time_loop = TimeLoop::new(inner, Schedule::Interval(Duration::from_secs(30)));
        let ctx = Context::new(
            LoopId::new(),
            CycleType::Time,
            StopCondition::max_iterations(1),
            (),
        );
        let mut state = 0u32;

        let result = time_loop.execute(ctx, &mut state).await;

        assert!(result.is_success());
        assert_eq!(state, 1);
    }

    #[tokio::test]
    async fn test_time_loop_schedule_access() {
        let inner = echo_inner();
        let schedule = Schedule::Interval(Duration::from_secs(120));
        let time_loop = TimeLoop::new(inner, schedule);

        match time_loop.schedule() {
            Schedule::Interval(d) => assert_eq!(*d, Duration::from_secs(120)),
            _ => panic!("expected Interval"),
        }
    }

    #[test]
    fn test_schedule_cron() {
        let schedule = Schedule::Cron("0 */6 * * *".into());
        match schedule {
            Schedule::Cron(expr) => assert_eq!(expr, "0 */6 * * *"),
            _ => panic!("expected Cron"),
        }
    }
}
