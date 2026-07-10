//! **Anthropic LLM client** — implements [`LlmClient`](praxis_core::agent::LlmClient)
//! for Anthropic's Messages API.
//!
//! # Usage
//! ```ignore
//! use praxis_runtime::AnthropicClient;
//!
//! let client = AnthropicClient::from_env("claude-sonnet-4-20250514").unwrap();
//! // or with custom base_url / api_key:
//! let client = AnthropicClient::new(
//!     "https://api.anthropic.com/v1",
//!     "sk-ant-...",
//!     "claude-sonnet-4-20250514",
//! );
//! ```

use praxis_core::agent::{
    ChatMessage, ChatRequest, ChatResponse, LlmClient, LlmError, Role, ToolCall, Usage,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};

// ── Anthropic API types (internal, for JSON serialization) ───────────────

/// Request body for Anthropic's Messages API.
#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: String,
    content: Vec<AnthropicContentBlock>,
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

#[derive(Serialize)]
struct AnthropicTool {
    name: String,
    description: String,
    #[serde(rename = "input_schema")]
    input_schema: serde_json::Value,
}

/// Response body from Anthropic's Messages API.
#[derive(Deserialize)]
struct AnthropicResponse {
    #[allow(dead_code)]
    id: Option<String>,
    #[allow(dead_code)]
    #[serde(rename = "type")]
    type_: Option<String>,
    #[allow(dead_code)]
    role: Option<String>,
    content: Vec<AnthropicResponseContentBlock>,
    #[allow(dead_code)]
    model: Option<String>,
    #[allow(dead_code)]
    stop_reason: Option<String>,
    usage: Option<AnthropicUsage>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum AnthropicResponseContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Deserialize)]
struct AnthropicUsage {
    input_tokens: Option<u32>,
    output_tokens: Option<u32>,
}

/// Error type for the Anthropic client.
#[derive(Debug, thiserror::Error)]
pub enum AnthropicError {
    /// HTTP request failed (network, timeout, etc.).
    #[error("HTTP request failed: {0}")]
    Http(String),
    /// API returned an error status.
    #[error("Anthropic API error (status {status}): {body}")]
    Api { status: u16, body: String },
    /// Failed to parse the API response JSON.
    #[error("Response parse error: {0}")]
    Parse(String),
    /// Missing API key.
    #[error("ANTHROPIC_API_KEY not set")]
    MissingApiKey,
}

impl From<AnthropicError> for LlmError {
    fn from(e: AnthropicError) -> Self {
        match e {
            AnthropicError::Http(msg) => LlmError::Request(msg),
            AnthropicError::Api { status, body } => {
                LlmError::Api(format!("HTTP {status}: {body}"))
            }
            AnthropicError::Parse(msg) => LlmError::Parse(msg),
            AnthropicError::MissingApiKey => {
                LlmError::Request("ANTHROPIC_API_KEY not set".into())
            }
        }
    }
}

// ── AnthropicClient ──────────────────────────────────────────────────────

/// An LLM client for Anthropic's Messages API.
///
/// Configure with `base_url` to point at the API:
/// * `Anthropic`: `https://api.anthropic.com/v1`
///
/// The API key is read from the `ANTHROPIC_API_KEY` environment variable by
/// default.
pub struct AnthropicClient {
    base_url: String,
    api_key: String,
    default_model: String,
    http_client: Client,
}

