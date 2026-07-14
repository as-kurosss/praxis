//! **SessionScroll** — bridges persistent [`SessionHistory`] with the
//! in-memory conversation state in [`Agent::execute_impl`].
//!
//! When attached to an [`Agent`], `SessionScroll` automatically:
//! * Loads recent non-evicted turns into the conversation state before
//!   execution (when the state starts empty — i.e. a fresh session).
//! * Saves each turn (user input and assistant reply) into the SQLite-backed
//!   history so the record survives restarts.
//!
//! # Usage
//!
//! ```ignore
//! use crate::context::SessionScroll;
//!
//! let history = SessionHistory::open(data_dir, "my-session").unwrap();
//! let scroll = SessionScroll::new(history);
//!
//! let agent = Agent::new(client, config)
//!     .with_session_scroll(scroll);
//! ```

use super::session_history::{MemoryFact, SessionHistory, SessionHistoryError, TurnRecord};
use crate::agent::llm::ChatMessage;
use std::sync::Arc;

/// Bridge between `SessionHistory` and the agent's in-memory state.
pub struct SessionScroll {
    history: Arc<SessionHistory>,
    /// How many recent non-evicted turns to load into an empty state.
    max_load_turns: usize,
}

impl SessionScroll {
    /// Wrap a `SessionHistory` with the default settings.
    ///
    /// Default: loads up to 100 recent turns when the state is empty.
    pub fn new(history: SessionHistory) -> Self {
        Self {
            history: Arc::new(history),
            max_load_turns: 100,
        }
    }

    /// Set the maximum number of turns to load into an empty state.
    pub fn with_max_load_turns(mut self, n: usize) -> Self {
        self.max_load_turns = n;
        self
    }

    /// Borrow the inner `SessionHistory`.
    pub fn history(&self) -> &SessionHistory {
        &self.history
    }

    /// Load recent non-evicted turns into `state`.
    ///
    /// This is a no-op when `state` is already non-empty (i.e. this isn't the
    /// first call in a session).  Turns are prepended in chronological order.
    pub fn load_into_state(&self, state: &mut Vec<ChatMessage>) -> Result<(), SessionHistoryError> {
        if !state.is_empty() {
            return Ok(());
        }
        let records = self.history.get_recent(self.max_load_turns)?;
        for rec in &records {
            state.push(Self::record_to_message(rec));
        }
        Ok(())
    }

    /// Save a user or assistant turn into the session history.
    ///
    /// `role` must be `"user"` or `"assistant"`.  A new UUID is generated
    /// automatically for the record.
    pub fn save_turn(
        &self,
        role: &str,
        content: Option<&str>,
        token_count: i64,
    ) -> Result<(), SessionHistoryError> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let record = TurnRecord {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: self.history.session_id().to_string(),
            timestamp,
            role: role.to_string(),
            content: content.map(String::from),
            tool_calls: None,
            tool_results: None,
            token_count,
            evicted: false,
        };
        self.history.append_turn(&record)
    }

    /// Load recent turns into `state` asynchronously via `spawn_blocking`.
    pub async fn load_into_state_async(
        &self,
        state: &mut Vec<ChatMessage>,
    ) -> Result<(), SessionHistoryError> {
        if !state.is_empty() {
            return Ok(());
        }
        let history = self.history.clone();
        let n = self.max_load_turns;
        let records = tokio::task::spawn_blocking(move || history.get_recent(n))
            .await
            .map_err(|e| SessionHistoryError::Io(format!("spawn_blocking: {e}")))?
            .map_err(|e| SessionHistoryError::Io(format!("load: {e}")))?; // existing Io wrapping
        for rec in &records {
            state.push(Self::record_to_message(rec));
        }
        Ok(())
    }

    /// Save a turn asynchronously via `spawn_blocking`.
    pub async fn save_turn_async(
        &self,
        role: &str,
        content: Option<&str>,
        token_count: i64,
    ) -> Result<(), SessionHistoryError> {
        let history = self.history.clone();
        let role = role.to_string();
        let content = content.map(String::from);
        tokio::task::spawn_blocking(move || {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let record = TurnRecord {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: history.session_id().to_string(),
                timestamp,
                role,
                content,
                tool_calls: None,
                tool_results: None,
                token_count,
                evicted: false,
            };
            history.append_turn(&record)
        })
        .await
        .map_err(|e| SessionHistoryError::Io(format!("spawn_blocking: {e}")))?
    }

    /// Return relevant facts for a query.
    pub fn get_relevant_facts(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MemoryFact>, SessionHistoryError> {
        self.history.search_facts(query, limit)
    }

    /// Convert a `TurnRecord` into a `ChatMessage`.
    fn record_to_message(rec: &TurnRecord) -> ChatMessage {
        let role = match rec.role.as_str() {
            "system" => crate::agent::llm::Role::System,
            "assistant" => crate::agent::llm::Role::Assistant,
            "tool" => crate::agent::llm::Role::Tool,
            _ => crate::agent::llm::Role::User,
        };
        ChatMessage {
            role,
            content: rec.content.clone(),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
            qwenpaw_tag: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::llm::ChatMessage;

    #[test]
    fn test_load_into_empty_state() {
        let hist = SessionHistory::open_in_memory("scroll-test").unwrap();
        let scroll = SessionScroll::new(hist);

        // Save a few turns
        scroll.save_turn("user", Some("hello"), 5).unwrap();
        scroll.save_turn("assistant", Some("hi there"), 5).unwrap();

        let mut state = Vec::new();
        scroll.load_into_state(&mut state).unwrap();
        assert_eq!(state.len(), 2);
        assert_eq!(state[0].content.as_deref(), Some("hello"));
        assert_eq!(state[1].content.as_deref(), Some("hi there"));
    }

    #[test]
    fn test_skip_non_empty_state() {
        let hist = SessionHistory::open_in_memory("scroll-test-2").unwrap();
        let scroll = SessionScroll::new(hist);

        scroll.save_turn("user", Some("old msg"), 5).unwrap();

        // State already has content — should skip loading
        let mut state = vec![ChatMessage::user("existing")];
        scroll.load_into_state(&mut state).unwrap();
        assert_eq!(state.len(), 1);
        assert_eq!(state[0].content.as_deref(), Some("existing"));
    }
}
