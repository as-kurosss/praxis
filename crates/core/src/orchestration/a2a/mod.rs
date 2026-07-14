//! # A2A (Agent-to-Agent) Protocol
//!
//! Реализация Google A2A-протокола — Task-ориентированного протокола
//! взаимодействия между агентами.
//!
//! ## Компоненты
//!
//! * [`types`] — типы данных A2A (Task, TaskState, Message, AgentCard, ...)
//! * [`server`] — Axum-based HTTP сервер с REST API
//! * [`client`] — HTTP-клиент для взаимодействия с A2A-агентами
//! * [`transport`] — мост A2A ↔ ACP (реализация `AcpTransport`)
//!
//! ## Пример
//!
//! ```ignore
//! use praxis_core::orchestration::a2a::{A2AServer, A2AClient, AgentCard, TaskId, Task};
//!
//! // Серверная сторона
//! let card = AgentCard {
//!     name: "my-agent".into(),
//!     description: "Example agent".into(),
//!     url: "http://localhost:8088".into(),
//!     version: "1.0".into(),
//!     capabilities: vec![],
//! };
//! let server = A2AServer::new(card);
//! let router = server.into_router();
//!
//! // Клиентская сторона
//! let client = A2AClient::new();
//! let card = client.fetch_card("http://localhost:8088").await?;
//! let task = client.create_task("http://localhost:8088", "task-1").await?;
//! ```

mod client;
mod server;
pub mod transport;
mod types;

pub use client::A2AClient;
pub use server::A2AServer;
pub use server::ServerState;
pub use server::TaskStore;
pub use transport::A2ATransport;
pub use types::*;
