//! **Goal-based** cycle — reaches a goal through a series of steps.
//!
//! A sub-graph with conditional transitions and a verifier.
//! Must have `max_iterations` and/or `timeout`.
//! State MUST implement `serde::Serialize + Deserialize` for suspend/resume.
