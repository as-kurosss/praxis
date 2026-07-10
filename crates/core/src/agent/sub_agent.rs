//! **Sub-agent Spawning** — allows an Agent to spawn child agents at runtime.
//!
//! [`SpawnAgentTool`] implements the [`Tool`](super::Tool) trait so it can
//! be registered in an agent's [`ToolSet`](super::ToolSet).  When called, it
//! creates a new [`Agent`](super::Agent) with the specified task and system
//! prompt and returns the sub-agent's output as a JSON tool result.
//!
//! # Usage
//!
//! ```ignore
//! use std::sync::Arc;
//!
//! // Create a factory that produces LLM clients
//! let factory = Arc::new(|model: &str| {
//!     OpenAiClient::new(model, api_key)
//! });
//!
//! // Create the spawn tool with some sub-tools
//! let spawn_tool = SpawnAgentTool::new(factory, ToolSet::new());
//!
//! // Register in parent agent
//! let mut parent = Agent::new(client, config);
//! parent.add_tool(spawn_tool);
//! ```

use super::llm::LlmClient;
use super::runtime::{Agent, AgentConfig};
use super::tool::{Tool, ToolCategory, ToolError, ToolSet, ToolSpec};
use crate::loops::{Context, CycleType, Loop, LoopId, StopCondition};
use serde_json::{Value, json};
use std::sync::Arc;

/// Configuration for a sub-agent invocation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SpawnAgentConfig {
    /// System prompt (overrides the default).
    #[serde(default)]
    pub system_prompt: String,
    /// Model identifier (e.g. "gpt-4o", "claude-3-5-sonnet").
    #[serde(default = "default_model")]
    pub model: String,
    /// Maximum iterations for the sub-agent.
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
}

fn default_model() -> String {
    "gpt-4o".into()
}

fn default_max_iterations() -> u32 {
    25
}

impl Default for SpawnAgentConfig {
    fn default() -> Self {
        Self {
            system_prompt: String::new(),
            model: default_model(),
            max_iterations: default_max_iterations(),
        }
    }
}

/// A [`Tool`](super::Tool) that spawns a child agent and returns its output.
///
/// The sub-agent runs with its own isolated conversation state.  The parent
/// agent sees the result as a normal tool response.
///
/// # JSON arguments
///
/// ```json
/// {
///     "task": "Solve this problem",
///     "config": {
///         "system_prompt": "You are a specialist",
///         "model": "gpt-4o",
///         "max_iterations": 25
///     }
/// }
/// ```
///
/// If `config` is omitted, defaults are used for all fields.
pub struct SpawnAgentTool<L: LlmClient> {
    /// Factory that produces LLM client instances for sub-agents.
    client_factory: Arc<dyn Fn(&str) -> L + Send + Sync>,
    /// Tools that every spawned sub-agent can use.
    sub_tools: Arc<ToolSet>,
}

impl<L: LlmClient> SpawnAgentTool<L> {
    /// Create a new sub-agent spawning tool.
    ///
    /// * `client_factory` — produces a new [`LlmClient`] for each sub-agent
    ///   (called with the model identifier from the invocation config)
    /// * `sub_tools` — tools that every spawned sub-agent can call
    pub fn new(
        client_factory: Arc<dyn Fn(&str) -> L + Send + Sync>,
        sub_tools: impl Into<Arc<ToolSet>>,
    ) -> Self {
        Self {
            client_factory,
            sub_tools: sub_tools.into(),
        }
    }

    /// The tool's name.
    pub const NAME: &str = "spawn_agent";
}

