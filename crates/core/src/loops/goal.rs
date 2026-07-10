//! **Goal-based** loop — reaches a goal through a series of steps.
//!
//! Iterates until a verifier confirms the goal or a stop condition is met.
//! State MUST implement `serde::Serialize + Deserialize` for suspend/resume.

use super::loop_trait::{Context, Loop, LoopResult, elapsed_ms};
use super::types::{LoopStatus, StopReason};
use super::verifier::{Verdict, Verifier};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::time::Instant;

/// Handler function type for goal-based loops.
pub type GoalHandler<S, E> = dyn Fn(&mut S) -> Result<(), E> + Send + Sync;

/// A **Goal-based** loop.
///
/// Repeatedly executes a handler function, then checks a `Verifier`.
/// Stops when the verifier returns `Met`, or when `max_iterations`/`timeout` is hit.
///
/// # Type parameters
/// * `S` — mutable state type (must be `Clone + Send + Serialize + DeserializeOwned` for suspend/resume)
/// * `E` — error type produced by the handler
pub struct GoalLoop<S, E> {
    handler: Box<GoalHandler<S, E>>,
    verifier: Box<dyn Verifier<S>>,
}

impl<S, E> GoalLoop<S, E> {
    /// Create a new goal-based loop.
    pub fn new(handler: Box<GoalHandler<S, E>>, verifier: Box<dyn Verifier<S>>) -> Self {
        Self { handler, verifier }
    }
}

