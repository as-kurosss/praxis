//! **OpenAI-compatible LLM client** — implements [`LlmClient`](praxis_core::agent::LlmClient)
//! for any OpenAI-compatible API (`OpenAI`, `Ollama`, `vLLM`, local LLMs, etc.).
//!
//! # Usage
//! ```ignore
//! use praxis_runtime::OpenAiClient;
//!
//! let client = OpenAiClient::from_env("gpt-4o").unwrap();
//! // or with custom base_url:
//! let client = OpenAiClient::new("http://localhost:11434/v1", "ollama", "llama3");
//! ```

use praxis_core::agent::{
    ChatMessage, ChatRequest, ChatResponse, LlmClient, LlmError, Role, ToolCall, ToolSpec, Usage,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
// ── OpenAI API types (internal, for JSON serialization) ────────────────

/// Request body for `OpenAI` chat completions API.
#[derive(Serialize)]
struct OpenAiRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Serialize)]
struct OpenAiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Serialize)]
struct OpenAiToolCall {
    id: String,
    #[serde(rename = "type")]
    type_: String,
    function: OpenAiFunctionCall,
}

#[derive(Serialize)]
struct OpenAiFunctionCall {
    name: String,
    /// Stringified JSON arguments.
    arguments: String,
}

#[derive(Serialize)]
struct OpenAiTool {
    #[serde(rename = "type")]
    type_: String,
    function: ToolSpec,
}

/// Response body from `OpenAI` chat completions API.
#[derive(Deserialize)]
struct OpenAiResponse {
    #[allow(dead_code)]
    id: Option<String>,
    choices: Vec<OpenAiChoice>,
    usage: Option<OpenAiUsage>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiResponseMessage,
    #[allow(dead_code)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiResponseMessage {
    #[allow(dead_code)]
    role: Option<String>,
    content: Option<String>,
    tool_calls: Option<Vec<OpenAiResponseToolCall>>,
}

#[derive(Deserialize)]
struct OpenAiResponseToolCall {
    id: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    type_: Option<String>,
    function: OpenAiResponseFunction,
}

#[derive(Deserialize)]
struct OpenAiResponseFunction {
    name: String,
    /// Stringified JSON arguments.
    arguments: String,
}

#[derive(Deserialize)]
struct OpenAiUsage {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
}

/// Error type for the `OpenAI` client.
#[derive(Debug, thiserror::Error)]
pub enum OpenAiError {
    /// HTTP request failed (network, timeout, etc.).
    #[error("HTTP request failed: {0}")]
    Http(String),
    /// API returned an error status.
    #[error("OpenAI API error (status {status}): {body}")]
    Api { status: u16, body: String },
    /// Failed to parse the API response JSON.
    #[error("Response parse error: {0}")]
    Parse(String),
    /// Missing API key.
    #[error("OPENAI_API_KEY not set")]
    MissingApiKey,
}

impl From<OpenAiError> for LlmError {
    fn from(e: OpenAiError) -> Self {
        match e {
            OpenAiError::Http(msg) => LlmError::Request(msg),
            OpenAiError::Api { status, body } => LlmError::Api(format!("HTTP {status}: {body}")),
            OpenAiError::Parse(msg) => LlmError::Parse(msg),
            OpenAiError::MissingApiKey => LlmError::Request("OPENAI_API_KEY not set".into()),
        }
    }
}

// ── OpenAiClient ───────────────────────────────────────────────────────

/// An LLM client for any OpenAI-compatible chat completions API.
///
/// Configure with `base_url` to point at any provider:
/// * `OpenAI`: `https://api.openai.com/v1`
/// * `Ollama`: `http://localhost:11434/v1`
/// * `vLLM`: `http://localhost:8000/v1`
///
/// The API key is read from the `OPENAI_API_KEY` environment variable by default.
pub struct OpenAiClient {
    base_url: String,
    api_key: String,
    default_model: String,
    http_client: Client,
}

impl OpenAiClient {
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

    /// Create a client using `OPENAI_API_KEY` from the environment.
    ///
    /// # Errors
    /// Returns [`OpenAiError::MissingApiKey`] if the env var is not set.
    pub fn from_env(default_model: impl Into<String>) -> Result<Self, OpenAiError> {
        let api_key = std::env::var("OPENAI_API_KEY").map_err(|_| OpenAiError::MissingApiKey)?;
        Ok(Self::new(
            "https://api.openai.com/v1",
            api_key,
            default_model,
        ))
    }

