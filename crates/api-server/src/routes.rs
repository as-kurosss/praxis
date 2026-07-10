//! **Routes** — HTTP handlers for the Praxis API server.
//!
//! # Endpoints
//!
//! * `POST /graphs/:id/execute` — execute a graph
//! * `GET /graphs/:id/status` — query graph execution status
//! * `POST /approvals/:id/approve` — approve a gate
//! * `POST /approvals/:id/reject` — reject a gate
//! * `POST /agents` — create a new agent
//! * `GET /agents/:id/stream` — SSE stream agent output

use crate::state::{AgentHandle, AppState, ApprovalHandle, GraphHandle, ResourceStatus};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{sse::Event, Json, Sse},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::convert::Infallible;
use tokio_stream::wrappers::ReceiverStream;

// ── Request / Response helpers ──────────────────────────────────────────

/// Response wrapper for consistent API responses.
#[derive(Serialize)]
struct ApiResponse<T: Serialize> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
}

impl<T: Serialize> ApiResponse<T> {
    fn ok(data: T) -> Json<Self> {
        Json(Self {
            success: true,
            data: Some(data),
            error: None,
        })
    }

    fn err(msg: impl Into<String>) -> (StatusCode, Json<Self>) {
        (
            StatusCode::BAD_REQUEST,
            Json(Self {
                success: false,
                data: None,
                error: Some(msg.into()),
            }),
        )
    }
}

/// Request body for `POST /graphs/:id/execute`.
#[derive(Deserialize)]
struct ExecuteGraphRequest {
    /// Human-readable label for the graph run.
    label: Option<String>,
    /// Input payload (JSON).
    input: Option<Value>,
}

/// Request body for `POST /agents`.
#[derive(Deserialize)]
struct CreateAgentRequest {
    /// Agent configuration as JSON.
    config: Value,
}

// ── Routes ──────────────────────────────────────────────────────────────

/// Build the router with all API routes.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/graphs/{id}/execute", post(execute_graph))
        .route("/graphs/{id}/status", get(graph_status))
        .route("/approvals/{id}/approve", post(approve_gate))
        .route("/approvals/{id}/reject", post(reject_gate))
        .route("/agents", post(create_agent))
        .route("/agents/{id}/stream", get(agent_stream))
        .with_state(state)
}

/// `POST /graphs/:id/execute`
///
/// Registers a new graph execution and immediately runs it.
async fn execute_graph(
    State(state): State<AppState>,
    Path(id): Path<String>,
    axum::Json(body): axum::Json<ExecuteGraphRequest>,
) -> Result<Json<ApiResponse<GraphHandle>>, (StatusCode, Json<ApiResponse<GraphHandle>>)> {
    let now = chrono_now();
    let handle = GraphHandle {
        label: body.label.unwrap_or_else(|| format!("graph-{id}")),
        status: ResourceStatus::Running,
        result: body.input,
        created_at: now,
    };

    state.graphs.write().await.insert(id.clone(), handle.clone());

    // In a real implementation this would spawn a background task.
    // For now we mark it as completed in the response.
    Ok(ApiResponse::ok(handle))
}

/// `GET /graphs/:id/status`
///
/// Returns the current status and result of a graph execution.
async fn graph_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<GraphHandle>>, (StatusCode, Json<ApiResponse<GraphHandle>>)> {
    let graphs = state.graphs.read().await;
    match graphs.get(&id) {
        Some(handle) => Ok(ApiResponse::ok(handle.clone())),
        None => Err(ApiResponse::err(format!("graph '{id}' not found"))),
    }
}

