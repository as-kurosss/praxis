use super::types::{CycleType, LoopId, LoopStatus, StopCondition, StopReason};
use std::time::Instant;

/// Execution context passed into a loop.
///
/// Contains the loop identity, cycle classification, hard limits,
/// and the domain-specific input payload.
#[derive(Debug, Clone)]
pub struct Context<C> {
    /// Unique loop identifier.
    pub id: LoopId,
    /// The cycle type classification.
    pub cycle_type: CycleType,
    /// Hard stop limits for this execution.
    pub stop_condition: StopCondition,
    /// Domain-specific input payload.
    pub input: C,
}

impl<C> Context<C> {
    /// Create a new execution context.
    pub fn new(id: LoopId, cycle_type: CycleType, stop_condition: StopCondition, input: C) -> Self {
        Self {
            id,
            cycle_type,
            stop_condition,
            input,
        }
    }
}

/// Result produced by a single loop execution.
#[derive(Debug, Clone)]
pub struct LoopResult<O> {
    /// Domain-specific output payload. `None` when the loop failed.
    pub output: Option<O>,
    /// Final status after execution.
    pub status: LoopStatus,
    /// Number of iterations executed.
    pub iterations: u32,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
}

impl<O> LoopResult<O> {
    /// Create a new successful loop result.
    pub fn success(output: O, iterations: u32, duration_ms: u64) -> Self {
        Self {
            output: Some(output),
            status: LoopStatus::Completed(StopReason::Complete),
            iterations,
            duration_ms,
        }
    }

    /// Create a new failed loop result.
    pub fn failure(error: impl Into<String>, iterations: u32, duration_ms: u64) -> Self {
        Self {
            output: None,
            status: LoopStatus::Failed(error.into()),
            iterations,
            duration_ms,
        }
    }

    /// Returns `true` if the loop completed with a success stop reason.
    pub fn is_success(&self) -> bool {
        matches!(
            &self.status,
            LoopStatus::Completed(StopReason::Complete | StopReason::GoalMet)
        )
    }
}

/// Convert `Instant::elapsed()` to milliseconds as `u64`, saturating at `u64::MAX`.
#[must_use]
pub fn elapsed_ms(start: &Instant) -> u64 {
    start.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}

/// The core **Loop** trait.
///
/// Every orchestration cycle is represented as an implementation of this trait.
/// The trait uses associated types so that each concrete loop defines its own
/// input context, internal state, and output types.
#[async_trait::async_trait]
pub trait Loop: Send + Sync {
    /// Input context type (e.g. a prompt, a command, an event).
    type Context: Send + 'static;
    /// Internal state type (must be `Serialize + Deserialize` for Goal-based).
    type State: Send + 'static;
    /// Output result type.
    type Output: Send + 'static;

    /// Execute the loop with the given context and mutable state.
    ///
    /// For **Turn-based** loops this runs once and returns.
    /// For **Goal-based** loops this iterates until the verifier confirms
    /// or a stop condition is hit.
    async fn execute(
        &self,
        ctx: Context<Self::Context>,
        state: &mut Self::State,
    ) -> LoopResult<Self::Output>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A mock loop for testing the trait contract.
    struct MockLoop;

    #[async_trait::async_trait]
    impl Loop for MockLoop {
        type Context = String;
        type State = u32;
        type Output = String;

        async fn execute(
            &self,
            ctx: Context<Self::Context>,
            state: &mut Self::State,
        ) -> LoopResult<Self::Output> {
            *state += 1;
            LoopResult::success(format!("processed: {}", ctx.input), *state, 42)
        }
    }

    #[tokio::test]
    async fn test_loop_trait_baseline() {
        let ctx = Context::new(
            LoopId::new(),
            CycleType::Turn,
            StopCondition::timeout(Duration::from_secs(10)),
            "hello".to_string(),
        );
        let mut state = 0u32;
        let loop_impl = MockLoop;

        let result = loop_impl.execute(ctx, &mut state).await;

        assert_eq!(result.iterations, 1);
        assert!(result.is_success());
        assert_eq!(result.output, Some("processed: hello".to_string()));
        assert!(result.duration_ms > 0);
    }

    #[tokio::test]
    async fn test_loop_state_mutation() {
        let ctx = Context::new(
            LoopId::new(),
            CycleType::Goal,
            StopCondition::max_iterations(5),
            "test".to_string(),
        );
        let mut state = 0u32;
        let loop_impl = MockLoop;

        let _r1 = loop_impl.execute(ctx, &mut state).await;
        assert_eq!(state, 1);

        let ctx2 = Context::new(
            LoopId::new(),
            CycleType::Goal,
            StopCondition::max_iterations(5),
            "again".to_string(),
        );
        let _r2 = loop_impl.execute(ctx2, &mut state).await;
        assert_eq!(state, 2);
    }

    #[test]
    fn test_context_new() {
        let ctx = Context::new(
            LoopId::default(),
            CycleType::Turn,
            StopCondition::max_iterations(1),
            42,
        );
        assert_eq!(ctx.input, 42);
        assert_eq!(ctx.cycle_type, CycleType::Turn);
    }

    #[test]
    fn test_loop_result_success() {
        let r = LoopResult::<&str>::success("ok", 1, 10);
        assert!(r.is_success());
        assert_eq!(r.output, Some("ok"));
    }

    #[test]
    fn test_loop_result_failure() {
        let r = LoopResult::<&str>::failure("timeout", 3, 5000);
        assert!(!r.is_success());
        assert!(r.output.is_none());
    }
}
