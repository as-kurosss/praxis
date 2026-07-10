//! **Simple Agent** — basic usage of `Agent`, `Loop`, and `LoopResult`.
//!
//! Run:
//! ```bash
//! cargo run --package praxis-examples --bin simple_agent
//! ```

use praxis_core::agent::{Agent, AgentConfig};
use praxis_core::loops::{Context, CycleType, Loop, LoopId, StopCondition};
use praxis_runtime::OpenAiClient;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("═══ Praxis — Simple Agent ═══");

    // Create an LLM client from environment variables:
    //   OPENAI_API_KEY, OPENAI_API_URL, OPENAI_MODEL
    let client = OpenAiClient::from_env("gpt-4o")?;

    // Configure the agent
    let config = AgentConfig {
        model: "gpt-4o".into(),
        system_prompt: "You are a concise assistant.".into(),
        temperature: Some(0.5),
        max_tokens: Some(1024),
        scroll_strategy: None,
    };

    let agent = Agent::new(client, config);
    println!("Agent created ✓");

    // Build the execution context
    let ctx = Context::new(
        LoopId::new(),
        CycleType::Turn,
        StopCondition::new(Some(25), Some(Duration::from_secs(120))),
        "What is the capital of France?".to_string(),
    );

    println!("Executing agent…");
    let mut state = Vec::new();
    let result = agent.execute(ctx, &mut state).await;

    match result.output {
        Some(answer) => println!("Answer: {answer}"),
        None => eprintln!("Agent failed: {:?}", result.status),
    }

    println!("Iterations: {}", result.iterations);
    println!("Duration:   {} ms", result.duration_ms);
    Ok(())
}
