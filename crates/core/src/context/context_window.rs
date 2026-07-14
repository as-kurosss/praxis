//! **Context Window Resolver** — per-model context-window awareness for scroll strategies.
//!
//! Provides a static catalog of known model context windows, a resolver that
//! picks the right window (explicit config > catalog match > 128k default),
//! and a conservative token-count estimator.
//!
//! # Usage
//!
//! ```ignore
//! use crate::context::context_window::{resolve_context_window, estimate_tokens};
//!
//! let window = resolve_context_window("gpt-4o", None);
//! let tokens = estimate_tokens(&messages);
//! if tokens > window * 9 / 10 {
//!     // Trigger compaction…
//! }
//! ```

use crate::agent::llm::ChatMessage;

/// Default context window when no match is found (128k tokens).
pub const DEFAULT_CONTEXT_WINDOW: usize = 128_000;

/// Default trigger ratio (90%) — evict when estimated usage exceeds this.
pub const DEFAULT_TRIGGER_RATIO: f64 = 0.9;

// ── Catalog ──────────────────────────────────────────────────────────────────

/// Look up the context-window size for a known model prefix.
///
/// Returns `None` when the model is unknown (caller falls back to default).
fn lookup_context_window(model: &str) -> Option<usize> {
    // Longest prefixes first so more-specific models are matched before
    // their generic family prefix.
    const CATALOG: &[(&str, usize)] = &[
        // ── Claude 4 ──────────────────────────────────────────────────
        ("claude-4-8-opus", 200_000),
        ("claude-4-7-opus", 200_000),
        ("claude-4-6-opus", 200_000),
        ("claude-4-6-sonnet", 200_000),
        ("claude-4-5-sonnet", 200_000),
        ("claude-4-5-haiku", 200_000),
        ("claude-4-opus", 200_000),
        // ── Claude 3.5 ────────────────────────────────────────────────
        ("claude-3-5-sonnet", 200_000),
        ("claude-3-5-haiku", 200_000),
        // ── Claude 3 ──────────────────────────────────────────────────
        ("claude-3-opus", 200_000),
        ("claude-3-sonnet", 200_000),
        ("claude-3-haiku", 200_000),
        // ── GPT-4.1 ───────────────────────────────────────────────────
        ("gpt-4.1", 1_000_000),
        ("gpt-4.1-mini", 1_000_000),
        ("gpt-4.1-nano", 1_000_000),
        // ── GPT-4o ────────────────────────────────────────────────────
        ("gpt-4o", 128_000),
        ("gpt-4o-mini", 128_000),
        // ── GPT-4 ─────────────────────────────────────────────────────
        ("gpt-4-turbo", 128_000),
        ("gpt-4", 8_192),
        // ── GPT-3.5 ───────────────────────────────────────────────────
        ("gpt-3.5-turbo", 16_385),
        // ── o-series ──────────────────────────────────────────────────
        ("o3", 200_000),
        ("o1", 200_000),
        // ── Gemini ────────────────────────────────────────────────────
        ("gemini-2.5-pro", 1_000_000),
        ("gemini-2.5-flash", 1_000_000),
        ("gemini-2.0-flash", 1_000_000),
        ("gemini-2.0-pro", 2_000_000),
        ("gemini-1.5-pro", 2_000_000),
        ("gemini-1.5-flash", 1_000_000),
        // ── DeepSeek ──────────────────────────────────────────────────
        ("deepseek-r1", 128_000),
        ("deepseek-v3", 128_000),
        ("deepseek-v2", 128_000),
        // ── Qwen ──────────────────────────────────────────────────────
        ("qwen-3", 128_000),
        ("qwen-2.5", 128_000),
        ("qwen-2", 128_000),
        // ── Grok ──────────────────────────────────────────────────────
        ("grok-3", 128_000),
        ("grok-2", 128_000),
        // ── Llama ─────────────────────────────────────────────────────
        ("llama-3.3", 128_000),
        ("llama-3.2", 128_000),
        ("llama-3.1", 128_000),
        ("llama-3", 8_192),
        // ── Mistral ───────────────────────────────────────────────────
        ("mistral-large", 128_000),
        ("mistral-small", 32_000),
        // ── Mixtral ───────────────────────────────────────────────────
        ("mixtral-8x22b", 65_536),
        ("mixtral-8x7b", 32_768),
        // ── Command ───────────────────────────────────────────────────
        ("command-r-plus", 128_000),
        ("command-r", 128_000),
        // ── Cohere ────────────────────────────────────────────────────
        ("cohere-command", 128_000),
    ];

    let lower = model.to_lowercase();
    for (prefix, size) in CATALOG {
        if lower.starts_with(prefix) {
            return Some(*size);
        }
    }
    None
}

/// Resolve the effective context-window size for a model.
///
/// Precedence:
/// 1. `explicit` — caller-provided override (from `AgentConfig` or similar)
/// 2. Catalog match — looked up by model prefix
/// 3. [`DEFAULT_CONTEXT_WINDOW`] — 128k fallback
pub fn resolve_context_window(model: &str, explicit: Option<usize>) -> usize {
    explicit
        .or_else(|| lookup_context_window(model))
        .unwrap_or(DEFAULT_CONTEXT_WINDOW)
}

