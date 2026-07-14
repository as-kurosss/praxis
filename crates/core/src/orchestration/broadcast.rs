//! **Broadcast** — sends the same input to all agents concurrently and
//! collects all outputs.

use crate::loops::{Context, Loop, LoopResult};
use std::sync::Arc;

/// A loop that broadcasts its input to all child loops concurrently.
///
/// Each child receives the same [`Context`] and starts with an empty state.
/// Outputs are collected into a `Vec<O>`.
pub struct Broadcast<L, C, S, O> {
    agents: Vec<Arc<L>>,
    _phantom: std::marker::PhantomData<(C, S, O)>,
}

impl<L, C, S, O> Broadcast<L, C, S, O>
where
    L: Loop<Context = C, State = S, Output = O> + Send + 'static,
    C: Clone + Send + Sync + 'static,
    S: Default + Send + Sync + 'static,
    O: Send + Sync + 'static,
{
    /// Create a broadcast with the given child loops.
    pub fn new(agents: Vec<L>) -> Self {
        Self {
            agents: agents.into_iter().map(Arc::new).collect(),
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<L, C, S, O> Loop for Broadcast<L, C, S, O>
where
    L: Loop<Context = C, State = S, Output = O> + Send + 'static,
    C: Clone + Send + Sync + 'static,
    S: Default + Send + Sync + 'static,
    O: Send + Sync + 'static,
{
    type Context = C;
    type State = S;
    type Output = Vec<O>;

    async fn execute(
        &self,
        ctx: Context<Self::Context>,
        _state: &mut Self::State,
    ) -> LoopResult<Self::Output> {
        let start = std::time::Instant::now();

        let mut handles = Vec::with_capacity(self.agents.len());
        for agent in &self.agents {
            let ctx = ctx.clone();
            let agent = Arc::clone(agent);
            handles.push(tokio::spawn(async move {
                let mut state = S::default();
                agent.execute(ctx, &mut state).await
            }));
        }

        let mut outputs = Vec::with_capacity(self.agents.len());
        let mut iterations = 0u32;
        let mut any_failed = false;
        let mut error_msg = String::new();

        for handle in handles {
            match handle.await {
                Ok(result) => {
                    iterations = iterations.max(result.iterations);
                    if result.is_success() {
                        if let Some(out) = result.output {
                            outputs.push(out);
                        }
                    } else {
                        any_failed = true;
                        error_msg = match &result.status {
                            crate::loops::LoopStatus::Failed(msg) => msg.clone(),
                            _ => "unknown error".into(),
                        };
                    }
                }
                Err(e) => {
                    any_failed = true;
                    error_msg = format!("task join error: {e}");
                }
            }
        }

        let elapsed = crate::loops::elapsed_ms(&start);
        if any_failed {
            LoopResult::failure(format!("broadcast: {error_msg}"), iterations, elapsed)
        } else {
            LoopResult::success(outputs, iterations, elapsed)
        }
    }
}
