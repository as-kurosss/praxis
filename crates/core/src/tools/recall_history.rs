//! **RecallHistoryTool** — lets an agent search and recall episodic memory.
//!
//! When [`EpisodicMemory`] is configured, this tool is auto-registered so the
//! agent can retrieve past turns and capped tool results on demand.
//!
//! # Operations
//!
//! | Operation      | Arguments                         | Returns                           |
//! |----------------|-----------------------------------|-----------------------------------|
//! | `search`       | `query: str`, `k: usize`          | Ordered list of matching entries  |
//! | `expand`       | `turn_id: str`                    | Full text of a specific entry     |
//! | `recall_tool`  | `tool_call_id: str`               | Full result of a capped tool call |

use crate::agent::tool::{Tool, ToolCategory, ToolError, ToolSpec};
use crate::memory::EpisodicMemory;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

/// A tool that grants the agent access to its episodic memory.
///
/// # JSON arguments
///
/// ## `search` — find relevant past turns
/// ```json
/// { "operation": "search", "query": "database deployment", "k": 5 }
/// ```
///
/// ## `expand` — retrieve a full turn by ID
/// ```json
/// { "operation": "expand", "turn_id": "turn_17" }
/// ```
///
/// ## `recall_tool` — retrieve a capped tool result
/// ```json
/// { "operation": "recall_tool", "tool_call_id": "call_abc123" }
/// ```
pub struct RecallHistoryTool {
    memory: Arc<Mutex<EpisodicMemory>>,
}

impl RecallHistoryTool {
    /// The tool name exposed to the LLM.
    pub const NAME: &str = "recall_history";

    /// Create a new `RecallHistoryTool` wrapping the given memory store.
    #[must_use]
    pub fn new(memory: Arc<Mutex<EpisodicMemory>>) -> Self {
        Self { memory }
    }
}