#[async_trait::async_trait]
impl<L: LlmClient + 'static> Tool for SpawnAgentTool<L> {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: Self::NAME.to_string(),
            description: "Spawns a child agent to complete a subtask. Use this for delegating \
                          work to a specialised sub-agent.  Returns the sub-agent's output text \
                          along with iteration and timing metadata."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "The task for the sub-agent"
                    },
                    "config": {
                        "type": "object",
                        "description": "Optional sub-agent configuration",
                        "properties": {
                            "system_prompt": { "type": "string" },
                            "model": { "type": "string" },
                            "max_iterations": { "type": "integer", "minimum": 1 }
                        }
                    }
                },
                "required": ["task"]
            }),
            category: ToolCategory::Generic,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let task =
            args.get("task")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::InvalidArgs {
                    tool: Self::NAME.to_string(),
                    message: "missing required 'task' string".into(),
                })?;

        let config: SpawnAgentConfig = args
            .get("config")
            .map(|c| serde_json::from_value(c.clone()))
            .transpose()
            .map_err(|e| ToolError::InvalidArgs {
                tool: Self::NAME.to_string(),
                message: format!("invalid config: {e}"),
            })?
            .unwrap_or_default();

        // Build the system prompt
        let system_prompt = if config.system_prompt.is_empty() {
            "You are a helpful assistant.".to_string()
        } else {
            config.system_prompt
        };

        // Create the sub-agent
        let client = (self.client_factory)(&config.model);
        let agent_config = AgentConfig {
            model: config.model.clone(),
            system_prompt,
            temperature: None,
            max_tokens: None,
            scroll_strategy: None,
        };
        let sub_agent = Agent::with_tools(client, agent_config, (*self.sub_tools).clone());

        // Build execution context
        let ctx = Context::new(
            LoopId::new(),
            CycleType::Turn,
            StopCondition::new(Some(config.max_iterations), None),
            task.to_string(),
        );

        // Run the sub-agent (isolated state)
        let mut sub_state = Vec::new();
        let result = sub_agent.execute(ctx, &mut sub_state).await;

        if result.is_success() {
            Ok(json!({
                "output": result.output.unwrap_or_default(),
                "iterations": result.iterations,
                "duration_ms": result.duration_ms
            }))
        } else {
            let error = match &result.status {
                crate::loops::LoopStatus::Failed(msg) => msg.clone(),
                _ => "sub-agent failed".to_string(),
            };
            Ok(json!({
                "error": error,
                "iterations": result.iterations,
                "duration_ms": result.duration_ms
            }))
        }
    }
}

impl<L: LlmClient> Clone for SpawnAgentTool<L> {
    fn clone(&self) -> Self {
        Self {
            client_factory: self.client_factory.clone(),
            sub_tools: Arc::clone(&self.sub_tools),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::llm::{ChatMessage, ChatRequest, ChatResponse, LlmError};
    use serde_json::json;
    use std::sync::Arc;

    /// Local EchoTool because `EchoTool` in `tool.rs` is `#[cfg(test)]`-private.
    struct EchoTool {
        name: String,
        call_count: std::sync::atomic::AtomicUsize,
    }

    #[async_trait::async_trait]
    impl super::super::tool::Tool for EchoTool {
        fn spec(&self) -> super::super::tool::ToolSpec {
            super::super::tool::ToolSpec {
                name: self.name.clone(),
                description: "Echoes input".into(),
                parameters: serde_json::json!({"type": "object"}),
                category: super::super::tool::ToolCategory::Generic,
            }
        }

        async fn call(
            &self,
            args: serde_json::Value,
        ) -> Result<serde_json::Value, super::super::tool::ToolError> {
            self.call_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(args)
        }
    }

    impl EchoTool {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                call_count: std::sync::atomic::AtomicUsize::new(0),
            }
        }
    }

    /// A deterministic mock LLM that returns responses in sequence.
    struct SequenceLlm {
        responses: Vec<Result<ChatResponse, LlmError>>,
        index: std::sync::atomic::AtomicUsize,
    }

    impl SequenceLlm {
        fn new(responses: Vec<Result<ChatResponse, LlmError>>) -> Self {
            Self {
                responses,
                index: std::sync::atomic::AtomicUsize::new(0),
            }
        }
    }

