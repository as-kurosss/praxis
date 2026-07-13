//! **Router** — routes execution to a specific agent based on a routing
//! function that inspects the context.

use crate::loops::{Context, Loop, LoopResult};
use std::sync::Arc;

/// A loop that routes execution to one of its child agents based on a
/// routing function.
///
/// The routing function receives a reference to the context and returns
/// the index of the agent to execute.
///
/// # Type parameters
/// * `L` — the inner loop type
/// * `C` — context type
/// * `S` — state type
/// * `O` — output type
pub struct Router<L, C, S, O> {
    agents: Vec<L>,
    router: Arc<dyn Fn(&C) -> usize + Send + Sync>,
    _phantom: std::marker::PhantomData<(C, S, O)>,
}

impl<L, C, S, O> Router<L, C, S, O>
where
    L: Loop<Context = C, State = S, Output = O> + Send + 'static,
    C: Send + Sync + 'static,
    S: Default + Send + Sync + 'static,
    O: Send + Sync + 'static,
{
    /// Create a new router.
    ///
    /// * `agents` — list of possible agents
    /// * `router` — function that returns the index of the agent to run
    ///   (based on the context). The index must be valid for `agents.len()`.
    pub fn new(agents: Vec<L>, router: Arc<dyn Fn(&C) -> usize + Send + Sync>) -> Self {
        Self {
            agents,
            router,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<L, C, S, O> Loop for Router<L, C, S, O>
where
    L: Loop<Context = C, State = S, Output = O> + Send + 'static,
    C: Send + Sync + 'static,
    S: Default + Send + Sync + 'static,
    O: Send + Sync + 'static,
{
    type Context = C;
    type State = S;
    type Output = O;

    async fn execute(
        &self,
        ctx: Context<Self::Context>,
        _state: &mut Self::State,
    ) -> LoopResult<Self::Output> {
        let start = std::time::Instant::now();
        let idx = (self.router)(&ctx.input);

        if idx >= self.agents.len() {
            return LoopResult::failure(
                format!(
                    "router index {idx} out of bounds (agents: {})",
                    self.agents.len()
                ),
                1,
                crate::loops::elapsed_ms(&start),
            );
        }

        let mut state = S::default();
        let result = self.agents[idx].execute(ctx, &mut state).await;
        let elapsed = crate::loops::elapsed_ms(&start);

        if result.is_success() {
            if let Some(output) = result.output {
                LoopResult::success(output, result.iterations, elapsed)
            } else {
                LoopResult::failure(
                    String::from("router: no output from agent"),
                    result.iterations,
                    elapsed,
                )
            }
        } else {
            let msg = match &result.status {
                crate::loops::LoopStatus::Failed(msg) => msg.clone(),
                _ => format!("router agent {idx} failed"),
            };
            LoopResult::failure(msg, result.iterations, elapsed)
        }
    }
}
