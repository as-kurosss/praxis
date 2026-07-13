//! **Routes** — HTTP handlers for the Praxis API server.
//!
//! # API Endpoints
//!
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | `GET` | `/` | Web Console (SPA) |
//! | `GET` | `/api/providers` | List providers |
//! | `POST` | `/api/providers` | Create a provider |
//! | `PUT` | `/api/providers/{id}` | Update a provider |
//! | `DELETE` | `/api/providers/{id}` | Delete a provider |
//! | `GET` | `/api/agents` | List agent definitions |
//! | `POST` | `/api/agents` | Create an agent |
//! | `GET` | `/api/agents/{id}` | Get agent definition |
//! | `PUT` | `/api/agents/{id}` | Update an agent |
//! | `DELETE` | `/api/agents/{id}` | Delete an agent |
//! | `POST` | `/api/agents/{id}/chat` | Send a message (non-streaming) |
//! | `GET` | `/api/agents/{id}/chat/stream` | SSE stream chat |
//! | `GET` | `/api/agents/{id}/sessions` | List sessions for an agent |
//! | `DELETE` | `/api/sessions/{id}` | Delete a session |
//! | `GET` | `/api/skills` | List skills |
//! | `POST` | `/api/skills` | Add a skill |
//! | `DELETE` | `/api/skills/{id}` | Delete a skill |
//! | `POST` | `/api/skills/{id}/toggle` | Toggle skill enabled |
//! | `POST` | `/api/skills/import` | Import a skill from URL |
//! | `GET` | `/api/settings` | Get settings |
//! | `PUT` | `/api/settings` | Update settings |
//! | `GET` | `/api/memory/search` | Search memory |
//! | `GET` | `/api/security/policies` | List security policies |
//! | `GET` | `/api/observe/traces` | List traces |
//! | `GET` | `/api/logs` | List log entries |
//! | `GET` | `/api/sessions/{id}/title` | Get session title |
//! | `PUT` | `/api/sessions/{id}/title` | Update session title |

use crate::state::AppState;
use axum::{
    Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Json, Sse, sse::Event},
    routing::{delete, get, post, put},
};
use futures::stream::{self, StreamExt};
use praxis_core::agent::{Agent, AgentConfig, ChatMessage, StreamChunk, ToolSet};
use praxis_core::loops::{Context, CycleType, Loop, LoopId, StopCondition};
use praxis_core::registry::{
    AgentDefinition, ProviderConfig, ProviderKind, ScrollConfig, Session, ToolBinding,
};
use praxis_core::tools::{CalculatorTool, CustomTool, TimeTool};
use praxis_runtime::{AnthropicClient, GeminiClient, OpenAiClient};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::time::Duration;
use tokio_stream::wrappers::ReceiverStream;
use tower_http::services::{ServeDir, ServeFile};

// ── Response wrapper ──────────────────────────────────────────────────

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

// ── Request types ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CreateProviderRequest {
    id: Option<String>,
    kind: ProviderKind,
    label: String,
    api_key: String,
    model: String,
    api_url: Option<String>,
}

#[derive(Deserialize)]
struct CreateAgentRequest {
    id: Option<String>,
    name: String,
    description: Option<String>,
    provider_id: String,
    system_prompt: String,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    scroll_strategy: Option<ScrollConfig>,
    tools: Option<Vec<ToolBinding>>,
}

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
    session_id: Option<String>,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
}

#[derive(Serialize)]
struct ChatResponse {
    session_id: String,
    message: String,
}

#[derive(Serialize)]
struct AgentSummary {
    id: String,
    name: String,
    description: Option<String>,
    provider_id: String,
    system_prompt: String,
    tool_count: usize,
    created_at: String,
    updated_at: String,
}

#[derive(Deserialize)]
struct ChatStreamParams {
    message: String,
    session_id: Option<String>,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
}

#[derive(Serialize)]
struct SessionSummaryResponse {
    id: String,
    agent_id: String,
    title: Option<String>,
    message_count: usize,
    created_at: String,
    updated_at: String,
    preview: Vec<String>,
}

// ── Build the router ──────────────────────────────────────────────────

