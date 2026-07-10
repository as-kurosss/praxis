use clap::Parser;
use praxis_core::agent::{Agent, AgentConfig};
use praxis_core::loops::{Context, CycleType, Loop, LoopId, StopCondition};
use praxis_runtime::OpenAiClient;
use std::time::Duration;

/// Praxis — Agent Orchestration Framework
#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Prompt to send to the agent
    prompt: String,

    /// `OpenAI`-compatible API base URL
    #[arg(
        long,
        default_value = "https://api.openai.com/v1",
        env = "OPENAI_API_URL"
    )]
    api_url: String,

    /// Model to use
    #[arg(long, default_value = "gpt-4o", env = "OPENAI_MODEL")]
    model: String,

    /// API key (defaults to `OPENAI_API_KEY` env var)
    #[arg(long, env = "OPENAI_API_KEY")]
    api_key: Option<String>,

    /// Maximum iterations for the agent loop
    #[arg(long, default_value = "25")]
    max_iterations: u32,

    /// Timeout in seconds
    #[arg(long, default_value = "120")]
    timeout: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Resolve API key: arg > env > error
    let api_key = match args.api_key {
        Some(k) => k,
        None => std::env::var("OPENAI_API_KEY").map_err(
            |_| "OPENAI_API_KEY not set. Provide via --api-key or OPENAI_API_KEY env var.",
        )?,
    };

    // Create the LLM client
    let client = OpenAiClient::new(&args.api_url, api_key, &args.model);

    // Create an agent (no tools for now)
    let agent = Agent::new(
        client,
        AgentConfig {
            model: args.model.clone(),
            system_prompt: "You are a helpful assistant.".into(),
            temperature: None,
            max_tokens: None,
        },
    );

    // Build the execution context
    let ctx = Context::new(
        LoopId::new(),
        CycleType::Turn,
        StopCondition::new(
            Some(args.max_iterations),
            Some(Duration::from_secs(args.timeout)),
        ),
        args.prompt,
    );

    let mut state = Vec::new();
    let result = agent.execute(ctx, &mut state).await;

    if result.is_success() {
        if let Some(output) = &result.output {
            println!("{output}");
        }
    } else {
        eprintln!(
            "Agent failed after {} iterations ({duration_ms}ms): {status:?}",
            result.iterations,
            duration_ms = result.duration_ms,
            status = result.status,
        );
    }

    Ok(())
}
