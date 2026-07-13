//! **Plan Mode** — creates a step-by-step task plan and executes it.
//!
//! The [`PlanLoop`] takes a high-level task description, breaks it down into
//! ordered steps, executes each step sequentially (updating progress), and
//! reports the final result.
//!
//! # Flow
//! 1. **Plan** — generate a structured plan from the task description
//! 2. **Execute** — run each step in sequence, tracking progress
//! 3. **Report** — return the completed plan with results

use super::loop_trait::{Context, Loop, LoopResult, elapsed_ms};
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// A single step in a plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    /// Step number (1-based).
    pub id: u32,
    /// Description of what this step accomplishes.
    pub description: String,
    /// Status of this step.
    pub status: StepStatus,
    /// Optional output from executing this step.
    pub output: Option<String>,
}

/// Status of a plan step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepStatus {
    /// Step is pending execution.
    Pending,
    /// Step is currently executing.
    InProgress,
    /// Step completed successfully.
    Completed,
    /// Step failed.
    Failed(String),
}

/// A complete plan consisting of ordered steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    /// Title of the plan.
    pub title: String,
    /// Ordered list of steps.
    pub steps: Vec<PlanStep>,
}

impl Plan {
    /// Create a new plan from a title and step descriptions.
    pub fn new(title: impl Into<String>, step_descriptions: Vec<String>) -> Self {
        let steps: Vec<PlanStep> = step_descriptions
            .into_iter()
            .enumerate()
            .map(|(i, desc)| PlanStep {
                id: (i + 1) as u32,
                description: desc,
                status: StepStatus::Pending,
                output: None,
            })
            .collect();
        Self {
            title: title.into(),
            steps,
        }
    }

    /// Returns `true` if all steps are completed.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.steps.iter().all(|s| s.status == StepStatus::Completed)
    }

    /// Returns the current step index (0-based), or `None` if all done.
    #[must_use]
    pub fn current_step_index(&self) -> Option<usize> {
        self.steps
            .iter()
            .position(|s| s.status == StepStatus::Pending)
    }
}

/// The overall result of plan execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanOutput {
    /// The complete plan with step results.
    pub plan: Plan,
    /// Overall success or failure.
    pub success: bool,
    /// Summary message.
    pub summary: String,
}

/// A function that executes a single plan step and returns the result.
///
/// The handler receives the step description and should return `Ok(output)` on
/// success or `Err(message)` on failure.
pub type StepHandler = dyn Fn(&str) -> Result<String, String> + Send + Sync;

/// A function that generates a plan from a task description.
///
/// Receives the task description and should return a list of step descriptions.
pub type PlanGenerator = dyn Fn(&str) -> Result<Vec<String>, String> + Send + Sync;

/// **Plan Loop** — creates and executes a step-by-step plan.
///
/// # Type parameters
/// * `C` — context type (typically the task description)
///
/// # State
/// [`PlanState`] — holds the plan and execution progress.
///
/// # Execution
/// 1. Generate a plan from the task (using `plan_generator`)
/// 2. Execute each step sequentially (using `step_handler`)
/// 3. Return the completed plan with results
pub struct PlanLoop {
    plan_generator: Box<PlanGenerator>,
    step_handler: Box<StepHandler>,
}

impl PlanLoop {
    /// Create a new plan loop.
    pub fn new(plan_generator: Box<PlanGenerator>, step_handler: Box<StepHandler>) -> Self {
        Self {
            plan_generator,
            step_handler,
        }
    }
}

/// State for the Plan Loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanState {
    /// The generated plan.
    pub plan: Option<Plan>,
    /// Current step being executed (0-based index).
    pub current_step: usize,
}

impl Default for PlanState {
    fn default() -> Self {
        Self {
            plan: None,
            current_step: 0,
        }
    }
}

#[async_trait::async_trait]
impl Loop for PlanLoop {
    type Context = String;
    type State = PlanState;
    type Output = PlanOutput;