pub fn router(state: AppState) -> Router {
    let dist_dir = state.dist_dir.clone();

    let serve_dir = ServeDir::new(&dist_dir).fallback(ServeFile::new(dist_dir.join("index.html")));

    Router::new()
        // Provider CRUD
        .route("/api/providers", get(list_providers).post(create_provider))
        .route(
            "/api/providers/{id}",
            put(update_provider).delete(delete_provider),
        )
        // Agent CRUD
        .route("/api/agents", get(list_agents).post(create_agent))
        .route(
            "/api/agents/{id}",
            get(get_agent).put(update_agent).delete(delete_agent),
        )
        // Chat
        .route("/api/agents/{id}/chat", post(chat_handler))
        .route("/api/agents/{id}/chat/stream", get(chat_stream_handler))
        // Sessions
        .route("/api/agents/{id}/sessions", get(list_sessions))
        .route(
            "/api/sessions/{id}",
            get(get_session_detail).delete(delete_session),
        )
        .route(
            "/api/sessions/{id}/title",
            get(get_session_title).put(update_session_title),
        )
        // Skills
        .route("/api/skills", get(list_skills).post(create_skill))
        .route("/api/skills/{id}", delete(delete_skill))
        .route("/api/skills/{id}/toggle", post(toggle_skill))
        .route("/api/skills/import", post(import_skill))
        // Settings
        .route("/api/settings", get(get_settings).put(update_settings))
        // Memory
        .route("/api/memory/search", get(search_memory))
        // Security
        .route("/api/security/policies", get(list_security_policies))
        // Observability
        .route("/api/observe/traces", get(list_traces))
        // Logs
        .route("/api/logs", get(list_logs))
        .with_state(state)
        // Static files (SPA)
        .fallback_service(serve_dir)
}

// ── Helpers ───────────────────────────────────────────────────────────

fn build_tool_set(tools: &[ToolBinding]) -> ToolSet {
    let mut ts = ToolSet::new();
    for binding in tools {
        match binding {
            ToolBinding::Builtin {
                name,
                enabled: true,
            } => {
                match name.as_str() {
                    "calculator" => ts.add(CalculatorTool),
                    "time" | "current_time" => ts.add(TimeTool),
                    _ => { /* unknown builtin — skip */ }
                }
            }
            ToolBinding::Custom {
                name,
                description,
                schema,
                enabled: true,
            } => {
                ts.add(CustomTool::new(name, description, schema.clone()));
            }
            _ => {}
        }
    }
    ts
}

fn scroll_strategy(config: &ScrollConfig) -> Option<praxis_core::memory::ScrollStrategy> {
    match config {
        ScrollConfig::Truncate { max_messages } => {
            Some(praxis_core::memory::ScrollStrategy::Truncate {
                max_messages: *max_messages,
            })
        }
        ScrollConfig::SlidingWindow { window_size } => {
            Some(praxis_core::memory::ScrollStrategy::SlidingWindow {
                window_size: *window_size,
            })
        }
        ScrollConfig::NoOp => None,
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(3)])
    }
}

/// Create an OpenAI-compatible client for Ollama or OpenAI.
fn openai_client(provider: &ProviderConfig) -> OpenAiClient {
    let default_url = if provider.kind == ProviderKind::Ollama {
        "http://localhost:11434/v1"
    } else {
        "https://api.openai.com/v1"
    };
    let url = provider.api_url.as_deref().unwrap_or(default_url);
    OpenAiClient::new(url, &provider.api_key, &provider.model)
}

/// Dispatch agent execution to the right LLM provider.
async fn run_agent_execution(
    provider: &ProviderConfig,
    config: AgentConfig,
    tool_set: ToolSet,
    ctx: Context<String>,
    state: &mut Vec<ChatMessage>,
) -> praxis_core::loops::LoopResult<String> {
    match provider.kind {
        ProviderKind::Openai | ProviderKind::Ollama => {
            let agent = Agent::with_tools(openai_client(provider), config, tool_set);
            agent.execute(ctx, state).await
        }
        ProviderKind::Anthropic => {
            let url = provider
                .api_url
                .as_deref()
                .unwrap_or("https://api.anthropic.com/v1");
            let client = AnthropicClient::new(url, &provider.api_key, &provider.model);
            let agent = Agent::with_tools(client, config, tool_set);
            agent.execute(ctx, state).await
        }
        ProviderKind::Gemini => {
            let client = GeminiClient::new(&provider.api_key, &provider.model);
            let agent = Agent::with_tools(client, config, tool_set);
            agent.execute(ctx, state).await
        }
    }
}

