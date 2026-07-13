//! **State Persistence** — save and load graph snapshots and arbitrary state to JSON files.
//!
//! # Usage
//!
//! ```ignore
//! use praxis_core::persistence::{save_snapshot, load_snapshot};
//! use praxis_core::loops::GraphSnapshot;
//!
//! let snapshot = GraphSnapshot {
//!     current_node: "node_1".into(),
//!     state: vec![1u32, 2, 3],
//! };
//! save_snapshot("/tmp/snapshot.json", &snapshot)?;
//! let loaded: GraphSnapshot<Vec<u32>> = load_snapshot("/tmp/snapshot.json")?;
//! ```

use crate::error::Result;
use crate::loops::GraphSnapshot;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::path::Path;

/// Save any serializable value to a JSON file.
///
/// # Errors
/// Returns an error if serialization or file I/O fails.
pub fn save_json<T: Serialize>(path: impl AsRef<Path>, value: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(value)?;
    std::fs::write(path.as_ref(), json)?;
    Ok(())
}

/// Load any deserializable value from a JSON file.
///
/// # Errors
/// Returns an error if file I/O or deserialization fails.
pub fn load_json<T: DeserializeOwned>(path: impl AsRef<Path>) -> Result<T> {
    let json = std::fs::read_to_string(path.as_ref())?;
    Ok(serde_json::from_str(&json)?)
}

/// Save a [`GraphSnapshot`] to a JSON file.
///
/// Convenience wrapper around [`save_json`].
///
/// # Errors
/// Returns an error if serialization or file I/O fails.
pub fn save_snapshot<S: Serialize>(
    path: impl AsRef<Path>,
    snapshot: &GraphSnapshot<S>,
) -> Result<()> {
    save_json(path, snapshot)
}

