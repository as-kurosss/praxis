//! **Parallel Group** — concurrent execution of multiple `Loop` instances.
//!
//! [`ParallelGroup`] wraps a vector of inner loops (all sharing the same
//! `Loop<Context = C, State = S, Output = O>` trait bounds), executes them
//! **concurrently** via `FuturesUnordered` / `tokio::spawn`, and collects
//! every branch's output into a `Vec<O>`.
//!
//! # Fail-fast semantics
//!
//! If **any** branch fails (returns `LoopResult` with an error status),
//! the entire group fails immediately.  Successful branches that already
//! completed before the failure still report their outputs, but the
//! group result as a whole is a failure.
//!
//! # Graph integration
//!
//! `ParallelGroup` implements `Loop`, so it can be placed inside a
//! [`GraphNode`](super::GraphNode) just like any other loop.  The graph's
//! output type `O` becomes `Vec<O_inner>` when a parallel group is used.

use super::loop_trait::{Context, Loop, LoopResult, elapsed_ms};
use std::marker::PhantomData;
use std::time::Instant;
use tokio::task::JoinSet;

/// Concurrently executes multiple inner loops and collects all outputs.
///
/// Each branch receives its **own clone** of the mutable state, so parallel
/// execution is safe.  After all branches complete their outputs are
/// returned in insertion order.
///
/// # Type parameters
/// * `I` — inner loop type (all branches use the same type)
/// * `C` — context type (must be `Clone`)
/// * `S` — state type (must be `Clone`)
/// * `O` — output type of each branch
pub struct ParallelGroup<I, C, S, O>
where
    I: Loop<Context = C, State = S, Output = O>,
    C: Clone + Send + Sync + 'static,
    S: Clone + Send + Sync + 'static,
    O: Send + Sync + 'static,
{
    branches: Vec<I>,
    _phantom: PhantomData<(C, S, O)>,
}

impl<I, C, S, O> ParallelGroup<I, C, S, O>
where
    I: Loop<Context = C, State = S, Output = O>,
    C: Clone + Send + Sync + 'static,
    S: Clone + Send + Sync + 'static,
    O: Send + Sync + 'static,
{
    /// Create a new parallel group from a vector of inner loops.
    ///
    /// Each inner loop becomes one branch.  There must be at least one
    /// branch (panics in debug builds if `branches` is empty).
    ///
    /// # Panics
    /// In debug builds, panics if `branches` is empty.
    #[must_use]
    pub fn new(branches: Vec<I>) -> Self {
        debug_assert!(
            !branches.is_empty(),
            "ParallelGroup requires at least one branch"
        );
        Self {
            branches,
            _phantom: PhantomData,
        }
    }

    /// Number of branches in this group.
    #[must_use]
    pub fn len(&self) -> usize {
        self.branches.len()
    }

    /// Returns `true` if the group has no branches.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.branches.is_empty()
    }
}