/// Dispatch streaming agent execution to the right LLM provider.
async fn run_agent_streaming(
    provider: &ProviderConfig,
    config: AgentConfig,
    tool_set: ToolSet,
    ctx: Context<String>,
    state: &mut Vec<ChatMessage>,
    tx: tokio::sync::mpsc::Sender<StreamChunk>,
) -> praxis_core::loops::LoopResult<String> {
    match provider.kind {
        ProviderKind::Openai | ProviderKind::Ollama => {
            let agent = Agent::with_tools(openai_client(provider), config, tool_set);
            agent.execute_stream(ctx, state, tx).await
        }
        ProviderKind::Anthropic => {
            let url = provider
                .api_url
                .as_deref()
                .unwrap_or("https://api.anthropic.com/v1");
            let client = AnthropicClient::new(url, &provider.api_key, &provider.model);
            let agent = Agent::with_tools(client, config, tool_set);
            agent.execute_stream(ctx, state, tx).await
        }
        ProviderKind::Gemini => {
            let client = GeminiClient::new(&provider.api_key, &provider.model);
            let agent = Agent::with_tools(client, config, tool_set);
            agent.execute_stream(ctx, state, tx).await
        }
    }
}

// ── Provider CRUD ────────────────────────────────────────────────────

async fn list_providers(State(state): State<AppState>) -> Json<ApiResponse<Vec<ProviderConfig>>> {
    ApiResponse::ok(state.registry.list_providers())
}

async fn create_provider(
    State(state): State<AppState>,
    axum::Json(body): axum::Json<CreateProviderRequest>,
) -> Result<Json<ApiResponse<ProviderConfig>>, (StatusCode, Json<ApiResponse<()>>)> {
    let id = body.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let config = ProviderConfig {
        id,
        kind: body.kind,
        label: body.label,
        api_url: body.api_url,
        api_key: body.api_key,
        model: body.model,
        notes: None,
    };
    state
        .registry
        .upsert_provider(config.clone())
        .map_err(|e| ApiResponse::err(format!("Failed to save provider: {e}")))?;
    Ok(ApiResponse::ok(config))
}

async fn update_provider(
    State(state): State<AppState>,
    Path(id): Path<String>,
    axum::Json(body): axum::Json<CreateProviderRequest>,
) -> Result<Json<ApiResponse<ProviderConfig>>, (StatusCode, Json<ApiResponse<()>>)> {
    let config = ProviderConfig {
        id,
        kind: body.kind,
        label: body.label,
        api_url: body.api_url,
        api_key: body.api_key,
        model: body.model,
        notes: None,
    };
    state
        .registry
        .upsert_provider(config.clone())
        .map_err(|e| ApiResponse::err(format!("Failed to save provider: {e}")))?;
    Ok(ApiResponse::ok(config))
}