/// Load a [`GraphSnapshot`] from a JSON file.
///
/// Convenience wrapper around [`load_json`].
///
/// # Errors
/// Returns an error if file I/O or deserialization fails.
pub fn load_snapshot<S: DeserializeOwned>(path: impl AsRef<Path>) -> Result<GraphSnapshot<S>> {
    load_json(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loops::{
        Context, CycleType, GraphSnapshot, LoopId, LoopResult, NodeId, StopCondition, StopReason,
    };
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    fn test_path(name: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut p = std::env::temp_dir();
        let stem = name.trim_end_matches(".json");
        p.push(format!("{stem}_{id}.json"));
        p
    }

    #[test]
    fn test_json_roundtrip_simple() {
        let path = test_path("praxis_test_simple.json");
        let value = 42u64;
        save_json(&path, &value).unwrap();
        let loaded: u64 = load_json(&path).unwrap();
        assert_eq!(loaded, value);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_snapshot_in_memory_serde() {
        let snapshot = GraphSnapshot {
            current_node: NodeId::from_id("test_node"),
            state: "some_state".to_string(),
        };
        let json = snapshot.to_json().unwrap();
        let loaded: GraphSnapshot<String> = GraphSnapshot::from_json(&json).unwrap();
        assert_eq!(loaded.current_node.to_string(), "test_node");
        assert_eq!(loaded.state, "some_state");
    }

    #[test]
    fn test_snapshot_file_roundtrip() {
        let path = test_path("praxis_test_snapshot.json");
        let snapshot = GraphSnapshot {
            current_node: NodeId::from_id("node_1"),
            state: vec![1u32, 2, 3],
        };
        save_snapshot(&path, &snapshot).unwrap();
        let loaded: GraphSnapshot<Vec<u32>> = load_snapshot(&path).unwrap();
        assert_eq!(loaded.current_node.to_string(), "node_1");
        assert_eq!(loaded.state, vec![1, 2, 3]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_context_serde_roundtrip() {
        let ctx = Context::new(
            LoopId::from("ctx-1".to_string()),
            CycleType::Goal,
            StopCondition::new(Some(10), Some(Duration::from_secs(30))),
            "hello".to_string(),
        );
        let json = serde_json::to_string(&ctx).unwrap();
        let loaded: Context<String> = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.input, "hello");
        assert_eq!(loaded.cycle_type, CycleType::Goal);
        assert_eq!(loaded.stop_condition.max_iterations, Some(10));
        // Duration roundtrip: 30s = 30000ms
        assert!(loaded.stop_condition.timeout.is_some());
    }

    #[test]
    fn test_loop_result_serde() {
        let result = LoopResult::<String>::success("ok".into(), 5, 100);
        let json = serde_json::to_string(&result).unwrap();
        let loaded: LoopResult<String> = serde_json::from_str(&json).unwrap();
        assert!(loaded.is_success());
        assert_eq!(loaded.output, Some("ok".to_string()));
        assert_eq!(loaded.iterations, 5);
        assert_eq!(loaded.duration_ms, 100);
    }

    #[test]
    fn test_loop_result_failure_serde() {
        let result = LoopResult::<String>::failure("something went wrong", 3, 5000);
        let json = serde_json::to_string(&result).unwrap();
        let loaded: LoopResult<String> = serde_json::from_str(&json).unwrap();
        assert!(!loaded.is_success());
        assert!(loaded.output.is_none());
        assert_eq!(loaded.iterations, 3);
    }

    #[test]
    fn test_agent_config_serde() {
        use crate::agent::runtime::AgentConfig;

        let config = AgentConfig {
            model: "gpt-4o".into(),
            model_id: None,
            system_prompt: "You are a test agent.".into(),
            temperature: Some(0.7),
            max_tokens: Some(4096),
            scroll_strategy: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        let loaded: AgentConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.model, "gpt-4o");
        assert_eq!(loaded.system_prompt, "You are a test agent.");
        assert_eq!(loaded.temperature, Some(0.7));
        assert_eq!(loaded.max_tokens, Some(4096));
        assert!(loaded.scroll_strategy.is_none());
    }

    #[test]
    fn test_agent_config_no_scroll_strategy_in_json() {
        use crate::agent::runtime::AgentConfig;
        // Ensure scroll_strategy is excluded from JSON
        let config = AgentConfig {
            model: "claude-3-5-sonnet".into(),
            model_id: None,
            system_prompt: "Help.".into(),
            temperature: None,
            max_tokens: None,
            scroll_strategy: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        // scroll_strategy should not appear in the JSON
        assert!(
            !json.contains("scroll_strategy"),
            "scroll_strategy should be skipped in serialization"
        );
    }

    #[test]
    fn test_stop_condition_duration_roundtrip() {
        let cond = StopCondition::new(Some(5), Some(Duration::from_secs(60)));
        let json = serde_json::to_string(&cond).unwrap();
        let loaded: StopCondition = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.max_iterations, Some(5));
        // 60 seconds = 60000 ms
        let dur = loaded.timeout.unwrap();
        assert_eq!(dur.as_secs(), 60);
    }

    #[test]
    fn test_stop_condition_no_timeout() {
        let cond = StopCondition::max_iterations(3);
        let json = serde_json::to_string(&cond).unwrap();
        let loaded: StopCondition = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.max_iterations, Some(3));
        assert!(loaded.timeout.is_none());
    }

    #[test]
    fn test_loop_id_serde() {
        let id = LoopId::from("my-id".to_string());
        let json = serde_json::to_string(&id).unwrap();
        let loaded: LoopId = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.to_string(), "my-id");
    }

    #[test]
    fn test_loop_status_serde() {
        use crate::loops::LoopStatus;

        let cases = vec![
            LoopStatus::Running,
            LoopStatus::Paused,
            LoopStatus::Completed(StopReason::Complete),
            LoopStatus::Completed(StopReason::GoalMet),
            LoopStatus::Completed(StopReason::MaxIterations { max: 10 }),
            LoopStatus::Completed(StopReason::Timeout { elapsed_ms: 5000 }),
            LoopStatus::Completed(StopReason::Cancelled),
            LoopStatus::Failed("critical error".into()),
        ];

        for status in cases {
            let json = serde_json::to_string(&status).unwrap();
            let loaded: LoopStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(loaded, status, "roundtrip failed for {status:?}");
        }
    }

    #[test]
    fn test_file_not_found_error() {
        let path = test_path("praxis_test_nonexistent.json");
        let result: std::result::Result<GraphSnapshot<String>, crate::error::Error> =
            load_snapshot(&path);
        assert!(result.is_err());
    }
}