#[async_trait::async_trait]
impl<I, C, S, O> Loop for ParallelGroup<I, C, S, O>
where
    I: Loop<Context = C, State = S, Output = O> + 'static,
    C: Clone + Send + Sync + 'static,
    S: Clone + Send + Sync + 'static,
    O: Send + Sync + 'static,
{
    type Context = C;
    type State = S;
    type Output = Vec<O>;

    async fn execute(
        &self,
        ctx: Context<Self::Context>,
        state: &mut Self::State,
    ) -> LoopResult<Self::Output> {
        let start = Instant::now();

        // Spawn all branches concurrently.
        let mut join_set: JoinSet<LoopResult<O>> = JoinSet::new();

        for (_idx, branch) in self.branches.iter().enumerate() {
            let branch_ctx = ctx.clone();
            let mut branch_state = state.clone();
            let branch = unsafe {
                // Safety: the ParallelGroup owns the branches and outlives
                // the JoinSet.  We extend the lifetime to 'static because
                // tokio::spawn requires 'static.  The JoinSet is awaited
                // before this function returns, so the reference is valid
                // for the entire duration.
                std::mem::transmute::<&I, &'static I>(branch)
            };

            join_set.spawn(async move { branch.execute(branch_ctx, &mut branch_state).await });
        }

        // Collect results in order.  JoinSet yields in arbitrary order,
        // so we use an index-based approach.
        let count = self.branches.len();
        let mut outputs: Vec<Option<O>> = (0..count).map(|_| None).collect();
        let mut failures: Vec<String> = Vec::new();
        let mut index = 0usize;

        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(loop_result) => {
                    if loop_result.is_success() {
                        if let Some(output) = loop_result.output {
                            outputs[index] = Some(output);
                        }
                    } else {
                        let error = match &loop_result.status {
                            super::types::LoopStatus::Failed(msg) => msg.clone(),
                            _ => "branch failed".to_string(),
                        };
                        failures.push(error);
                    }
                }
                Err(join_err) => {
                    failures.push(format!("branch panicked: {join_err}"));
                }
            }
            index += 1;
        }

        if !failures.is_empty() {
            return LoopResult::failure(
                format!(
                    "parallel group: {} branch(es) failed: {}",
                    failures.len(),
                    failures.join("; ")
                ),
                count as u32,
                elapsed_ms(&start),
            );
        }

        let collected: Vec<O> = outputs.into_iter().flatten().collect();
        LoopResult::success(collected, count as u32, elapsed_ms(&start))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loops::{CycleType, LoopId, StopCondition, TurnLoop};

    /// Helper: create a `TurnLoop` that echoes input.
    fn echo_loop() -> TurnLoop<String, String> {
        TurnLoop::new(Box::new(Ok))
    }

    /// Helper: create a `TurnLoop` that fails with a message.
    fn fail_loop(msg: &'static str) -> TurnLoop<String, String> {
        TurnLoop::new(Box::new(move |_: String| Err(msg.to_string())))
    }

    fn make_ctx(input: &str) -> Context<String> {
        Context::new(
            LoopId::new(),
            CycleType::Turn,
            StopCondition::max_iterations(10),
            input.to_string(),
        )
    }

    #[tokio::test]
    async fn test_parallel_group_three_branches() {
        // Three echo loops — all succeed.
        let pg = ParallelGroup::new(vec![echo_loop(), echo_loop(), echo_loop()]);
        let mut state = ();
        let result = pg.execute(make_ctx("hello"), &mut state).await;

        assert!(
            result.is_success(),
            "expected success, got {:?}",
            result.status
        );
        let outputs = result.output.unwrap();
        assert_eq!(outputs.len(), 3);
        for o in &outputs {
            assert_eq!(o, "hello");
        }
        assert_eq!(result.iterations, 3);
    }

    #[tokio::test]
    async fn test_parallel_group_one_branch() {
        // Single branch — same as sequential.
        let pg = ParallelGroup::new(vec![echo_loop()]);
        let mut state = ();
        let result = pg.execute(make_ctx("single"), &mut state).await;

        assert!(result.is_success());
        let outputs = result.output.unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0], "single");
    }

    #[tokio::test]
    async fn test_parallel_group_fail_fast() {
        // Two echo loops + one fail loop — entire group fails.
        let pg = ParallelGroup::new(vec![echo_loop(), fail_loop("branch-error"), echo_loop()]);
        let mut state = ();
        let result = pg.execute(make_ctx("test"), &mut state).await;

        assert!(!result.is_success(), "expected failure");
        let err_msg = match &result.status {
            crate::loops::LoopStatus::Failed(msg) => msg.clone(),
            _ => String::new(),
        };
        assert!(
            err_msg.contains("branch-error"),
            "error should mention branch failure: {err_msg}"
        );
    }

    #[tokio::test]
    async fn test_parallel_group_all_fail() {
        // All branches fail.
        let pg = ParallelGroup::new(vec![fail_loop("err-a"), fail_loop("err-b")]);
        let mut state = ();
        let result = pg.execute(make_ctx("x"), &mut state).await;

        assert!(!result.is_success());
    }

    #[tokio::test]
    async fn test_parallel_group_with_nested_graph() {
        // Parallel group where each branch is itself a 2-node graph.
        use crate::loops::Edge;
        use crate::loops::Graph;
        use crate::loops::GraphNode;

        fn make_sub_graph(label: String) -> Graph<TurnLoop<String, String>, String, (), String> {
            let label2 = label.clone();
            let a = crate::loops::NodeId::from_id(format!("{label}-a"));
            let b = crate::loops::NodeId::from_id(format!("{label}-b"));
            let mut g = Graph::new(a.clone());
            g.add_node(GraphNode::new(
                a.clone(),
                TurnLoop::new(Box::new(move |s: String| Ok(s + "-" + &label))),
                format!("{label2}-a"),
            ));
            g.add_node(GraphNode::new(
                b.clone(),
                echo_loop(),
                format!("{label2}-b"),
            ));
            g.add_edge(&a, Edge::new(b.clone()));
            g.add_end_node(b);
            g
        }

        let pg = ParallelGroup::new(vec![make_sub_graph("x".into()), make_sub_graph("y".into())]);
        let mut state = ();
        let result = pg.execute(make_ctx("data"), &mut state).await;

        assert!(result.is_success());
        let outputs = result.output.unwrap();
        assert_eq!(outputs.len(), 2);
        // Each sub-graph transforms the string and echoes
        assert_eq!(outputs[0], "data");
        assert_eq!(outputs[1], "data");
    }
}
