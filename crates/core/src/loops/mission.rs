//! **Mission Mode** — multi-phase autonomous execution with planning,
//! execution, and self-correction.
//!
//! The [`MissionLoop`] implements an autonomous agent loop that iteratively
//! plans, executes, and self-corrects to accomplish complex tasks.
//!
//! # Phases
//! 1. **Plan** — create a mission plan with objectives
//! 2. **Execute** — attempt to accomplish objectives
//! 3. **Verify** — check if objectives are met
//! 4. **Correct** — adjust approach if verification fails
//! 5. **Repeat** — iterate until success or max attempts

use super::loop_trait::{Context, Loop, LoopResult, elapsed_ms};
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Current phase of a mission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MissionPhase {
    /// Planning the mission.
    Plan,
    /// Executing the current plan.
    Execute,
    /// Verifying results.
    Verify,
    /// Self-correcting after verification failure.
    Correct,
    /// Mission completed.
    Complete,
}

impl MissionPhase {
    /// Human-readable phase name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Execute => "execute",
            Self::Verify => "verify",
            Self::Correct => "correct",
            Self::Complete => "complete",
        }
    }
}

/// A single objective in a mission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionObjective {
    /// Description of the objective.
    pub description: String,
    /// Whether this objective has been met.
    pub met: bool,
}

/// The mission plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionPlan {
    /// High-level goal.
    pub goal: String,
    /// Ordered objectives.
    pub objectives: Vec<MissionObjective>,
    /// Strategy description.
    pub strategy: String,
}

/// The output of a mission execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionOutput {
    /// Whether the mission was successful.
    pub success: bool,
    /// Number of correction attempts made.
    pub correction_attempts: u32,
    /// Final plan state.
    pub plan: MissionPlan,
    /// Execution log.
    pub log: Vec<String>,
    /// Summary message.
    pub summary: String,
}

/// Handler for creating a mission plan from a goal.
pub type MissionPlanner = dyn Fn(&str) -> Result<MissionPlan, String> + Send + Sync;

/// Handler for executing a mission and returning results.
pub type MissionExecutor = dyn Fn(&MissionPlan) -> Result<String, String> + Send + Sync;

/// Handler for verifying mission results.
pub type MissionVerifier = dyn Fn(&MissionPlan, &str) -> Result<bool, String> + Send + Sync;

/// Handler for self-correction after verification failure.
pub type MissionCorrector = dyn Fn(&mut MissionPlan, &str) -> Result<(), String> + Send + Sync;

/// State for the Mission Loop (serializable for checkpointing).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionState {
    /// Current mission phase.
    pub phase: MissionPhase,
    /// The mission plan.
    pub plan: Option<MissionPlan>,
    /// Execution log entries.
    pub log: Vec<String>,
    /// Number of correction attempts made.
    pub correction_attempts: u32,
    /// Result from the last execution phase.
    pub last_execution_result: Option<String>,
}

impl Default for MissionState {
    fn default() -> Self {
        Self {
            phase: MissionPhase::Plan,
            plan: None,
            log: Vec::new(),
            correction_attempts: 0,
            last_execution_result: None,
        }
    }
}

/// **Mission Loop** — multi-phase autonomous execution.
///
/// Executes a mission by cycling through Plan → Execute → Verify → Correct
/// phases until the mission is complete or max attempts are exhausted.
///
/// # Configuration
/// * `max_corrections` — maximum number of self-correction attempts (default: 3)
pub struct MissionLoop {
    planner: Box<MissionPlanner>,
    executor: Box<MissionExecutor>,
    verifier: Box<MissionVerifier>,
    corrector: Box<MissionCorrector>,
    max_corrections: u32,
}

impl MissionLoop {
    /// Create a new mission loop with the given handlers.
    pub fn new(
        planner: Box<MissionPlanner>,
        executor: Box<MissionExecutor>,
        verifier: Box<MissionVerifier>,
        corrector: Box<MissionCorrector>,
    ) -> Self {
        Self {
            planner,
            executor,
            verifier,
            corrector,
            max_corrections: 3,
        }
    }

    /// Set the maximum number of correction attempts.
    #[must_use]
    pub fn with_max_corrections(mut self, max: u32) -> Self {
        self.max_corrections = max;
        self
    }
}

#[async_trait::async_trait]
impl Loop for MissionLoop {
    type Context = String;
    type State = MissionState;
    type Output = MissionOutput;

