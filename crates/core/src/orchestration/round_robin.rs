//! **RoundRobin** — cycles through agents in sequence, passing the output
//! of one as the input to the next.

use crate::loops::{Context, Loop, LoopResult};

/// A loop that cycles through child agents in round-robin fashion.
///
/// Each agent receives the previous agent's output as its input.
/// After all agents have run for `rounds` cycles, the last output is returned.
///
/// # Type parameters
/// * `L` — the inner loop type
/// * `C` — context type (must be `Clone` so it can be rebuilt for each agent)
/// * `S` — state type (each agent starts fresh)
/// * `O` — output type (must be `Into<C>` so it can become the next context)
pub struct RoundRobin<L, C, S, O> {
    agents: Vec<L>,
    rounds: u32,
    _phantom: std::marker::PhantomData<(C, S, O)>,
}

impl<L, C, S, O> RoundRobin<L, C, S, O>
where
    L: Loop<Context = C, State = S, Output = O> + Send + 'static,
    C: Clone + Send + Sync + 'static,
    S: Default + Send + Sync + 'static,
    O: Into<C> + Clone + Send + Sync + 'static,
{
    /// Create a round-robin with the given agents and number of rounds.
    ///
    /// Each round cycles through all agents in order.
    pub fn new(agents: Vec<L>, rounds: u32) -> Self {
        Self {
            agents,
            rounds,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<L, C, S, O> Loop for RoundRobin<L, C, S, O>
where
    L: Loop<Context = C, State = S, Output = O> + Send + 'static,
    C: Clone + Send + Sync + 'static,
    S: Default + Send + Sync + 'static,
    O: Into<C> + Clone + Send + Sync + 'static,
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
        let mut current_ctx = ctx;
        let mut iterations = 0u32;
        let mut last_output: Option<O> = None;

        for round in 0..self.rounds {
            for (idx, agent) in self.agents.iter().enumerate() {
                let mut state = S::default();
                let result = agent.execute(current_ctx.clone(), &mut state).await;
                iterations = iterations.max(result.iterations);

                if result.is_success() {
                    if let Some(output) = result.output {
                        last_output = Some(output.clone());
                        // Rebuild context with this output
                        let input: C = output.into();
                        current_ctx = Context::new(
                            crate::loops::LoopId::new(),
                            crate::loops::CycleType::Turn,
                            current_ctx.stop_condition.clone(),
                            input,
                        );
                    }
                } else {
                    let elapsed = crate::loops::elapsed_ms(&start);
                    let msg = match &result.status {
                        crate::loops::LoopStatus::Failed(msg) => msg.clone(),
                        _ => format!("round-robin agent {idx} failed in round {}", round + 1),
                    };
                    return LoopResult::failure(msg, iterations, elapsed);
                }
            }
        }

        let elapsed = crate::loops::elapsed_ms(&start);
        match last_output {
            Some(output) => LoopResult::success(output, iterations, elapsed),
            None => LoopResult::failure(
                String::from("round-robin: no output produced"),
                iterations,
                elapsed,
            ),
        }
    }
}
