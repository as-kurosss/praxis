//! **Streaming** — demonstrates streaming agent execution with `execute_stream`.
//!
//! Run:
//! ```bash
//! cargo run --package praxis-examples --bin streaming
//! ```

use praxis_core::agent::{Agent, AgentConfig, StreamChunk};
use praxis_core::loops::{Context, CycleType, LoopId, StopCondition};
use praxis_runtime::OpenAiClient;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("═══ Praxis — Streaming ═══");

    let client = OpenAiClient::from_env("gpt-4o")?;
    let agent = Agent::new(client, AgentConfig::default());

    let ctx = Context::new(
        LoopId::new(),
        CycleType::Turn,
        StopCondition::new(Some(25), Some(Duration::from_secs(120))),
        "Write a haiku about Rust.".to_string(),
    );

    let (tx, mut rx) = tokio::sync::mpsc::channel(256);

    // Spawn the agent in the background (move agent into the task)
    let handle = tokio::spawn(async move {
        let mut state = Vec::new();
        agent.execute_stream(ctx, &mut state, tx).await
    });

    // Stream chunks as they arrive
    while let Some(chunk) = rx.recv().await {
        match chunk {
            StreamChunk::Token(text) => print!("{text}"),
            StreamChunk::ToolCallStart { id, name } => {
                println!("\n[Tool Call] {name} ({id})")
            }
            StreamChunk::ToolCallEnd { id } => {
                println!("[Tool End] {id}")
            }
            StreamChunk::Done => {
                println!("\n[Done]");
                break;
            }
            StreamChunk::Error(msg) => {
                eprintln!("\n[Error] {msg}");
                break;
            }
        }
    }

    let result = handle.await.unwrap();
    println!("\nIterations: {}", result.iterations);
    println!("Duration:   {} ms", result.duration_ms);
    Ok(())
}
