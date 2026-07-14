//! **Approval Gate** — human-in-the-loop node that pauses execution until
//! an external decision (approve / reject) is received.
//!
//! [`ApprovalGate`] implements [`Loop`] so it can be placed inside a
//! [`Graph`](super::Graph).  When the graph reaches this node, execution
//! blocks until an external caller calls [`approve`](ApprovalGate::approve)
//! or [`reject`](ApprovalGate::reject).
//!
//! # Usage
//!
//! ```ignore
//! let gate = ApprovalGate::<String, ()>::new();
//! let gate_handle = gate.handle();
//!
//! let node = GraphNode::new(id, gate, "human-approval");
//!
//! // Spawn graph in a background task
//! tokio::spawn(async { graph.execute(ctx, &mut state).await });
//!
//! // Later, externally decide:
//! gate_handle.approve();
//! // or
//! gate_handle.reject("Not safe".into());
//! ```

use super::loop_trait::{Context, Loop, LoopResult, elapsed_ms};
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, Notify};

/// The decision made by a human (or external system) for an approval gate.
#[derive(Debug, Clone)]
pub enum ApprovalDecision {
    /// Proceed with execution.
    Approve,
    /// Reject with a reason.
    Reject(String),
}

/// Handle used to approve or reject an [`ApprovalGate`] from outside the graph.
///
/// Clone this and hand it to your UI, API handler, or CLI tool.
#[derive(Clone)]
pub struct ApprovalHandle {
    inner: Arc<Mutex<Option<ApprovalDecision>>>,
    notify: Arc<Notify>,
}

impl ApprovalHandle {
    /// Approve the gate — execution continues.
    pub async fn approve(&self) {
        let mut lock = self.inner.lock().await;
        *lock = Some(ApprovalDecision::Approve);
        self.notify.notify_one();
    }

    /// Reject the gate — execution fails with the given reason.
    pub async fn reject(&self, reason: impl Into<String>) {
        let mut lock = self.inner.lock().await;
        *lock = Some(ApprovalDecision::Reject(reason.into()));
        self.notify.notify_one();
    }
}

/// A graph node that pauses execution until approved or rejected.
///
/// The gate implements `Loop<Context = C, State = S, Output = S>` and clones
/// the current state as its output when approved.
///
/// # Type parameters
/// * `C` — context type
/// * `S` — state type (must be `Clone`)
pub struct ApprovalGate<C, S>
where
    C: Send + Sync + 'static,
    S: Send + Sync + 'static,
{
    inner: Arc<Mutex<Option<ApprovalDecision>>>,
    notify: Arc<Notify>,
    _phantom: PhantomData<(C, S)>,
}

impl<C, S> ApprovalGate<C, S>
where
    C: Send + Sync + 'static,
    S: Send + Sync + 'static,
{
    /// Create a new approval gate.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
            notify: Arc::new(Notify::new()),
            _phantom: PhantomData,
        }
    }

    /// Get a handle that can approve/reject this gate from outside.
    ///
    /// The handle can be cloned and shared across threads/tasks.
    pub fn handle(&self) -> ApprovalHandle {
        ApprovalHandle {
            inner: Arc::clone(&self.inner),
            notify: Arc::clone(&self.notify),
        }
    }
}

impl<C, S> Default for ApprovalGate<C, S>
where
    C: Send + Sync + 'static,
    S: Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl<C, S> Loop for ApprovalGate<C, S>
where
    C: Send + Sync + 'static,
    S: Clone + Send + Sync + 'static,
{
    type Context = C;
    type State = S;
    type Output = S;

    async fn execute(
        &self,
        _ctx: Context<Self::Context>,
        state: &mut Self::State,
    ) -> LoopResult<Self::Output> {
        let start = Instant::now();

        loop {
            // Check if a decision has been made
            {
                let mut lock = self.inner.lock().await;
                if let Some(decision) = lock.take() {
                    match decision {
                        ApprovalDecision::Approve => {
                            return LoopResult::success(state.clone(), 1, elapsed_ms(&start));
                        }
                        ApprovalDecision::Reject(reason) => {
                            return LoopResult::failure(reason, 1, elapsed_ms(&start));
                        }
                    }
                }
            }

            // Wait for a notification
            self.notify.notified().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loops::{CycleType, LoopId, StopCondition};

    fn make_ctx(input: &str) -> Context<String> {
        Context::new(
            LoopId::new(),
            CycleType::Turn,
            StopCondition::max_iterations(10),
            input.to_string(),
        )
    }

    #[tokio::test]
    async fn test_approval_gate_approve() {
        let gate = ApprovalGate::<String, String>::new();
        let handle = gate.handle();

        // Spawn the gate execution
        let jh = tokio::spawn(async move {
            let mut state = "pending".to_string();
            gate.execute(make_ctx("test"), &mut state).await
        });

        // Give the gate a moment to start waiting, then approve
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        handle.approve().await;

        let result = jh.await.unwrap();
        assert!(result.is_success());
        assert_eq!(result.output, Some("pending".to_string()));
    }

    #[tokio::test]
    async fn test_approval_gate_reject() {
        let gate = ApprovalGate::<String, String>::new();
        let handle = gate.handle();

        let jh = tokio::spawn(async move {
            let mut state = "data".to_string();
            gate.execute(make_ctx("test"), &mut state).await
        });

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        handle.reject("Not safe").await;

        let result = jh.await.unwrap();
        assert!(!result.is_success());
        assert!(
            matches!(&result.status, crate::loops::LoopStatus::Failed(msg) if msg.contains("Not safe"))
        );
    }

    #[tokio::test]
    async fn test_approval_gate_approve_before_execute() {
        // Decision made before execute is called.
        let gate = ApprovalGate::<String, String>::new();
        let handle = gate.handle();
        handle.approve().await;

        let mut state = "ready".to_string();
        let result = gate.execute(make_ctx("test"), &mut state).await;

        assert!(result.is_success());
        assert_eq!(result.output, Some("ready".to_string()));
    }
}
