# Praxis — Agent Orchestration Framework

[![Rust](https://img.shields.io/badge/Rust-2024-edition?logo=rust)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

**Praxis** is a state-graph orchestrator for LLM-powered agent systems. It provides a
principled execution model built on four primitive cycles — **Turn**, **Goal**, **Time**,
and **Proactive** — that compose into complex agent workflows.

## Architecture

```
praxis/
├── crates/
│   ├── core/        # Domain types, Loop Engine, Agent, Tool abstractions
│   ├── runtime/     # Concrete LLM client implementations (OpenAI-compatible)
│   └── cli/         # Binary entrypoint
```

### Core Concepts

- **Loop Engine** — four primitive execution cycles:
  - `TurnLoop` — single request/response (the simplest cycle)
  - `GoalLoop` — iterates until a verifier confirms the goal
  - `TimeLoop` — wraps a loop with schedule metadata (interval / cron)
  - `ProactiveLoop` — wraps a loop with an event filter
- **State Graph** (`Graph`) — composes loops into a directed acyclic graph with
  conditional edges and nested sub-graphs. The graph itself implements `Loop`,
  enabling recursive composition.
- **Agent** — an LLM-powered tool-calling loop built on the `Loop` trait.
  Wraps an `LlmClient` + `ToolSet` and runs: call LLM → execute tools → repeat
  until final answer.
- **LlmClient trait** — abstract interface for any LLM provider (OpenAI, Anthropic, local, etc.).
- **Tool trait** — define capabilities that agents can invoke.

## Getting Started

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (edition 2024)
- An OpenAI-compatible API key

### Build

```bash
cargo build --release
```

### Run

Set your API credentials and run the agent CLI:

```bash
export OPENAI_API_KEY="sk-..."
export OPENAI_API_URL="https://api.openai.com/v1"
export OPENAI_MODEL="gpt-4o"

./target/release/praxis "Your prompt here"
```

Or pass everything explicitly:

```bash
./target/release/praxis "Hello, world!" \
    --api-url "https://api.openai.com/v1" \
    --model "gpt-4o" \
    --api-key "sk-..."
```

### CLI Options

| Argument           | Env Variable       | Default                          | Description                    |
|--------------------|--------------------|----------------------------------|--------------------------------|
| `prompt`           | —                  | — (required)                     | Prompt to send to the agent    |
| `--api-url`        | `OPENAI_API_URL`   | `https://api.openai.com/v1`      | OpenAI-compatible API base URL |
| `--model`          | `OPENAI_MODEL`     | `gpt-4o`                         | Model identifier               |
| `--api-key`        | `OPENAI_API_KEY`   | — (required if not in env)       | API key                        |
| `--max-iterations` | —                  | `25`                             | Maximum agent loop iterations   |
| `--timeout`        | —                  | `120`                            | Timeout in seconds             |

The `OpenAiClient` works with any OpenAI-compatible API, including **Ollama**, **vLLM**,
**OpenRouter**, **Together AI**, and custom private endpoints.

### Using a Custom Endpoint

```bash
export OPENAI_API_URL="https://your-proxy.example.com/v1"
export OPENAI_MODEL="your-model"

./target/release/praxis "Tell me a joke"
```

## Examples

### Basic Agent

```rust
use praxis_core::agent::{Agent, AgentConfig};
use praxis_core::loops::{Context, CycleType, Loop, LoopId, StopCondition};
use praxis_runtime::OpenAiClient;
use std::time::Duration;

let client = OpenAiClient::from_env("gpt-4o")?;
let agent = Agent::new(client, AgentConfig::default());

let ctx = Context::new(
    LoopId::new(),
    CycleType::Turn,
    StopCondition::new(Some(25), Some(Duration::from_secs(120))),
    "What is the capital of France?".to_string(),
);

let mut state = Vec::new();
let result = agent.execute(ctx, &mut state).await;
println!("{}", result.output.unwrap_or_default());
```

## Development

### Run tests

```bash
cargo test --workspace
```

### Lint

```bash
cargo clippy --workspace -- -W clippy::all -W clippy::pedantic
cargo fmt
```

## License

MIT