async fn delete_provider(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<bool>>, (StatusCode, Json<ApiResponse<bool>>)> {
    state
        .registry
        .delete_provider(&id)
        .map_err(|e| ApiResponse::err(format!("Failed to delete provider: {e}")))?;
    Ok(ApiResponse::ok(true))
}

// ── Agent CRUD ───────────────────────────────────────────────────────

async fn list_agents(State(state): State<AppState>) -> Json<ApiResponse<Vec<AgentSummary>>> {
    let agents = state.registry.list_agents();
    let summaries: Vec<AgentSummary> = agents
        .into_iter()
        .map(|a| AgentSummary {
            id: a.id,
            name: a.name,
            description: a.description,
            provider_id: a.provider_id,
            system_prompt: truncate(&a.system_prompt, 80),
            tool_count: a.tools.len(),
            created_at: a.created_at,
            updated_at: a.updated_at,
        })
        .collect();
    ApiResponse::ok(summaries)
}

async fn get_agent(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<AgentDefinition>>, (StatusCode, Json<ApiResponse<()>>)> {
    match state.registry.get_agent(&id) {
        Some(agent) => Ok(ApiResponse::ok(agent)),
        None => Err(ApiResponse::err(format!("Agent '{id}' not found"))),
    }
}

async fn create_agent(
    State(state): State<AppState>,
    axum::Json(body): axum::Json<CreateAgentRequest>,
) -> Result<Json<ApiResponse<AgentDefinition>>, (StatusCode, Json<ApiResponse<()>>)> {
    let now = praxis_core::registry::timestamp();
    let id = body.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let def = AgentDefinition {
        id,
        name: body.name,
        description: body.description,
        provider_id: body.provider_id,
        system_prompt: body.system_prompt,
        temperature: body.temperature,
        max_tokens: body.max_tokens,
        scroll_strategy: body.scroll_strategy.unwrap_or_default(),
        tools: body.tools.unwrap_or_default(),
        created_at: now.clone(),
        updated_at: now,
    };
    state
        .registry
        .upsert_agent(def.clone())
        .map_err(|e| ApiResponse::err(format!("Failed to save agent: {e}")))?;
    Ok(ApiResponse::ok(def))
}

async fn update_agent(
    State(state): State<AppState>,
    Path(id): Path<String>,
    axum::Json(body): axum::Json<CreateAgentRequest>,
) -> Result<Json<ApiResponse<AgentDefinition>>, (StatusCode, Json<ApiResponse<()>>)> {
    // Keep original created_at
    let created_at = state
        .registry
        .get_agent(&id)
        .map(|a| a.created_at)
        .unwrap_or_else(|| praxis_core::registry::timestamp());

    let now = praxis_core::registry::timestamp();
    let def = AgentDefinition {
        id,
        name: body.name,
        description: body.description,
        provider_id: body.provider_id,
        system_prompt: body.system_prompt,
        temperature: body.temperature,
        max_tokens: body.max_tokens,
        scroll_strategy: body.scroll_strategy.unwrap_or_default(),
        tools: body.tools.unwrap_or_default(),
        created_at,
        updated_at: now,
    };
    state
        .registry
        .upsert_agent(def.clone())
        .map_err(|e| ApiResponse::err(format!("Failed to save agent: {e}")))?;
    Ok(ApiResponse::ok(def))
}

async fn delete_agent(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<bool>>, (StatusCode, Json<ApiResponse<bool>>)> {
    state
        .registry
        .delete_agent(&id)
        .map_err(|e| ApiResponse::err(format!("Failed to delete agent: {e}")))?;
    Ok(ApiResponse::ok(true))
}

// ── Chat ─────────────────────────────────────────────────────────────

async fn chat_handler(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    axum::Json(body): axum::Json<ChatRequest>,
) -> Result<Json<ApiResponse<ChatResponse>>, (StatusCode, Json<ApiResponse<()>>)> {
    // 1. Look up the agent definition
    let def = state
        .registry
        .get_agent(&agent_id)
        .ok_or_else(|| ApiResponse::err(format!("Agent '{agent_id}' not found")))?;

    // 2. Look up the provider
    let provider = state
        .registry
        .get_provider(&def.provider_id)
        .ok_or_else(|| ApiResponse::err(format!("Provider '{}' not found", def.provider_id)))?;

    // 3. Build tool set
    let tool_set = build_tool_set(&def.tools);

    // 4. Build agent config
    let config = AgentConfig {
        model: provider.model.clone(),
        system_prompt: def.system_prompt.clone(),
        temperature: body.temperature.or(def.temperature),
        max_tokens: body.max_tokens.or(def.max_tokens),
        scroll_strategy: scroll_strategy(&def.scroll_strategy),
    };

    // 5. Get or create session
    let session_id = body
        .session_id
        .unwrap_or_else(|| format!("sess_{}", uuid::Uuid::new_v4()));

    // 6. Build context
    let ctx = Context::new(
        LoopId::new(),
        CycleType::Turn,
        StopCondition::new(Some(25), Some(Duration::from_secs(120))),
        body.message,
    );

    // 7. Load existing session messages or start fresh
    let mut state_messages: Vec<ChatMessage> = state
        .sessions
        .get_session(&session_id)
        .map(|s| s.messages)
        .unwrap_or_default();

    // 8. Execute with provider dispatch
    let result = run_agent_execution(&provider, config, tool_set, ctx, &mut state_messages).await;

    // 9. Save session
    let mut session = state
        .sessions
        .get_session(&session_id)
        .unwrap_or_else(|| Session::new(&agent_id));
    session.id = session_id.clone();
    session.agent_id = agent_id;
    session.messages = state_messages;
    let _ = state.sessions.upsert_session(session);

    match result.output {
        Some(output) => Ok(ApiResponse::ok(ChatResponse {
            session_id,
            message: output,
        })),
        None => Err(ApiResponse::err(format!(
            "Agent failed: {:?}",
            result.status
        ))),
    }
}

async fn chat_stream_handler(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    axum::extract::Query(params): axum::extract::Query<ChatStreamParams>,
) -> Result<
    Sse<impl futures::Stream<Item = Result<Event, Infallible>>>,
    (StatusCode, Json<ApiResponse<()>>),
> {
    let def = state
        .registry
        .get_agent(&agent_id)
        .ok_or_else(|| ApiResponse::err(format!("Agent '{agent_id}' not found")))?;

    let provider = state
        .registry
        .get_provider(&def.provider_id)
        .ok_or_else(|| ApiResponse::err(format!("Provider '{}' not found", def.provider_id)))?;

    let tool_set = build_tool_set(&def.tools);

    let config = AgentConfig {
        model: provider.model.clone(),
        system_prompt: def.system_prompt.clone(),
        temperature: params.temperature.or(def.temperature),
        max_tokens: params.max_tokens.or(def.max_tokens),
        scroll_strategy: scroll_strategy(&def.scroll_strategy),
    };

    let session_id = params
        .session_id
        .unwrap_or_else(|| format!("sess_{}", uuid::Uuid::new_v4()));

    let ctx = Context::new(
        LoopId::new(),
        CycleType::Turn,
        StopCondition::new(Some(25), Some(Duration::from_secs(120))),
        params.message.clone(),
    );

    let mut state_messages: Vec<ChatMessage> = state
        .sessions
        .get_session(&session_id)
        .map(|s| s.messages)
        .unwrap_or_default();

    let (tx, rx) = tokio::sync::mpsc::channel(256);
    let state_clone = state.clone();
    let agent_id_clone = agent_id.clone();
    let session_id_for_spawn = session_id.clone();
    let session_id_for_event = session_id.clone();

    tokio::spawn(async move {
        let _result = run_agent_streaming(
            &provider,
            config,
            tool_set,
            ctx,
            &mut state_messages,
            tx.clone(),
        )
        .await;

        // IMPORTANT: Drop the original tx sender IMMEDIATELY after streaming
        // completes, BEFORE saving the session.  The Sse stream wraps rx
        // (ReceiverStream) and will NOT close until ALL Sender handles are
        // dropped.  If we keep tx alive during session save, the SSE connection
        // stays open, the browser doesn't get a clean connection-close, and
        // EventSource fires 'error' — causing the client to issue a fallback
        // POST and creating a second LLM request (duplicate user message).
        drop(tx);

        // Save session on completion
        let mut session = state_clone
            .sessions
            .get_session(&session_id_for_spawn)
            .unwrap_or_else(|| Session::new(&agent_id_clone));
        session.id = session_id_for_spawn;
        session.agent_id = agent_id_clone;
        session.messages = state_messages;
        let _ = state_clone.sessions.upsert_session(session);
    });

    // Prepend a session_id event so the frontend knows the session
    let session_event = stream::once(async move {
        Ok(Event::default()
            .data(session_id_for_event)
            .event("session_id"))
    });

    let stream = session_event.chain(ReceiverStream::new(rx).map(|chunk| {
        match chunk {
            StreamChunk::Token(text) => Ok(Event::default().data(text).event("token")),
            StreamChunk::Reasoning(text) => Ok(Event::default().data(text).event("reasoning")),
            StreamChunk::ToolCallStart { id, name } => Ok(Event::default()
                .data(serde_json::json!({"id": id, "name": name}).to_string())
                .event("tool_call_start")),
            StreamChunk::ToolCallEnd { id } => Ok(Event::default()
                .data(serde_json::json!({"id": id}).to_string())
                .event("tool_call_end")),
            StreamChunk::ToolCallArguments { id, arguments } => Ok(Event::default()
                .data(serde_json::json!({"id": id, "arguments": arguments}).to_string())
                .event("tool_call_arguments")),
            StreamChunk::Done => Ok(Event::default().data("").event("done")),
            StreamChunk::Error(msg) => Ok(Event::default().data(msg).event("error")),
        }
    }));

    Ok(Sse::new(stream))
}

// ── Sessions ─────────────────────────────────────────────────────────

#[derive(Serialize)]
struct SessionDetailResponse {
    id: String,
    agent_id: String,
    title: Option<String>,
    messages: Vec<ChatMessage>,
    message_count: usize,
    created_at: String,
    updated_at: String,
}

async fn get_session_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<SessionDetailResponse>>, (StatusCode, Json<ApiResponse<()>>)> {
    let session = state
        .sessions
        .get_session(&id)
        .ok_or_else(|| ApiResponse::err(format!("Session '{id}' not found")))?;
    Ok(ApiResponse::ok(SessionDetailResponse {
        id: session.id,
        agent_id: session.agent_id,
        title: session.title,
        message_count: session.messages.len(),
        messages: session.messages,
        created_at: session.created_at,
        updated_at: session.updated_at,
    }))
}

async fn list_sessions(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Json<ApiResponse<Vec<SessionSummaryResponse>>> {
    let summaries = state.sessions.list_sessions(&agent_id);
    let result: Vec<SessionSummaryResponse> = summaries
        .into_iter()
        .map(|s| {
            let preview = state
                .sessions
                .get_session(&s.id)
                .map(|session| {
                    session
                        .messages
                        .iter()
                        .take(3)
                        .filter_map(|m| m.content.clone())
                        .collect()
                })
                .unwrap_or_default();
            SessionSummaryResponse {
                id: s.id,
                agent_id: s.agent_id,
                title: s.title,
                message_count: s.message_count,
                created_at: s.created_at,
                updated_at: s.updated_at,
                preview,
            }
        })
        .collect();
    ApiResponse::ok(result)
}

async fn delete_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<bool>>, (StatusCode, Json<ApiResponse<bool>>)> {
    state
        .sessions
        .delete_session(&id)
        .map_err(|e| ApiResponse::err(format!("Failed to delete session: {e}")))?;
    Ok(ApiResponse::ok(true))
}

// ── Session Title ────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct SessionTitleRequest {
    title: String,
}

async fn get_session_title(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<Option<String>>>, (StatusCode, Json<ApiResponse<()>>)> {
    let session = state
        .sessions
        .get_session(&id)
        .ok_or_else(|| ApiResponse::err(format!("Session '{id}' not found")))?;
    Ok(ApiResponse::ok(session.title))
}

async fn update_session_title(
    State(state): State<AppState>,
    Path(id): Path<String>,
    axum::Json(body): axum::Json<SessionTitleRequest>,
) -> Result<Json<ApiResponse<bool>>, (StatusCode, Json<ApiResponse<bool>>)> {
    let mut session = state
        .sessions
        .get_session(&id)
        .ok_or_else(|| ApiResponse::err(format!("Session '{id}' not found")))?;
    session.title = Some(body.title);
    session.updated_at = praxis_core::registry::timestamp();
    state
        .sessions
        .upsert_session(session)
        .map_err(|e| ApiResponse::err(format!("Failed to update session title: {e}")))?;
    Ok(ApiResponse::ok(true))
}

// ── Skills (in-memory store) ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SkillEntry {
    id: String,
    name: String,
    description: String,
    enabled: bool,
    source_url: Option<String>,
    version: Option<String>,
    created_at: String,
}

#[derive(Deserialize)]
struct CreateSkillRequest {
    name: String,
    description: String,
    source_url: Option<String>,
    version: Option<String>,
}

#[derive(Deserialize)]
struct ToggleSkillRequest {
    enabled: bool,
}

#[derive(Deserialize)]
struct ImportSkillRequest {
    url: String,
}

static SKILLS: std::sync::LazyLock<std::sync::Mutex<Vec<SkillEntry>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(Vec::new()));

async fn list_skills() -> Json<ApiResponse<Vec<SkillEntry>>> {
    let skills = SKILLS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    ApiResponse::ok(skills)
}

async fn create_skill(
    axum::Json(body): axum::Json<CreateSkillRequest>,
) -> Json<ApiResponse<SkillEntry>> {
    let entry = SkillEntry {
        id: uuid::Uuid::new_v4().to_string(),
        name: body.name,
        description: body.description,
        enabled: true,
        source_url: body.source_url,
        version: body.version,
        created_at: praxis_core::registry::timestamp(),
    };
    SKILLS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(entry.clone());
    ApiResponse::ok(entry)
}

async fn delete_skill(Path(id): Path<String>) -> Json<ApiResponse<bool>> {
    let mut skills = SKILLS.lock().unwrap_or_else(|e| e.into_inner());
    let len_before = skills.len();
    skills.retain(|s| s.id != id);
    ApiResponse::ok(skills.len() < len_before)
}

async fn toggle_skill(
    Path(id): Path<String>,
    axum::Json(body): axum::Json<ToggleSkillRequest>,
) -> Result<Json<ApiResponse<bool>>, (StatusCode, Json<ApiResponse<bool>>)> {
    let mut skills = SKILLS.lock().unwrap_or_else(|e| e.into_inner());
    let skill = skills
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or_else(|| ApiResponse::err(format!("Skill '{id}' not found")))?;
    skill.enabled = body.enabled;
    Ok(ApiResponse::ok(true))
}

async fn import_skill(
    axum::Json(body): axum::Json<ImportSkillRequest>,
) -> Result<Json<ApiResponse<SkillEntry>>, (StatusCode, Json<ApiResponse<()>>)> {
    // Stub: create a skill entry from a URL without actually fetching
    let name = body.url.rsplit('/').next().unwrap_or(&body.url).to_string();
    let entry = SkillEntry {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        description: format!("Imported from {}", body.url),
        enabled: true,
        source_url: Some(body.url),
        version: None,
        created_at: praxis_core::registry::timestamp(),
    };
    SKILLS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(entry.clone());
    Ok(ApiResponse::ok(entry))
}

// ── Settings (in-memory store) ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppSettings {
    default_scroll_strategy: ScrollConfig,
    default_model: String,
    default_temperature: Option<f32>,
    default_max_tokens: Option<u32>,
    theme: String,
    language: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            default_scroll_strategy: ScrollConfig::Truncate { max_messages: 50 },
            default_model: "gpt-4o".into(),
            default_temperature: None,
            default_max_tokens: None,
            theme: "dark".into(),
            language: "en".into(),
        }
    }
}