// ── Token estimator ──────────────────────────────────────────────────────────

/// Estimate the token count of a single [`ChatMessage`].
///
/// Uses a conservative heuristic: 4 bytes ≈ 1 token. 1 token overhead per
/// message for role/metadata. This is intentionally a rough upper-bound;
/// precise tokenization would require model-specific tokenizers.
pub fn estimate_token_count(msg: &ChatMessage) -> usize {
    let mut total = 1usize; // overhead for role + metadata

    if let Some(ref content) = msg.content {
        total += content.len() / 4;
    }
    if let Some(ref reasoning) = msg.reasoning_content {
        total += reasoning.len() / 4;
    }
    if let Some(ref tool_calls) = msg.tool_calls {
        for tc in tool_calls {
            total += tc.name.len() / 4;
            total += tc.arguments.to_string().len() / 4;
        }
    }

    total
}

/// Estimate the total token count of a slice of messages.
pub fn estimate_tokens(messages: &[ChatMessage]) -> usize {
    messages.iter().map(estimate_token_count).sum()
}

/// Apply a scroll strategy with token-awareness.
///
/// 1. Resolves the context window from `model` and `explicit_window`.
/// 2. Estimates total tokens in `messages`.
/// 3. If the estimate exceeds `window * trigger_ratio`, the scroll strategy
///    is applied to bring the conversation under budget.
///
/// This is a thin wrapper around [`ScrollStrategy::apply`] intended for use
/// in the agent's execution loop (see [`crate::agent::runtime`]).
pub fn apply_strategy_with_context_window(
    strategy: &crate::agent::memory::ScrollStrategy,
    messages: &mut Vec<ChatMessage>,
    model: &str,
    explicit_window: Option<usize>,
) {
    let window = resolve_context_window(model, explicit_window);
    let threshold = (window as f64 * DEFAULT_TRIGGER_RATIO) as usize;

    // 1. Apply the strategy (message-count-based eviction).
    strategy.apply(messages);

    // 2. If still over the token threshold, drop oldest non-system messages
    //    until the estimate is within budget.
    let mut estimated = estimate_tokens(messages);
    if estimated <= threshold {
        return;
    }

    // Remove oldest non-system messages until under threshold (O(n)).
    messages.retain(|msg| {
        if msg.role == crate::agent::llm::Role::System {
            return true;
        }
        if estimated <= threshold {
            return true;
        }
        estimated = estimated.saturating_sub(estimate_token_count(msg));
        false
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::llm::{ChatMessage, Role, ToolCall};

    // ── Catalog ──────────────────────────────────────────────────────

    #[test]
    fn test_resolve_explicit_overrides() {
        assert_eq!(resolve_context_window("unknown-model", Some(999)), 999);
    }

    #[test]
    fn test_resolve_known_model() {
        assert_eq!(resolve_context_window("gpt-4o", None), 128_000);
        assert_eq!(resolve_context_window("claude-3-5-sonnet", None), 200_000);
        assert_eq!(resolve_context_window("gemini-1.5-pro", None), 2_000_000);
    }

    #[test]
    fn test_resolve_unknown_falls_back_to_default() {
        assert_eq!(
            resolve_context_window("foobar-42", None),
            DEFAULT_CONTEXT_WINDOW
        );
    }

    #[test]
    fn test_resolve_longest_prefix_wins() {
        // Should match "gpt-4.1-mini" (1M), not "gpt-4" (8k)
        assert_eq!(resolve_context_window("gpt-4.1-mini-123", None), 1_000_000);
    }

    #[test]
    fn test_resolve_case_insensitive() {
        assert_eq!(resolve_context_window("GPT-4O", None), 128_000);
    }

    // ── Token estimator ──────────────────────────────────────────────

    #[test]
    fn test_estimate_empty_message() {
        let msg = ChatMessage {
            role: Role::User,
            content: None,
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
            qwenpaw_tag: None,
        };
        // 1 token overhead, no content
        assert_eq!(estimate_token_count(&msg), 1);
    }

    #[test]
    fn test_estimate_text_message() {
        let msg = ChatMessage::user("Hello, world! This is a test message.");
        let count = estimate_token_count(&msg);
        // content bytes / 4 + 1 overhead
        assert!(count > 1, "expected non-trivial token count, got {count}");
    }

    #[test]
    fn test_estimate_with_tool_calls() {
        let msg = ChatMessage {
            role: Role::Assistant,
            content: None,
            reasoning_content: None,
            tool_calls: Some(vec![ToolCall {
                id: "call_1".into(),
                name: "get_weather".into(),
                arguments: serde_json::json!({"city": "London"}),
            }]),
            tool_call_id: None,
            qwenpaw_tag: None,
        };
        // name: 10 bytes / 4 = 2
        // arguments JSON: ~21 bytes / 4 = 5
        // overhead: 1
        // total: ~8
        assert!(estimate_token_count(&msg) > 1);
    }

    #[test]
    fn test_estimate_multiple_messages() {
        let msgs = vec![
            ChatMessage::user("short"),
            ChatMessage::assistant("A bit longer response here for testing"),
        ];
        let total = estimate_tokens(&msgs);
        assert!(total > 2); // At least 1 per message + some content
    }
}
