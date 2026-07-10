//! **State Graph** — directed execution graph that composes `Loop` primitives.
//!
//! Each [`GraphNode`] wraps a `Loop`.  Execution starts at a start node and
//! follows directed edges until an end node is reached or a failure occurs.
//! Conditions on edges allow dynamic routing based on the previous node's
//! [`LoopResult`].
//!
//! The graph itself implements [`Loop`], so graphs can be nested.

use super::loop_trait::{Context, Loop, LoopResult, elapsed_ms};
use std::collections::{HashMap, HashSet};

/// Unique identifier for a graph node.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NodeId(String);

impl NodeId {
    /// Create a new unique node ID.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Create a node ID from a string.
    pub fn from_id(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl Default for NodeId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A node in the execution graph, wrapping a `Loop`.
pub struct GraphNode<I> {
    id: NodeId,
    label: String,
    inner: I,
}

impl<I> GraphNode<I> {
    /// Create a new graph node wrapping the given loop.
    pub fn new(id: NodeId, inner: I, label: impl Into<String>) -> Self {
        Self {
            id,
            inner,
            label: label.into(),
        }
    }

    /// Unique identifier of this node.
    pub fn id(&self) -> &NodeId {
        &self.id
    }

    /// Human-readable label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The inner loop.
    pub fn inner(&self) -> &I {
        &self.inner
    }
}

/// Condition function for graph edges.
///
/// Receives a reference to the source node's [`LoopResult`] and returns
/// `true` if this edge should be taken.
pub type EdgeCondition<O> = dyn Fn(&LoopResult<O>) -> bool + Send + Sync;

/// A directed edge between two graph nodes.
pub struct Edge<O> {
    /// Target node ID.
    pub to: NodeId,
    /// Optional condition. `None` means unconditional (always taken).
    pub condition: Option<Box<EdgeCondition<O>>>,
}

impl<O> Edge<O> {
    /// Create an unconditional edge to the target node.
    pub fn new(to: NodeId) -> Self {
        Self {
            to,
            condition: None,
        }
    }

    /// Create a conditional edge.
    pub fn with_condition(to: NodeId, condition: Box<EdgeCondition<O>>) -> Self {
        Self {
            to,
            condition: Some(condition),
        }
    }
}

/// A **State Graph** — directed execution graph that composes `Loop` primitives.
///
/// The graph itself implements [`Loop`], so it can be used anywhere a loop is
/// expected, including inside another graph (recursive composition).
///
/// # Type parameters
/// * `I` — inner loop type (must implement `Loop<Context = C, State = S, Output = O>`)
/// * `C` — context type, shared across all nodes
/// * `S` — state type, shared mutably across all nodes
/// * `O` — output type of each node and the graph itself
pub struct Graph<I, C, S, O>
where
    C: Send + Sync + 'static,
    S: Send + Sync + 'static,
    O: Send + Sync + 'static,
{
    nodes: HashMap<NodeId, GraphNode<I>>,
    adjacency: HashMap<NodeId, Vec<Edge<O>>>,
    start_node: NodeId,
    end_nodes: HashSet<NodeId>,
    _phantom: std::marker::PhantomData<(C, S, O)>,
}

impl<I, C, S, O> Graph<I, C, S, O>
where
    C: Send + Sync + 'static,
    S: Send + Sync + 'static,
    O: Send + Sync + 'static,
{
    /// Create a new graph with the given start node ID.
    ///
    /// The start node must be added via [`add_node`] before execution.
    pub fn new(start_node: NodeId) -> Self {
        Self {
            nodes: HashMap::new(),
            adjacency: HashMap::new(),
            start_node,
            end_nodes: HashSet::new(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Add a node to the graph.
    pub fn add_node(&mut self, node: GraphNode<I>) {
        let id = node.id.clone();
        self.nodes.insert(id, node);
    }

    /// Add a directed edge between two nodes.
    pub fn add_edge(&mut self, from: &NodeId, edge: Edge<O>) {
        self.adjacency.entry(from.clone()).or_default().push(edge);
    }

    /// Mark a node as an end (terminal) node.
    ///
    /// Execution stops when an end node finishes successfully.
    pub fn add_end_node(&mut self, node_id: NodeId) {
        self.end_nodes.insert(node_id);
    }

    /// The start node of this graph.
    pub fn start_node(&self) -> &NodeId {
        &self.start_node
    }

    /// Set of end (terminal) node IDs.
    pub fn end_nodes(&self) -> &HashSet<NodeId> {
        &self.end_nodes
    }
}

#[async_trait::async_trait]
impl<I, C, S, O> Loop for Graph<I, C, S, O>
where
    I: Loop<Context = C, State = S, Output = O>,
    C: Clone + Send + Sync + 'static,
    S: Send + Sync + 'static,
    O: Send + Sync + 'static,
{
    type Context = C;
    type State = S;
    type Output = O;

    async fn execute(
        &self,
        ctx: Context<Self::Context>,
        state: &mut Self::State,
    ) -> LoopResult<Self::Output> {
        use std::time::Instant;
        let start = Instant::now();
        let max_iter = ctx.stop_condition.max_iterations.unwrap_or(u32::MAX);
        let timeout = ctx.stop_condition.timeout;
        let mut current = self.start_node.clone();

        for iteration in 1..=max_iter {
            // Check graph-level timeout
            if let Some(limit) = timeout
                && start.elapsed() >= limit
            {
                let elapsed = elapsed_ms(&start);
                return LoopResult::failure(
                    format!("graph timeout after {elapsed}ms"),
                    iteration,
                    elapsed,
                );
            }

            // Look up current node
            let node = match self.nodes.get(&current) {
                Some(n) => n,
                None => {
                    return LoopResult::failure(
                        format!("graph node not found: {current}"),
                        iteration,
                        elapsed_ms(&start),
                    );
                }
            };

            // Execute the node's loop
            let result = node.inner.execute(ctx.clone(), state).await;

            // Determine routing BEFORE consuming result
            let is_end_node = self.end_nodes.contains(&current);
            let next = self.adjacency.get(&current).and_then(|edge_list| {
                edge_list
                    .iter()
                    .find(|e| e.condition.as_ref().is_none_or(|cond| cond(&result)))
                    .map(|e| e.to.clone())
            });

            // Save success flag before moving result into graph_result
            let node_success = result.is_success();

            // Wrap result with graph-level iteration count and elapsed time
            let graph_result = LoopResult {
                output: result.output,
                status: result.status,
                iterations: iteration,
                duration_ms: elapsed_ms(&start),
            };

            if is_end_node {
                return graph_result;
            }

            match next {
                Some(target) => current = target,
                None => {
                    if !node_success {
                        // Node failed — propagate failure
                        return graph_result;
                    }
                    // Node succeeded but no edge matches — routing error
                    return LoopResult::failure(
                        format!("no matching edge from node '{current}'"),
                        iteration,
                        elapsed_ms(&start),
                    );
                }
            }
        }

        // Max iterations exhausted
        LoopResult::failure(
            format!("graph max iterations ({max_iter}) exceeded"),
            max_iter,
            elapsed_ms(&start),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loops::{CycleType, LoopId, LoopStatus, StopCondition, TurnLoop};

    /// Helper: create a `TurnLoop` that echoes input.
    fn echo_loop() -> TurnLoop<String, String> {
        TurnLoop::new(Box::new(Ok))
    }

    fn make_ctx(input: &str, max_it: u32) -> Context<String> {
        Context::new(
            LoopId::new(),
            CycleType::Turn,
            StopCondition::max_iterations(max_it),
            input.to_string(),
        )
    }

    async fn run_graph<O>(
        graph: &impl Loop<Context = String, State = (), Output = O>,
        input: &str,
        max_it: u32,
    ) -> LoopResult<O> {
        let ctx = make_ctx(input, max_it);
        let mut state = ();
        graph.execute(ctx, &mut state).await
    }

    // ── Single node ─────────────────────────────────────────────

    #[tokio::test]
    async fn test_graph_single_node() {
        let start = NodeId::from_id("n1");
        let mut graph = Graph::new(start.clone());
        graph.add_node(GraphNode::new(start.clone(), echo_loop(), "echo"));
        graph.add_end_node(start);

        let result = run_graph(&graph, "hello", 10).await;

        assert!(result.is_success());
        assert_eq!(result.output, Some("hello".to_string()));
        assert_eq!(result.iterations, 1);
    }

    // ── Two-node chain ──────────────────────────────────────────

    #[tokio::test]
    async fn test_graph_two_node_chain() {
        let a = NodeId::from_id("a");
        let b = NodeId::from_id("b");
        let mut graph = Graph::new(a.clone());
        graph.add_node(GraphNode::new(a.clone(), echo_loop(), "step-a"));
        graph.add_node(GraphNode::new(b.clone(), echo_loop(), "step-b"));
        graph.add_edge(&a, Edge::new(b.clone()));
        graph.add_end_node(b);

        let result = run_graph(&graph, "data", 10).await;

        assert!(result.is_success());
        assert_eq!(result.output, Some("data".to_string()));
        assert_eq!(result.iterations, 2);
    }

    // ── Three-node chain with state mutation ────────────────────

    #[tokio::test]
    async fn test_graph_three_node_state_mutation() {
        use crate::loops::GoalLoop;
        use crate::loops::verifier::AlwaysMet;

        type TestGraph = Graph<GoalLoop<Vec<String>, String>, (), Vec<String>, Vec<String>>;

        let a = NodeId::from_id("a");
        let b = NodeId::from_id("b");
        let c = NodeId::from_id("c");

        let mut graph = TestGraph::new(a.clone());
        // Each node appends its marker to the state
        graph.add_node(GraphNode::new(
            a.clone(),
            GoalLoop::new(
                Box::new(|s: &mut Vec<String>| {
                    s.push("a".to_string());
                    Ok(())
                }),
                Box::new(AlwaysMet),
            ),
            "push-a",
        ));
        graph.add_node(GraphNode::new(
            b.clone(),
            GoalLoop::new(
                Box::new(|s: &mut Vec<String>| {
                    s.push("b".to_string());
                    Ok(())
                }),
                Box::new(AlwaysMet),
            ),
            "push-b",
        ));
        graph.add_node(GraphNode::new(
            c.clone(),
            GoalLoop::new(
                Box::new(|s: &mut Vec<String>| {
                    s.push("c".to_string());
                    Ok(())
                }),
                Box::new(AlwaysMet),
            ),
            "push-c",
        ));
        graph.add_edge(&a, Edge::new(b.clone()));
        graph.add_edge(&b, Edge::new(c.clone()));
        graph.add_end_node(c);

        let ctx = Context::new(
            LoopId::new(),
            CycleType::Goal,
            StopCondition::max_iterations(10),
            (),
        );
        let mut state: Vec<String> = Vec::new();

        let result = graph.execute(ctx, &mut state).await;

        assert!(result.is_success());
        assert_eq!(
            state,
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
        assert_eq!(result.iterations, 3);
    }

    // ── Conditional edge (success → b, failure → c) ────────────

    #[tokio::test]
    async fn test_graph_conditional_edge() {
        let a = NodeId::from_id("a");
        let b = NodeId::from_id("b");
        let c = NodeId::from_id("c");
        let mut graph = Graph::new(a.clone());

        // Node a always fails
        let fail_loop = TurnLoop::new(Box::new(|_: String| Err("oops".to_string())));

        graph.add_node(GraphNode::new(a.clone(), fail_loop, "fail"));
        graph.add_node(GraphNode::new(b.clone(), echo_loop(), "ok"));
        graph.add_node(GraphNode::new(c.clone(), echo_loop(), "fallback"));

        // If a succeeds → b; if a fails → c
        let on_success =
            Box::new(|r: &LoopResult<String>| r.is_success()) as Box<EdgeCondition<String>>;
        let on_failure =
            Box::new(|r: &LoopResult<String>| !r.is_success()) as Box<EdgeCondition<String>>;

        graph.add_edge(&a, Edge::with_condition(b.clone(), on_success));
        graph.add_edge(&a, Edge::with_condition(c.clone(), on_failure));
        graph.add_end_node(c);

        let result = run_graph(&graph, "test", 10).await;

        // a fails → route to c → c succeeds
        assert!(result.is_success());
        assert_eq!(result.output, Some("test".to_string()));
        // a (fails, 1 iter) + c (succeeds, 1 iter) = 2 graph iters
        assert_eq!(result.iterations, 2);
    }

    // ── Failure stops the graph mid-chain ──────────────────────

    #[tokio::test]
    async fn test_graph_failure_stops_chain() {
        let a = NodeId::from_id("a");
        let b = NodeId::from_id("b");
        let mut graph = Graph::new(a.clone());

        let fail_loop = TurnLoop::new(Box::new(|_: String| Err("crash".to_string())));

        graph.add_node(GraphNode::new(a.clone(), fail_loop, "fail"));
        graph.add_node(GraphNode::new(b.clone(), echo_loop(), "never-reached"));
        // No edge from a — execution stops after a fails with no matching edge
        graph.add_end_node(b);

        let result = run_graph(&graph, "x", 10).await;

        assert!(!result.is_success());
        assert_eq!(result.status, LoopStatus::Failed("crash".into()));
        assert_eq!(result.iterations, 1);
    }

    // ── End node stops execution ───────────────────────────────

    #[tokio::test]
    async fn test_graph_end_node_stops() {
        let a = NodeId::from_id("a");
        let b = NodeId::from_id("b");
        let c = NodeId::from_id("c");
        let mut graph = Graph::new(a.clone());

        graph.add_node(GraphNode::new(a.clone(), echo_loop(), "step-a"));
        graph.add_node(GraphNode::new(b.clone(), echo_loop(), "step-b"));
        graph.add_node(GraphNode::new(c.clone(), echo_loop(), "step-c"));
        graph.add_edge(&a, Edge::new(b.clone()));
        graph.add_edge(&b, Edge::new(c.clone()));
        // b is an end node → graph should stop after b, never reaching c
        graph.add_end_node(b);

        let result = run_graph(&graph, "stop-at-b", 10).await;

        assert!(result.is_success());
        assert_eq!(result.iterations, 2); // a + b
    }

    // ── Nested graph (graph within a graph) ────────────────────

    #[tokio::test]
    async fn test_graph_nested() {
        let inner_a = NodeId::from_id("ia");
        let inner_b = NodeId::from_id("ib");

        let mut inner = Graph::new(inner_a.clone());
        inner.add_node(GraphNode::new(
            inner_a.clone(),
            TurnLoop::new(Box::new(|s: String| Ok(s + "-inner"))),
            "inner-a",
        ));
        inner.add_node(GraphNode::new(
            inner_b.clone(),
            TurnLoop::new(Box::new(|s: String| Ok(s + "-done"))),
            "inner-b",
        ));
        inner.add_edge(&inner_a, Edge::new(inner_b.clone()));
        inner.add_end_node(inner_b);

        // Outer graph wraps the inner graph as a single node.
        // Both outer nodes must be the same type `I` — here `Graph<...>`.
        let outer_a = NodeId::from_id("oa");
        let outer_b = NodeId::from_id("ob");
        let mut outer_b_inner = Graph::new(outer_b.clone());
        outer_b_inner.add_node(GraphNode::new(outer_b.clone(), echo_loop(), "echo-inner"));
        outer_b_inner.add_end_node(outer_b.clone());

        let mut outer = Graph::new(outer_a.clone());
        outer.add_node(GraphNode::new(outer_a.clone(), inner, "nested-graph"));
        outer.add_node(GraphNode::new(outer_b.clone(), outer_b_inner, "outer-b"));
        outer.add_edge(&outer_a, Edge::new(outer_b.clone()));
        outer.add_end_node(outer_b);

        let result = run_graph(&outer, "nest", 10).await;

        assert!(result.is_success());
        // The outer graph passes the same context to both nodes.
        // outer-a (inner graph) outputs "nest-inner-done" but its result
        // is discarded; outer-b (echo graph) receives context "nest" and
        // outputs "nest".
        assert_eq!(result.output, Some("nest".to_string()));
        assert_eq!(result.iterations, 2);
    }
}
