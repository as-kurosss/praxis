//! **Praxis API Server** — HTTP API for managing agents, providers, and sessions.
//!
//! # Quick start
//!
//! ```bash
//! cargo run --package praxis-api-server
//! ```
//!
//! The server listens on `127.0.0.1:3000` by default (override with
//! `PRAXIS_HOST` / `PRAXIS_PORT` environment variables).
//!
//! Data is persisted to `PRAXIS_DATA` or `{pwd}/praxis-data` by default.

mod routes;
pub mod state;

use state::AppState;
use std::net::SocketAddr;
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    // Initialise logging
    tracing_subscriber::fmt::init();

    // Determine data directory from environment or use default
    let data_dir: PathBuf = std::env::var("PRAXIS_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            cwd.join("praxis-data")
        });

    tracing::info!("Using data directory: {}", data_dir.display());

    // Build shared state (opens/create registry + session store)
    let state = AppState::new(data_dir).unwrap_or_else(|e| {
        tracing::error!("Failed to initialise server state: {e}");
        std::process::exit(1);
    });

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
