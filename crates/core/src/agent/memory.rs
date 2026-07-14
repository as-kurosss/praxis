//! **Memory / Scroll Context** — conversation history management for agents.
//!
//! Provides [`ScrollStrategy`] and [`ScrollContext`] to manage the length
//! of conversation histories, preventing unbounded token growth.
//!
//! # Strategies
//!
//! * [`Truncate`](ScrollStrategy::Truncate) — keep system prompt + last N messages
//! * [`SlidingWindow`](ScrollStrategy::SlidingWindow) — keep only the last N messages
//! * [`Summarize`](ScrollStrategy::Summarize) — compress old messages via a summarizer
//! * [`NoOp`](ScrollStrategy::NoOp) — keep everything

use crate::agent::llm::{ChatMessage, Role};
use std::sync::Arc;

/// Summarizer function type.
type SummarizerFn = Arc<dyn Fn(&[ChatMessage]) -> String + Send + Sync>;

/// Strategy for managing the length of a conversation history.
///
/// Choose a strategy based on your token budget and agent requirements.
#[derive(Clone)]
pub enum ScrollStrategy {
    /// Keep the system message (if any) plus the most recent
    /// `max_messages - 1` non-system messages.
    Truncate {
        /// Maximum total messages to retain after trimming.
        max_messages: usize,
    },
    /// Keep only the last `window_size` messages regardless of role.
    /// System messages may be evicted if they fall outside the window.
    SlidingWindow {
        /// Number of most recent messages to retain.
        window_size: usize,
    },
    /// Keep old non-system messages compressed as a single summary
    /// injected as a system message, plus the most recent messages.
    ///
    /// The `summarizer` callback receives a slice of messages to
    /// compress and returns a summary string.
    Summarize {
        /// Target total messages (summary + system + recent).
        max_messages: usize,
        /// Callback that compresses a batch of messages into a summary.
        summarizer: SummarizerFn,
    },
    /// Keep all messages — no trimming applied.
    NoOp,
}

impl std::fmt::Debug for ScrollStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Truncate { max_messages } => f
                .debug_struct("Truncate")
                .field("max_messages", max_messages)
                .finish(),
            Self::SlidingWindow { window_size } => f
                .debug_struct("SlidingWindow")
                .field("window_size", window_size)
                .finish(),
            Self::Summarize {
                max_messages,
                summarizer: _,
            } => f
                .debug_struct("Summarize")
                .field("max_messages", max_messages)
                .field("summarizer", &"<fn>")
                .finish(),
            Self::NoOp => f.debug_struct("NoOp").finish(),
        }
    }
}

impl ScrollStrategy {
    /// Apply this strategy to the given message list, trimming from the
    /// front (oldest messages) as needed.
    ///
    /// The strategy is applied idempotently: calling it multiple times on
    /// the same data has the same effect as calling it once.
    pub fn apply(&self, messages: &mut Vec<ChatMessage>) {
        match self {
            Self::Truncate { max_messages } => {
                if messages.len() <= *max_messages {
                    return;
                }
                // Count system messages — we want to keep them.
                let sys_count = messages.iter().filter(|m| m.role == Role::System).count();

                // If system messages alone exceed the limit, trim from end.
                if sys_count >= *max_messages {
                    messages.truncate(*max_messages);
                    return;
                }

                // Target: keep system messages + (max_messages - sys_count) recent non-system
                let non_sys_to_keep = max_messages - sys_count;

                // Collect indices: first system messages, then non-system from the end
                let mut kept: Vec<ChatMessage> = Vec::with_capacity(*max_messages);

                // 1. Add all system messages (in order)
                let sys_indices: Vec<usize> = messages
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.role == Role::System)
                    .map(|(i, _)| i)
                    .collect();

                for &i in &sys_indices {
                    if kept.len() < *max_messages {
                        kept.push(messages[i].clone());
                    }
                }

                // 2. Add the most recent non-system messages (preserving order)
                let mut recent: Vec<ChatMessage> = Vec::with_capacity(non_sys_to_keep);
                for msg in messages.iter().rev() {
                    if msg.role == Role::System {
                        continue;
                    }
                    if recent.len() < non_sys_to_keep {
                        recent.push(msg.clone());
                    }
                }
                recent.reverse();
                kept.extend(recent);

                *messages = kept;
            }
            Self::SlidingWindow { window_size } => {
                if messages.len() <= *window_size {
                    return;
                }
                // Keep the last window_size messages
                let start = messages.len() - window_size;
                let kept: Vec<ChatMessage> = messages.drain(start..).collect();
                *messages = kept;
            }
            Self::Summarize {
                max_messages,
                summarizer,
            } => {
                if messages.len() <= *max_messages {
                    return;
                }
                // Keep all system messages
                let sys_indices: Vec<usize> = messages
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.role == Role::System)
                    .map(|(i, _)| i)
                    .collect();
                let sys_count = sys_indices.len();

                // If system messages alone exceed limit, trim from end.
                if sys_count >= *max_messages {
                    messages.truncate(*max_messages);
                    return;
                }

                // Reserve 1 slot for the summary message
                let summary_slot = 1;
                let recent_slots = max_messages.saturating_sub(sys_count + summary_slot);

                // Collect non-system messages
                let non_sys: Vec<(usize, &ChatMessage)> = messages
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.role != Role::System)
                    .collect();

