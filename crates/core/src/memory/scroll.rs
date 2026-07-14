//! **ScrollContext** — ties WorkingMemory eviction to EpisodicMemory recording.
//!
//! When the scroll strategy evicts old messages from the working context, this
//! module captures them and records them as [`EpisodicEntry`] items so nothing
//! is lost.

use crate::agent::llm::ChatMessage;
use crate::agent::memory::ScrollStrategy;
use crate::memory::{EpisodicEntry, EpisodicMemory};
use std::sync::{Arc, Mutex};

/// Find the index of the boundary before the active-turn tail.
///
/// The active turn is defined as the most recent user message that is **not**
/// a synthetic loop-continuation message. Everything from that message onward
/// is considered the "protected tail" and should not be evicted.
///
/// Returns `0` when no real user message is found (nothing to protect).
pub fn find_active_tail_boundary(messages: &[ChatMessage]) -> usize {
    messages
        .iter()
        .rposition(|m| m.role == crate::agent::llm::Role::User && !m.is_synthetic())
        .unwrap_or(0)
}

/// Apply a scroll strategy with active-turn protection.
///
/// The most recent non-synthetic user message and everything after it
/// is pinned and excluded from eviction.  The strategy is applied only
/// to the messages before that boundary.
pub fn apply_with_active_turn_protection(
    strategy: &ScrollStrategy,
    messages: &mut Vec<ChatMessage>,
) {
    let boundary = find_active_tail_boundary(messages);
    if boundary == 0 {
        strategy.apply(messages);
        return;
    }
    let protected = messages.split_off(boundary);
    strategy.apply(messages);
    messages.extend(protected);
}

/// Build an [`EpisodicEntry`] from evicted messages without recording it.
///
/// Returns `None` when no messages were evicted (`before.len() <= after.len()`).
pub fn build_evicted_entry(
    turn_id: &str,
    input: &str,
    before: &[ChatMessage],
    after: &[ChatMessage],
) -> Option<EpisodicEntry> {
    if before.len() <= after.len() {
        return None;
    }

    // Scroll strategies only remove messages from the front.
    // The first (before.len() - after.len()) messages were evicted.
    let evicted_count = before.len() - after.len();
    let evicted = &before[..evicted_count];

    let mut output = String::new();
    for msg in evicted {
        if let Some(content) = &msg.content {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(content);
        }
    }

    let keywords = EpisodicMemory::extract_keywords(input);

    Some(EpisodicEntry {
        turn_id: turn_id.to_string(),
        timestamp: std::time::SystemTime::now(),
        input: input.to_string(),
        output,
        tool_calls: vec![],
        keywords,
    })
}

/// Record evicted messages into episodic memory (synchronous).
///
/// Given the conversation state before and after a scroll strategy was applied,
/// extract the messages that were removed (the first N messages that differ)
/// and record them as an episodic entry.
///
/// Returns `true` if an entry was recorded.
pub fn record_evicted_turn(
    episodic: &mut EpisodicMemory,
    turn_id: &str,
    input: &str,
    before: &[ChatMessage],
    after: &[ChatMessage],
) -> bool {
    if let Some(entry) = build_evicted_entry(turn_id, input, before, after) {
        episodic.record(entry);
        true
    } else {
        false
    }
}

/// Async variant of [`record_evicted_turn`] that uses
/// `tokio::task::spawn_blocking` for SQLite-backed memory.
///
/// For the in-memory backend the write is done synchronously (no overhead).
pub async fn record_evicted_turn_async(
    episodic: &Arc<Mutex<EpisodicMemory>>,
    turn_id: &str,
    input: &str,
    before: &[ChatMessage],
    after: &[ChatMessage],
) -> bool {
    let entry = match build_evicted_entry(turn_id, input, before, after) {
        Some(e) => e,
        None => return false,
    };

    // Quick peek: is the backend persistent?
    let is_persistent = match episodic.lock() {
        Ok(mem) => mem.is_persistent(),
        Err(e) => {
            tracing::warn!("episodic mutex poisoned during is_persistent check: {e}");
            return false;
        }
    };

    if is_persistent {
        // Offload the SQLite write to a blocking thread
        let arc = episodic.clone();
        if let Err(e) = tokio::task::spawn_blocking(move || {
            if let Ok(mut mem) = arc.lock() {
                mem.record(entry);
            } else {
                tracing::warn!("episodic mutex poisoned during record (persistent)");
            }
        })
        .await
        {
            tracing::warn!("spawn_blocking for episodic record failed: {e}");
        }
    } else {
        // In-memory: fast path, record directly
        match episodic.lock() {
            Ok(mut mem) => mem.record(entry),
            Err(e) => {
                tracing::warn!("episodic mutex poisoned during record (in-memory): {e}");
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::llm::ChatMessage;

    #[test]
    fn test_record_evicted_turn_no_eviction() {
        let mut episodic = EpisodicMemory::new();
        let before = vec![ChatMessage::user("hello")];
        let after = before.clone();
        let recorded = record_evicted_turn(&mut episodic, "t1", "hello", &before, &after);
        assert!(!recorded);
        assert_eq!(episodic.len(), 0);
    }

    #[test]
    fn test_record_evicted_turn_with_eviction() {
        let mut episodic = EpisodicMemory::new();
        let before = vec![
            ChatMessage::user("old msg"),
            ChatMessage::assistant("old response"),
            ChatMessage::user("new msg"),
        ];
        let after = vec![ChatMessage::user("new msg")];

        let recorded = record_evicted_turn(&mut episodic, "t1", "old msg", &before, &after);
        assert!(recorded);
        assert_eq!(episodic.len(), 1);

        let entry = episodic.recall("t1").unwrap();
        assert_eq!(entry.input, "old msg");
        assert!(entry.output.contains("old response"));
    }
}