    /// Create a client with a custom base URL, using `OPENAI_API_KEY` from env.
    ///
    /// # Errors
    /// Returns [`OpenAiError::MissingApiKey`] if the env var is not set.
    pub fn custom(
        base_url: impl Into<String>,
        default_model: impl Into<String>,
    ) -> Result<Self, OpenAiError> {
        let api_key = std::env::var("OPENAI_API_KEY").map_err(|_| OpenAiError::MissingApiKey)?;
        Ok(Self::new(base_url, api_key, default_model))
    }
}

#[async_trait::async_trait]
impl LlmClient for OpenAiClient {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, LlmError> {
        let openai_req = self.build_request(&request);
        let response = self.send_request(&openai_req).await?;
        Ok(response)
    }
}

// ── Internal helpers ───────────────────────────────────────────────────

impl OpenAiClient {
    /// Build the `OpenAI` request body from a `ChatRequest`.
    fn build_request(&self, request: &ChatRequest) -> OpenAiRequest {
        OpenAiRequest {
            model: self.default_model.clone(),
            messages: request.messages.iter().map(to_openai_message).collect(),
            tools: request.tools.as_ref().map(|tools| {
                tools
                    .iter()
                    .map(|spec| OpenAiTool {
                        type_: "function".into(),
                        function: spec.clone(),
                    })
                    .collect()
            }),
            temperature: request.temperature,
            max_tokens: request.max_tokens,
        }
    }

    /// Send the request to the OpenAI-compatible API and parse the response.
    async fn send_request(&self, request: &OpenAiRequest) -> Result<ChatResponse, OpenAiError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let resp = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(request)
            .send()
            .await
            .map_err(|e| OpenAiError::Http(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp
                .text()
                .await
                .unwrap_or_else(|_| "<cannot read body>".into());
            return Err(OpenAiError::Api {
                status: status.as_u16(),
                body,
            });
        }

        let openai_resp: OpenAiResponse = resp
            .json()
            .await
            .map_err(|e| OpenAiError::Parse(e.to_string()))?;

        from_openai_response(openai_resp)
    }
}

// ── JSON mapping functions ─────────────────────────────────────────────

/// Map a praxis `ChatMessage` to an `OpenAI` API message.
fn to_openai_message(msg: &ChatMessage) -> OpenAiMessage {
    let tool_calls = msg.tool_calls.as_ref().map(|calls| {
        calls
            .iter()
            .map(|tc| OpenAiToolCall {
                id: tc.id.clone(),
                type_: "function".into(),
                function: OpenAiFunctionCall {
                    name: tc.name.clone(),
                    arguments: tc.arguments.to_string(),
                },
            })
            .collect()
    });

    OpenAiMessage {
        role: msg.role.as_str().to_string(),
        content: msg.content.clone(),
        tool_calls,
        tool_call_id: msg.tool_call_id.clone(),
    }
}

