# Praxis — Agent Orchestration Framework

[![Rust](https://img.shields.io/badge/Rust-2024-edition?logo=rust)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

**Praxis** is a state-graph orchestrator for LLM-powered agent systems. It provides a
principled execution model built on four primitive cycles — **Turn**, **Goal**, **Time**,
and **Proactive** — that compose into complex agent workflows.

## Features

- **Loop Engine** — four primitive execution cycles (Turn, Goal, Time, Proactive) that compose recursively; state-graph with conditional edges and nested sub-graphs
- **Agent Runtime** — LLM-powered tool-calling agent with streaming, configurable system prompts, temperature, max tokens, and scroll strategies
- **Multiple LLM Providers** — OpenAI-compatible, Anthropic Claude, Google Gemini
- **Tool Ecosystem** — Shell, Calculator, Time, and extensible `Tool` trait with `ToolCategory` for policy routing
- **Sub-agent Spawning** — agents can spawn and communicate with child agents with isolated state
- **Multi-Agent Orchestration** — Supervisor, RoundRobin, Broadcast, Router patterns (all implement `Loop`)
- **Agent Communication Protocol (ACP)** — typed message passing between agents with TTL, routing, and pluggable transports (in-memory, TCP, stdio)
- **A2A Protocol (Agent-to-Agent)** — Google A2A-compatible inter-agent communication: Agent Card discovery, task lifecycle (create/get/cancel), SSE streaming, with a transport bridge to ACP
- **Plugin Architecture** — WASM-based plugin system with TOML/JSON manifests, sandboxed execution via `wasmtime`, and policy-based host access control
- **Governance Matrix** — per-agent resource access control: Allow/Deny/Ask matrix by tool category, ToolGuard (AllowList/BlockList), and FileGuard (restricted paths with sensitive pattern blocking)
- **Sandbox & Governance** — policy enforcement (shell blocklists, file path restrictions, network allow/deny), sandboxed execution via `DirectSandbox` with async trait API
- **Scheduler** — cron-like task scheduling engine with Interval/Cron/Once/Recurring schedules and persistent JSON-backed task definitions
- **Memory System** — multi-layer memory: `WorkingMemory` with scroll strategies (Truncate, SlidingWindow, Summarize), `EpisodicMemory` with keyword-indexed IDF-weighted recall, `DistilledMemory` with periodic summarization
- **Human-in-the-Loop** — Approval gates for safe agent execution
- **MCP Integration** — Model Context Protocol client for external tools and resources
- **State Persistence** — JSON save/load for graph snapshots and agent state
- **HTTP API** — axum-based server for remote execution and approval management
- **Custom Tools CLI** — pass arbitrary tool schemas via `--tool` flag

## Architecture

```
praxis/
├── crates/
│   ├── core/         # Domain types, Loop Engine, Agent, Tools, Memory, Sandbox,
│   │                 # Scheduler, Orchestration, ACP, A2A, Plugin, Governance
│   ├── runtime/      # LLM client implementations (OpenAI, Anthropic, Gemini)
│   ├── mcp/          # Model Context Protocol client (stdio transport)
│   ├── cli/          # Binary entrypoint
│   └── api-server/   # HTTP API (axum)
├── examples/         # Runnable example binaries
└── ARCHITECTURE.md   # Detailed architecture documentation
```

See [ARCHITECTURE.md](ARCHITECTURE.md) for detailed documentation.

## Getting Started

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (edition 2024)
- An LLM API key (OpenAI, Anthropic, or Gemini)

### Build

```bash
cargo build --release
```

### Run CLI

```bash
export OPENAI_API_KEY="sk-..."
export OPENAI_API_URL="https://api.openai.com/v1"
export OPENAI_MODEL="gpt-4o"

cargo run --release --bin praxis "What is the capital of France?"
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
| `--tool`           | —                  | —                                | Custom tool schema (name=desc:json) |

The `OpenAiClient` works with any OpenAI-compatible API, including **Ollama**, **vLLM**,
**OpenRouter**, **Together AI**, and custom private endpoints.

### Run Examples

```bash
# Multi-agent orchestration (mock, no API key needed)
cargo run --package praxis-examples --bin multi_agent

# Approval workflow (mock)
cargo run --package praxis-examples --bin approval_workflow

# Persistent graph snapshot
cargo run --package praxis-examples --bin persistent_graph

# Streaming agent (requires API key)
cargo run --package praxis-examples --bin streaming

# Simple agent (requires API key)
cargo run --package praxis-examples --bin simple_agent
```

### Run API Server

```bash
cargo run --package praxis-api-server
# Listens on 127.0.0.1:3000 by default
```

## Quick Example

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
