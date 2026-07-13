//! **Agent** runtime — an LLM-powered agent that implements the [`Loop`] trait.
//!
//! The [`Agent`] holds an LLM client, a set of tools, and a configuration.
//! When executed it runs a tool-calling loop:  call LLM → execute tools →
//! feed results back → repeat until the LLM produces a final text response.

use super::llm::{ChatMessage, ChatRequest, LlmClient, StreamChunk, ToolCall};
use super::tool::ToolSet;
use crate::loops::{Context, Loop, LoopResult};
use crate::memory::EpisodicMemory;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Instant;

/// Configuration for an [`Agent`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Model identifier (e.g. "gpt-4o", "claude-3-5-sonnet").
    pub model: String,
    /// System prompt for the agent.
    pub system_prompt: String,
    /// Sampling temperature (None = provider default).
    pub temperature: Option<f32>,
    /// Maximum tokens in the LLM response (None = provider default).
    pub max_tokens: Option<u32>,
    /// Scroll strategy for managing conversation history length.
    /// `None` means no trimming (equivalent to [`ScrollStrategy::NoOp`]).
    /// Skipped during serialization (closures are not serializable).
    #[serde(skip)]
    pub scroll_strategy: Option<super::memory::ScrollStrategy>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            model: "gpt-4o".into(),
            system_prompt: "You are a helpful assistant.".into(),
            temperature: None,
            max_tokens: None,
            scroll_strategy: None,
        }
    }
}

/// An LLM-powered agent that uses tools to accomplish tasks.
///
/// Implements [`Loop`] — can be used directly or as a node in a [`Graph`].
///
/// # Type parameters
/// * `L` — the LLM client type (must implement [`LlmClient`]).
///
/// # Execution flow
/// 1. Add the user message (from `ctx.input`) to the conversation state.
/// 2. Call the LLM with the full conversation + tool schemas.
/// 3. If the LLM responds with tool calls:
///    a. Execute each tool and append results to the conversation.
///    b. Go back to step 2 (auto-continue).
/// 4. If the LLM responds with text, return it as the final output.
pub struct Agent<L: LlmClient> {
    client: L,
    config: AgentConfig,
    tools: ToolSet,
    episodic_memory: Option<Mutex<EpisodicMemory>>,
    turn_counter: AtomicU64,
}

impl<L: LlmClient> Agent<L> {
    /// Create a new agent with the given LLM client and config.
    pub fn new(client: L, config: AgentConfig) -> Self {
        Self {
            client,
            config,
            tools: ToolSet::new(),
            episodic_memory: None,
            turn_counter: AtomicU64::new(0),
        }
    }

    /// Create an agent with pre-configured tools.
    pub fn with_tools(client: L, config: AgentConfig, tools: ToolSet) -> Self {
        Self {
            client,
            config,
            tools,
            episodic_memory: None,
            turn_counter: AtomicU64::new(0),
        }
    }

    /// Attach an episodic memory to this agent for full history recording.
    pub fn with_episodic_memory(mut self, memory: EpisodicMemory) -> Self {
        self.episodic_memory = Some(Mutex::new(memory));
        self
    }

    /// Reference to the optional episodic memory (for inspection).
    pub fn episodic_memory(&self) -> Option<&Mutex<EpisodicMemory>> {
        self.episodic_memory.as_ref()
    }

