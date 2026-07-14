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
│   ├── core/                   # Domain types, Loop Engine, Agent, Tools,
│   │                           # A2A Protocol, Plugin Architecture, Governance
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

### 7. A2A Protocol (Agent-to-Agent)

The A2A module implements the [Google A2A](https://github.com/google/A2A) inter-agent
communication protocol. It is gated behind the `a2a` feature (in default features).

**Types:**
- `AgentCard` — agent metadata (name, description, capabilities, auth)
- `Task` / `TaskId` / `TaskState` — task lifecycle (Submitted → Working → Completed/Failed/Canceled)
- `A2AError` / `A2AResult` — typed error handling

**Features:**
- Server: Axum-based HTTP server with agent card discovery (`/.well-known/agent-card`),
  task CRUD (`/tasks`, `/tasks/{id}`, `/tasks/{id}/cancel`)
- Client: Async HTTP client with `reqwest`
- Transport: `A2ATransport` bridges A2A messages to the internal ACP (`AcpTransport` trait)
- SSE streaming (requires `axum` feature `ss`)

### 8. Plugin Architecture

The Plugin module allows extending agent capabilities via WASM-based plugins.
Gated behind the `plugin-wasm` feature.

**Components:**
- `PluginManifest` — TOML/JSON manifest with name, version, author, and tool declarations
- `PluginRegistry` — centralized registry with register, unregister, get, and tool lookup
- `PluginLoader` — filesystem scanner that discovers manifests in `.agents/plugins/`
- `PluginHost` — WASM sandbox based on `wasmtime` (v28, cranelift backend) with
  `HostAccessPolicy` for fine-grained capability control

**Lifecycle:**
1. Plugin manifests are discovered by `PluginLoader` from disk
2. Manifest is parsed and registered in `PluginRegistry`
3. WASM bytecode is compiled by `PluginHost::compile()` into a `CompiledPlugin`
4. Functions are invoked via `PluginHost::call_function()` with policy enforcement

### 9. Governance Matrix

The Governance module provides multi-layer access control for every agent:

**Layers:**
- **AgentGovernance** — per-agent Allow/Deny/Ask matrix for tool categories
  (Shell, FileRead, FileWrite, Network, Generic). Integrates with `AccessPolicyEvaluator`.
- **GovernanceRegistry** — global registry of agent matrices with fallback to a default policy
- **ToolGuard** — tool-level filtering in three modes:
  - `AllowList` — only explicitly listed tools are permitted
  - `BlockList` — only explicitly listed tools are blocked
  - `AllowAll` — all tools permitted (with optional category blocking)
- **FileGuard** — path-level filesystem access control:
  - Restricted mode: only paths within allowed directories
  - Built-in blocked patterns: `.ssh`, `.git`, `etc/passwd`, `etc/shadow`
  - Custom blocked patterns via `add_blocked_pattern()`

### 10. MCP Integration

The `mcp` crate implements the [Model Context Protocol](https://modelcontextprotocol.io)
client for connecting to external tool servers:

- JSON-RPC over stdio transport
- Tool discovery (list tools from MCP server)
- Tool invocation (call tools via MCP)
- McpRegistry manages multiple MCP servers

### 11. Orchestration Patterns

High-level patterns for coordinating multiple agents:

| Pattern | Description |
|---------|-------------|
| **Supervisor** | One agent delegates to workers, collects results |
| **RoundRobin** | Agents process sequentially, each building on the previous |
| **Broadcast** | Same input sent to all agents concurrently |
| **Router** | Select agent based on routing function |

### 12. LLM Providers

Multiple provider implementations through the `LlmClient` trait:

- **OpenAI** (`OpenAiClient`) — works with any OpenAI-compatible API
- **Anthropic** (`AnthropicClient`) — Claude models
- **Gemini** (`GeminiClient`) — Google Gemini models

### 13. State Persistence

The `persistence` module handles serialization/deserialization:

- `GraphSnapshot` captures execution position + state
- `save_json` / `load_json` for arbitrary serializable types
- `save_snapshot` / `load_snapshot` for graph snapshots
- All key types implement `Serialize` + `Deserialize`

### 14. API Server

The `api-server` crate exposes HTTP endpoints:

| Endpoint | Description |
|----------|-------------|
| `POST /graphs/{id}/execute` | Execute a graph |
| `GET /graphs/{id}/status` | Query graph execution status |
| `POST /approvals/{id}/approve` | Approve a gate |
| `POST /approvals/{id}/reject` | Reject a gate |
| `POST /agents` | Create a new agent |
| `GET /agents/{id}/stream` | SSE stream agent output |

### 15. Error Handling

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
|- **324+ tests** across core, runtime, and API server
