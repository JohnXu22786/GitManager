//! Finite current-scene assertion bridges; no production authority constructor.
use super::*;
use crate::product_store::scope::PreparedOwnedTransitionV2;

const COMMITTED: &str = "retained replay context does not match the committed snapshot";
fn refused(result: Result<AcceptedScene>) {
    assert!(matches!(result, Err(DecisionError::Invalid(ref text)) if text == COMMITTED));
}

pub(crate) enum CurrentScenePendingForTest {
    Scalar(PreparedScopedChange),
    Rehearsal(PreparedScopedChange),
    Owned(PreparedOwnedScopedChangeV2),
    Transition(PreparedOwnedTransitionV2),
    Correspondences(BTreeMap<Digest, ScopeCorrespondence>),
}

impl<R: RuntimeAdapter> DecisionEngine<R> {
    /// The case contains genuine preparations/proofs from production APIs.
    /// Each separately named integration test reaches exactly one boundary.
    pub(crate) fn assert_current_scene_pending_mode_for_test(
        &self,
        current: &ProjectSnapshot,
        scenario: &ScenarioSpec,
        pending: CurrentScenePendingForTest,
    ) {
        let capture = || self.accept_current_scene(current, scenario, Disclosure::Synthetic);
        match pending {
            CurrentScenePendingForTest::Scalar(prepared) => {
                self.with_scoped(Some(prepared), || { refused(capture()); Ok(()) }).unwrap();
            }
            CurrentScenePendingForTest::Rehearsal(prepared) => {
                self.with_rehearsal(Some((vec![prepared], vec![])), || { refused(capture()); Ok(()) }).unwrap();
            }
            CurrentScenePendingForTest::Owned(prepared) => {
                self.with_owned(OwnedDecisionContext {
                    prepared, mode: OwnedChoiceMode::Activate, next: None,
                }, || { refused(capture()); Ok(()) }).unwrap();
            }
            CurrentScenePendingForTest::Transition(prepared) => {
                assert!(self.pending_owned_transition.borrow().is_none());
                self.pending_owned_transition.replace(Some(OwnedTransitionDecisionContext {
                    prepared, next: None, current_scene: true,
                }));
                let result = capture();
                self.pending_owned_transition.replace(None);
                self.clear_admission_cache();
                refused(result);
            }
            CurrentScenePendingForTest::Correspondences(proofs) => {
                assert!(!proofs.is_empty());
                self.with_correspondences(proofs, || { refused(capture()); Ok(()) }).unwrap();
            }
        }
        assert!(self.pending_scope.borrow().is_none());
        assert!(self.pending_owned.borrow().is_none());
        assert!(self.pending_owned_transition.borrow().is_none());
        assert!(self.pending_rehearsal.borrow().is_none());
        assert!(self.pending_correspondences.borrow().is_empty());
    }

    /// Native LocalRuntime is the independent low-fuel oracle. The public route
    /// must preserve its existing incomplete-evidence error, including when a
    /// separate live write occurred during the runtime callback.
    pub(crate) fn assert_current_scene_low_fuel_for_test(
        mut self,
        current: &ProjectSnapshot,
        scenario: &ScenarioSpec,
    ) {
        self.limits.fuel = 1;
        let context = ScopedExecutionContext::committed(current).unwrap();
        let (native, _) = execute_admitted(
            &LocalRuntime::default(), current.program().unwrap(), scenario,
            &current.decisions, self.limits.clone(), "accepted", Some(&context),
        ).unwrap();
        assert_eq!(native.state, EvidenceState::Inconclusive);
        assert_eq!(native.errors, vec!["BudgetExhausted(\"evaluation fuel exhausted\")"]);
        assert!(native.trace.is_empty());
        assert!(native.observations.is_empty());
        let result = self.accept_current_scene(current, scenario, Disclosure::Synthetic);
        assert!(matches!(result, Err(DecisionError::Invalid(ref text))
            if text == "accepted scene must execute completely"));
    }
}
