# Roadmap — Praxis

> _A state-graph orchestrator for LLM-powered agent systems in Rust._

---

## Phase 1 — Core Framework Completeness

Make the existing primitives production-ready.

| # | Task | Status |
|---|---|---|
| 1.1 | **Serializable state for GoalLoop** — require `Serialize + Deserialize` on GoalLoop state; add suspend/resume round-trip tests | 🟢 |
| 1.2 | **Graph persistence** — serialization of full graph (nodes, edges, current position, accumulated state) for pause/resume | 🔴 |
| 1.3 | **Agent as a proper graph node** — ensure `Agent<L>` works cleanly inside a `Graph`, respecting cycle classification | 🔴 |
| 1.4 | **Time-based runtime** — actual scheduler that triggers `TimeLoop` on interval/cron | 🔴 |
| 1.5 | **Proactive runtime** — event listener that triggers `ProactiveLoop` on matching events | 🔴 |

**Goal:** A self-contained framework where any agent flow can be expressed as a serializable graph of typed cycles.

---

## Phase 2 — Multi-Agent & Communication

Parallel and hierarchical agent orchestration.

| # | Task | Status |
|---|---|---|
| 2.1 | **Sub-agent spawning** — spawn child agents from within a tool or a loop | 🔴 |
| 2.2 | **Agent Communication Protocol** — typed message passing between agents (ACP-like) | 🔴 |
| 2.3 | **Swarm / topology patterns** — supervisor, round-robin, broadcast, DAG-based orchestration | 🔴 |

---

## Phase 3 — Memory & Context

Never forget, nothing summarized away.

| # | Task | Status |
|---|---|---|
| 3.1 | **Scroll context** — full conversation history with eviction + on-demand recall | 🔴 |
| 3.2 | **Long-term memory** — persistent storage with similarity search (ReMe-like) | 🔴 |
| 3.3 | **Working context** — live token-budgeted context window with smart eviction | 🔴 |

---

## Phase 4 — Interfaces

Make the framework accessible.

| # | Task | Status |
|---|---|---|
| 4.1 | **REST API** — expose agent execution via HTTP (Axum / Actix) | 🔴 |
| 4.2 | **TUI** — terminal user interface for interactive chat | 🔴 |
| 4.3 | **Web Console** — browser-based UI for agent management | 🔴 |

---

## Phase 5 — Channels

Reach users where they are.

| # | Task | Status |
|---|---|---|
| 5.1 | **Telegram / Discord / Slack** — channel adapters | 🔴 |
| 5.2 | **MCP integration** — Model Context Protocol tool/resource bridge | 🔴 |

---

## Phase 6 — Security & Governance

Safe agent execution.

| # | Task | Status |
|---|---|---|
| 6.1 | **Tool policy gating** — per-tool allow/deny/ask configuration | 🔴 |
| 6.2 | **Execution sandbox** — OS-level isolation for code execution | 🔴 |
| 6.3 | **File guard** — restricted filesystem access policies | 🔴 |

---

### Legend

- 🔴 Not started
- 🟡 In progress
- 🟢 Done