    #[async_trait::async_trait]
    impl LlmClient for SequenceLlm {
        async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, LlmError> {
            let idx = self.index.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if idx < self.responses.len() {
                self.responses[idx].clone()
            } else {
                Err(LlmError::Request("no more responses".into()))
            }
        }
    }

    #[tokio::test]
    async fn test_spawn_agent_text_response() {
        // Sub-agent returns text.
        let client_factory = Arc::new(|_model: &str| {
            SequenceLlm::new(vec![Ok(ChatResponse {
                message: ChatMessage::assistant("Sub result"),
                usage: None,
            })])
        });

        let spawn_tool = SpawnAgentTool::new(client_factory, ToolSet::new());
        let args = json!({"task": "Do something"});

        let result = spawn_tool.call(args).await.unwrap();
        assert_eq!(result["output"], "Sub result");
        assert_eq!(result["iterations"], 1);
        // duration_ms may be 0 for instant mock execution
        assert!(result["duration_ms"].as_u64().is_some());
    }

    #[tokio::test]
    async fn test_spawn_agent_with_tools() {
        // Sub-agent has tools available.
        let client_factory = Arc::new(|_model: &str| {
            // The sub-agent always makes a tool call then returns text
            SequenceLlm::new(vec![
                Ok(ChatResponse {
                    message: ChatMessage::with_tool_calls(vec![super::super::llm::ToolCall {
                        id: "c1".into(),
                        name: "echo".into(),
                        arguments: json!("hello"),
                    }]),
                    usage: None,
                }),
                Ok(ChatResponse {
                    message: ChatMessage::assistant("Tool result received"),
                    usage: None,
                }),
            ])
        });

        let mut sub_tools = ToolSet::new();
        sub_tools.add(EchoTool::new("echo"));

        let spawn_tool = SpawnAgentTool::new(client_factory, sub_tools);
        let args = json!({"task": "Use tools", "config": {"max_iterations": 10}});

        let result = spawn_tool.call(args).await.unwrap();
        assert_eq!(result["output"], "Tool result received");
        // Sub-agent runs 2 LLM calls (tool + text) = 2 iterations
        assert_eq!(result["iterations"], 2);
    }

    #[tokio::test]
    async fn test_spawn_agent_missing_task() {
        let client_factory = Arc::new(|_model: &str| SequenceLlm::new(vec![]));
        let spawn_tool = SpawnAgentTool::<SequenceLlm>::new(client_factory, ToolSet::new());
        let result = spawn_tool.call(json!({})).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing required"));
    }

    #[tokio::test]
    async fn test_spawn_agent_with_config() {
        // Custom system prompt.
        let client_factory = Arc::new(|_model: &str| {
            SequenceLlm::new(vec![Ok(ChatResponse {
                message: ChatMessage::assistant("Specialised output"),
                usage: None,
            })])
        });

        let spawn_tool = SpawnAgentTool::new(client_factory, ToolSet::new());
        let args = json!({
            "task": "Special task",
            "config": {
                "system_prompt": "You are a Rust expert",
                "max_iterations": 5
            }
        });

        let result = spawn_tool.call(args).await.unwrap();
        assert_eq!(result["output"], "Specialised output");
    }

    #[tokio::test]
    async fn test_spawn_agent_forwards_tool_error() {
        // Sub-agent encounters an error.
        let client_factory = Arc::new(|_model: &str| {
            SequenceLlm::new(vec![Err(LlmError::Request("sub-agent crashed".into()))])
        });
        let spawn_tool = SpawnAgentTool::new(client_factory, ToolSet::new());
        let args = json!({"task": "Do something"});

        let result = spawn_tool.call(args).await.unwrap();
        assert!(
            result.get("error").is_some(),
            "expected error field, got: {result}"
        );
        // Even on failure, we return Ok with an error field (not a ToolError),
        // so the parent agent can handle the failure gracefully.
        assert!(
            result["error"].as_str().unwrap().contains("LLM error"),
            "error should mention LLM error: {}",
            result["error"]
        );
    }
}