    async fn execute(
        &self,
        ctx: Context<Self::Context>,
        state: &mut Self::State,
    ) -> LoopResult<Self::Output> {
        let start = Instant::now();

        // Phase 1: Generate plan (if not already in state)
        let plan = if let Some(ref existing) = state.plan {
            existing.clone()
        } else {
            let step_descriptions = match (self.plan_generator)(&ctx.input) {
                Ok(steps) => {
                    if steps.is_empty() {
                        return LoopResult::failure(
                            "plan generator returned no steps",
                            1,
                            elapsed_ms(&start),
                        );
                    }
                    steps
                }
                Err(e) => {
                    return LoopResult::failure(
                        format!("plan generation failed: {e}"),
                        1,
                        elapsed_ms(&start),
                    );
                }
            };
            let plan = Plan::new(&ctx.input, step_descriptions);
            state.plan = Some(plan.clone());
            plan
        };

        // Phase 2: Execute pending steps sequentially
        let total = plan.steps.len();
        let mut completed: Vec<PlanStep> = plan.steps.clone();

        while let Some(idx) = completed
            .iter()
            .position(|s| s.status == StepStatus::Pending)
        {
            // Mark current step as in-progress
            let description = completed[idx].description.clone();
            completed[idx].status = StepStatus::InProgress;
            state.current_step = idx;

            // Execute the step
            let result = (self.step_handler)(&description);

            match result {
                Ok(output) => {
                    completed[idx].status = StepStatus::Completed;
                    completed[idx].output = Some(output);
                }
                Err(e) => {
                    completed[idx].status = StepStatus::Failed(e.clone());
                    // Return partial results on failure
                    let final_plan = Plan {
                        title: plan.title.clone(),
                        steps: completed,
                    };
                    return LoopResult::success(
                        PlanOutput {
                            plan: final_plan,
                            success: false,
                            summary: format!("Plan failed at step {} of {}: {e}", idx + 1, total),
                        },
                        1,
                        elapsed_ms(&start),
                    );
                }
            }
        }

        // All steps completed successfully
        let final_plan = Plan {
            title: plan.title,
            steps: completed,
        };

        LoopResult::success(
            PlanOutput {
                success: true,
                summary: format!("Plan completed all {total} steps successfully"),
                plan: final_plan,
            },
            1,
            elapsed_ms(&start),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loops::{CycleType, LoopId, StopCondition};

    fn echo_plan_generator(task: &str) -> Result<Vec<String>, String> {
        // Simple plan: create two steps from the task
        Ok(vec![
            format!("Analyze: {task}"),
            format!("Implement: {task}"),
        ])
    }

    fn success_handler(step: &str) -> Result<String, String> {
        Ok(format!("executed: {step}"))
    }

    fn fail_handler(step: &str) -> Result<String, String> {
        if step.contains("fail") {
            Err("intentional failure".into())
        } else {
            Ok(format!("executed: {step}"))
        }
    }

    #[tokio::test]
    async fn test_plan_loop_success() {
        let plan_loop = PlanLoop::new(Box::new(echo_plan_generator), Box::new(success_handler));
        let ctx = Context::new(
            LoopId::new(),
            CycleType::Turn,
            StopCondition::max_iterations(1),
            "build feature X".into(),
        );
        let mut state = PlanState::default();

        let result = plan_loop.execute(ctx, &mut state).await;
        assert!(result.is_success());
        let output = result.output.unwrap();
        assert!(output.success);
        assert_eq!(output.plan.steps.len(), 2);
        assert!(output.plan.is_complete());
        assert_eq!(output.plan.steps[0].status, StepStatus::Completed);
        assert_eq!(output.plan.steps[1].status, StepStatus::Completed);
    }

    #[tokio::test]
    async fn test_plan_loop_step_failure() {
        let plan_loop = PlanLoop::new(
            Box::new(|_| Ok(vec!["step 1".into(), "step_fail".into(), "step 3".into()])),
            Box::new(fail_handler),
        );
        let ctx = Context::new(
            LoopId::new(),
            CycleType::Turn,
            StopCondition::max_iterations(1),
            "test task".into(),
        );
        let mut state = PlanState::default();

        let result = plan_loop.execute(ctx, &mut state).await;
        assert!(result.is_success()); // Returns success with failure info in output
        let output = result.output.unwrap();
        assert!(!output.success);
        assert_eq!(output.plan.steps[0].status, StepStatus::Completed);
        assert_eq!(
            output.plan.steps[1].status,
            StepStatus::Failed("intentional failure".into())
        );
        // Step 3 should not have been executed
        assert_eq!(output.plan.steps[2].status, StepStatus::Pending);
    }

    #[tokio::test]
    async fn test_plan_generator_error() {
        let plan_loop = PlanLoop::new(
            Box::new(|_| Err("generator error".into())),
            Box::new(success_handler),
        );
        let ctx = Context::new(
            LoopId::new(),
            CycleType::Turn,
            StopCondition::max_iterations(1),
            "test".into(),
        );
        let mut state = PlanState::default();

        let result = plan_loop.execute(ctx, &mut state).await;
        assert!(!result.is_success());
        assert!(format!("{:?}", result.status).contains("generator error"));
    }

    #[tokio::test]
    async fn test_plan_from_existing_state() {
        // Simulate resuming from saved state with an existing plan
        let mut state = PlanState {
            plan: Some(Plan::new(
                "existing plan",
                vec!["step 1".into(), "step 2".into()],
            )),
            current_step: 0,
        };

        let plan_loop = PlanLoop::new(
            Box::new(|_| unreachable!("should not be called")),
            Box::new(success_handler),
        );

        let ctx = Context::new(
            LoopId::new(),
            CycleType::Turn,
            StopCondition::max_iterations(1),
            "irrelevant".into(),
        );

        let result = plan_loop.execute(ctx, &mut state).await;
        assert!(result.is_success());
        let output = result.output.unwrap();
        assert!(output.success);
        assert_eq!(output.plan.steps.len(), 2);
    }

    #[tokio::test]
    async fn test_empty_plan_returns_error() {
        let plan_loop = PlanLoop::new(Box::new(|_| Ok(vec![])), Box::new(success_handler));
        let ctx = Context::new(
            LoopId::new(),
            CycleType::Turn,
            StopCondition::max_iterations(1),
            "empty".into(),
        );
        let mut state = PlanState::default();

        let result = plan_loop.execute(ctx, &mut state).await;
        assert!(!result.is_success());
    }
}