/// Map an `OpenAI` API response to a praxis `ChatResponse`.
fn from_openai_response(resp: OpenAiResponse) -> Result<ChatResponse, OpenAiError> {
    let choice = resp
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| OpenAiError::Parse("empty choices array".into()))?;

    let message = choice.message;

    let tool_calls: Option<Vec<ToolCall>> = message
        .tool_calls
        .map(|calls| {
            calls
                .into_iter()
                .map(|tc| {
                    let arguments = serde_json::from_str(&tc.function.arguments).map_err(|e| {
                        OpenAiError::Parse(format!(
                            "tool call `{}` arguments: {e}",
                            tc.function.name
                        ))
                    })?;
                    Ok(ToolCall {
                        id: tc.id,
                        name: tc.function.name,
                        arguments,
                    })
                })
                .collect::<Result<Vec<_>, OpenAiError>>()
        })
        .transpose()?;

    let chat_msg = ChatMessage {
        role: Role::Assistant,
        content: message.content,
        tool_calls,
        tool_call_id: None,
    };

    let usage = resp.usage.map(|u| Usage {
        prompt_tokens: u.prompt_tokens.unwrap_or(0),
        completion_tokens: u.completion_tokens.unwrap_or(0),
    });

    Ok(ChatResponse {
        message: chat_msg,
        usage,
    })
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{bearer_token, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // ── JSON serialization tests ───────────────────────────────────────

    #[test]
    fn test_serialize_simple_message() {
        let msg = ChatMessage::user("hello");
        let openai = to_openai_message(&msg);
        assert_eq!(openai.role, "user");
        assert_eq!(openai.content.as_deref(), Some("hello"));
        assert!(openai.tool_calls.is_none());
        assert!(openai.tool_call_id.is_none());
    }

    #[test]
    fn test_serialize_system_message() {
        let msg = ChatMessage::system("be helpful");
        let openai = to_openai_message(&msg);
        assert_eq!(openai.role, "system");
        assert_eq!(openai.content.as_deref(), Some("be helpful"));
    }

    #[test]
    fn test_serialize_tool_message() {
        let msg = ChatMessage::tool_result("call_1", &json!("ok"));
        let openai = to_openai_message(&msg);
        assert_eq!(openai.role, "tool");
        assert_eq!(openai.content.as_deref(), Some("\"ok\""));
        assert_eq!(openai.tool_call_id.as_deref(), Some("call_1"));
    }

    #[test]
    fn test_serialize_with_tool_calls() {
        let tc = ToolCall {
            id: "tc1".into(),
            name: "echo".into(),
            arguments: json!({"msg": "ping"}),
        };
        let msg = ChatMessage::with_tool_calls(vec![tc]);
        let openai = to_openai_message(&msg);
        assert_eq!(openai.role, "assistant");
        assert!(openai.content.is_none());
        let calls = openai.tool_calls.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "tc1");
        assert_eq!(calls[0].type_, "function");
        assert_eq!(calls[0].function.name, "echo");
        assert_eq!(calls[0].function.arguments, "{\"msg\":\"ping\"}");
    }

    #[test]
    fn test_serialize_tools_param() {
        let spec = ToolSpec {
            name: "test_tool".into(),
            description: "A test".into(),
            parameters: json!({"type": "object"}),
        };
        let openai_tool = OpenAiTool {
            type_: "function".into(),
            function: spec,
        };
        let json = serde_json::to_value(&openai_tool).unwrap();
        assert_eq!(json["type"], "function");
        assert_eq!(json["function"]["name"], "test_tool");
        assert_eq!(json["function"]["description"], "A test");
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

        let client = OpenAiClient::new("http://x", "key", "gpt-4o");
        let openai = client.build_request(&req);

        assert_eq!(openai.model, "gpt-4o");
        assert_eq!(openai.messages.len(), 2);
        assert_eq!(openai.messages[0].role, "system");
        assert_eq!(openai.messages[1].role, "user");
        assert!(openai.tools.is_some());
        assert_eq!(openai.temperature, Some(0.5));
        assert_eq!(openai.max_tokens, Some(100));
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
        let client = OpenAiClient::new("http://x", "key", "gpt-4o");
        let openai = client.build_request(&req);
        assert!(openai.tools.is_none());
        assert!(openai.temperature.is_none());
        assert!(openai.max_tokens.is_none());
    }

    // ── JSON deserialization tests ─────────────────────────────────────

    #[test]
    fn test_deserialize_text_response() {
        let json = serde_json::json!({
            "id": "chatcmpl-123",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello!"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5
            }
        });
        let resp: OpenAiResponse = serde_json::from_value(json).unwrap();
        let result = from_openai_response(resp).unwrap();

        assert_eq!(result.message.content.as_deref(), Some("Hello!"));
        assert!(result.message.tool_calls.is_none());
        let usage = result.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
    }

    #[test]
    fn test_deserialize_tool_call_response() {
        let json = serde_json::json!({
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": {
                            "name": "echo",
                            "arguments": "{\"msg\": \"ping\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        });
        let resp: OpenAiResponse = serde_json::from_value(json).unwrap();
        let result = from_openai_response(resp).unwrap();

        assert!(result.message.content.is_none());
        let calls = result.message.tool_calls.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_abc");
        assert_eq!(calls[0].name, "echo");
        assert_eq!(calls[0].arguments, json!({"msg": "ping"}));
    }

    #[test]
    fn test_deserialize_no_usage() {
        let json = serde_json::json!({
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "ok"
                }
            }]
        });
        let resp: OpenAiResponse = serde_json::from_value(json).unwrap();
        let result = from_openai_response(resp).unwrap();
        assert!(result.usage.is_none());
    }

    #[test]
    fn test_deserialize_empty_choices_error() {
        let json = serde_json::json!({
            "choices": []
        });
        let resp: OpenAiResponse = serde_json::from_value(json).unwrap();
        let err = from_openai_response(resp).unwrap_err();
        assert!(err.to_string().contains("empty choices"));
    }

    // ── OpenAiError → LlmError conversion ──────────────────────────────

    #[test]
    fn test_openai_error_to_llm_error_http() {
        let err: LlmError = OpenAiError::Http("timeout".into()).into();
        assert!(matches!(err, LlmError::Request(_)));
    }

    #[test]
    fn test_openai_error_to_llm_error_api() {
        let err: LlmError = OpenAiError::Api {
            status: 401,
            body: "invalid key".into(),
        }
        .into();
        assert!(matches!(err, LlmError::Api(_)));
    }

    #[test]
    fn test_openai_error_to_llm_error_parse() {
        let err: LlmError = OpenAiError::Parse("bad json".into()).into();
        assert!(matches!(err, LlmError::Parse(_)));
    }

    // ── HTTP mock integration tests ────────────────────────────────────

    #[tokio::test]
    async fn test_send_request_success() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(bearer_token("test-key"))
            .and(header("Content-Type", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "hi there"
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 5,
                    "completion_tokens": 3
                }
            })))
            .mount(&mock_server)
            .await;

        let client = OpenAiClient::new(mock_server.uri(), "test-key", "gpt-4o");
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
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(401)
                    .set_body_json(json!({"error": {"message": "Invalid key"}})),
            )
            .mount(&mock_server)
            .await;

        let client = OpenAiClient::new(mock_server.uri(), "bad-key", "gpt-4o");
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
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [{
                            "id": "call_1",
                            "type": "function",
                            "function": {
                                "name": "echo",
                                "arguments": "{\"x\": 1}"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            })))
            .mount(&mock_server)
            .await;

        let client = OpenAiClient::new(mock_server.uri(), "key", "gpt-4o");
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

    // ── Edge-case wiremock tests ─────────────────────────────────────

    #[tokio::test]
    async fn test_empty_content_response() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": ""
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 1,
                    "completion_tokens": 1
                }
            })))
            .mount(&mock_server)
            .await;

        let client = OpenAiClient::new(mock_server.uri(), "test-key", "gpt-4o");
        let req = ChatRequest {
            messages: vec![ChatMessage::user("hello")],
            tools: None,
            temperature: None,
            max_tokens: None,
        };

        let result = client.chat(req).await.unwrap();
        assert_eq!(result.message.content.as_deref(), Some(""));
        let usage = result.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 1);
    }

    #[tokio::test]
    async fn test_multiple_choices_response() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [
                    {
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": "first choice"
                        },
                        "finish_reason": "stop"
                    },
                    {
                        "index": 1,
                        "message": {
                            "role": "assistant",
                            "content": "second choice"
                        },
                        "finish_reason": "stop"
                    }
                ],
                "usage": {
                    "prompt_tokens": 5,
                    "completion_tokens": 3
                }
            })))
            .mount(&mock_server)
            .await;

        let client = OpenAiClient::new(mock_server.uri(), "test-key", "gpt-4o");
        let req = ChatRequest {
            messages: vec![ChatMessage::user("hello")],
            tools: None,
            temperature: None,
            max_tokens: None,
        };

        let result = client.chat(req).await.unwrap();
        assert_eq!(result.message.content.as_deref(), Some("first choice"));
    }

    #[tokio::test]
    async fn test_zero_tokens_usage() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "ok"
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 0,
                    "completion_tokens": 0
                }
            })))
            .mount(&mock_server)
            .await;

        let client = OpenAiClient::new(mock_server.uri(), "test-key", "gpt-4o");
        let req = ChatRequest {
            messages: vec![ChatMessage::user("hello")],
            tools: None,
            temperature: None,
            max_tokens: None,
        };

        let result = client.chat(req).await.unwrap();
        let usage = result.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 0);
        assert_eq!(usage.completion_tokens, 0);
    }

    #[tokio::test]
    async fn test_tool_call_empty_arguments() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [{
                            "id": "call_empty",
                            "type": "function",
                            "function": {
                                "name": "noop",
                                "arguments": "{}"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            })))
            .mount(&mock_server)
            .await;

        let client = OpenAiClient::new(mock_server.uri(), "test-key", "gpt-4o");
        let req = ChatRequest {
            messages: vec![ChatMessage::user("do nothing")],
            tools: None,
            temperature: None,
            max_tokens: None,
        };

        let result = client.chat(req).await.unwrap();
        let calls = result.message.tool_calls.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "noop");
        assert_eq!(calls[0].arguments, json!({}));
    }

    #[tokio::test]
    async fn test_http_500_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(500)
                    .set_body_json(json!({"error": {"message": "Internal server error", "type": "server_error"}})),
            )
            .mount(&mock_server)
            .await;

        let client = OpenAiClient::new(mock_server.uri(), "test-key", "gpt-4o");
        let req = ChatRequest {
            messages: vec![ChatMessage::user("hello")],
            tools: None,
            temperature: None,
            max_tokens: None,
        };

        let err = client.chat(req).await.unwrap_err();
        assert!(matches!(err, LlmError::Api(_)));
        assert!(err.to_string().contains("500"));
    }

    #[tokio::test]
    async fn test_http_429_rate_limit() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(429)
                    .set_body_json(json!({"error": {"message": "Rate limit exceeded. Please retry after 20s.", "type": "rate_limit"}})),
            )
            .mount(&mock_server)
            .await;

        let client = OpenAiClient::new(mock_server.uri(), "test-key", "gpt-4o");
        let req = ChatRequest {
            messages: vec![ChatMessage::user("hello")],
            tools: None,
            temperature: None,
            max_tokens: None,
        };

        let err = client.chat(req).await.unwrap_err();
        assert!(matches!(err, LlmError::Api(_)));
        assert!(err.to_string().contains("429"));
        assert!(err.to_string().contains("Rate limit"));
    }

    #[tokio::test]
    async fn test_malformed_json_response() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not valid json at all"))
            .mount(&mock_server)
            .await;

        let client = OpenAiClient::new(mock_server.uri(), "test-key", "gpt-4o");
        let req = ChatRequest {
            messages: vec![ChatMessage::user("hello")],
            tools: None,
            temperature: None,
            max_tokens: None,
        };

        let err = client.chat(req).await.unwrap_err();
        assert!(matches!(err, LlmError::Parse(_)));
    }

    #[tokio::test]
    async fn test_content_with_special_characters() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "Hello, 世界! 🌍\nNew line here.\tTabbed."
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 1,
                    "completion_tokens": 8
                }
            })))
            .mount(&mock_server)
            .await;

        let client = OpenAiClient::new(mock_server.uri(), "test-key", "gpt-4o");
        let req = ChatRequest {
            messages: vec![ChatMessage::user("hello")],
            tools: None,
            temperature: None,
            max_tokens: None,
        };

        let result = client.chat(req).await.unwrap();
        assert_eq!(result.message.content.as_deref(), Some("Hello, 世界! 🌍\nNew line here.\tTabbed."));
    }

    #[tokio::test]
    async fn test_concurrent_requests() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "response"
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 1,
                    "completion_tokens": 1
                }
            })))
            .mount(&mock_server)
            .await;

        let client = OpenAiClient::new(mock_server.uri(), "test-key", "gpt-4o");
        let req_a = ChatRequest {
            messages: vec![ChatMessage::user("A")],
            tools: None,
            temperature: None,
            max_tokens: None,
        };
        let req_b = ChatRequest {
            messages: vec![ChatMessage::user("B")],
            tools: None,
            temperature: None,
            max_tokens: None,
        };

        let (res_a, res_b) = tokio::join!(client.chat(req_a), client.chat(req_b));
        assert!(res_a.is_ok());
        assert!(res_b.is_ok());
        assert_eq!(res_a.unwrap().message.content.as_deref(), Some("response"));
        assert_eq!(res_b.unwrap().message.content.as_deref(), Some("response"));
    }

    // ── E2E test (conditional) ────────────────────────────────────────

    #[tokio::test]
    async fn test_e2e_real_api() {
        let api_key = match std::env::var("OPENAI_API_KEY") {
            Ok(k) if !k.is_empty() && k != "test-key" => k,
            _ => {
                eprintln!("Skipping e2e test: OPENAI_API_KEY not set");
                return;
            }
        };
        let client = OpenAiClient::new(
            std::env::var("OPENAI_API_URL").unwrap_or_else(|_| "https://api.openai.com/v1".into()),
            api_key,
            &std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into()),
        );
        let req = ChatRequest {
            messages: vec![
                ChatMessage::system("You are a helpful assistant. Reply very briefly."),
                ChatMessage::user("Say just 'hello'"),
            ],
            tools: None,
            temperature: Some(0.0),
            max_tokens: Some(50),
        };
        let result = client.chat(req).await.unwrap();
        assert!(result.message.content.is_some());
        assert!(!result.message.content.as_deref().unwrap_or("").is_empty());
        assert!(result.usage.is_some());
    }
}
