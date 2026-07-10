//! **Agent** runtime — an LLM-powered agent that implements the [`Loop`] trait.
//!
//! The [`Agent`] holds an LLM client, a set of tools, and a configuration.
//! When executed it runs a tool-calling loop:  call LLM → execute tools →
//! feed results back → repeat until the LLM produces a final text response.

use super::llm::{ChatMessage, ChatRequest, LlmClient};
use super::tool::ToolSet;
use crate::loops::{Context, Loop, LoopResult};
use std::time::Instant;

/// Configuration for an [`Agent`].
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Model identifier (e.g. "gpt-4o", "claude-3-5-sonnet").
    pub model: String,
    /// System prompt for the agent.
    pub system_prompt: String,
    /// Sampling temperature (None = provider default).
    pub temperature: Option<f32>,
    /// Maximum tokens in the LLM response (None = provider default).
    pub max_tokens: Option<u32>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            model: "gpt-4o".into(),
            system_prompt: "You are a helpful assistant.".into(),
            temperature: None,
            max_tokens: None,
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
}

impl<L: LlmClient> Agent<L> {
    /// Create a new agent with the given LLM client and config.
    pub fn new(client: L, config: AgentConfig) -> Self {
        Self {
            client,
            config,
            tools: ToolSet::new(),
        }
    }

    /// Create an agent with pre-configured tools.
    pub fn with_tools(client: L, config: AgentConfig, tools: ToolSet) -> Self {
        Self {
            client,
            config,
            tools,
        }
    }

    /// Register a tool that the agent can call.
    pub fn add_tool<T: crate::agent::tool::Tool + 'static>(&mut self, tool: T) {
        self.tools.add(tool);
    }

    /// Reference to the tool set.
    pub fn tools(&self) -> &ToolSet {
        &self.tools
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
        let start = Instant::now();
        let max_iter = ctx.stop_condition.max_iterations.unwrap_or(25);
        let timeout = ctx.stop_condition.timeout;

        // Add user message to conversation state
        state.push(ChatMessage::user(ctx.input));

        for iteration in 1..=max_iter {
            // Check graph-level timeout
            if let Some(limit) = timeout
                && start.elapsed() >= limit
            {
                let elapsed = crate::loops::elapsed_ms(&start);
                return LoopResult::failure(
                    format!("agent timeout after {elapsed}ms"),
                    iteration,
                    elapsed,
                );
            }

            // Build request: system prompt + conversation state
            let mut messages = Vec::with_capacity(state.len() + 1);
            messages.push(ChatMessage::system(&self.config.system_prompt));
            messages.extend(state.iter().cloned());

            let request = ChatRequest {
                messages,
                tools: Some(self.tools.specs()),
                temperature: self.config.temperature,
                max_tokens: self.config.max_tokens,
            };

            // Call LLM
            let response = match self.client.chat(request).await {
                Ok(r) => r,
                Err(e) => {
                    return LoopResult::failure(
                        format!("LLM error: {e}"),
                        iteration,
                        crate::loops::elapsed_ms(&start),
                    );
                }
            };

            // Add assistant message to conversation
            let assistant_msg = response.message;
            let has_tool_calls = assistant_msg
                .tool_calls
                .as_ref()
                .is_some_and(|calls| !calls.is_empty());

            state.push(assistant_msg);

            if has_tool_calls {
                // Execute each tool and append results
                let tool_calls = state
                    .last()
                    .and_then(|m| m.tool_calls.as_ref())
                    .cloned()
                    .unwrap_or_default();

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
                // Continue — LLM will see tool results next iteration
            } else {
                // No tool calls — final text response
                let text = state
                    .last()
                    .and_then(|m| m.content.clone())
                    .unwrap_or_default();
                return LoopResult::success(text, iteration, crate::loops::elapsed_ms(&start));
            }
        }

        // Max iterations exceeded
        LoopResult::failure(
            format!("agent max iterations ({max_iter}) exceeded"),
            max_iter,
            crate::loops::elapsed_ms(&start),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::llm::{ChatResponse, LlmError, ToolCall};
    use crate::agent::tool::{Tool, ToolError, ToolSpec};
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
            ToolSet::from_tools(vec![Box::new(echo.clone())]),
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
            ToolSet::from_tools(vec![Box::new(echo.clone())]),
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
            ToolSet::from_tools(vec![Box::new(FailTool)]),
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
            ToolSet::from_tools(vec![Box::new(echo)]),
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
            ToolSet::from_tools(vec![Box::new(echo)]),
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
}