    async fn execute(
        &self,
        ctx: Context<Self::Context>,
        state: &mut Self::State,
    ) -> LoopResult<Self::Output> {
        let start = Instant::now();

        // Phase 1: Plan
        if state.phase == MissionPhase::Plan {
            let plan = match (self.planner)(&ctx.input) {
                Ok(p) => p,
                Err(e) => {
                    return LoopResult::failure(
                        format!("mission planning failed: {e}"),
                        1,
                        elapsed_ms(&start),
                    );
                }
            };
            state
                .log
                .push(format!("[plan] Created mission plan: {}", plan.goal));
            state.plan = Some(plan);
            state.phase = MissionPhase::Execute;
        }

        let mut plan = state.plan.clone().unwrap_or_else(|| MissionPlan {
            goal: ctx.input.clone(),
            objectives: Vec::new(),
            strategy: String::new(),
        });

        let max_corrections = self.max_corrections;

        // Execute-Verify-Correct loop
        loop {
            match state.phase {
                MissionPhase::Execute => {
                    state.log.push("[execute] Starting execution".into());
                    let result = match (self.executor)(&plan) {
                        Ok(r) => r,
                        Err(e) => {
                            state.log.push(format!("[execute] Failed: {e}"));
                            state.last_execution_result = Some(format!("error: {e}"));
                            // Move to verify to see if we can correct
                            state.phase = MissionPhase::Verify;
                            continue;
                        }
                    };
                    state.log.push("[execute] Execution completed".into());
                    state.last_execution_result = Some(result);
                    state.phase = MissionPhase::Verify;
                }
                MissionPhase::Verify => {
                    let exec_result = state.last_execution_result.as_deref().unwrap_or("");
                    match (self.verifier)(&plan, exec_result) {
                        Ok(true) => {
                            state.log.push("[verify] Verification passed".into());
                            for obj in &mut plan.objectives {
                                obj.met = true;
                            }
                            state.phase = MissionPhase::Complete;
                        }
                        Ok(false) => {
                            state.log.push(
                                "[verify] Verification failed — initiating correction".into(),
                            );
                            if state.correction_attempts >= max_corrections {
                                state
                                    .log
                                    .push("[verify] Max correction attempts reached".into());
                                // Return partial success with failure info
                                return LoopResult::success(
                                    MissionOutput {
                                        success: false,
                                        correction_attempts: state.correction_attempts,
                                        plan,
                                        log: state.log.clone(),
                                        summary: format!(
                                            "Mission failed after {} correction attempts",
                                            state.correction_attempts,
                                        ),
                                    },
                                    1,
                                    elapsed_ms(&start),
                                );
                            }
                            state.phase = MissionPhase::Correct;
                        }
                        Err(e) => {
                            state.log.push(format!("[verify] Verification error: {e}"));
                            return LoopResult::failure(
                                format!("mission verification error: {e}"),
                                1,
                                elapsed_ms(&start),
                            );
                        }
                    }
                }
                MissionPhase::Correct => {
                    state.correction_attempts += 1;
                    let exec_result = state.last_execution_result.as_deref().unwrap_or("");
                    state.log.push(format!(
                        "[correct] Attempting correction ({}/{max_corrections})",
                        state.correction_attempts,
                    ));
                    match (self.corrector)(&mut plan, exec_result) {
                        Ok(()) => {
                            state.log.push("[correct] Correction applied".into());
                            state.phase = MissionPhase::Execute;
                        }
                        Err(e) => {
                            state.log.push(format!("[correct] Correction failed: {e}"));
                            return LoopResult::failure(
                                format!("mission correction failed: {e}"),
                                1,
                                elapsed_ms(&start),
                            );
                        }
                    }
                }
                MissionPhase::Complete => {
                    let all_met = plan.objectives.iter().all(|o| o.met);
                    return LoopResult::success(
                        MissionOutput {
                            success: all_met,
                            correction_attempts: state.correction_attempts,
                            plan,
                            log: state.log.clone(),
                            summary: if all_met {
                                "Mission completed successfully".into()
                            } else {
                                "Mission completed with some objectives unmet".into()
                            },
                        },
                        1,
                        elapsed_ms(&start),
                    );
                }
                MissionPhase::Plan => {
                    // Should not reach here; handled above
                    unreachable!("Plan phase should have been handled");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loops::{CycleType, LoopId, StopCondition};

    fn make_planner(goal: &str) -> MissionPlan {
        MissionPlan {
            goal: goal.to_string(),
            objectives: vec![
                MissionObjective {
                    description: "Understand the problem".into(),
                    met: false,
                },
                MissionObjective {
                    description: "Implement solution".into(),
                    met: false,
                },
            ],
            strategy: "Analyze, then implement".into(),
        }
    }

    #[tokio::test]
    async fn test_mission_loop_success() {
        let mission = MissionLoop::new(
            Box::new(|goal| Ok(make_planner(goal))),
            Box::new(|_plan| Ok("execution result".into())),
            Box::new(|plan, _result| {
                let mut p = plan.clone();
                for obj in &mut p.objectives {
                    let _ = obj;
                }
                Ok(true)
            }),
            Box::new(|_plan, _result| Ok(())),
        );

        let ctx = Context::new(
            LoopId::new(),
            CycleType::Goal,
            StopCondition::max_iterations(10),
            "Build a web app".into(),
        );
        let mut state = MissionState::default();

        let result = mission.execute(ctx, &mut state).await;
        assert!(result.is_success());
        let output = result.output.unwrap();
        assert!(output.success);
        assert_eq!(output.correction_attempts, 0);
    }

    #[tokio::test]
    async fn test_mission_loop_with_correction() {
        let call_count = std::sync::atomic::AtomicU32::new(0);

        let mission = MissionLoop::new(
            Box::new(|goal| {
                Ok(MissionPlan {
                    goal: goal.to_string(),
                    objectives: vec![MissionObjective {
                        description: "Pass verification".into(),
                        met: false,
                    }],
                    strategy: "try".into(),
                })
            }),
            Box::new(|_plan| Ok("output".into())),
            Box::new(move |_plan, _result| {
                let prev = call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(prev + 1 >= 2)
            }),
            Box::new(|plan, _result| {
                plan.objectives[0].description = "retrying".into();
                Ok(())
            }),
        );

        let ctx = Context::new(
            LoopId::new(),
            CycleType::Goal,
            StopCondition::max_iterations(10),
            "Task with retry".into(),
        );
        let mut state = MissionState::default();

        let result = mission.execute(ctx, &mut state).await;
        assert!(result.is_success());
        let output = result.output.unwrap();
        assert!(output.success);
        assert_eq!(output.correction_attempts, 1);
    }

    #[tokio::test]
    async fn test_mission_loop_max_corrections_exceeded() {
        let mission = MissionLoop::new(
            Box::new(|goal| {
                Ok(MissionPlan {
                    goal: goal.to_string(),
                    objectives: vec![MissionObjective {
                        description: "impossible".into(),
                        met: false,
                    }],
                    strategy: "try".into(),
                })
            }),
            Box::new(|_plan| Ok("output".into())),
            Box::new(|_plan, _result| Ok(false)), // Always fails verification
            Box::new(|plan, _result| {
                plan.objectives[0].description = "retry".into();
                Ok(())
            }),
        )
        .with_max_corrections(2);

        let ctx = Context::new(
            LoopId::new(),
            CycleType::Goal,
            StopCondition::max_iterations(10),
            "Impossible task".into(),
        );
        let mut state = MissionState::default();

        let result = mission.execute(ctx, &mut state).await;
        assert!(result.is_success()); // Returns partial results
        let output = result.output.unwrap();
        assert!(!output.success);
        assert_eq!(output.correction_attempts, 2);
    }

    #[tokio::test]
    async fn test_mission_planner_error() {
        let mission = MissionLoop::new(
            Box::new(|_| Err("planner error".into())),
            Box::new(|_| Ok("".into())),
            Box::new(|_, _| Ok(true)),
            Box::new(|_, _| Ok(())),
        );

        let ctx = Context::new(
            LoopId::new(),
            CycleType::Goal,
            StopCondition::max_iterations(10),
            "test".into(),
        );
        let mut state = MissionState::default();

        let result = mission.execute(ctx, &mut state).await;
        assert!(!result.is_success());
    }

    #[tokio::test]
    async fn test_mission_phase_names() {
        assert_eq!(MissionPhase::Plan.name(), "plan");
        assert_eq!(MissionPhase::Execute.name(), "execute");
        assert_eq!(MissionPhase::Verify.name(), "verify");
        assert_eq!(MissionPhase::Correct.name(), "correct");
        assert_eq!(MissionPhase::Complete.name(), "complete");
    }
}