                let total_non_sys = non_sys.len();

                // If there's nothing to summarize, fall back to truncation
                let old_count = total_non_sys.saturating_sub(recent_slots);
                if old_count == 0 {
                    // Just keep as many as we can
                    let mut kept: Vec<ChatMessage> = Vec::with_capacity(*max_messages);
                    for &i in &sys_indices {
                        kept.push(messages[i].clone());
                    }
                    let to_keep = recent_slots.min(total_non_sys);
                    for (_, msg) in non_sys.iter().rev().take(to_keep).rev() {
                        kept.push((*msg).clone());
                    }
                    *messages = kept;
                    return;
                }

                // Build the summary input (old messages)
                let old_msgs: Vec<ChatMessage> = non_sys
                    .iter()
                    .take(old_count)
                    .map(|(_, m)| (*m).clone())
                    .collect();

                // Call summarizer
                let summary_text = summarizer(&old_msgs);

                // Build result: system messages + summary + recent non-system
                let mut kept: Vec<ChatMessage> = Vec::with_capacity(*max_messages);
                // 1. Original system messages
                for &i in &sys_indices {
                    kept.push(messages[i].clone());
                }
                // 2. Summary as a system message
                kept.push(ChatMessage::system(format!(
                    "Previous conversation summary: {summary_text}"
                )));
                // 3. Recent non-system messages
                let recent_msgs: Vec<ChatMessage> = non_sys
                    .iter()
                    .skip(old_count)
                    .map(|(_, m)| (*m).clone())
                    .collect();
                kept.extend(recent_msgs);

                *messages = kept;
            }
            Self::NoOp => {
                // Nothing to do
            }
        }
    }
}

/// A wrapper around [`Vec<ChatMessage>`] with an attached [`ScrollStrategy`].
///
/// Automatically applies the strategy after each [`push`](ScrollContext::push).
#[derive(Debug, Clone)]
pub struct ScrollContext {
    /// The conversation messages.
    messages: Vec<ChatMessage>,
    /// The strategy to apply on every mutation.
    strategy: ScrollStrategy,
}

impl ScrollContext {
    /// Create a new scroll context with the given strategy.
    pub fn new(strategy: ScrollStrategy) -> Self {
        Self {
            messages: Vec::new(),
            strategy,
        }
    }

    /// Create a scroll context from existing messages.
    pub fn from_messages(messages: Vec<ChatMessage>, strategy: ScrollStrategy) -> Self {
        Self { messages, strategy }
    }

    /// The current conversation messages (after applying the strategy).
    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    /// Consume the scroll context and return the inner messages.
    pub fn into_messages(self) -> Vec<ChatMessage> {
        self.messages
    }

    /// Push a new message and apply the scroll strategy.
    pub fn push(&mut self, msg: ChatMessage) {
        self.messages.push(msg);
        self.strategy.apply(&mut self.messages);
    }

    /// Apply the scroll strategy to the current messages.
    ///
    /// Useful after bulk mutations on [`messages_mut`](ScrollContext::messages_mut).
    pub fn apply_strategy(&mut self) {
        self.strategy.apply(&mut self.messages);
    }

