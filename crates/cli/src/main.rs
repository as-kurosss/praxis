use clap::Parser;
use praxis_core::agent::{
    Agent, AgentConfig, TaskId, TaskTracker, Tool, ToolCategory, ToolError, ToolSet, ToolSpec,
};
use praxis_core::loops::{Context, CycleType, Loop, LoopId, StopCondition};
use praxis_runtime::OpenAiClient;
use serde_json::Value;
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

    /// Tool specification(s) in `name=description:json_schema` format
    #[arg(long = "tool", value_name = "NAME=DESC:JSON_SCHEMA")]
    tools: Vec<String>,

    /// Run the agent in background mode (returns immediately with task ID)
    #[arg(long, default_value_t = false)]
    background: bool,
}

// ── Built-in tools ────────────────────────────────────────────────────────

/// A built-in tool that echoes back the `message` field from its input.
struct EchoTool;

#[async_trait::async_trait]
impl Tool for EchoTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "echo".into(),
            description: "Echoes back the input message".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "message": {"type": "string"}
                },
                "required": ["message"]
            }),
            category: ToolCategory::Generic,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let message = args
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs {
                tool: "echo".into(),
                message: "missing 'message' field".into(),
            })?;
        Ok(serde_json::json!({ "echo": message }))
    }
}

/// A tool created from a `--tool` CLI argument.
///
/// Its spec is passed to the LLM, but at runtime it returns a note that no
/// dedicated handler was registered.
struct CliTool {
    spec: ToolSpec,
}

#[async_trait::async_trait]
impl Tool for CliTool {
    fn spec(&self) -> ToolSpec {
        self.spec.clone()
    }

    async fn call(&self, _args: Value) -> Result<Value, ToolError> {
        Ok(serde_json::json!({
            "note": "tool spec registered, no runtime handler",
            "tool": self.spec.name,
        }))
    }
}

// ── Parser helpers ─────────────────────────────────────────────────────────

/// Parse a `--tool` argument string into a [`ToolSpec`].
///
/// Format: `name=description:json_schema`
fn parse_tool_arg(input: &str) -> Result<ToolSpec, String> {
    let (name, rest) = input.split_once('=').ok_or_else(|| {
        format!("invalid tool format '{input}': expected name=description:json_schema")
    })?;

    if name.is_empty() {
        return Err("tool name cannot be empty".into());
    }

    let (description, params_str) = rest.split_once(':').ok_or_else(|| {
        format!("invalid tool format '{input}': expected name=description:json_schema")
    })?;

    if description.is_empty() {
        return Err("tool description cannot be empty".into());
    }

    let parameters: Value = serde_json::from_str(params_str)
        .map_err(|e| format!("invalid JSON schema for tool '{name}': {e}"))?;

    Ok(ToolSpec {
        name: name.to_string(),
        description: description.to_string(),
        parameters,
        category: ToolCategory::Generic,
    })
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

    // Build tool set with the built-in echo tool and any CLI-provided tools
    let mut tool_set = ToolSet::new();
    tool_set.add(EchoTool);

    for arg in &args.tools {
        match parse_tool_arg(arg) {
            Ok(spec) => {
                tool_set.add(CliTool { spec });
            }
            Err(e) => {
                eprintln!("Warning: --tool parse error: {e}");
            }
        }
    }

    // Create an agent with tools
    let agent = Agent::with_tools(
        client,
        AgentConfig {
            model: args.model.clone(),
            model_id: None,
            system_prompt: "You are a helpful assistant.".into(),
            temperature: None,
            max_tokens: None,
            scroll_strategy: None,
        },
        tool_set,
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

    if args.background {
        // Background mode: spawn via TaskTracker
        let tracker = TaskTracker::new();
        let task_id = TaskId::new();
        let handle = tracker
            .spawn_background(task_id.clone(), agent, ctx, Vec::new())
            .await;

        println!("Task {} started in background", task_id);
        println!("Use the API to poll status at GET /api/tasks/{}", task_id.0);

        // Wait a bit and show initial status
        tokio::time::sleep(Duration::from_secs(2)).await;
        let summary = handle.summary().await;
        println!("Status: {:?}", summary.status);
        println!("Elapsed: {}ms", summary.elapsed_ms);
    } else {
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
    }

    Ok(())
}