#[async_trait::async_trait]
impl Tool for RecallHistoryTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: Self::NAME.to_string(),
            description: "Search and retrieve past conversation turns and tool results \
                          from the agent's episodic memory. Use this when you need \
                          context that was evicted from the working conversation."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "operation": {
                        "type": "string",
                        "enum": ["search", "expand", "recall_tool"],
                        "description": "What to do:\n\
                            - search: find relevant turns by keyword\n\
                            - expand: get the full text of a specific turn\n\
                            - recall_tool: retrieve a capped tool result"
                    },
                    "query": {
                        "type": "string",
                        "description": "Keywords to search for (used with 'search' operation)"
                    },
                    "k": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 50,
                        "description": "Maximum results (used with 'search' operation, default 5)"
                    },
                    "turn_id": {
                        "type": "string",
                        "description": "Turn identifier to expand (used with 'expand' operation)"
                    },
                    "tool_call_id": {
                        "type": "string",
                        "description": "Tool call identifier to recall (used with 'recall_tool' operation)"
                    }
                },
                "required": ["operation"]
            }),
            category: ToolCategory::Generic,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let operation = args
            .get("operation")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArgs {
                tool: Self::NAME.to_string(),
                message: "missing required 'operation' field".into(),
            })?
            .to_string();

        let memory = self.memory.clone();

        match operation.as_str() {
            "search" => {
                let query = args
                    .get("query")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ToolError::InvalidArgs {
                        tool: Self::NAME.to_string(),
                        message: "'search' operation requires 'query' string".into(),
                    })?
                    .to_string();
                let k = args.get("k").and_then(Value::as_u64).unwrap_or(5).min(50) as usize;

                tokio::task::spawn_blocking(move || {
                    let mut mem = memory.lock().map_err(|e| ToolError::Execution {
                        tool: Self::NAME.to_string(),
                        message: format!("mutex poisoned: {e}"),
                    })?;
                    let results = mem.search(&query, k);
                    let entries: Vec<Value> = results
                        .into_iter()
                        .map(|entry| {
                            json!({
                                "turn_id": entry.turn_id,
                                "input": entry.input,
                                "output": entry.output,
                                "keywords": entry.keywords,
                            })
                        })
                        .collect();
                    Ok(json!({ "results": entries, "count": entries.len() }))
                })
                .await
                .map_err(|e| ToolError::Execution {
                    tool: Self::NAME.to_string(),
                    message: format!("spawn_blocking join error: {e}"),
                })?
            }
            "expand" => {
                let turn_id = args
                    .get("turn_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ToolError::InvalidArgs {
                        tool: Self::NAME.to_string(),
                        message: "'expand' operation requires 'turn_id' string".into(),
                    })?
                    .to_string();

                tokio::task::spawn_blocking(move || {
                    let mut mem = memory.lock().map_err(|e| ToolError::Execution {
                        tool: Self::NAME.to_string(),
                        message: format!("mutex poisoned: {e}"),
                    })?;
                    match mem.recall(&turn_id) {
                        Some(entry) => Ok(json!({
                            "turn_id": entry.turn_id,
                            "input": entry.input,
                            "output": entry.output,
                            "tool_calls": entry.tool_calls,
                            "keywords": entry.keywords,
                        })),
                        None => Ok(json!({
                            "error": format!("turn '{turn_id}' not found in episodic memory")
                        })),
                    }
                })
                .await
                .map_err(|e| ToolError::Execution {
                    tool: Self::NAME.to_string(),
                    message: format!("spawn_blocking join error: {e}"),
                })?
            }
            "recall_tool" => {
                let tool_call_id = args
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ToolError::InvalidArgs {
                        tool: Self::NAME.to_string(),
                        message: "'recall_tool' operation requires 'tool_call_id' string".into(),
                    })?
                    .to_string();

                tokio::task::spawn_blocking(move || {
                    let mem = memory.lock().map_err(|e| ToolError::Execution {
                        tool: Self::NAME.to_string(),
                        message: format!("mutex poisoned: {e}"),
                    })?;
                    match mem.recall_tool(&tool_call_id) {
                        Some((tool_name, arguments, result)) => Ok(json!({
                            "tool_name": tool_name,
                            "arguments": arguments,
                            "result": result,
                        })),
                        None => Ok(json!({
                            "error": format!("tool call '{tool_call_id}' not found in capped results")
                        })),
                    }
                })
                .await
                .map_err(|e| ToolError::Execution {
                    tool: Self::NAME.to_string(),
                    message: format!("spawn_blocking join error: {e}"),
                })?
            }
            other => Err(ToolError::InvalidArgs {
                tool: Self::NAME.to_string(),
                message: format!("unknown operation '{other}'"),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::EpisodicEntry;

    async fn run_tool(tool: &RecallHistoryTool, args: Value) -> Result<Value, ToolError> {
        tool.call(args).await
    }

    #[tokio::test]
    async fn test_search_no_results() {
        let memory = Arc::new(Mutex::new(EpisodicMemory::new()));
        let tool = RecallHistoryTool::new(memory);

        let result = run_tool(
            &tool,
            json!({"operation": "search", "query": "nonexistent"}),
        )
        .await
        .unwrap();
        assert_eq!(result["count"], 0);
    }

    #[tokio::test]
    async fn test_search_with_results() {
        let memory = Arc::new(Mutex::new(EpisodicMemory::new()));
        {
            let mut mem = memory.lock().unwrap();
            mem.record(EpisodicEntry {
                turn_id: "t1".into(),
                timestamp: std::time::SystemTime::now(),
                input: "deploy the database".into(),
                output: "Deployment complete".into(),
                tool_calls: vec![],
                keywords: vec!["deploy".into(), "database".into()],
            });
        }

        let tool = RecallHistoryTool::new(memory);
        let result = run_tool(&tool, json!({"operation": "search", "query": "deploy"}))
            .await
            .unwrap();
        assert_eq!(result["count"], 1);
        assert_eq!(result["results"][0]["turn_id"], "t1");
    }

    #[tokio::test]
    async fn test_expand_not_found() {
        let memory = Arc::new(Mutex::new(EpisodicMemory::new()));
        let tool = RecallHistoryTool::new(memory);

        let result = run_tool(
            &tool,
            json!({"operation": "expand", "turn_id": "nonexistent"}),
        )
        .await
        .unwrap();
        assert!(result["error"].as_str().unwrap().contains("not found"));
    }

    #[tokio::test]
    async fn test_recall_tool_not_found() {
        let memory = Arc::new(Mutex::new(EpisodicMemory::new()));
        let tool = RecallHistoryTool::new(memory);

        let result = run_tool(
            &tool,
            json!({"operation": "recall_tool", "tool_call_id": "call_xyz"}),
        )
        .await
        .unwrap();
        assert!(result["error"].as_str().unwrap().contains("not found"));
    }

    #[tokio::test]
    async fn test_invalid_operation() {
        let memory = Arc::new(Mutex::new(EpisodicMemory::new()));
        let tool = RecallHistoryTool::new(memory);

        let err = run_tool(&tool, json!({"operation": "fly"}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unknown operation"));
    }
}
