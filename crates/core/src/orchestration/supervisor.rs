//! **Supervisor** — a supervisor agent delegates tasks to worker agents and
//! aggregates their outputs.

use crate::loops::{Context, Loop, LoopResult};
use std::sync::Arc;

/// A supervisor-worker orchestration pattern.
///
/// The supervisor agent receives the task, decides how to split it, and
/// delegates subtasks to worker agents. Results from workers are collected
/// and returned.
///
/// # Type parameters
/// * `L` — the inner loop type used for both supervisor and workers
/// * `C` — context type (must be `Clone`)
/// * `S` — state type
/// * `O` — output type
pub struct Supervisor<L, C, S, O> {
    supervisor: Arc<L>,
    workers: Vec<Arc<L>>,
    worker_count: usize,
    _phantom: std::marker::PhantomData<(C, S, O)>,
}

impl<L, C, S, O> Supervisor<L, C, S, O>
where
    L: Loop<Context = C, State = S, Output = O> + Send + Sync + 'static,
    C: Clone + Send + Sync + 'static,
    S: Default + Send + Sync + 'static,
    O: Default + Send + Sync + 'static,
{
    /// Create a new supervisor with one supervisor agent and multiple workers.
    ///
    /// * `supervisor` — the supervisor agent (receives the full task)
    /// * `workers` — worker agents (receive subtasks)
    pub fn new(supervisor: L, workers: Vec<L>) -> Self {
        let worker_count = workers.len();
        Self {
            supervisor: Arc::new(supervisor),
            workers: workers.into_iter().map(Arc::new).collect(),
            worker_count,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<L, C, S, O> Loop for Supervisor<L, C, S, O>
where
    L: Loop<Context = C, State = S, Output = O> + Send + Sync + 'static,
    C: Clone + Send + Sync + 'static,
    S: Default + Send + Sync + 'static,
    O: Default + Send + Sync + 'static,
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

        // Phase 1: Run supervisor
        let mut sup_state = S::default();
        let sup_result = self.supervisor.execute(ctx.clone(), &mut sup_state).await;
        let mut iterations = sup_result.iterations;

        if !sup_result.is_success() {
            let msg = match &sup_result.status {
                crate::loops::LoopStatus::Failed(msg) => msg.clone(),
                _ => "supervisor failed".into(),
            };
            return LoopResult::failure(msg, iterations, crate::loops::elapsed_ms(&start));
        }

        // Phase 2: Run all workers concurrently with cloned contexts
        let mut handles = Vec::with_capacity(self.worker_count);
        for (idx, worker) in self.workers.iter().enumerate() {
            let ctx = ctx.clone();
            let worker = Arc::clone(worker);
            let name = format!("worker_{idx}");
            handles.push(tokio::spawn(async move {
                let mut state = S::default();
                let result = worker.execute(ctx, &mut state).await;
                (name, result)
            }));
        }

        let mut any_failed = false;
        let mut error_msg = String::new();

        for handle in handles {
            match handle.await {
                Ok((_name, result)) => {
                    iterations = iterations.max(result.iterations);
                    if !result.is_success() {
                        any_failed = true;
                        error_msg = match &result.status {
                            crate::loops::LoopStatus::Failed(msg) => msg.clone(),
                            _ => "worker failed".into(),
                        };
                    }
                }
                Err(e) => {
                    any_failed = true;
                    error_msg = format!("worker join error: {e}");
                }
            }
        }

        let elapsed = crate::loops::elapsed_ms(&start);
        if any_failed {
            LoopResult::failure(error_msg, iterations, elapsed)
        } else {
            // Return the supervisor's output as the final result
            LoopResult::success(sup_result.output.unwrap_or_default(), iterations, elapsed)
        }
    }
}
