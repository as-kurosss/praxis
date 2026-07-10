//! **ScrollContext** — ties WorkingMemory eviction to EpisodicMemory recording.
//!
//! When the scroll strategy evicts old messages from the working context, this
//! module captures them and records them as [`EpisodicEntry`] items so nothing
//! is lost.

use crate::agent::llm::ChatMessage;
use crate::memory::{EpisodicEntry, EpisodicMemory};

/// Record evicted messages into episodic memory.
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
    if before.len() <= after.len() {
        return false;
    }

    // Scroll strategies only remove messages from the front.
    // The first (before.len() - after.len()) messages were evicted.
    let evicted_count = before.len() - after.len();
    let evicted = &before[..evicted_count];

    // Build an episodic entry from evicted messages
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

    let entry = EpisodicEntry {
        turn_id: turn_id.to_string(),
        timestamp: std::time::SystemTime::now(),
        input: input.to_string(),
        output,
        tool_calls: vec![],
        keywords,
    };

    episodic.record(entry);
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