    /// Register a tool that the agent can call.
    pub fn add_tool<T: crate::agent::tool::Tool + 'static>(&mut self, tool: T) {
        self.tools.add(tool);
    }

    /// Reference to the tool set.
    pub fn tools(&self) -> &ToolSet {
        &self.tools
    }

    /// Execute the agent with [`StreamChunk`] output.
    ///
    /// Accepts a [`tokio::sync::mpsc::Sender`] that will receive tokens,
    /// tool call boundaries, and a final `Done` or `Error` chunk.
    /// Wrap in `tokio::spawn` to run in the background.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    /// let agent_clone = /* share via Arc or spawn */;
    /// let result = agent.execute_stream(ctx, &mut state, tx).await;
    /// ```
    pub async fn execute_stream(
        &self,
        ctx: Context<String>,
        state: &mut Vec<ChatMessage>,
        chunk_sender: tokio::sync::mpsc::Sender<StreamChunk>,
    ) -> LoopResult<String> {
        let result = self
            .execute_impl(ctx, state, Some(chunk_sender.clone()))
            .await;
        let _ = chunk_sender.try_send(StreamChunk::Done);
        result
    }

    /// Internal execution with optional chunk streaming.
    async fn execute_impl(
        &self,
        ctx: Context<String>,
        state: &mut Vec<ChatMessage>,
        chunk_sender: Option<tokio::sync::mpsc::Sender<StreamChunk>>,
    ) -> LoopResult<String> {
        let start = Instant::now();
        let max_iter = ctx.stop_condition.max_iterations.unwrap_or(25);
        let timeout = ctx.stop_condition.timeout;

        // Helper to send a chunk (best-effort)
        let send_chunk = |sender: &Option<tokio::sync::mpsc::Sender<StreamChunk>>,
                          chunk: StreamChunk| {
            if let Some(tx) = sender {
                let _ = tx.try_send(chunk);
            }
        };

        // Add user message to conversation state
        let input = ctx.input.clone();
        // Skip if the last message is already the same user input
        // (avoids duplication when a streaming attempt and a fallback POST
        // race on the same session — both call execute_impl, but only one
        // should push the user message).
        let is_dup = state
            .last()
            .map(|m| m.role.as_str() == "user" && m.content.as_deref() == Some(&ctx.input))
            .unwrap_or(false);
        if !is_dup {
            state.push(ChatMessage::user(ctx.input));
        }

        // Apply scroll strategy and record evicted turns in episodic memory
        if let Some(ref strategy) = self.config.scroll_strategy {
            let before = state.clone();
            strategy.apply(state);
            if before.len() > state.len() {
                let counter = self.turn_counter.fetch_add(1, Ordering::SeqCst);
                if let Some(ref episodic_mutex) = self.episodic_memory {
                    if let Ok(mut episodic) = episodic_mutex.lock() {
                        let turn_id = format!("turn_{}", counter + 1);
                        crate::memory::record_evicted_turn(
                            &mut episodic,
                            &turn_id,
                            &input,
                            &before,
                            state,
                        );
                    }
                }
            }
        }

        for iteration in 1..=max_iter {
            // Check graph-level timeout
            if let Some(limit) = timeout
                && start.elapsed() >= limit
            {
                let elapsed = crate::loops::elapsed_ms(&start);
                send_chunk(
                    &chunk_sender,
                    StreamChunk::Error(format!("timeout after {elapsed}ms")),
                );
                return LoopResult::failure(
                    format!("agent timeout after {elapsed}ms"),
                    iteration,
                    elapsed,
                );
            }

            // Build request: system prompt + conversation state
            // Filter out empty assistant messages (no content, no tool calls) —
            // they can appear from partial saves and cause 400 errors on some providers.
            let mut messages = Vec::with_capacity(state.len() + 1);
            messages.push(ChatMessage::system(&self.config.system_prompt));
            messages.extend(
                state
                    .iter()
                    .filter(|m| {
                        if m.role.as_str() == "assistant"
                            && m.content.is_none()
                            && m.reasoning_content.is_none()
                        {
                            m.tool_calls.as_ref().is_some_and(|c| !c.is_empty())
                        } else {
                            true
                        }
                    })
                    .cloned(),
            );

            let request = ChatRequest {
                messages,
                tools: Some(self.tools.specs()),
                temperature: self.config.temperature,
                max_tokens: self.config.max_tokens,
            };

            // Call LLM — streaming (when chunk_sender is present) or non-streaming
            let reasoning_text;
            let (response_text, has_content, has_tool_calls, tool_calls) = if chunk_sender.is_some()
            {
                // ── Streaming path ──
                // If chat_stream is unsupported (e.g. mock clients), fall back
                // to non-streaming chat + emit the full text as a single chunk.
                let mut rx = match self.client.chat_stream(request.clone()).await {
                    Ok(rx) => rx,
                    Err(_e) => {
                        // Streaming not supported / failed — fall back to non-streaming
                        let response = match self.client.chat(request).await {
                            Ok(r) => r,
                            Err(e) => {
                                send_chunk(
                                    &chunk_sender,
                                    StreamChunk::Error(format!("LLM error: {e}")),
                                );
                                return LoopResult::failure(
                                    format!("LLM error: {e}"),
                                    iteration,
                                    crate::loops::elapsed_ms(&start),
                                );
                            }
                        };
                        let fallback_msg = response.message;
                        let fallback_text = fallback_msg.content.clone().unwrap_or_default();
                        let fallback_tc = fallback_msg.tool_calls.unwrap_or_default();
                        let has_tc = !fallback_tc.is_empty();
                        // Emit the full text as a single Token chunk (mimics streaming)
                        if !has_tc && !fallback_text.is_empty() {
                            send_chunk(&chunk_sender, StreamChunk::Token(fallback_text.clone()));
                        }
                        if has_tc {
                            // Re-run through the non-streaming path below
                            // by setting response_text, has_content, etc.
                            // We store the result in a tuple and continue
                            let _has_content = !fallback_text.is_empty();
                            state.push(ChatMessage::with_tool_calls(fallback_tc.clone()));
                            for tc in &fallback_tc {
                                send_chunk(
                                    &chunk_sender,
                                    StreamChunk::ToolCallStart {
                                        id: tc.id.clone(),
                                        name: tc.name.clone(),
                                    },
                                );
                            }
                            for tc in &fallback_tc {
                                let result =
                                    self.tools.execute(&tc.name, tc.arguments.clone()).await;
                                match result {
                                    Ok(value) => {
                                        state.push(ChatMessage::tool_result(&tc.id, &value))
                                    }
                                    Err(e) => state.push(ChatMessage::tool_result(
                                        &tc.id,
                                        &serde_json::json!({"error": e.to_string()}),
                                    )),
                                }
                            }
                            for tc in &fallback_tc {
                                send_chunk(
                                    &chunk_sender,
                                    StreamChunk::ToolCallEnd { id: tc.id.clone() },
                                );
                            }
                            if let Some(ref strategy) = self.config.scroll_strategy {
                                strategy.apply(state);
                            }
                            continue;
                        } else {
                            return LoopResult::success(
                                fallback_text,
                                iteration,
                                crate::loops::elapsed_ms(&start),
                            );
                        }
                    }
                };

                let mut full_text = String::new();
                let mut stream_tool_calls: Vec<ToolCall> = Vec::new();
                let mut stream_reasoning_text = String::new();

                while let Some(chunk) = rx.recv().await {
                    match chunk {
                        StreamChunk::Token(t) => {
                            full_text.push_str(&t);
                            send_chunk(&chunk_sender, StreamChunk::Token(t));
                        }
                        StreamChunk::Reasoning(t) => {
                            stream_reasoning_text.push_str(&t);
                            send_chunk(&chunk_sender, StreamChunk::Reasoning(t));
                        }
                        StreamChunk::ToolCallStart { id, name } => {
                            if !stream_tool_calls.iter().any(|tc| tc.id == id) {
                                stream_tool_calls.push(ToolCall {
                                    id: id.clone(),
                                    name: name.clone(),
                                    arguments: serde_json::Value::Null,
                                });
                            }
                            send_chunk(&chunk_sender, StreamChunk::ToolCallStart { id, name });
                        }
                        StreamChunk::ToolCallArguments { id, arguments } => {
                            if let Some(tc) = stream_tool_calls.iter_mut().find(|tc| tc.id == id) {
                                tc.arguments = arguments;
                            }
                        }
                        StreamChunk::Error(msg) => {
                            send_chunk(&chunk_sender, StreamChunk::Error(msg.clone()));
                            return LoopResult::failure(
                                msg,
                                iteration,
                                crate::loops::elapsed_ms(&start),
                            );
                        }
                        _ => {}
                    }
                }

                reasoning_text = stream_reasoning_text.clone();
                let output_text = if !full_text.is_empty() {
                    full_text.clone()
                } else {
                    stream_reasoning_text.clone()
                };
                let has_content = !full_text.is_empty() || !stream_reasoning_text.is_empty();
                let has_tool_calls = !stream_tool_calls.is_empty();
                (output_text, has_content, has_tool_calls, stream_tool_calls)
            } else {
                // ── Non-streaming path (existing logic) ──
                let response = match self.client.chat(request).await {
                    Ok(r) => r,
                    Err(e) => {
                        send_chunk(&chunk_sender, StreamChunk::Error(format!("LLM error: {e}")));
                        return LoopResult::failure(
                            format!("LLM error: {e}"),
                            iteration,
                            crate::loops::elapsed_ms(&start),
                        );
                    }
                };

                let assistant_msg = response.message;
                reasoning_text = assistant_msg.reasoning_content.clone().unwrap_or_default();
                // Treat reasoning_content as valid content so the guard below passes
                let has_content_non = assistant_msg
                    .content
                    .as_ref()
                    .is_some_and(|c| !c.is_empty())
                    || assistant_msg
                        .reasoning_content
                        .as_ref()
                        .is_some_and(|c| !c.is_empty());
                let has_tc = assistant_msg
                    .tool_calls
                    .as_ref()
                    .is_some_and(|calls| !calls.is_empty());

                let text = if assistant_msg
                    .content
                    .as_ref()
                    .is_some_and(|c| !c.is_empty())
                {
                    assistant_msg.content.clone().unwrap()
                } else {
                    // Fall back to reasoning_content when content is null
                    assistant_msg.reasoning_content.clone().unwrap_or_default()
                };
                let tc_list = assistant_msg.tool_calls.unwrap_or_default();

                // Non-streaming: emit whole text as single Token chunk (only if no tool calls)
                // (streaming path already emits tokens in real-time above)
                if !has_tc {
                    send_chunk(&chunk_sender, StreamChunk::Token(text.clone()));
                }

                (text, has_content_non, has_tc, tc_list)
            };

            // Guard: skip empty assistant messages (allow reasoning_content as valid output)
            let has_reasoning = !reasoning_text.is_empty();
            if !has_content && !has_tool_calls && !has_reasoning {
                let msg = "LLM returned empty response (no content, no tool calls)";
                send_chunk(&chunk_sender, StreamChunk::Error(msg.into()));
                return LoopResult::failure(msg, iteration, crate::loops::elapsed_ms(&start));
            }

            // Push to state
            let mut msg = if has_tool_calls {
                ChatMessage::with_tool_calls(tool_calls.clone())
            } else {
                ChatMessage::assistant(&response_text)
            };
            if has_reasoning {
                msg.reasoning_content = Some(reasoning_text.clone());
            }
            state.push(msg);

            if has_tool_calls {
                // Emit ToolCallStart for each tool
                for tc in &tool_calls {
                    send_chunk(
                        &chunk_sender,
                        StreamChunk::ToolCallStart {
                            id: tc.id.clone(),
                            name: tc.name.clone(),
                        },
                    );
                }
                // Execute each tool and append results
                for tc in &tool_calls {
                    let result = self.tools.execute(&tc.name, tc.arguments.clone()).await;
                    match result {
                        Ok(value) => {
                            state.push(ChatMessage::tool_result(&tc.id, &value));
                        }
                        Err(e) => {
                            state.push(ChatMessage::tool_result(
                                &tc.id,
                                &serde_json::json!({"error": e.to_string()}),
                            ));
                        }
                    }
                }

                // Emit tool call end chunks
                for tc in &tool_calls {
                    send_chunk(
                        &chunk_sender,
                        StreamChunk::ToolCallEnd { id: tc.id.clone() },
                    );
                }
                // Apply scroll strategy to trim history before next iteration
                if let Some(ref strategy) = self.config.scroll_strategy {
                    strategy.apply(state);
                }
                // Continue — LLM will see tool results next iteration
            } else {
                // Token already emitted during streaming; non-streaming also emitted above
                return LoopResult::success(
                    response_text,
                    iteration,
                    crate::loops::elapsed_ms(&start),
                );
            }
        }

        // Max iterations exceeded
        send_chunk(
            &chunk_sender,
            StreamChunk::Error(format!("max iterations ({max_iter}) exceeded")),
        );
        LoopResult::failure(
            format!("agent max iterations ({max_iter}) exceeded"),
            max_iter,
            crate::loops::elapsed_ms(&start),
        )
    }
}

