//! **Google Gemini LLM client** — implements [`LlmClient`] for Geminii API.
//!
//! Uses the `generateContent` endpoint with function calling support.
//!
//! # Usage
//! ```ignore
//! use praxis_runtime::GeminiClient;
//!
//! let client = GeminiClient::from_env("gemini-2.0-flash").unwrap();
//! ```

use praxis_core::agent::{
    ChatMessage, ChatRequest, ChatResponse, LlmClient, LlmError, Role, ToolCall, ToolSpec, Usage,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Gemini API types ─────────────────────────────────────────────────

#[derive(Serialize)]
struct GeminiRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiContent>,
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<GeminiTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Serialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum GeminiPart {
    Text {
        text: String,
    },
    FunctionCall {
        function_call: GeminiFunctionCall,
    },
    FunctionResponse {
        function_response: GeminiFunctionResponse,
    },
}

#[derive(Serialize, Deserialize)]
struct GeminiFunctionCall {
    name: String,
    #[serde(default)]
    args: Value,
}

#[derive(Serialize)]
struct GeminiFunctionResponse {
    name: String,
    response: Value,
}

#[derive(Serialize)]
struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
}

#[derive(Serialize)]
struct GeminiTool {
    function_declarations: Vec<GeminiFunctionDeclaration>,
}

#[derive(Serialize)]
struct GeminiFunctionDeclaration {
    name: String,
    description: String,
    parameters: Value,
}

// ── Response types ───────────────────────────────────────────────────

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
    #[serde(default)]
    usage_metadata: Option<GeminiUsageMetadata>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: GeminiResponseContent,
    #[allow(dead_code)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct GeminiResponseContent {
    #[serde(default)]
    parts: Vec<GeminiResponsePart>,
    #[allow(dead_code)]
    role: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum GeminiResponsePart {
    Text { text: String },
    FunctionCall { function_call: GeminiFunctionCall },
}

#[derive(Deserialize)]
struct GeminiUsageMetadata {
    #[serde(default)]
    prompt_token_count: Option<u32>,
    #[serde(default)]
    candidates_token_count: Option<u32>,
}

// ── Error ─────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum GeminiError {
    #[error("HTTP request failed: {0}")]
    Http(String),
    #[error("Gemini API error (status {status}): {body}")]
    Api { status: u16, body: String },
    #[error("Response parse error: {0}")]
    Parse(String),
    #[error("GEMINI_API_KEY not set")]
    MissingApiKey,
    #[error("No candidates in response")]
    NoCandidates,
}

impl From<GeminiError> for LlmError {
    fn from(e: GeminiError) -> Self {
        match e {
            GeminiError::Http(msg) => LlmError::Request(msg),
            GeminiError::Api { status, body } => LlmError::Api(format!("HTTP {status}: {body}")),
            GeminiError::Parse(msg) => LlmError::Parse(msg),
            GeminiError::MissingApiKey => LlmError::Request("GEMINI_API_KEY not set".into()),
            GeminiError::NoCandidates => {
                LlmError::Request("no candidates in Gemini response".into())
            }
        }
    }
}

// ── GeminiClient ─────────────────────────────────────────────────────

/// An LLM client for Google's Gemini API.
///
/// Reads the API key from the `GEMINI_API_KEY` environment variable by default.
pub struct GeminiClient {
    api_key: String,
    default_model: String,
    http_client: Client,
}

