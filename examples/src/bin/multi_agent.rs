//! **Multi-Agent** — demonstrates Supervisor, RoundRobin, Broadcast, and Router
//! orchestration patterns.
//!
//! Each pattern is backed by a mock loop so the example runs without an LLM.
//!
//! Run:
//! ```bash
//! cargo run --package praxis-examples --bin multi_agent
//! ```

use praxis_core::loops::{Context, CycleType, Loop, LoopId, LoopResult, StopCondition};
use praxis_core::orchestration::{Broadcast, RoundRobin, Router, Supervisor};
use std::sync::Arc;
use std::time::Duration;

/// A mock loop that echoes its input prefixed with a label.
struct EchoLoop {
    label: String,
    delay_ms: u64,
}

#[async_trait::async_trait]
impl Loop for EchoLoop {
    type Context = String;
    type State = u32;
    type Output = String;

    async fn execute(
        &self,
        ctx: Context<Self::Context>,
        state: &mut Self::State,
    ) -> LoopResult<Self::Output> {
        *state += 1;
        if self.delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
        }
        LoopResult::success(
            format!("[{}] processed: {}", self.label, ctx.input),
            *state,
            0,
        )
    }
}

impl EchoLoop {
    fn new(label: &str) -> Self {
        Self {
            label: label.to_string(),
            delay_ms: 0,
        }
    }

    fn with_delay(label: &str, delay_ms: u64) -> Self {
        Self {
            label: label.to_string(),
            delay_ms,
        }
    }
}

fn make_ctx(input: &str) -> Context<String> {
    Context::new(
        LoopId::new(),
        CycleType::Turn,
        StopCondition::new(Some(10), Some(Duration::from_secs(30))),
        input.to_string(),
    )
}

#[tokio::main]
async fn main() {
    println!("═══ Praxis — Multi-Agent Orchestration ═══\n");

    // ── 1. Broadcast ────────────────────────────────────────────
    println!("--- Broadcast ---");
    let agents = vec![
        EchoLoop::with_delay("A", 10),
        EchoLoop::with_delay("B", 20),
        EchoLoop::with_delay("C", 10),
    ];
    let broadcast = Broadcast::<_, String, u32, String>::new(agents);
    let mut state = 0u32;
    let result = broadcast.execute(make_ctx("hello"), &mut state).await;
    println!("  Outputs: {:?}", result.output);
    println!();

    // ── 2. RoundRobin ───────────────────────────────────────────
    println!("--- RoundRobin (2 rounds) ---");
    let agents = vec![EchoLoop::new("R1"), EchoLoop::new("R2")];
    let rr = RoundRobin::<_, String, u32, String>::new(agents, 2);
    let mut state = 0u32;
    let result = rr.execute(make_ctx("cycle"), &mut state).await;
    println!("  Final output: {:?}", result.output);
    println!();

    // ── 3. Router ────────────────────────────────────────────────
    println!("--- Router ---");
    let agents = vec![
        EchoLoop::new("Router-A"),
        EchoLoop::new("Router-B"),
    ];
    let router_fn: Arc<dyn Fn(&String) -> usize + Send + Sync> = Arc::new(|input| {
        if input.contains("B") { 1 } else { 0 }
    });
    let router = Router::<_, String, u32, String>::new(agents, router_fn);
    let mut state = 0u32;
    let result = router.execute(make_ctx("go to B"), &mut state).await;
    println!("  Routed output: {:?}", result.output);
    println!();

    // ── 4. Supervisor ────────────────────────────────────────────
    println!("--- Supervisor ---");
    let sup = EchoLoop::new("Supervisor");
    let workers = vec![
        EchoLoop::new("Worker-1"),
        EchoLoop::new("Worker-2"),
    ];
    let supervisor = Supervisor::<_, String, u32, String>::new(sup, workers);
    let mut state = 0u32;
    let result = supervisor.execute(make_ctx("delegate"), &mut state).await;
    println!("  Supervisor result: {:?}", result.output);
    println!();

    println!("═══ Done ═══");
}