static SETTINGS: std::sync::LazyLock<std::sync::Mutex<AppSettings>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(AppSettings::default()));

async fn get_settings() -> Json<ApiResponse<AppSettings>> {
    let settings = SETTINGS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    ApiResponse::ok(settings)
}

#[derive(Deserialize)]
struct UpdateSettingsRequest {
    default_scroll_strategy: Option<ScrollConfig>,
    default_model: Option<String>,
    default_temperature: Option<Option<f32>>,
    default_max_tokens: Option<Option<u32>>,
    theme: Option<String>,
    language: Option<String>,
}

async fn update_settings(
    axum::Json(body): axum::Json<UpdateSettingsRequest>,
) -> Json<ApiResponse<AppSettings>> {
    let mut settings = SETTINGS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(scroll) = body.default_scroll_strategy {
        settings.default_scroll_strategy = scroll;
    }
    if let Some(model) = body.default_model {
        settings.default_model = model;
    }
    if let Some(temp) = body.default_temperature {
        settings.default_temperature = temp;
    }
    if let Some(tokens) = body.default_max_tokens {
        settings.default_max_tokens = tokens;
    }
    if let Some(theme) = body.theme {
        settings.theme = theme;
    }
    if let Some(lang) = body.language {
        settings.language = lang;
    }
    ApiResponse::ok(settings.clone())
}