#[async_trait::async_trait]
impl<L: LlmClient + 'static> Loop for Agent<L> {
    type Context = String;
    type State = Vec<ChatMessage>;
    type Output = String;

    async fn execute(
        &self,
        ctx: Context<Self::Context>,
        state: &mut Self::State,
    ) -> LoopResult<Self::Output> {
        self.execute_impl(ctx, state, None).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::llm::{ChatResponse, LlmError, ToolCall};
    use crate::agent::tool::{Tool, ToolCategory, ToolError, ToolSpec};
    use crate::loops::{CycleType, LoopId, StopCondition};
    use serde_json::json;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    // ── Mock LLM client ──────────────────────────────────────────────

    struct MockLlm {
        /// Pre-defined responses returned in sequence.
        responses: Vec<Result<ChatResponse, LlmError>>,
        /// Tracks how many times `chat` was called.
        call_count: Arc<AtomicUsize>,
    }

    impl MockLlm {
        fn new(responses: Vec<Result<ChatResponse, LlmError>>) -> Self {
            Self {
                responses,
                call_count: Arc::new(AtomicUsize::new(0)),
            }
        }

        #[allow(dead_code)]
        fn call_count(&self) -> usize {
            self.call_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl LlmClient for MockLlm {
        async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, LlmError> {
            let idx = self.call_count.fetch_add(1, Ordering::SeqCst);
            self.responses[idx].clone()
        }
    }

    // ── Mock tools ───────────────────────────────────────────────────

    /// A tool that records invocations and returns a fixed value.
    #[derive(Clone)]
    struct EchoTool {
        name: String,
        call_count: Arc<AtomicUsize>,
    }

    impl EchoTool {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                call_count: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn times_called(&self) -> usize {
            self.call_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl Tool for EchoTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: self.name.clone(),
                description: "Echoes input".into(),
                parameters: json!({"type": "object"}),
                category: ToolCategory::Generic,
            }
        }

        async fn call(&self, args: serde_json::Value) -> Result<serde_json::Value, ToolError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(args)
        }
    }

    /// A tool that always fails.
    struct FailTool;

    #[async_trait::async_trait]
    impl Tool for FailTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "fail".into(),
                description: "Always fails".into(),
                parameters: json!({"type": "object"}),
                category: ToolCategory::Generic,
            }
        }

        async fn call(&self, _args: serde_json::Value) -> Result<serde_json::Value, ToolError> {
            Err(ToolError::Execution {
                tool: "fail".into(),
                message: "intentional failure".into(),
            })
        }
    }

    // ── Helper to build a Context ─────────────────────────────────────

    fn ctx(input: &str, max_iter: u32, timeout_secs: u64) -> Context<String> {
        Context::new(
            LoopId::new(),
            CycleType::Turn,
            StopCondition::new(Some(max_iter), Some(Duration::from_secs(timeout_secs))),
            input.to_string(),
        )
    }

    // ── Tests ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_text_response() {
        // LLM returns a plain text response (no tool calls).
        let client = MockLlm::new(vec![Ok(ChatResponse {
            message: ChatMessage::assistant("Hello, world!"),
            usage: None,
        })]);

        let agent = Agent::new(client, AgentConfig::default());
        let mut state = Vec::new();
        let result = agent.execute(ctx("hi", 5, 30), &mut state).await;

        assert!(result.is_success());
        assert_eq!(result.output, Some("Hello, world!".into()));
        assert_eq!(result.iterations, 1);
    }

    #[tokio::test]
    async fn test_single_tool_call_then_text() {
        // LLM first returns a tool call, then a text response.
        let tool_call = ToolCall {
            id: "call_1".into(),
            name: "echo".into(),
            arguments: json!({"msg": "pong"}),
        };

        let client = MockLlm::new(vec![
            Ok(ChatResponse {
                message: ChatMessage::with_tool_calls(vec![tool_call]),
                usage: None,
            }),
            Ok(ChatResponse {
                message: ChatMessage::assistant("Done"),
                usage: None,
            }),
        ]);

        let echo = EchoTool::new("echo");
        let agent = Agent::with_tools(
            client,
            AgentConfig::default(),
            ToolSet::from_tools(vec![Arc::new(echo.clone())]),
        );
        let mut state = Vec::new();
        let result = agent.execute(ctx("ping", 5, 30), &mut state).await;

        assert!(result.is_success());
        assert_eq!(result.output, Some("Done".into()));
        assert_eq!(result.iterations, 2);
        assert_eq!(echo.times_called(), 1);
        // State: [user, assistant(tool_call), tool_result, assistant(text)]
        assert_eq!(state.len(), 4);
    }

    #[tokio::test]
    async fn test_multiple_tool_calls_in_one_response() {
        // LLM returns two tool calls in one assistant message.
        let t1 = ToolCall {
            id: "c1".into(),
            name: "echo".into(),
            arguments: json!("a"),
        };
        let t2 = ToolCall {
            id: "c2".into(),
            name: "echo".into(),
            arguments: json!("b"),
        };

        let client = MockLlm::new(vec![
            Ok(ChatResponse {
                message: ChatMessage::with_tool_calls(vec![t1, t2]),
                usage: None,
            }),
            Ok(ChatResponse {
                message: ChatMessage::assistant("all done"),
                usage: None,
            }),
        ]);

        let echo = EchoTool::new("echo");
        let agent = Agent::with_tools(
            client,
            AgentConfig::default(),
            ToolSet::from_tools(vec![Arc::new(echo.clone())]),
        );
        let mut state = Vec::new();
        let result = agent.execute(ctx("go", 5, 30), &mut state).await;

        assert!(result.is_success());
        assert_eq!(echo.times_called(), 2);
        // State: [user, assistant(2 tool calls), tool_result, tool_result, assistant(text)]
        assert_eq!(state.len(), 5);
    }

    #[tokio::test]
    async fn test_tool_execution_error() {
        // When a tool fails, the error is returned as a tool result.
        let tool_call = ToolCall {
            id: "cfail".into(),
            name: "fail".into(),
            arguments: json!({}),
        };

        let client = MockLlm::new(vec![
            Ok(ChatResponse {
                message: ChatMessage::with_tool_calls(vec![tool_call]),
                usage: None,
            }),
            Ok(ChatResponse {
                message: ChatMessage::assistant("handled error"),
                usage: None,
            }),
        ]);

        let agent = Agent::with_tools(
            client,
            AgentConfig::default(),
            ToolSet::from_tools(vec![Arc::new(FailTool)]),
        );
        let mut state = Vec::new();
        let result = agent.execute(ctx("do it", 5, 30), &mut state).await;

        assert!(result.is_success());
        assert_eq!(result.output, Some("handled error".into()));
        // State: [user, assistant(tool_call), tool_result(error), assistant(text)]
        // Tool result is at index 2
        let tool_result = &state[2];
        assert_eq!(tool_result.role, crate::agent::llm::Role::Tool);
        assert!(
            tool_result
                .content
                .as_deref()
                .unwrap()
                .contains("intentional failure")
        );
    }

    #[tokio::test]
    async fn test_tool_not_found() {
        // LLM calls a tool that isn't registered.
        let tool_call = ToolCall {
            id: "cmissing".into(),
            name: "nonexistent".into(),
            arguments: json!({}),
        };

        let client = MockLlm::new(vec![
            Ok(ChatResponse {
                message: ChatMessage::with_tool_calls(vec![tool_call]),
                usage: None,
            }),
            Ok(ChatResponse {
                message: ChatMessage::assistant("got error"),
                usage: None,
            }),
        ]);

        // Empty toolset — no tools registered
        let agent = Agent::new(client, AgentConfig::default());
        let mut state = Vec::new();
        let result = agent.execute(ctx("test", 5, 30), &mut state).await;

        assert!(result.is_success());
        // State: [user, assistant(tool_call), tool_result(not_found), assistant(text)]
        // Tool result is at index 2
        let tool_result = &state[2];
        assert!(
            tool_result
                .content
                .as_deref()
                .unwrap()
                .contains("not found")
        );
    }

    #[tokio::test]
    async fn test_llm_error() {
        // LLM client returns an error.
        let client = MockLlm::new(vec![Err(LlmError::Request("network failure".into()))]);

        let agent = Agent::new(client, AgentConfig::default());
        let mut state = Vec::new();
        let result = agent.execute(ctx("hi", 5, 30), &mut state).await;

        assert!(!result.is_success());
        assert!(result.output.is_none());
        assert_eq!(result.iterations, 1);
        // Error message should contain LLM error
        assert!(
            matches!(&result.status, crate::loops::LoopStatus::Failed(msg) if msg.contains("LLM error"))
        );
    }

    #[tokio::test]
    async fn test_max_iterations_exceeded() {
        // Agent keeps calling tools indefinitely → hits max iterations.
        let tool_call = ToolCall {
            id: "c".into(),
            name: "echo".into(),
            arguments: json!("loop"),
        };

        // Always returns a tool call, never text
        let client = MockLlm::new(vec![
            Ok(ChatResponse {
                message: ChatMessage::with_tool_calls(vec![tool_call.clone()]),
                usage: None,
            }),
            Ok(ChatResponse {
                message: ChatMessage::with_tool_calls(vec![tool_call]),
                usage: None,
            }),
        ]);

        let echo = EchoTool::new("echo");
        let agent = Agent::with_tools(
            client,
            AgentConfig::default(),
            ToolSet::from_tools(vec![Arc::new(echo)]),
        );
        let mut state = Vec::new();
        let result = agent.execute(ctx("loop", 2, 30), &mut state).await;

        assert!(!result.is_success());
        assert_eq!(result.iterations, 2);
        assert!(
            matches!(&result.status, crate::loops::LoopStatus::Failed(msg) if msg.contains("max iterations"))
        );
    }

    #[tokio::test]
    async fn test_timeout() {
        // Agent exceeds the timeout limit.
        // Use a real clock-based timeout: we give it 1ms timeout and force
        // a tool-call loop that will take at least one iteration.
        let tool_call = ToolCall {
            id: "c".into(),
            name: "echo".into(),
            arguments: json!("x"),
        };

        let client = MockLlm::new(vec![
            Ok(ChatResponse {
                message: ChatMessage::with_tool_calls(vec![tool_call]),
                usage: None,
            }),
            // Second call — should not be reached due to timeout, but needed for safety
            Ok(ChatResponse {
                message: ChatMessage::assistant("done"),
                usage: None,
            }),
        ]);

        let echo = EchoTool::new("echo");
        let agent = Agent::with_tools(
            client,
            AgentConfig::default(),
            ToolSet::from_tools(vec![Arc::new(echo)]),
        );
        let mut state = Vec::new();
        let result = agent.execute(ctx("fast", 5, 0), &mut state).await;

        assert!(!result.is_success());
        assert_eq!(result.iterations, 1);
    }

    #[tokio::test]
    async fn test_conversation_accumulation() {
        // Verify that the conversation state accumulates messages.
        let client = MockLlm::new(vec![Ok(ChatResponse {
            message: ChatMessage::assistant("first response"),
            usage: None,
        })]);

        // Manually seed state with a prior message
        let mut state = vec![ChatMessage::user("prior context")];

        let agent = Agent::new(client, AgentConfig::default());
        let _ = agent.execute(ctx("new question", 5, 30), &mut state).await;

        // State after execute: [prior_context, user(new question), assistant(first response)]
        // Note: system prompt is included in the HTTP request but NOT pushed to state
        assert_eq!(state.len(), 3);
        assert_eq!(state[0].content.as_deref(), Some("prior context"));
        assert_eq!(state[1].content.as_deref(), Some("new question"));
        assert_eq!(state[2].content.as_deref(), Some("first response"));
    }

    #[tokio::test]
    async fn test_default_config() {
        let config = AgentConfig::default();
        assert_eq!(config.model, "gpt-4o");
        assert_eq!(config.system_prompt, "You are a helpful assistant.");
        assert!(config.temperature.is_none());
        assert!(config.max_tokens.is_none());
    }

    #[tokio::test]
    async fn test_with_tools_and_add_tool() {
        let echo = EchoTool::new("echo");
        let mut agent = Agent::new(MockLlm::new(vec![]), AgentConfig::default());
        agent.add_tool(echo);
        assert_eq!(agent.tools().specs().len(), 1);
        assert_eq!(agent.tools().specs()[0].name, "echo");
    }

    // ── Streaming Tests ────────────────────────────────────────────────

    #[tokio::test]
    async fn test_stream_text_response() {
        // Agent returns text — stream receives Token then Done.
        let client = MockLlm::new(vec![Ok(ChatResponse {
            message: ChatMessage::assistant("Streamed hello"),
            usage: None,
        })]);
        let agent = Agent::new(client, AgentConfig::default());
        let mut state = Vec::new();
        let (tx, mut rx) = tokio::sync::mpsc::channel(256);

        let result = agent.execute_stream(ctx("hi", 5, 30), &mut state, tx).await;

        assert!(result.is_success());
        assert_eq!(result.output, Some("Streamed hello".into()));

        // Collect chunks
        let mut tokens = Vec::new();
        let mut seen_done = false;
        while let Some(chunk) = rx.recv().await {
            match chunk {
                StreamChunk::Token(t) => tokens.push(t),
                StreamChunk::Done => {
                    seen_done = true;
                    break;
                }
                other => panic!("unexpected chunk: {other:?}"),
            }
        }
        assert_eq!(tokens, vec!["Streamed hello"]);
        assert!(seen_done);
    }

    #[tokio::test]
    async fn test_stream_tool_call() {
        // Agent does one tool call then text — stream sends ToolCallStart/End + Token + Done.
        let tool_call = ToolCall {
            id: "c1".into(),
            name: "echo".into(),
            arguments: json!("ping"),
        };
        let client = MockLlm::new(vec![
            Ok(ChatResponse {
                message: ChatMessage::with_tool_calls(vec![tool_call]),
                usage: None,
            }),
            Ok(ChatResponse {
                message: ChatMessage::assistant("done"),
                usage: None,
            }),
        ]);

        let echo = EchoTool::new("echo");
        let agent = Agent::with_tools(
            client,
            AgentConfig::default(),
            ToolSet::from_tools(vec![Arc::new(echo)]),
        );
        let mut state = Vec::new();
        let (tx, mut rx) = tokio::sync::mpsc::channel(256);

        let result = agent
            .execute_stream(ctx("ping", 5, 30), &mut state, tx)
            .await;

        assert!(result.is_success());
        assert_eq!(result.output, Some("done".into()));

        let mut chunks = Vec::new();
        while let Some(chunk) = rx.recv().await {
            match &chunk {
                StreamChunk::Done => {
                    chunks.push(chunk);
                    break;
                }
                _ => chunks.push(chunk),
            }
        }

        // Should have: ToolCallStart, ToolCallEnd, Token, Done
        assert!(
            chunks
                .iter()
                .any(|c| matches!(c, StreamChunk::ToolCallStart { .. }))
        );
        assert!(
            chunks
                .iter()
                .any(|c| matches!(c, StreamChunk::ToolCallEnd { .. }))
        );
        assert!(
            chunks
                .iter()
                .any(|c| matches!(c, StreamChunk::Token(t) if t == "done"))
        );
    }

    #[tokio::test]
    async fn test_stream_llm_error() {
        // Agent gets LLM error — stream sends Error then Done.
        let client = MockLlm::new(vec![Err(LlmError::Request("stream crash".into()))]);
        let agent = Agent::new(client, AgentConfig::default());
        let mut state = Vec::new();
        let (tx, mut rx) = tokio::sync::mpsc::channel(256);

        let result = agent.execute_stream(ctx("go", 5, 30), &mut state, tx).await;

        assert!(!result.is_success());

        let mut chunks = Vec::new();
        while let Some(chunk) = rx.recv().await {
            match &chunk {
                StreamChunk::Done => {
                    chunks.push(chunk);
                    break;
                }
                _ => chunks.push(chunk),
            }
        }
        // Should have at least an Error chunk
        assert!(chunks.iter().any(|c| matches!(c, StreamChunk::Error(_))));
    }
}
