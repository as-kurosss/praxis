use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Unique identifier for a loop instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LoopId(String);

impl LoopId {
    /// Create a new unique loop ID.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl Default for LoopId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<String> for LoopId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for LoopId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The four primitive orchestration cycle types.
///
/// Every loop is classified by one cycle type that determines
/// how it is triggered, executed, and stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CycleType {
    /// Single request → single response (one handler invocation).
    Turn,
    /// Iterate until a verifier confirms the goal or limits are exhausted.
    Goal,
    /// Triggered by a schedule (wrapper — delegates to Turn/Goal).
    Time,
    /// Triggered by an external event (wrapper — delegates to Turn/Goal).
    Proactive,
}

impl CycleType {
    /// Human-readable name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Turn => "turn",
            Self::Goal => "goal",
            Self::Time => "time",
            Self::Proactive => "proactive",
        }
    }
}

/// Why a loop stopped executing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopReason {
    /// The goal was achieved (Goal-based).
    GoalMet,
    /// Maximum number of iterations reached.
    MaxIterations { max: u32 },
    /// Operation timed out.
    Timeout { elapsed_ms: u64 },
    /// Loop was cancelled externally.
    Cancelled,
    /// Loop completed naturally (Turn-based / Time-based single execution).
    Complete,
}

/// Hard limits for a loop's execution.
///
/// At least one limit MUST be set for every loop.
/// For Turn-based loops, `timeout` is recommended.
/// For Goal-based loops, both `max_iterations` and `timeout` are required.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopCondition {
    /// Maximum number of iterations before forced stop.
    pub max_iterations: Option<u32>,
    /// Maximum wall-clock duration before forced stop (serialized as milliseconds).
    #[serde(with = "opt_duration_millis")]
    pub timeout: Option<Duration>,
}

mod opt_duration_millis {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(dur: &Option<Duration>, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match dur {
            Some(d) => {
                let ms = d.as_millis() as u64;
                Serialize::serialize(&ms, s)
            }
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(d: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let ms: Option<u64> = Option::deserialize(d)?;
        Ok(ms.map(Duration::from_millis))
    }
}

impl StopCondition {
    /// Create a condition with only a max-iterations limit.
    pub fn max_iterations(max: u32) -> Self {
        Self {
            max_iterations: Some(max),
            timeout: None,
        }
    }

    /// Create a condition with only a timeout limit.
    pub fn timeout(duration: Duration) -> Self {
        Self {
            max_iterations: None,
            timeout: Some(duration),
        }
    }

    /// Create a condition with both limits.
    ///
    /// # Panics
    /// In debug builds, panics if both limits are `None`.
    pub fn new(max_iterations: Option<u32>, timeout: Option<Duration>) -> Self {
        debug_assert!(
            max_iterations.is_some() || timeout.is_some(),
            "StopCondition: at least one limit (max_iterations or timeout) must be set"
        );
        Self {
            max_iterations,
            timeout,
        }
    }
}

/// Runtime status of a loop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoopStatus {
    /// Loop is executing or ready to execute.
    Running,
    /// Loop execution was paused (can be resumed).
    Paused,
    /// Loop completed successfully with a stop reason.
    Completed(StopReason),
    /// Loop failed with an error description.
    Failed(String),
}

impl LoopStatus {
    /// Returns `true` if the loop is in a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed(_) | Self::Failed(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_loop_id_new_is_unique() {
        let id1 = LoopId::new();
        let id2 = LoopId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_loop_id_from_string() {
        let id = LoopId::from("test-42".to_string());
        assert_eq!(id.to_string(), "test-42");
    }

    #[test]
    fn test_loop_id_default() {
        let id = LoopId::default();
        assert!(!id.to_string().is_empty());
    }

    #[test]
    fn test_cycle_type_name() {
        assert_eq!(CycleType::Turn.name(), "turn");
        assert_eq!(CycleType::Goal.name(), "goal");
        assert_eq!(CycleType::Time.name(), "time");
        assert_eq!(CycleType::Proactive.name(), "proactive");
    }

    #[test]
    fn test_cycle_type_equality() {
        assert_eq!(CycleType::Turn, CycleType::Turn);
        assert_ne!(CycleType::Turn, CycleType::Goal);
    }

    #[test]
    fn test_stop_reason_variants() {
        let reasons = [
            StopReason::GoalMet,
            StopReason::MaxIterations { max: 5 },
            StopReason::Timeout { elapsed_ms: 1000 },
            StopReason::Cancelled,
            StopReason::Complete,
        ];
        assert_eq!(reasons.len(), 5);
        assert_eq!(reasons[0], StopReason::GoalMet);
    }

    #[test]
    fn test_stop_condition_max_iterations() {
        let cond = StopCondition::max_iterations(10);
        assert_eq!(cond.max_iterations, Some(10));
        assert!(cond.timeout.is_none());
    }

    #[test]
    fn test_stop_condition_timeout() {
        let cond = StopCondition::timeout(Duration::from_secs(30));
        assert_eq!(cond.timeout, Some(Duration::from_secs(30)));
        assert!(cond.max_iterations.is_none());
    }

    #[test]
    fn test_stop_condition_both() {
        let cond = StopCondition::new(Some(5), Some(Duration::from_secs(10)));
        assert_eq!(cond.max_iterations, Some(5));
        assert_eq!(cond.timeout, Some(Duration::from_secs(10)));
    }

    #[test]
    fn test_loop_status_running_not_terminal() {
        assert!(!LoopStatus::Running.is_terminal());
        assert!(!LoopStatus::Paused.is_terminal());
    }

    #[test]
    fn test_loop_status_terminal() {
        assert!(LoopStatus::Completed(StopReason::Complete).is_terminal());
        assert!(LoopStatus::Failed("error".into()).is_terminal());
    }

    #[test]
    fn test_loop_status_equality() {
        assert_eq!(
            LoopStatus::Completed(StopReason::GoalMet),
            LoopStatus::Completed(StopReason::GoalMet),
        );
        assert_ne!(LoopStatus::Running, LoopStatus::Paused);
    }
}