// ── Memory Search (stub) ──────────────────────────────────────────────────

#[derive(Deserialize)]
struct MemorySearchParams {
    #[allow(dead_code)]
    q: Option<String>,
}

#[derive(Serialize)]
struct MemorySearchResult {
    id: String,
    content: String,
    agent_id: String,
    session_id: String,
    similarity: f64,
    created_at: String,
}

async fn search_memory(
    _query: Query<MemorySearchParams>,
) -> Json<ApiResponse<Vec<MemorySearchResult>>> {
    // Stub: return empty results
    let results: Vec<MemorySearchResult> = Vec::new();
    ApiResponse::ok(results)
}

// ── Security Policies (stub) ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SecurityPolicy {
    id: String,
    name: String,
    description: String,
    action: String,
    rules: Vec<SecurityRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SecurityRule {
    id: String,
    name: String,
    action: String,
    pattern: String,
}

static POLICIES: std::sync::LazyLock<std::sync::Mutex<Vec<SecurityPolicy>>> =
    std::sync::LazyLock::new(|| {
        std::sync::Mutex::new(vec![
            SecurityPolicy {
                id: "pol_shell".into(),
                name: "Shell Access".into(),
                description: "Controls shell command execution by agents".into(),
                action: "ask".into(),
                rules: vec![
                    SecurityRule {
                        id: "rule_shell_rm".into(),
                        name: "Restrict rm -rf".into(),
                        action: "deny".into(),
                        pattern: "rm\\s+-rf".into(),
                    },
                    SecurityRule {
                        id: "rule_shell_curl".into(),
                        name: "Network requests".into(),
                        action: "ask".into(),
                        pattern: "(curl|wget|nc)".into(),
                    },
                ],
            },
            SecurityPolicy {
                id: "pol_filesystem".into(),
                name: "Filesystem Access".into(),
                description: "Controls file system read/write access".into(),
                action: "allow".into(),
                rules: vec![SecurityRule {
                    id: "rule_fs_read".into(),
                    name: "Read access".into(),
                    action: "allow".into(),
                    pattern: "/workspace/**".into(),
                }],
            },
        ])
    });