#[async_trait::async_trait]
impl<S: Clone + Serialize + DeserializeOwned + Send + 'static, E: std::fmt::Debug + Send + 'static>
    Loop for GoalLoop<S, E>
{
    type Context = ();
    type State = S;
    type Output = S;

    async fn execute(
        &self,
        ctx: Context<Self::Context>,
        state: &mut Self::State,
    ) -> LoopResult<Self::Output> {
        let start = Instant::now();
        let max_iter = ctx.stop_condition.max_iterations.unwrap_or(u32::MAX);
        let timeout = ctx.stop_condition.timeout;

        for iteration in 1..=max_iter {
            // Check timeout before iteration
            if let Some(limit) = timeout
                && start.elapsed() >= limit
            {
                let elapsed = elapsed_ms(&start);
                return LoopResult {
                    output: None,
                    status: LoopStatus::Completed(StopReason::Timeout {
                        elapsed_ms: elapsed,
                    }),
                    iterations: iteration,
                    duration_ms: elapsed,
                };
            }

            // Execute handler
            if let Err(e) = (self.handler)(state) {
                return LoopResult::failure(
                    format!("handler error: {e:?}"),
                    iteration,
                    elapsed_ms(&start),
                );
            }

            // Verify goal
            match self.verifier.verify(state) {
                Verdict::Met => {
                    return LoopResult::success(state.clone(), iteration, elapsed_ms(&start));
                }
                Verdict::Error => {
                    return LoopResult::failure(
                        "verifier error".to_string(),
                        iteration,
                        elapsed_ms(&start),
                    );
                }
                Verdict::NotMet => {
                    // Continue to next iteration
                }
            }
        }

        // Max iterations exhausted
        LoopResult {
            output: None,
            status: LoopStatus::Completed(StopReason::MaxIterations { max: max_iter }),
            iterations: max_iter,
            duration_ms: elapsed_ms(&start),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loops::verifier::AlwaysMet;
    use crate::loops::{CycleType, LoopId, StopCondition};
    use serde::{Deserialize, Serialize};
    use std::time::Duration;

    /// A simple counter state for testing.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct Counter {
        value: u32,
    }

    /// Verifier: goal met when counter reaches target.
    struct TargetVerifier {
        target: u32,
    }

    impl Verifier<Counter> for TargetVerifier {
        fn verify(&self, state: &Counter) -> Verdict {
            if state.value >= self.target {
                Verdict::Met
            } else {
                Verdict::NotMet
            }
        }
    }

    #[tokio::test]
    async fn test_goal_loop_single_step() {
        // Goal is already met — should complete in 1 iteration
        let loop_impl = GoalLoop::<Counter, String>::new(
            Box::new(|s: &mut Counter| {
                s.value += 1;
                Ok(())
            }),
            Box::new(TargetVerifier { target: 1 }),
        );
        let ctx = Context::new(
            LoopId::new(),
            CycleType::Goal,
            StopCondition::max_iterations(10),
            (),
        );
        let mut state = Counter { value: 0 };

        let result = loop_impl.execute(ctx, &mut state).await;

        assert!(result.is_success());
        assert_eq!(result.iterations, 1);
    }

    #[tokio::test]
    async fn test_goal_loop_multiple_steps() {
        let loop_impl = GoalLoop::<Counter, String>::new(
            Box::new(|s: &mut Counter| {
                s.value += 1;
                Ok(())
            }),
            Box::new(TargetVerifier { target: 5 }),
        );
        let ctx = Context::new(
            LoopId::new(),
            CycleType::Goal,
            StopCondition::max_iterations(10),
            (),
        );
        let mut state = Counter { value: 0 };

        let result = loop_impl.execute(ctx, &mut state).await;

        assert!(result.is_success());
        assert_eq!(result.iterations, 5);
    }

    #[tokio::test]
    async fn test_goal_loop_max_iterations_exceeded() {
        let loop_impl = GoalLoop::<Counter, String>::new(
            Box::new(|s: &mut Counter| {
                s.value += 1;
                Ok(())
            }),
            Box::new(TargetVerifier { target: 100 }),
        );
        let ctx = Context::new(
            LoopId::new(),
            CycleType::Goal,
            StopCondition::max_iterations(3),
            (),
        );
        let mut state = Counter { value: 0 };

        let result = loop_impl.execute(ctx, &mut state).await;

        assert!(!result.is_success());
        assert_eq!(
            result.status,
            LoopStatus::Completed(StopReason::MaxIterations { max: 3 })
        );
        assert_eq!(result.iterations, 3);
    }

    #[tokio::test]
    async fn test_goal_loop_handler_error() {
        let loop_impl = GoalLoop::<Counter, &str>::new(
            Box::new(|_: &mut Counter| Err("something broke")),
            Box::new(AlwaysMet),
        );
        let ctx = Context::new(
            LoopId::new(),
            CycleType::Goal,
            StopCondition::max_iterations(5),
            (),
        );
        let mut state = Counter { value: 0 };

        let result = loop_impl.execute(ctx, &mut state).await;

        assert!(!result.is_success());
        assert_eq!(
            result.status,
            LoopStatus::Failed("handler error: \"something broke\"".into())
        );
        assert_eq!(result.iterations, 1);
    }

    #[tokio::test]
    async fn test_goal_loop_always_met_verifier() {
        let loop_impl = GoalLoop::<Counter, String>::new(
            Box::new(|s: &mut Counter| {
                s.value += 1;
                Ok(())
            }),
            Box::new(AlwaysMet),
        );
        let ctx = Context::new(
            LoopId::new(),
            CycleType::Goal,
            StopCondition::max_iterations(10),
            (),
        );
        let mut state = Counter { value: 0 };

        let result = loop_impl.execute(ctx, &mut state).await;

        assert!(result.is_success());
        assert_eq!(result.iterations, 1);
    }

    #[tokio::test]
    async fn test_goal_state_serialize_roundtrip() {
        // Execute a GoalLoop, then serialize and deserialize the final state.
        let loop_impl = GoalLoop::<Counter, String>::new(
            Box::new(|s: &mut Counter| {
                s.value += 1;
                Ok(())
            }),
            Box::new(TargetVerifier { target: 3 }),
        );
        let ctx = Context::new(
            LoopId::new(),
            CycleType::Goal,
            StopCondition::max_iterations(10),
            (),
        );
        let mut state = Counter { value: 0 };

        let result = loop_impl.execute(ctx, &mut state).await;

        assert!(result.is_success());
        assert_eq!(result.iterations, 3);

        // Serialize the output state (which is a clone of the final state)
        let output = result.output.unwrap();
        let json = serde_json::to_string(&output).expect("serialize should succeed");

        // Deserialize back
        let deserialized: Counter =
            serde_json::from_str(&json).expect("deserialize should succeed");

        assert_eq!(deserialized, output);
        assert_eq!(deserialized.value, 3);
    }

    #[tokio::test]
    async fn test_goal_state_resume() {
        // Simulate suspend/resume: save partial state, restore, continue.
        let loop_impl = GoalLoop::<Counter, String>::new(
            Box::new(|s: &mut Counter| {
                s.value += 1;
                Ok(())
            }),
            Box::new(TargetVerifier { target: 5 }),
        );

        // First execution: count 0 → 3, interrupted before goal.
        let mut state = Counter { value: 0 };

        // Manually simulate partial progress (as if graph saved state mid-way)
        state.value = 3;

        // Serialize state (suspend)
        let serialized = serde_json::to_string(&state).expect("suspend serialize should succeed");

        // Deserialize state (resume)
        let mut restored: Counter =
            serde_json::from_str(&serialized).expect("resume deserialize should succeed");

        // Continue execution from restored state
        let ctx = Context::new(
            LoopId::new(),
            CycleType::Goal,
            StopCondition::max_iterations(10),
            (),
        );
        let result = loop_impl.execute(ctx, &mut restored).await;

        assert!(result.is_success());
        assert_eq!(result.iterations, 2); // 2 more steps to reach 5
        assert_eq!(restored.value, 5);
    }

    #[tokio::test]
    async fn test_goal_loop_timeout() {
        let loop_impl = GoalLoop::<Counter, String>::new(
            Box::new(|s: &mut Counter| {
                s.value += 1;
                Ok(())
            }),
            Box::new(TargetVerifier { target: 1000 }),
        );
        let ctx = Context::new(
            LoopId::new(),
            CycleType::Goal,
            StopCondition::new(None, Some(Duration::ZERO)),
            (),
        );
        let mut state = Counter { value: 0 };

        let result = loop_impl.execute(ctx, &mut state).await;

        assert!(!result.is_success());
        assert!(result.duration_ms < 1000);
    }
}