impl GeminiClient {
    /// Create a new client with explicit configuration.
    pub fn new(api_key: impl Into<String>, default_model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            default_model: default_model.into(),
            http_client: Client::new(),
        }
    }

    /// Create a client using `GEMINI_API_KEY` from the environment.
    pub fn from_env(default_model: impl Into<String>) -> Result<Self, GeminiError> {
        let api_key = std::env::var("GEMINI_API_KEY").map_err(|_| GeminiError::MissingApiKey)?;
        Ok(Self::new(api_key, default_model))
    }

    fn build_url(&self) -> String {
        format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.default_model, self.api_key
        )
    }

    fn convert_messages(
        &self,
        request: &ChatRequest,
    ) -> (Option<GeminiContent>, Vec<GeminiContent>) {
        let mut sys_instruction = None;
        let mut contents = Vec::new();

        for msg in &request.messages {
            match msg.role {
                Role::System => {
                    if let Some(ref text) = msg.content {
                        sys_instruction = Some(GeminiContent {
                            role: "user".into(),
                            parts: vec![GeminiPart::Text { text: text.clone() }],
                        });
                    }
                }
                Role::User => {
                    contents.push(GeminiContent {
                        role: "user".into(),
                        parts: vec![GeminiPart::Text {
                            text: msg.content.clone().unwrap_or_default(),
                        }],
                    });
                }
                Role::Assistant => {
                    let mut parts = Vec::new();
                    if let Some(ref text) = msg.content
                        && !text.is_empty()
                    {
                        parts.push(GeminiPart::Text { text: text.clone() });
                    }
                    if let Some(ref calls) = msg.tool_calls {
                        for tc in calls {
                            parts.push(GeminiPart::FunctionCall {
                                function_call: GeminiFunctionCall {
                                    name: tc.name.clone(),
                                    args: tc.arguments.clone(),
                                },
                            });
                        }
                    }
                    contents.push(GeminiContent {
                        role: "model".into(),
                        parts,
                    });
                }
                Role::Tool => {
                    contents.push(GeminiContent {
                        role: "user".into(),
                        parts: vec![GeminiPart::FunctionResponse {
                            function_response: GeminiFunctionResponse {
                                name: msg.tool_call_id.clone().unwrap_or_default(),
                                response: serde_json::json!({
                                    "result": msg.content.clone().unwrap_or_default()
                                }),
                            },
                        }],
                    });
                }
            }
        }

        (sys_instruction, contents)
    }

    fn convert_tools(tools: &[ToolSpec]) -> Vec<GeminiTool> {
        if tools.is_empty() {
            return vec![];
        }
        vec![GeminiTool {
            function_declarations: tools
                .iter()
                .map(|t| GeminiFunctionDeclaration {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.parameters.clone(),
                })
                .collect(),
        }]
    }

    fn parse_response(&self, response: GeminiResponse) -> Result<ChatResponse, GeminiError> {
        let candidate = response
            .candidates
            .into_iter()
            .next()
            .ok_or(GeminiError::NoCandidates)?;

        let mut content = None;
        let mut tool_calls = Vec::new();

        for part in candidate.content.parts {
            match part {
                GeminiResponsePart::Text { text } => {
                    content = Some(text);
                }
                GeminiResponsePart::FunctionCall { function_call } => {
                    tool_calls.push(ToolCall {
                        id: format!("fc_{}", function_call.name),
                        name: function_call.name,
                        arguments: function_call.args,
                    });
                }
            }
        }

        let usage = response.usage_metadata.map(|u| Usage {
            prompt_tokens: u.prompt_token_count.unwrap_or(0),
            completion_tokens: u.candidates_token_count.unwrap_or(0),
        });

        Ok(ChatResponse {
            message: if tool_calls.is_empty() {
                ChatMessage::assistant(content.unwrap_or_default())
            } else {
                ChatMessage::with_tool_calls(tool_calls)
            },
            usage,
        })
    }
}

#[async_trait::async_trait]
impl LlmClient for GeminiClient {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, LlmError> {
        let (sys_instruction, contents) = self.convert_messages(&request);
        let gemini_tools = Self::convert_tools(request.tools.as_deref().unwrap_or(&[]));

        let gemini_request = GeminiRequest {
            system_instruction: sys_instruction,
            contents,
            tools: if gemini_tools.is_empty() {
                None
            } else {
                Some(gemini_tools)
            },
            generation_config: Some(GeminiGenerationConfig {
                temperature: request.temperature,
                max_output_tokens: request.max_tokens,
            }),
        };

        let resp = self
            .http_client
            .post(self.build_url())
            .json(&gemini_request)
            .send()
            .await
            .map_err(|e| GeminiError::Http(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(GeminiError::Api {
                status: status.as_u16(),
                body,
            }
            .into());
        }

        let gemini_resp: GeminiResponse = resp
            .json()
            .await
            .map_err(|e| GeminiError::Parse(e.to_string()))?;

        self.parse_response(gemini_resp).map_err(Into::into)
    }
}
