# Praxis Architecture

## Overview

Praxis is a state-graph orchestrator for LLM-powered agent systems. It provides a
principled execution model built on four primitive cycles — **Turn**, **Goal**,
**Time**, and **Proactive** — that compose into complex agent workflows.

```
┌─────────────────────────────────────────────────┐
│                   Praxis                         │
│  ┌─────────┐  ┌──────────┐  ┌────────────────┐  │
│  │  Loop   │  │  Graph   │  │    Agent       │  │
│  │ Engine  │──│  (DAG)   │──│ (LLM + Tools)  │  │
│  └─────────┘  └──────────┘  └────────────────┘  │
│       │              │              │            │
│       ▼              ▼              ▼            │
│  ┌─────────┐  ┌──────────┐  ┌────────────────┐  │
│  │  Core   │  │ Runtime  │  │     MCP        │  │
│  │ Types   │  │ Clients  │  │  Integration   │  │
│  └─────────┘  └──────────┘  └────────────────┘  │
└─────────────────────────────────────────────────┘
```

## Workspace Layout

```
praxis/
├── Cargo.toml                  # Workspace manifest
├── crates/
│   ├── core/                   # Domain types, Loop Engine, Agent, Tools
│   ├── runtime/                # LLM client implementations (OpenAI, Anthropic, Gemini)
│   ├── mcp/                    # Model Context Protocol client (JSON-RPC over stdio)
│   ├── cli/                    # Binary entrypoint (CLI agent)
│   └── api-server/             # HTTP API (axum-based)
├── examples/
│   └── src/bin/                # Runnable example binaries
├── ARCHITECTURE.md
└── README.md
```

## Core Concepts

### 1. The Loop Trait

The fundamental abstraction is the `Loop` trait:

```rust
#[async_trait]
pub trait Loop: Send + Sync {
    type Context: Send + 'static;
    type State: Send + 'static;
    type Output: Send + 'static;

    async fn execute(
        &self,
        ctx: Context<Self::Context>,
        state: &mut Self::State,
    ) -> LoopResult<Self::Output>;
}
```

Every executable unit in Praxis implements `Loop`. This includes:
- **Primitive cycles** (Turn, Goal, Time, Proactive)
- **Graphs** (compositions of loops)
- **Agents** (LLM-powered tool-calling loops)
- **Orchestration patterns** (Supervisor, RoundRobin, Broadcast, Router)
- **Approval gates** (human-in-the-loop)

### 2. Four Primitive Cycles

| Cycle | Behavior | Use case |
|-------|----------|----------|
| **Turn** | Single request → response | Q&A, simple commands |
| **Goal** | Iterate until verifier confirms | Complex tasks with validation |
| **Time** | Scheduled execution | Periodic checks, monitoring |
| **Proactive** | Event-triggered | Alert handling, reactive agents |

### 3. State Graph

The `Graph` composes loops into a directed acyclic graph with conditional edges.
Graphs themselves implement `Loop`, enabling recursive composition.

- Nodes wrap any `Loop` implementation
- Edges can have conditions (closures over the previous node's output)
- Graph execution follows edges until an end node
- Supports nested graphs (sub-graphs as nodes)

### 4. Agent

The `Agent` wraps an `LlmClient` + `ToolSet` into a `Loop` that runs:
1. Add user message to conversation
2. Call LLM (with tools)
3. If tool calls → execute each tool, append results, repeat from step 2
4. If text response → return as output

Supports streaming via `execute_stream()` with `StreamChunk` events.

### 5. Memory Management

`ScrollStrategy` manages conversation history length:
- **Truncate** — keep system + N most recent messages
- **SlidingWindow** — keep last N messages regardless of role
- **Summarize** — compress old messages via LLM summary callback
- **NoOp** — keep everything

### 6. Tool System

Tools implement `Tool` trait with JSON schema for LLM function calling:

```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn schema(&self) -> serde_json::Value;
    async fn call(&self, args: serde_json::Value) -> Result<String, ToolError>;
}
```

Built-in tools: `CalculatorTool`, `TimeTool`, `ShellTool`, `EchoTool`.

### 7. MCP Integration

The `mcp` crate implements the [Model Context Protocol](https://modelcontextprotocol.io)
client for connecting to external tool servers:

- JSON-RPC over stdio transport
- Tool discovery (list tools from MCP server)
- Tool invocation (call tools via MCP)
- McpRegistry manages multiple MCP servers

### 8. Orchestration Patterns

High-level patterns for coordinating multiple agents:

| Pattern | Description |
|---------|-------------|
| **Supervisor** | One agent delegates to workers, collects results |
| **RoundRobin** | Agents process sequentially, each building on the previous |
| **Broadcast** | Same input sent to all agents concurrently |
| **Router** | Select agent based on routing function |

### 9. LLM Providers

Multiple provider implementations through the `LlmClient` trait:

- **OpenAI** (`OpenAiClient`) — works with any OpenAI-compatible API
- **Anthropic** (`AnthropicClient`) — Claude models
- **Gemini** (`GeminiClient`) — Google Gemini models

### 10. State Persistence

The `persistence` module handles serialization/deserialization:

- `GraphSnapshot` captures execution position + state
- `save_json` / `load_json` for arbitrary serializable types
- `save_snapshot` / `load_snapshot` for graph snapshots
- All key types implement `Serialize` + `Deserialize`

### 11. API Server

The `api-server` crate exposes HTTP endpoints:

| Endpoint | Description |
|----------|-------------|
| `POST /graphs/{id}/execute` | Execute a graph |
| `GET /graphs/{id}/status` | Query graph execution status |
| `POST /approvals/{id}/approve` | Approve a gate |
| `POST /approvals/{id}/reject` | Reject a gate |
| `POST /agents` | Create a new agent |
| `GET /agents/{id}/stream` | SSE stream agent output |

### 12. Error Handling

- `Result<T, E>` pattern throughout
- `thiserror` for typed errors in libraries
- `anyhow` pattern via `crate::error::Error` with `From` impls
- No `unwrap()` / `expect()` / `panic!()` in production code

## Data Flow

```
User Input
    │
    ▼
Context ──► Graph ──► Node (Loop)
                  │         │
                  │         ▼
                  │    Agent.execute()
                  │    ┌──────────────┐
                  │    │ LLM Call     │◄──── Tool schemas
                  │    │              │
                  │    │ Tool Calls?──┼──► Execute Tool
                  │    │     │        │       │
                  │    │     ▼        │       ▼
                  │    │  Response    │    Tool Result
                  │    └──────────────┘
                  │         │
                  ▼         ▼
            LoopResult ──► Output
```

## Testing Strategy

- **Unit tests** in `#[cfg(test)] mod tests` per file
- **Integration tests** in `tests/` directories
- **Mock LLM clients** via trait implementations
- **Wiremock** for HTTP-based providers
|- **323+ tests** across core, runtime, and API server