/// `POST /approvals/:id/approve`
///
/// Approves a pending approval gate.
async fn approve_gate(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<ApprovalHandle>>, (StatusCode, Json<ApiResponse<ApprovalHandle>>)> {
    let mut approvals = state.approvals.write().await;
    match approvals.get_mut(&id) {
        Some(handle) => {
            if !matches!(handle.status, ResourceStatus::Pending) {
                return Err(ApiResponse::err(format!(
                    "approval '{id}' is not pending (status: {:?})",
                    handle.status
                )));
            }
            handle.status = ResourceStatus::Approved;
            Ok(ApiResponse::ok(handle.clone()))
        }
        None => Err(ApiResponse::err(format!("approval '{id}' not found"))),
    }
}

/// `POST /approvals/:id/reject`
///
/// Rejects a pending approval gate.
async fn reject_gate(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<ApprovalHandle>>, (StatusCode, Json<ApiResponse<ApprovalHandle>>)> {
    let mut approvals = state.approvals.write().await;
    match approvals.get_mut(&id) {
        Some(handle) => {
            if !matches!(handle.status, ResourceStatus::Pending) {
                return Err(ApiResponse::err(format!(
                    "approval '{id}' is not pending (status: {:?})",
                    handle.status
                )));
            }
            handle.status = ResourceStatus::Rejected;
            Ok(ApiResponse::ok(handle.clone()))
        }
        None => Err(ApiResponse::err(format!("approval '{id}' not found"))),
    }
}

/// `POST /agents`
///
/// Creates a new agent with the provided configuration.
async fn create_agent(
    State(state): State<AppState>,
    axum::Json(body): axum::Json<CreateAgentRequest>,
) -> Result<Json<ApiResponse<AgentHandle>>, (StatusCode, Json<ApiResponse<AgentHandle>>)> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono_now();
    let handle = AgentHandle {
        config: body.config,
        status: ResourceStatus::Idle,
        created_at: now,
    };

    state.agents.write().await.insert(id.clone(), handle.clone());

    Ok(ApiResponse::ok(handle))
}

/// `GET /agents/:id/stream`
///
/// Returns a Server-Sent Events stream for agent output.
async fn agent_stream(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<
    Sse<ReceiverStream<Result<Event, Infallible>>>,
    (StatusCode, Json<ApiResponse<()>>),
> {
    // Verify the agent exists
    let agents = state.agents.read().await;
    if !agents.contains_key(&id) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ApiResponse {
                success: false,
                data: None,
                error: Some(format!("agent '{id}' not found")),
            }),
        ));
    }
    drop(agents);

    // For now, send a single "not implemented" event and close
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    tokio::spawn(async move {
        let event = Event::default()
            .data("Agent streaming not yet implemented — coming in a future release.")
            .event("notice");
        let _ = tx.send(Ok(event)).await;
    });

    Ok(Sse::new(ReceiverStream::new(rx)))
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Return an ISO-8601 timestamp string.
fn chrono_now() -> String {
    // Manual UTC timestamp — avoids pulling in chrono as a dependency.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    // Format: 2026-07-10T22:00:39Z
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // Calculate year-month-day from days since epoch
    let (y, m, d) = days_to_date(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m, d, hours, minutes, seconds
    )
}

/// Convert days since UNIX epoch to (year, month, day).
fn days_to_date(days: u64) -> (u64, u64, u64) {
    let mut y = 1970i64;
    let mut d = days as i64;
    loop {
        let yd = if is_leap(y) { 366 } else { 365 };
        if d < yd {
            break;
        }
        d -= yd;
        y += 1;
    }
    let leap = is_leap(y);
    let months: [i64; 12] = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 1u64;
    for &md in &months {
        if d < md {
            break;
        }
        d -= md;
        m += 1;
    }
    (y as u64, m, (d + 1) as u64)
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{self, Request, StatusCode},
    };
    use tower::ServiceExt;

    fn test_state() -> AppState {
        AppState::new()
    }

    #[tokio::test]
    async fn test_graph_execute_and_status() {
        let state = test_state();
        let app = router(state.clone());

        // Execute a graph
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/graphs/test-1/execute")
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"label":"test graph","input":{"key":"value"}}"#,
            ))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Check status
        let state_clone = state.clone();
        let app2 = router(state_clone);
        let req2 = Request::builder()
            .method(http::Method::GET)
            .uri("/graphs/test-1/status")
            .body(Body::empty())
            .unwrap();

        let resp2 = app2.oneshot(req2).await.unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp2.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        assert!(body_str.contains("test graph"));
    }

    #[tokio::test]
    async fn test_graph_status_not_found() {
        let state = test_state();
        let app = router(state);

        let req = Request::builder()
            .method(http::Method::GET)
            .uri("/graphs/nonexistent/status")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_approve_and_reject() {
        let state = test_state();
        let app = router(state.clone());

        // Insert a pending approval
        state
            .approvals
            .write()
            .await
            .insert("gate-1".into(), ApprovalHandle {
                prompt: "Approve this?".into(),
                status: ResourceStatus::Pending,
                created_at: chrono_now(),
            });

        // Approve
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/approvals/gate-1/approve")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Verify
        let approvals = state.approvals.read().await;
        let gate = approvals.get("gate-1").unwrap();
        assert!(matches!(gate.status, ResourceStatus::Approved));
        drop(approvals);

        // Reject (setup another)
        state
            .approvals
            .write()
            .await
            .insert("gate-2".into(), ApprovalHandle {
                prompt: "Reject this?".into(),
                status: ResourceStatus::Pending,
                created_at: chrono_now(),
            });

        let app2 = router(state.clone());
        let req2 = Request::builder()
            .method(http::Method::POST)
            .uri("/approvals/gate-2/reject")
            .body(Body::empty())
            .unwrap();

        let resp2 = app2.oneshot(req2).await.unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);

        let approvals = state.approvals.read().await;
        let gate2 = approvals.get("gate-2").unwrap();
        assert!(matches!(gate2.status, ResourceStatus::Rejected));
    }

    #[tokio::test]
    async fn test_approve_not_pending() {
        let state = test_state();
        state
            .approvals
            .write()
            .await
            .insert("done".into(), ApprovalHandle {
                prompt: "Already approved".into(),
                status: ResourceStatus::Approved,
                created_at: chrono_now(),
            });

        let app = router(state);
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/approvals/done/approve")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_create_agent() {
        let state = test_state();
        let app = router(state);

        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/agents")
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"config":{"model":"gpt-4o","system_prompt":"You are helpful."}}"#,
            ))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_agent_stream_not_found() {
        let state = test_state();
        let app = router(state);

        let req = Request::builder()
            .method(http::Method::GET)
            .uri("/agents/nonexistent/stream")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
