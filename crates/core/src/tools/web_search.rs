//! **WebSearchTool** — search the web for current information.
//!
//! Uses a configurable [`WebSearchProvider`] to fetch results from the web.
//! Ships with a [`MockWebSearchProvider`] for testing.

use crate::agent::tool::{Tool, ToolError, ToolSpec};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// A single search result from a web search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Error type for web search operations.
#[derive(Debug, thiserror::Error)]
pub enum WebSearchError {
    /// The search request failed (network, timeout, etc.).
    #[error("Search request failed: {0}")]
    RequestFailed(String),

    /// The response could not be parsed.
    #[error("Invalid response format: {0}")]
    InvalidResponse(String),

    /// An internal provider error occurred.
    #[error("Provider error: {0}")]
    Provider(String),
}

/// Kinds of supported web search providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WebSearchProviderKind {
    /// A mock provider that returns predefined results (for testing).
    Mock,
}

/// Configuration for the web search tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchConfig {
    /// Optional API key for the search provider.
    pub api_key: Option<String>,
    /// Which search provider to use.
    pub provider: WebSearchProviderKind,
    /// Default maximum number of results to return.
    pub max_results: usize,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            provider: WebSearchProviderKind::Mock,
            max_results: 5,
        }
    }
}

/// Trait for web search providers.
///
/// Implement this trait to wrap any search API (e.g. Exa, Google, Bing).
#[async_trait::async_trait]
pub trait WebSearchProvider: Send + Sync {
    /// Execute a search and return up to `count` results.
    async fn search(
        &self,
        query: &str,
        count: usize,
    ) -> Result<Vec<SearchResult>, WebSearchError>;
}

/// A mock provider that returns predefined results.
///
/// Useful in tests and offline environments.
pub struct MockWebSearchProvider {
    results: Vec<SearchResult>,
}

impl MockWebSearchProvider {
    /// Create a new mock provider with the given preset results.
    #[must_use]
    pub fn new(results: Vec<SearchResult>) -> Self {
        Self { results }
    }
}

#[async_trait::async_trait]
impl WebSearchProvider for MockWebSearchProvider {
    async fn search(
        &self,
        _query: &str,
        count: usize,
    ) -> Result<Vec<SearchResult>, WebSearchError> {
        let count = count.min(self.results.len());
        Ok(self.results[..count].to_vec())
    }
}

/// A tool that searches the web via a configurable provider.
///
/// # Arguments
/// * `query` — the search query (required)
/// * `count` — number of results to return (optional, default from config)
///
/// # Returns
/// A JSON object with `query` and an array of `results`, each having
/// `title`, `url`, and `snippet`.
pub struct WebSearchTool {
    config: WebSearchConfig,
    provider: Box<dyn WebSearchProvider>,
}

impl WebSearchTool {
    /// Create a new `WebSearchTool` with the given configuration and provider.
    #[must_use]
    pub fn new(config: WebSearchConfig, provider: Box<dyn WebSearchProvider>) -> Self {
        Self { config, provider }
    }
}

#[async_trait::async_trait]
impl Tool for WebSearchTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "web_search".into(),
            description:
                "Search the web for current information. Returns a list of results with titles, URLs, and snippets."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query"
                    },
                    "count": {
                        "type": "integer",
                        "description": "Number of results to return",
                        "default": 5
                    }
                },
                "required": ["query"]
            }),
            category: crate::agent::tool::ToolCategory::Network,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let query = args
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArgs {
                tool: "web_search".into(),
                message: "missing 'query' string".into(),
            })?;

        let count = args
            .get("count")
            .and_then(Value::as_u64)
            .map_or(self.config.max_results, |c| c as usize);

        let results = self
            .provider
            .search(query, count)
            .await
            .map_err(|e| ToolError::Execution {
                tool: "web_search".into(),
                message: e.to_string(),
            })?;

        let items: Vec<Value> = results
            .into_iter()
            .map(|r| {
                json!({
                    "title": r.title,
                    "url": r.url,
                    "snippet": r.snippet,
                })
            })
            .collect();

        Ok(json!({
            "query": query,
            "results": items,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_results() -> Vec<SearchResult> {
        vec![
            SearchResult {
                title: "Rust Programming Language".into(),
                url: "https://www.rust-lang.org".into(),
                snippet: "A language empowering everyone to build reliable and efficient software."
                    .into(),
            },
            SearchResult {
                title: "Awesome Rust".into(),
                url: "https://github.com/rust-unofficial/awesome-rust".into(),
                snippet: "A curated list of Rust code and resources.".into(),
            },
            SearchResult {
                title: "Rust by Example".into(),
                url: "https://doc.rust-lang.org/stable/rust-by-example/".into(),
                snippet: "Learn Rust with practical examples.".into(),
            },
        ]
    }

    #[tokio::test]
    async fn test_web_search_returns_results() {
        let provider = MockWebSearchProvider::new(mock_results());
        let tool = WebSearchTool::new(WebSearchConfig::default(), Box::new(provider));

        let args = json!({"query": "rust programming"});
        let result = tool.call(args).await.unwrap();

        let results = result["results"].as_array().unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0]["title"], "Rust Programming Language");
    }

    #[tokio::test]
    async fn test_web_search_missing_query_returns_error() {
        let provider = MockWebSearchProvider::new(mock_results());
        let tool = WebSearchTool::new(WebSearchConfig::default(), Box::new(provider));

        let args = json!({});
        let result = tool.call(args).await;
        let err = result.unwrap_err();
        assert!(
            matches!(&err, ToolError::InvalidArgs { tool, .. } if tool == "web_search"),
            "expected InvalidArgs, got {err:?}"
        );
    }

    #[tokio::test]
    async fn test_web_search_with_custom_count() {
        let provider = MockWebSearchProvider::new(mock_results());
        let tool = WebSearchTool::new(WebSearchConfig::default(), Box::new(provider));

        let args = json!({"query": "rust", "count": 2});
        let result = tool.call(args).await.unwrap();

        let results = result["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_web_search_empty_results() {
        let provider = MockWebSearchProvider::new(vec![]);
        let tool = WebSearchTool::new(WebSearchConfig::default(), Box::new(provider));

        let args = json!({"query": "nothing"});
        let result = tool.call(args).await.unwrap();

        let results = result["results"].as_array().unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_web_search_provider_error() {
        struct FailingProvider;

        #[async_trait::async_trait]
        impl WebSearchProvider for FailingProvider {
            async fn search(
                &self,
                _query: &str,
                _count: usize,
            ) -> Result<Vec<SearchResult>, WebSearchError> {
                Err(WebSearchError::RequestFailed("network error".into()))
            }
        }

        let tool = WebSearchTool::new(WebSearchConfig::default(), Box::new(FailingProvider));

        let args = json!({"query": "test"});
        let result = tool.call(args).await;
        let err = result.unwrap_err();
        assert!(
            matches!(&err, ToolError::Execution { tool, .. } if tool == "web_search"),
            "expected Execution error, got {err:?}"
        );
    }
}
