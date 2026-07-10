//! **Proactive** loop — reacts to an event (wrapper).
//!
//! Delegates `execute()` to an inner Turn/Goal loop.
//! An optional event filter controls which events trigger execution.

use super::loop_trait::{Context, Loop, LoopResult};

/// Event filter function type for proactive loops.
pub type EventFilter = dyn Fn(&str) -> bool + Send + Sync;

/// A **Proactive** loop — wrapper that adds event-filter metadata.
///
/// Delegates `execute()` to the inner loop. An optional event filter is used
/// by an external event listener to determine when to trigger execution.
pub struct ProactiveLoop<I> {
    inner: I,
    event_filter: Option<Box<EventFilter>>,
}

impl<I> ProactiveLoop<I> {
    /// Create a new proactive loop wrapping the given inner loop without a filter.
    pub fn new(inner: I) -> Self {
        Self {
            inner,
            event_filter: None,
        }
    }

    /// Create a proactive loop with an event filter.
    pub fn with_filter(inner: I, filter: Box<EventFilter>) -> Self {
        Self {
            inner,
            event_filter: Some(filter),
        }
    }

    /// The event filter, if set.
    pub fn event_filter(&self) -> Option<&EventFilter> {
        self.event_filter.as_deref()
    }
}

#[async_trait::async_trait]
impl<I: Loop + Send + Sync> Loop for ProactiveLoop<I> {
    type Context = I::Context;
    type State = I::State;
    type Output = I::Output;

    async fn execute(
        &self,
        ctx: Context<Self::Context>,
        state: &mut Self::State,
    ) -> LoopResult<Self::Output> {
        self.inner.execute(ctx, state).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loops::{CycleType, LoopId, StopCondition, TurnLoop};
    use std::time::Duration;

    fn echo_inner() -> TurnLoop<String, String> {
        TurnLoop::new(Box::new(Ok))
    }

    #[tokio::test]
    async fn test_proactive_loop_delegates_to_inner() {
        let inner = echo_inner();
        let proactive = ProactiveLoop::new(inner);
        let ctx = Context::new(
            LoopId::new(),
            CycleType::Proactive,
            StopCondition::timeout(Duration::from_secs(10)),
            "event-data".to_string(),
        );
        let mut state = ();

        let result = proactive.execute(ctx, &mut state).await;

        assert!(result.is_success());
        assert_eq!(result.output, Some("event-data".to_string()));
    }

    #[tokio::test]
    async fn test_proactive_loop_with_filter() {
        let inner = echo_inner();
        let filter = Box::new(|event: &str| event.starts_with("urgent:"));
        let proactive = ProactiveLoop::with_filter(inner, filter);
        let ctx = Context::new(
            LoopId::new(),
            CycleType::Proactive,
            StopCondition::timeout(Duration::from_secs(10)),
            "data".to_string(),
        );
        let mut state = ();

        let result = proactive.execute(ctx, &mut state).await;

        // Delegation still works regardless of filter
        assert!(result.is_success());
        assert_eq!(result.output, Some("data".to_string()));
    }

    #[test]
    fn test_proactive_loop_event_filter_access() {
        let inner = echo_inner();
        let filter = Box::new(|event: &str| event == "deploy");
        let proactive = ProactiveLoop::with_filter(inner, filter);

        let stored = proactive.event_filter().expect("filter should be set");
        assert!(stored("deploy"));
        assert!(!stored("rollback"));
    }

    #[test]
    fn test_proactive_loop_no_filter() {
        let inner = echo_inner();
        let proactive = ProactiveLoop::<TurnLoop<String, String>>::new(inner);

        assert!(proactive.event_filter().is_none());
    }
}
