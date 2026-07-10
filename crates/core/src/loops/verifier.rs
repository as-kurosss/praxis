/// The outcome of a single verification check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The goal has been achieved.
    Met,
    /// The goal has NOT been achieved yet.
    NotMet,
    /// Verification failed due to an error (e.g. invalid state).
    Error,
}

/// A **deterministic** verifier for goal-based cycles.
///
/// Verifiers are code (functions/traits), not prompts. They inspect the
/// current state and decide whether the goal is met.
///
/// # Type parameter
///
/// * `S` — the state type to verify against.
pub trait Verifier<S>: Send + Sync {
    /// Check whether the goal is achieved in the given state.
    fn verify(&self, state: &S) -> Verdict;
}

/// A verifier that always returns `Met`.
///
/// Useful as a default / placeholder for loops that don't need verification.
pub struct AlwaysMet;

impl<S> Verifier<S> for AlwaysMet {
    fn verify(&self, _state: &S) -> Verdict {
        Verdict::Met
    }
}

/// A verifier wrapping a closure.
pub struct FnVerifier<F, S> {
    func: F,
    _phantom: std::marker::PhantomData<fn(S) -> S>,
}

impl<F, S> FnVerifier<F, S> {
    /// Create a verifier from a function/closure.
    pub fn new(func: F) -> Self {
        Self {
            func,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<F, S> Verifier<S> for FnVerifier<F, S>
where
    F: Fn(&S) -> Verdict + Send + Sync,
{
    fn verify(&self, state: &S) -> Verdict {
        (self.func)(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ScoreState {
        score: u32,
    }

    /// A mock verifier: goal is met when score >= 90.
    struct ScoreVerifier {
        threshold: u32,
    }

    impl Verifier<ScoreState> for ScoreVerifier {
        fn verify(&self, state: &ScoreState) -> Verdict {
            if state.score >= self.threshold {
                Verdict::Met
            } else {
                Verdict::NotMet
            }
        }
    }

    #[test]
    fn test_verifier_met() {
        let verifier = ScoreVerifier { threshold: 90 };
        let state = ScoreState { score: 95 };
        assert_eq!(verifier.verify(&state), Verdict::Met);
    }

    #[test]
    fn test_verifier_not_met() {
        let verifier = ScoreVerifier { threshold: 90 };
        let state = ScoreState { score: 50 };
        assert_eq!(verifier.verify(&state), Verdict::NotMet);
    }

    #[test]
    fn test_verifier_edge_boundary() {
        let verifier = ScoreVerifier { threshold: 90 };
        let state = ScoreState { score: 90 };
        assert_eq!(verifier.verify(&state), Verdict::Met);
    }

    #[test]
    fn test_always_met() {
        let verifier = AlwaysMet;
        let state = ScoreState { score: 0 };
        assert_eq!(verifier.verify(&state), Verdict::Met);
    }

    #[test]
    fn test_fn_verifier() {
        let verifier = FnVerifier::new(|s: &ScoreState| {
            if s.score >= 75 {
                Verdict::Met
            } else {
                Verdict::NotMet
            }
        });
        assert_eq!(verifier.verify(&ScoreState { score: 80 }), Verdict::Met);
        assert_eq!(verifier.verify(&ScoreState { score: 70 }), Verdict::NotMet);
    }

    #[test]
    fn test_verdict_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<Verdict>();
        assert_sync::<Verdict>();
    }
}
