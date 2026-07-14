//! **DistilledMemory** — periodic summarisation of conversation segments.
//!
//! When the number of turns in a segment exceeds a threshold, the distill
//! callback compresses them into a concise summary that can be injected as
//! background context on future agent invocations.

use std::ops::Range;

/// A summary of a conversation segment.
#[derive(Debug, Clone)]
pub struct MemorySummary {
    /// Turn index range this segment covers (inclusive start, exclusive end).
    pub segment: Range<usize>,
    /// The distilled summary text.
    pub summary: String,
    /// Topics extracted from the segment.
    pub topics: Vec<String>,
    /// When this summary was created.
    pub timestamp: std::time::SystemTime,
}

/// Periodic summarisation of conversation segments.
///
/// Call [`DistilledMemory::record_turn`] after each agent turn. When the
/// accumulated turn count reaches `segment_size` the summarizer callback is
/// invoked with the raw messages and a summary is stored.
///
/// # Type parameters
///
/// * `S` — summarizer callable: `Fn(&[ChatMessage]) -> (String, Vec<String>)`
///   returning (summary_text, topics).
///
/// # Example
///
/// ```ignore
/// use praxis_core::memory::DistilledMemory;
///
/// let mut memory = DistilledMemory::new(
///     10,  // segment size
///     |msgs| ("summary".into(), vec!["topic".into()]),
/// );
///
/// // ... record_turn() calls ...
/// let summaries = memory.summaries();
/// ```
pub struct DistilledMemory<S> {
    /// Number of turns per segment.
    segment_size: usize,
    /// Callback that compresses a batch of messages into a summary + topics.
    summarizer: S,
    /// Stored summaries, most recent first.
    summaries: Vec<MemorySummary>,
    /// Running turn counter (monotonically increasing).
    turn_count: usize,
}

impl<S> DistilledMemory<S>
where
    S: Fn(&[crate::agent::llm::ChatMessage]) -> (String, Vec<String>),
{
    /// Create a new distilled memory.
    ///
    /// * `segment_size` — number of turns after which a summary is generated.
    /// * `summarizer` — callback receiving old messages, returning (summary, topics).
    pub fn new(segment_size: usize, summarizer: S) -> Self {
        Self {
            segment_size,
            summarizer,
            summaries: Vec::new(),
            turn_count: 0,
        }
    }

    /// Record a turn. When the accumulated turns reach `segment_size`,
    /// the summarizer is called and a summary is stored.
    ///
    /// `messages` should be the slice of messages from this segment
    /// (excluding the current turn — it is not yet part of the segment
    /// boundary check).
    pub fn record_turn(&mut self, messages: &[crate::agent::llm::ChatMessage]) {
        self.turn_count += 1;

        if self.turn_count.is_multiple_of(self.segment_size) && !messages.is_empty() {
            // Determine the range of this segment
            let end_idx = self.turn_count;
            let start_idx = end_idx.saturating_sub(self.segment_size);

            let (summary_text, topics) = (self.summarizer)(messages);

            self.summaries.push(MemorySummary {
                segment: start_idx..end_idx,
                summary: summary_text,
                topics,
                timestamp: std::time::SystemTime::now(),
            });
        }
    }

    /// All stored summaries, most recent first.
    #[must_use]
    pub fn summaries(&self) -> &[MemorySummary] {
        &self.summaries
    }

    /// The most recent summary, if any.
    #[must_use]
    pub fn latest(&self) -> Option<&MemorySummary> {
        self.summaries.last()
    }

    /// Inject summaries as a system-prompt-style context string.
    ///
    /// Returns `None` if there are no summaries.
    #[must_use]
    pub fn format_context(&self, max_summaries: usize) -> Option<String> {
        if self.summaries.is_empty() {
            return None;
        }

        let mut context = String::from("## Conversation Background\n\n");
        let count = self.summaries.len().min(max_summaries);
        let start = self.summaries.len() - count;

        for summary in &self.summaries[start..] {
            if !summary.topics.is_empty() {
                context.push_str(&format!("### Topics: {}\n", summary.topics.join(", ")));
            }
            context.push_str(&summary.summary);
            context.push('\n');
            context.push('\n');
        }

        Some(context)
    }

    /// Reset all accumulated state.
    pub fn clear(&mut self) {
        self.summaries.clear();
        self.turn_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::llm::ChatMessage;

    #[test]
    fn test_no_summary_before_segment() {
        let mut mem = DistilledMemory::new(10, |_msgs: &[ChatMessage]| {
            ("sum".into(), vec!["topic".into()])
        });
        mem.record_turn(&[]);
        assert!(mem.summaries().is_empty());
    }

    #[test]
    fn test_summary_at_segment_boundary() {
        let mut mem = DistilledMemory::new(3, |msgs: &[ChatMessage]| {
            let count = msgs.len();
            (format!("summarized {count} messages"), vec!["test".into()])
        });

        // Record 3 turns
        mem.record_turn(&[ChatMessage::assistant("first")]);
        mem.record_turn(&[ChatMessage::assistant("second")]);
        mem.record_turn(&[ChatMessage::assistant("third")]);

        assert_eq!(mem.summaries().len(), 1);
        assert_eq!(mem.summaries()[0].segment, 0..3);
        assert!(mem.summaries()[0].summary.contains("summarized"));
    }

    #[test]
    fn test_multiple_segments() {
        let mut mem = DistilledMemory::new(2, |msgs: &[ChatMessage]| {
            (format!("s{}", msgs.len()), vec![])
        });

        for i in 0..5 {
            mem.record_turn(&[ChatMessage::assistant(format!("turn {i}"))]);
        }

        // Segments at 2, 4 turns (not at 5 since 5 % 2 != 0)
        assert_eq!(mem.summaries().len(), 2);
    }

    #[test]
    fn test_format_context() {
        let mut mem = DistilledMemory::new(1, |_: &[ChatMessage]| {
            ("user asked about deployment".into(), vec!["deploy".into()])
        });

        mem.record_turn(&[ChatMessage::assistant("deploy")]);
        mem.record_turn(&[ChatMessage::assistant("testing")]);

        let ctx = mem.format_context(10);
        assert!(ctx.is_some());
        let ctx = ctx.unwrap();
        // Both summaries from the same summarizer, only "deployment" appears
        assert!(ctx.contains("deployment"));
        // Topic "deploy" should appear
        assert!(ctx.contains("deploy"));
        // Format header present
        assert!(ctx.contains("Conversation Background"));
    }

    #[test]
    fn test_format_context_empty() {
        let mem = DistilledMemory::<fn(&[ChatMessage]) -> (String, Vec<String>)>::new(5, |_| {
            ("".into(), vec![])
        });
        assert!(mem.format_context(5).is_none());
    }

    #[test]
    fn test_latest() {
        let mut mem = DistilledMemory::new(1, |_: &[ChatMessage]| ("summary".into(), vec![]));

        mem.record_turn(&[ChatMessage::assistant("a")]);
        mem.record_turn(&[ChatMessage::assistant("b")]);

        let latest = mem.latest();
        assert!(latest.is_some());
        assert_eq!(latest.unwrap().summary, "summary");
    }

    #[test]
    fn test_clear_resets_state() {
        let mut mem = DistilledMemory::new(1, |_: &[ChatMessage]| ("x".into(), vec![]));
        mem.record_turn(&[ChatMessage::assistant("a")]);
        assert!(!mem.summaries().is_empty());
        mem.clear();
        assert!(mem.summaries().is_empty());
    }
}