impl AnthropicClient {
    /// Create a new client with explicit configuration.
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        default_model: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            default_model: default_model.into(),
            http_client: Client::new(),
        }
    }

    /// Create a client using `ANTHROPIC_API_KEY` from the environment.
    ///
    /// # Errors
    /// Returns [`AnthropicError::MissingApiKey`] if the env var is not set.
    pub fn from_env(default_model: impl Into<String>) -> Result<Self, AnthropicError> {
        let api_key =
            std::env::var("ANTHROPIC_API_KEY").map_err(|_| AnthropicError::MissingApiKey)?;
        Ok(Self::new(
            "https://api.anthropic.com/v1",
            api_key,
            default_model,
        ))
    }

    /// Create a client with a custom base URL, using `ANTHROPIC_API_KEY` from
    /// env.
    ///
    /// # Errors
    /// Returns [`AnthropicError::MissingApiKey`] if the env var is not set.
    pub fn custom(
        base_url: impl Into<String>,
        default_model: impl Into<String>,
    ) -> Result<Self, AnthropicError> {
        let api_key =
            std::env::var("ANTHROPIC_API_KEY").map_err(|_| AnthropicError::MissingApiKey)?;
        Ok(Self::new(base_url, api_key, default_model))
    }
}

#[async_trait::async_trait]
impl LlmClient for AnthropicClient {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, LlmError> {
        let anthropic_req = self.build_request(&request);
        let response = self.send_request(&anthropic_req).await?;
        Ok(response)
    }
}

// ── Internal helpers ────────────────────────────────────────────────────

impl AnthropicClient {
    /// Build the Anthropic request body from a `ChatRequest`.
    fn build_request(&self, request: &ChatRequest) -> AnthropicRequest {
        let mut system: Option<String> = None;
        let mut messages: Vec<AnthropicMessage> = Vec::new();

        for msg in &request.messages {
            match msg.role {
                Role::System => {
                    if let Some(ref content) = msg.content {
                        let combined = match system.take() {
                            Some(mut s) => {
                                s.push('\n');
                                s.push_str(content);
                                s
                            }
                            None => content.clone(),
                        };
                        system = Some(combined);
                    }
                }
                _ => {
                    messages.push(to_anthropic_message(msg));
                }
            }
        }

        AnthropicRequest {
            model: self.default_model.clone(),
            max_tokens: request.max_tokens.unwrap_or(1024),
            messages,
            system,
            tools: request.tools.as_ref().map(|tools| {
                tools
                    .iter()
                    .map(|spec| AnthropicTool {
                        name: spec.name.clone(),
                        description: spec.description.clone(),
                        input_schema: spec.parameters.clone(),
                    })
                    .collect()
            }),
            temperature: request.temperature,
        }
    }

