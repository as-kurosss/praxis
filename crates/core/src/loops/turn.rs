//! **Turn-based** loop — processes a single request and returns the response.
//!
//! The simplest cycle: one handler invocation inside a graph node.
//! Runs once, returns immediately with the result.

use super::loop_trait::{Context, Loop, LoopResult, elapsed_ms};
use std::time::Instant;

/// Handler function type for turn-based loops.
pub type TurnHandler<I, O> = dyn Fn(I) -> Result<O, String> + Send + Sync;

/// A **Turn-based** loop.
///
/// Wraps a synchronous handler function and executes it once per `execute()` call.
///
/// # Type parameters
/// * `I` — input context type
/// * `O` — output result type
pub struct TurnLoop<I, O> {
    handler: Box<TurnHandler<I, O>>,
}

impl<I, O> TurnLoop<I, O> {
    /// Create a new turn-based loop with the given handler.
    pub fn new(handler: Box<TurnHandler<I, O>>) -> Self {
        Self { handler }
    }
}

#[async_trait::async_trait]
impl<I: Send + 'static, O: Send + 'static> Loop for TurnLoop<I, O> {
    type Context = I;
    type State = ();
    type Output = O;

    async fn execute(
        &self,
        ctx: Context<Self::Context>,
        _state: &mut Self::State,
    ) -> LoopResult<Self::Output> {
        let start = Instant::now();
        match (self.handler)(ctx.input) {
            Ok(output) => LoopResult::success(output, 1, elapsed_ms(&start)),
            Err(err) => LoopResult::failure(err, 1, elapsed_ms(&start)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loops::{CycleType, LoopId, StopCondition};

    /// Helper: create a `TurnLoop` that echoes input.
    fn echo_loop() -> TurnLoop<String, String> {
        TurnLoop::new(Box::new(Ok))
    }

    #[tokio::test]
    async fn test_turn_loop_echo() {
        let loop_impl = echo_loop();
        let ctx = Context::new(
            LoopId::new(),
            CycleType::Turn,
            StopCondition::timeout(std::time::Duration::from_secs(10)),
            "hello".to_string(),
        );
        let mut state = ();

        let result = loop_impl.execute(ctx, &mut state).await;

        assert!(result.is_success());
        assert_eq!(result.output, Some("hello".to_string()));
        assert_eq!(result.iterations, 1);
        assert!(result.duration_ms < 1000);
    }

    #[tokio::test]
    async fn test_turn_loop_transform() {
        let loop_impl = TurnLoop::new(Box::new(|input: String| Ok(input.len())));
        let ctx = Context::new(
            LoopId::new(),
            CycleType::Turn,
            StopCondition::timeout(std::time::Duration::from_secs(10)),
            "test".to_string(),
        );
        let mut state = ();

        let result = loop_impl.execute(ctx, &mut state).await;

        assert!(result.is_success());
        assert_eq!(result.output, Some(4));
    }

    #[tokio::test]
    async fn test_turn_loop_error() {
        let loop_impl = TurnLoop::new(Box::new(|input: String| {
            if input == "fail" {
                Err("processing failed".to_string())
            } else {
                Ok(input.len())
            }
        }));
        let ctx = Context::new(
            LoopId::new(),
            CycleType::Turn,
            StopCondition::timeout(std::time::Duration::from_secs(10)),
            "fail".to_string(),
        );
        let mut state = ();

        let result = loop_impl.execute(ctx, &mut state).await;

        assert!(!result.is_success());
        assert_eq!(
            result.status,
            crate::loops::LoopStatus::Failed("processing failed".into())
        );
        assert!(result.output.is_none());
    }

    #[tokio::test]
    async fn test_turn_loop_preserves_state_across_calls() {
        let loop_impl = echo_loop();
        let mut state = ();

        let r1 = loop_impl
            .execute(
                Context::new(
                    LoopId::new(),
                    CycleType::Turn,
                    StopCondition::max_iterations(1),
                    "a".to_string(),
                ),
                &mut state,
            )
            .await;
        assert_eq!(r1.output, Some("a".to_string()));

        let r2 = loop_impl
            .execute(
                Context::new(
                    LoopId::new(),
                    CycleType::Turn,
                    StopCondition::max_iterations(1),
                    "b".to_string(),
                ),
                &mut state,
            )
            .await;
        assert_eq!(r2.output, Some("b".to_string()));
    }
}