    /// Mutable access to the messages (call [`apply_strategy`](ScrollContext::apply_strategy)
    /// afterwards to enforce the strategy).
    pub fn messages_mut(&mut self) -> &mut Vec<ChatMessage> {
        &mut self.messages
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::llm::ChatMessage;

    fn sys(msg: &str) -> ChatMessage {
        ChatMessage::system(msg)
    }

    fn user(msg: &str) -> ChatMessage {
        ChatMessage::user(msg)
    }

    fn assistant(msg: &str) -> ChatMessage {
        ChatMessage::assistant(msg)
    }

    // ── Truncate ─────────────────────────────────────────────────

    #[test]
    fn test_truncate_under_limit() {
        let strategy = ScrollStrategy::Truncate { max_messages: 5 };
        let mut msgs = vec![sys("s1"), user("u1"), assistant("a1")];
        strategy.apply(&mut msgs);
        assert_eq!(msgs.len(), 3);
    }

    #[test]
    fn test_truncate_at_limit() {
        let strategy = ScrollStrategy::Truncate { max_messages: 5 };
        let mut msgs = vec![
            sys("s1"),
            user("u1"),
            assistant("a1"),
            user("u2"),
            assistant("a2"),
        ];
        strategy.apply(&mut msgs);
        assert_eq!(msgs.len(), 5);
    }

    #[test]
    fn test_truncate_over_limit() {
        let strategy = ScrollStrategy::Truncate { max_messages: 4 };
        let mut msgs = vec![
            sys("s1"),
            user("u1"),
            assistant("a1"),
            user("u2"),
            assistant("a2"),
            user("u3"),
        ];
        strategy.apply(&mut msgs);
        // Keep system + last 3 non-system = 4 total
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0].content.as_deref(), Some("s1"));
        // Last 3 non-system messages in order: u2, a2, u3
        assert_eq!(msgs[1].content.as_deref(), Some("u2"));
        assert_eq!(msgs[2].content.as_deref(), Some("a2"));
        assert_eq!(msgs[3].content.as_deref(), Some("u3"));
    }

    #[test]
    fn test_truncate_keeps_system() {
        let strategy = ScrollStrategy::Truncate { max_messages: 3 };
        let mut msgs = vec![
            sys("sys1"),
            user("u1"),
            assistant("a1"),
            user("u2"),
            assistant("a2"),
        ];
        strategy.apply(&mut msgs);
        // Keep sys + last 2 non-system = 3 total
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].content.as_deref(), Some("sys1"));
        // non-system messages: u1, a1, u2, a2 — keep last 2 in order: u2, a2
        assert_eq!(msgs[1].content.as_deref(), Some("u2"));
        assert_eq!(msgs[2].content.as_deref(), Some("a2"));
    }

    #[test]
    fn test_truncate_multiple_system_messages() {
        let strategy = ScrollStrategy::Truncate { max_messages: 4 };
        let mut msgs = vec![
            sys("global"),
            user("u1"),
            assistant("a1"),
            user("u2"),
            assistant("a2"),
            user("u3"),
        ];
        strategy.apply(&mut msgs);
        // Keep system + last 3 non-system = 4 total
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0].content.as_deref(), Some("global"));
    }

    #[test]
    fn test_truncate_only_system_messages() {
        let strategy = ScrollStrategy::Truncate { max_messages: 2 };
        let mut msgs = vec![sys("s1"), sys("s2"), sys("s3")];
        strategy.apply(&mut msgs);
        assert_eq!(msgs.len(), 2);
    }

    // ── SlidingWindow ─────────────────────────────────────────────

    #[test]
    fn test_sliding_window_under_limit() {
        let strategy = ScrollStrategy::SlidingWindow { window_size: 10 };
        let mut msgs = vec![user("u1"), assistant("a1")];
        strategy.apply(&mut msgs);
        assert_eq!(msgs.len(), 2);
    }

    #[test]
    fn test_sliding_window_trims() {
        let strategy = ScrollStrategy::SlidingWindow { window_size: 3 };
        let mut msgs = vec![
            user("u1"),
            assistant("a1"),
            user("u2"),
            assistant("a2"),
            user("u3"),
        ];
        strategy.apply(&mut msgs);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].content.as_deref(), Some("u2"));
        assert_eq!(msgs[1].content.as_deref(), Some("a2"));
        assert_eq!(msgs[2].content.as_deref(), Some("u3"));
    }

    // ── NoOp ──────────────────────────────────────────────────────

    #[test]
    fn test_noop_keeps_all() {
        let strategy = ScrollStrategy::NoOp;
        let mut msgs = vec![user("u1"), assistant("a1"), user("u2"), assistant("a2")];
        strategy.apply(&mut msgs);
        assert_eq!(msgs.len(), 4);
    }

    // ── Summarize ──────────────────────────────────────────────────

    #[test]
    fn test_summarize_under_limit() {
        let summarizer =
            Arc::new(|msgs: &[ChatMessage]| format!("summarized {} messages", msgs.len()));
        let strategy = ScrollStrategy::Summarize {
            max_messages: 10,
            summarizer,
        };
        let mut msgs = vec![sys("s1"), user("u1"), assistant("a1")];
        strategy.apply(&mut msgs);
        assert_eq!(msgs.len(), 3);
    }

    #[test]
    fn test_summarize_over_limit() {
        let summarizer =
            Arc::new(|msgs: &[ChatMessage]| format!("summarized {} messages", msgs.len()));
        let strategy = ScrollStrategy::Summarize {
            max_messages: 4,
            summarizer,
        };
        let mut msgs = vec![
            sys("s1"),
            user("u1"),
            assistant("a1"),
            user("u2"),
            assistant("a2"),
            user("u3"),
        ];
        strategy.apply(&mut msgs);
        // Should keep: sys + summary + 2 recent = 4
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0].content.as_deref(), Some("s1"));
        assert!(
            msgs[1]
                .content
                .as_deref()
                .unwrap()
                .contains("summarized 3 messages")
        );
        assert_eq!(msgs[2].content.as_deref(), Some("a2"));
        assert_eq!(msgs[3].content.as_deref(), Some("u3"));
    }

    #[test]
    fn test_summarize_fallback_to_truncate_when_no_old_messages() {
        let summarizer =
            Arc::new(|msgs: &[ChatMessage]| format!("summarized {} messages", msgs.len()));
        let strategy = ScrollStrategy::Summarize {
            max_messages: 3,
            summarizer,
        };
        let mut msgs = vec![sys("s1"), user("u1"), assistant("a1")];
        strategy.apply(&mut msgs);
        // At limit already, unchanged
        assert_eq!(msgs.len(), 3);
    }

    #[test]
    fn test_summarize_only_system_messages() {
        let summarizer =
            Arc::new(|msgs: &[ChatMessage]| format!("summarized {} messages", msgs.len()));
        let strategy = ScrollStrategy::Summarize {
            max_messages: 2,
            summarizer,
        };
        let mut msgs = vec![sys("s1"), sys("s2"), sys("s3")];
        strategy.apply(&mut msgs);
        assert_eq!(msgs.len(), 2);
    }

    #[test]
    fn test_summarize_calls_summarizer_with_correct_messages() {
        let captured = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured_clone = Arc::clone(&captured);
        let summarizer = Arc::new(move |msgs: &[ChatMessage]| {
            let mut c = captured_clone.lock().unwrap();
            for m in msgs {
                c.push(m.content.clone().unwrap_or_default());
            }
            "summary_result".to_string()
        });
        let strategy = ScrollStrategy::Summarize {
            max_messages: 3,
            summarizer,
        };
        let mut msgs = vec![
            sys("s1"),
            user("u1"),
            assistant("a1"),
            user("u2"),
            assistant("a2"),
            user("u3"),
        ];
        strategy.apply(&mut msgs);
        // sys + summary + 1 recent = 3
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].content.as_deref(), Some("s1"));
        assert!(
            msgs[1]
                .content
                .as_deref()
                .unwrap()
                .contains("summary_result")
        );

        // Verify summarizer was called with the correct old messages
        let captured = captured.lock().unwrap();
        // non-system: u1, a1, u2, a2, u3; slots: sys(1) + summary(1) + recent(1) = 3
        // recent_slots = 3 - 1 - 1 = 1, old = 5 - 1 = 4 → old = [u1, a1, u2, a2]
        assert_eq!(captured.len(), 4);
        assert_eq!(captured[0], "u1");
        assert_eq!(captured[3], "a2");
    }

    // ── ScrollContext ─────────────────────────────────────────────

    #[test]
    fn test_scroll_context_push_triggers_strategy() {
        let mut ctx = ScrollContext::new(ScrollStrategy::Truncate { max_messages: 3 });
        ctx.push(sys("s1"));
        ctx.push(user("u1"));
        ctx.push(assistant("a1"));
        assert_eq!(ctx.messages().len(), 3);

        // Fourth push triggers truncation
        ctx.push(user("u2"));
        // Should keep sys + last 2 non-system = 3 total
        assert_eq!(ctx.messages().len(), 3);
        assert_eq!(ctx.messages()[0].content.as_deref(), Some("s1"));
    }

    #[test]
    fn test_scroll_context_from_messages() {
        let msgs = vec![user("u1")];
        let ctx = ScrollContext::from_messages(msgs, ScrollStrategy::NoOp);
        assert_eq!(ctx.messages().len(), 1);
        assert_eq!(ctx.messages()[0].content.as_deref(), Some("u1"));
    }

    #[test]
    fn test_scroll_context_into_messages() {
        let mut ctx = ScrollContext::new(ScrollStrategy::NoOp);
        ctx.push(user("u1"));
        ctx.push(assistant("a1"));
        let msgs = ctx.into_messages();
        assert_eq!(msgs.len(), 2);
    }
}