    /// Send the request to the Anthropic API and parse the response.
    async fn send_request(
        &self,
        request: &AnthropicRequest,
    ) -> Result<ChatResponse, AnthropicError> {
        let url = format!("{}/messages", self.base_url.trim_end_matches('/'));

        let resp = self
            .http_client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(request)
            .send()
            .await
            .map_err(|e| AnthropicError::Http(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp
                .text()
                .await
                .unwrap_or_else(|_| "<cannot read body>".into());
            return Err(AnthropicError::Api {
                status: status.as_u16(),
                body,
            });
        }

        let anthropic_resp: AnthropicResponse = resp
            .json()
            .await
            .map_err(|e| AnthropicError::Parse(e.to_string()))?;

        Ok(from_anthropic_response(anthropic_resp))
    }
}

// ── JSON mapping functions ──────────────────────────────────────────────

/// Map a praxis `ChatMessage` to an Anthropic API message (system role is
/// filtered out by the caller).
fn to_anthropic_message(msg: &ChatMessage) -> AnthropicMessage {
    match msg.role {
        Role::Tool => {
            // Tool results are sent as user messages with a tool_result block.
            let content = vec![AnthropicContentBlock::ToolResult {
                tool_use_id: msg.tool_call_id.clone().unwrap_or_default(),
                content: msg.content.clone().unwrap_or_default(),
            }];
            AnthropicMessage {
                role: "user".into(),
                content,
            }
        }
        Role::Assistant => {
            let mut content: Vec<AnthropicContentBlock> = Vec::new();

            if let Some(ref text) = msg.content {
                content.push(AnthropicContentBlock::Text { text: text.clone() });
            }

            if let Some(ref calls) = msg.tool_calls {
                for tc in calls {
                    content.push(AnthropicContentBlock::ToolUse {
                        id: tc.id.clone(),
                        name: tc.name.clone(),
                        input: tc.arguments.clone(),
                    });
                }
            }

            AnthropicMessage {
                role: "assistant".into(),
                content,
            }
        }
        Role::User | Role::System => {
            let content = vec![AnthropicContentBlock::Text {
                text: msg.content.clone().unwrap_or_default(),
            }];
            AnthropicMessage {
                role: "user".into(),
                content,
            }
        }
    }
}

/// Map an Anthropic API response to a praxis `ChatResponse`.
fn from_anthropic_response(resp: AnthropicResponse) -> ChatResponse {
    let mut text_parts: Vec<String> = Vec::new();
    let mut calls: Vec<ToolCall> = Vec::new();

    for block in resp.content {
        match block {
            AnthropicResponseContentBlock::Text { text } => {
                text_parts.push(text);
            }
            AnthropicResponseContentBlock::ToolUse { id, name, input } => {
                calls.push(ToolCall {
                    id,
                    name,
                    arguments: input,
                });
            }
        }
    }

    let content = if text_parts.is_empty() {
        None
    } else {
        Some(text_parts.join(""))
    };

    let tool_calls = if calls.is_empty() {
        None
    } else {
        Some(calls)
    };

    let chat_msg = ChatMessage {
        role: Role::Assistant,
        content,
        tool_calls,
        tool_call_id: None,
    };

    let usage = resp.usage.map(|u| Usage {
        prompt_tokens: u.input_tokens.unwrap_or(0),
        completion_tokens: u.output_tokens.unwrap_or(0),
    });

    ChatResponse {
        message: chat_msg,
        usage,
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use praxis_core::agent::ToolSpec;
    use serde_json::json;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // ── JSON serialization tests ───────────────────────────────────────

    #[test]
    fn test_serialize_user_message() {
        let msg = ChatMessage::user("hello");
        let anthropic = to_anthropic_message(&msg);
        assert_eq!(anthropic.role, "user");
        assert_eq!(anthropic.content.len(), 1);
    }

    #[test]
    fn test_serialize_assistant_message() {
        let msg = ChatMessage::assistant("Hello!");
        let anthropic = to_anthropic_message(&msg);
        assert_eq!(anthropic.role, "assistant");
        assert_eq!(anthropic.content.len(), 1);
    }

    #[test]
    fn test_serialize_tool_result_message() {
        let msg = ChatMessage::tool_result("call_1", &json!("ok"));
        let anthropic = to_anthropic_message(&msg);
        assert_eq!(anthropic.role, "user");
        assert_eq!(anthropic.content.len(), 1);
    }

    #[test]
    fn test_serialize_message_with_tool_calls() {
        let tc = ToolCall {
            id: "tc1".into(),
            name: "echo".into(),
            arguments: json!({"msg": "ping"}),
        };
        let msg = ChatMessage::with_tool_calls(vec![tc]);
        let anthropic = to_anthropic_message(&msg);
        assert_eq!(anthropic.role, "assistant");
        assert!(anthropic.content.iter().any(|b| matches!(b, AnthropicContentBlock::ToolUse { .. })));
    }

    #[test]
    fn test_serialize_tools_param() {
        let spec = ToolSpec {
            name: "test_tool".into(),
            description: "A test".into(),
            parameters: json!({"type": "object"}),
        };
        let anthropic_tool = AnthropicTool {
            name: spec.name,
            description: spec.description,
            input_schema: spec.parameters,
        };
        let json = serde_json::to_value(&anthropic_tool).unwrap();
        assert_eq!(json["name"], "test_tool");
        assert_eq!(json["description"], "A test");
        assert_eq!(json["input_schema"], json!({"type": "object"}));
    }

    #[test]
    fn test_serialize_full_request() {
        let msg = ChatMessage::user("hello");
        let spec = ToolSpec {
            name: "echo".into(),
            description: "echo".into(),
            parameters: json!({"type": "object"}),
        };
        let req = ChatRequest {
            messages: vec![ChatMessage::system("You are"), msg],
            tools: Some(vec![spec]),
            temperature: Some(0.5),
            max_tokens: Some(100),
        };

        let client = AnthropicClient::new("http://x", "key", "claude-sonnet-4-20250514");
        let anthropic = client.build_request(&req);

        assert_eq!(anthropic.model, "claude-sonnet-4-20250514");
        assert_eq!(anthropic.max_tokens, 100);
        assert_eq!(anthropic.system.as_deref(), Some("You are"));
        assert_eq!(anthropic.messages.len(), 1);
        assert_eq!(anthropic.messages[0].role, "user");
        assert!(anthropic.tools.is_some());
        assert_eq!(anthropic.temperature, Some(0.5));
    }

    #[test]
    fn test_serialize_request_no_tools() {
        let msg = ChatMessage::user("hi");
        let req = ChatRequest {
            messages: vec![msg],
            tools: None,
            temperature: None,
            max_tokens: None,
        };
        let client = AnthropicClient::new("http://x", "key", "claude-sonnet-4-20250514");
        let anthropic = client.build_request(&req);
        assert!(anthropic.tools.is_none());
        assert!(anthropic.temperature.is_none());
        assert!(anthropic.system.is_none());
        assert_eq!(anthropic.max_tokens, 1024); // default
    }

    #[test]
    fn test_serialize_multiple_system_messages() {
        let req = ChatRequest {
            messages: vec![
                ChatMessage::system("You are helpful"),
                ChatMessage::system("Be concise"),
                ChatMessage::user("hello"),
            ],
            tools: None,
            temperature: None,
            max_tokens: None,
        };
        let client = AnthropicClient::new("http://x", "key", "claude-sonnet-4-20250514");
        let anthropic = client.build_request(&req);
        assert_eq!(anthropic.system.as_deref(), Some("You are helpful\nBe concise"));
        assert_eq!(anthropic.messages.len(), 1);
    }

    // ── JSON deserialization tests ─────────────────────────────────────

    #[test]
    fn test_deserialize_text_response() {
        let json = serde_json::json!({
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Hello!"}
            ],
            "model": "claude-sonnet-4-20250514",
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5
            }
        });
        let resp: AnthropicResponse = serde_json::from_value(json).unwrap();
        let result = from_anthropic_response(resp);

        assert_eq!(result.message.content.as_deref(), Some("Hello!"));
        assert!(result.message.tool_calls.is_none());
        let usage = result.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
    }

