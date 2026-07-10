//! **Praxis API Server** — HTTP API for executing graphs and managing agents.
//!
//! # Quick start
//!
//! ```bash
//! cargo run --package praxis-api-server
//! ```
//!
//! The server listens on `127.0.0.1:3000` by default (override with
//! `PRAXIS_HOST` / `PRAXIS_PORT` environment variables).

mod routes;
pub mod state;

use state::AppState;
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    // Initialise logging
    tracing_subscriber::fmt::init();

    // Build shared state
    let state = AppState::new();

    // Build router
    let app = routes::router(state);

    // Determine bind address from environment or use default
    let host = std::env::var("PRAXIS_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port: u16 = std::env::var("PRAXIS_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);

    let addr: SocketAddr = format!("{host}:{port}").parse().expect("invalid address");

    tracing::info!("Praxis API server starting on {addr}");

    // Start listening
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
