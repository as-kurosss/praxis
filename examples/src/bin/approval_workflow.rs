//! **Approval Workflow** — demonstrates a human-in-the-loop approval gate using
//! [`ApprovalGate`] and [`ApprovalHandle`].
//!
//! Run:
//! ```bash
//! cargo run --package praxis-examples --bin approval_workflow
//! ```

use praxis_core::loops::ApprovalGate;
use praxis_core::loops::{Context, CycleType, Loop, LoopId, StopCondition};
use std::time::Duration;

#[tokio::main]
async fn main() {
    println!("═══ Praxis — Approval Workflow ═══\n");

    // Create an approval gate (no inner loop — gate itself implements Loop)
    let gate = ApprovalGate::<String, String>::new();
    let handle = gate.handle();

    let ctx = Context::new(
        LoopId::new(),
        CycleType::Turn,
        StopCondition::new(Some(5), Some(Duration::from_secs(30))),
        "Approve this workflow step.".to_string(),
    );

    println!("[1] Spawning approval gate execution (will block until approved)…");
    let jh = tokio::spawn(async move {
        let mut state = "workflow-state".to_string();
        let result = gate.execute(ctx, &mut state).await;
        (result, state)
    });

    // Give the gate a moment to start waiting
    tokio::time::sleep(Duration::from_millis(50)).await;
    println!("  Gate is waiting for human decision.");

    // Simulate human approving
    println!("\n[2] Human approves the gate…");
    handle.approve().await;
    println!("  Approved!");

    let (result, state) = jh.await.unwrap();
    println!("  Result: {:?}", result.output);
    println!("  State:  {state}");

    if result.is_success() {
        println!("\n✅ Workflow approved and completed successfully!");
    }
    println!("\n═══ Done ═══");
}