    #[test]
    fn test_deserialize_tool_call_response() {
        let json = serde_json::json!({
            "id": "msg_456",
            "type": "message",
            "role": "assistant",
            "content": [
                {
                    "type": "tool_use",
                    "id": "tu_1",
                    "name": "get_weather",
                    "input": {"location": "SF"}
                }
            ],
            "model": "claude-sonnet-4-20250514",
            "stop_reason": "tool_use",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 20
            }
        });
        let resp: AnthropicResponse = serde_json::from_value(json).unwrap();
        let result = from_anthropic_response(resp);

        assert!(result.message.content.is_none());
        let calls = result.message.tool_calls.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "tu_1");
        assert_eq!(calls[0].name, "get_weather");
        assert_eq!(calls[0].arguments, json!({"location": "SF"}));
    }

    #[test]
    fn test_deserialize_text_and_tool_call_response() {
        let json = serde_json::json!({
            "id": "msg_789",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "I'll check the weather:"},
                {
                    "type": "tool_use",
                    "id": "tu_2",
                    "name": "get_weather",
                    "input": {"location": "NYC"}
                }
            ],
            "stop_reason": "tool_use",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 25
            }
        });
        let resp: AnthropicResponse = serde_json::from_value(json).unwrap();
        let result = from_anthropic_response(resp);

        assert_eq!(result.message.content.as_deref(), Some("I'll check the weather:"));
        let calls = result.message.tool_calls.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "get_weather");
    }

    #[test]
    fn test_deserialize_no_usage() {
        let json = serde_json::json!({
            "id": "msg_000",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "ok"}
            ],
            "stop_reason": "end_turn"
        });
        let resp: AnthropicResponse = serde_json::from_value(json).unwrap();
        let result = from_anthropic_response(resp);
        assert!(result.usage.is_none());
    }

    // ── AnthropicError → LlmError conversion ───────────────────────────

    #[test]
    fn test_anthropic_error_to_llm_error_http() {
        let err: LlmError = AnthropicError::Http("timeout".into()).into();
        assert!(matches!(err, LlmError::Request(_)));
    }

    #[test]
    fn test_anthropic_error_to_llm_error_api() {
        let err: LlmError = AnthropicError::Api {
            status: 401,
            body: "invalid key".into(),
        }
        .into();
        assert!(matches!(err, LlmError::Api(_)));
    }

    #[test]
    fn test_anthropic_error_to_llm_error_parse() {
        let err: LlmError = AnthropicError::Parse("bad json".into()).into();
        assert!(matches!(err, LlmError::Parse(_)));
    }

    // ── HTTP mock integration tests ────────────────────────────────────

    #[tokio::test]
    async fn test_send_request_success() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/messages"))
            .and(header("x-api-key", "test-key"))
            .and(header("Content-Type", "application/json"))
            .and(header("anthropic-version", "2023-06-01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "msg_123",
                "type": "message",
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "hi there"}
                ],
                "model": "claude-sonnet-4-20250514",
                "stop_reason": "end_turn",
                "usage": {
                    "input_tokens": 5,
                    "output_tokens": 3
                }
            })))
            .mount(&mock_server)
            .await;

        let client = AnthropicClient::new(mock_server.uri(), "test-key", "claude-sonnet-4-20250514");
        let req = ChatRequest {
            messages: vec![ChatMessage::user("hello")],
            tools: None,
            temperature: None,
            max_tokens: None,
        };

        let result = client.chat(req).await.unwrap();
        assert_eq!(result.message.content.as_deref(), Some("hi there"));
        let usage = result.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 5);
    }

    #[tokio::test]
    async fn test_send_request_api_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/messages"))
            .respond_with(
                ResponseTemplate::new(401)
                    .set_body_json(json!({"error": {"message": "Invalid key"}})),
            )
            .mount(&mock_server)
            .await;

        let client = AnthropicClient::new(mock_server.uri(), "bad-key", "claude-sonnet-4-20250514");
        let req = ChatRequest {
            messages: vec![ChatMessage::user("x")],
            tools: None,
            temperature: None,
            max_tokens: None,
        };

        let err = client.chat(req).await.unwrap_err();
        assert!(matches!(err, LlmError::Api(_)));
    }

    #[tokio::test]
    async fn test_send_request_tool_call() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "msg_tc",
                "type": "message",
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_use",
                        "id": "tu_1",
                        "name": "echo",
                        "input": {"x": 1}
                    }
                ],
                "stop_reason": "tool_use"
            })))
            .mount(&mock_server)
            .await;

        let client = AnthropicClient::new(mock_server.uri(), "key", "claude-sonnet-4-20250514");
        let req = ChatRequest {
            messages: vec![ChatMessage::user("use tool")],
            tools: None,
            temperature: None,
            max_tokens: None,
        };

        let result = client.chat(req).await.unwrap();
        let calls = result.message.tool_calls.unwrap();
        assert_eq!(calls[0].name, "echo");
        assert_eq!(calls[0].arguments, json!({"x": 1}));
    }
}
