//! **Persistent Graph** — demonstrates saving and loading graph state via the
//! `persistence` module.
//!
//! Run:
//! ```bash
//! cargo run --package praxis-examples --bin persistent_graph
//! ```

use praxis_core::loops::{GraphSnapshot, NodeId};
use praxis_core::persistence::{load_snapshot, save_snapshot};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("═══ Praxis — Persistent Graph ═══");

    let path = std::env::temp_dir().join("praxis_graph_snapshot.json");
    println!("Snapshot path: {}", path.display());

    // Create and save a snapshot
    let snapshot = GraphSnapshot {
        current_node: NodeId::from_id("verify-step"),
        state: vec!["phase1".to_string(), "phase2".to_string()],
    };
    save_snapshot(&path, &snapshot)?;
    println!("Saved snapshot ✓");

    // Load it back
    let loaded: GraphSnapshot<Vec<String>> = load_snapshot(&path)?;
    println!("Loaded snapshot ✓");
    println!("  Current node: {}", loaded.current_node);
    println!("  State:        {:?}", loaded.state);

    assert_eq!(loaded.current_node.to_string(), "verify-step");
    assert_eq!(loaded.state, vec!["phase1", "phase2"]);

    // Also demonstrate the in-memory JSON roundtrip
    let json = snapshot.to_json()?;
    let restored: GraphSnapshot<Vec<String>> = GraphSnapshot::from_json(&json)?;
    println!("In-memory roundtrip ✓");
    println!("  Node: {}", restored.current_node);

    // Cleanup
    std::fs::remove_file(&path)?;
    println!("\nCleanup done ✓");
    println!("═══ Done ═══");
    Ok(())
}