async fn list_security_policies() -> Json<ApiResponse<Vec<SecurityPolicy>>> {
    let policies = POLICIES.lock().unwrap_or_else(|e| e.into_inner()).clone();
    ApiResponse::ok(policies)
}

// ── Observability / Traces (stub) ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TraceSpan {
    id: String,
    trace_id: String,
    name: String,
    start_time: String,
    end_time: String,
    duration_ms: u64,
    status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TraceResponse {
    id: String,
    agent_id: String,
    session_id: String,
    spans: Vec<TraceSpan>,
    total_duration_ms: u64,
    total_tokens: u64,
    created_at: String,
}

static TRACES: std::sync::LazyLock<std::sync::Mutex<Vec<TraceResponse>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(Vec::new()));

async fn list_traces() -> Json<ApiResponse<Vec<TraceResponse>>> {
    let traces = TRACES.lock().unwrap_or_else(|e| e.into_inner()).clone();
    ApiResponse::ok(traces)
}

// ── Logs (stub) ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LogEntry {
    timestamp: String,
    level: String,
    message: String,
    target: String,
}

static LOGS: std::sync::LazyLock<std::sync::Mutex<Vec<LogEntry>>> =
    std::sync::LazyLock::new(|| {
        std::sync::Mutex::new(vec![
            LogEntry {
                timestamp: praxis_core::registry::timestamp(),
                level: "info".into(),
                message: "Praxis API server started".into(),
                target: "praxis_api_server".into(),
            },
            LogEntry {
                timestamp: praxis_core::registry::timestamp(),
                level: "info".into(),
                message: "Loading registry...".into(),
                target: "praxis_api_server".into(),
            },
        ])
    });

async fn list_logs() -> Json<ApiResponse<Vec<LogEntry>>> {
    let logs = LOGS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    ApiResponse::ok(logs)
}
