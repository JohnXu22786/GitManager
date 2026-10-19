//! Exact existing-meter controls for the factory-local policy context.
use super::*;
use super::policy_reference_tests::old_preparation;

const MAX: u64 = 10_000_000;
const SENTINEL: &str = "policy context test callback boundary";
const FUEL: &str = "Common factual admission: budget: cumulative proof/initialization budget exhausted";
const CANCEL: &str = "Common factual admission: budget: cancelled or cumulative elapsed budget exhausted";
const FOREIGN: &str = "Common factual admission: owned persisted proof: source is not a checked owned recipe";

impl OwnedPolicyCheck<'_> {
    pub(super) fn remaining_units_for_test(&self) -> u64 {
        self.meter.remaining_units_for_test()
    }
}
fn invalid(reason: &str) -> StoreError { StoreError::Invalid(reason.into()) }
fn assert_invalid<T>(result: &Result<T>, reason: &str) {
    assert!(matches!(result, Err(StoreError::Invalid(found)) if found == reason), "expected exact typed Invalid: {reason}");
}
fn token() -> Arc<AtomicBool> { Arc::new(AtomicBool::new(false)) }
fn budget(units: u64) -> ProofBudget { ProofBudget { units, elapsed_millis: 60_000 } }
fn selected<F>(
    current: &ProjectSnapshot, layer: &Digest, operation: &str, units: u64,
    primary: Arc<AtomicBool>, secondary: Option<Arc<AtomicBool>>, check: F,
) -> Result<PreparedOwnedTransitionV2>
where F: for<'gate> FnOnce(&mut OwnedPolicyCheck<'gate>) -> Result<()>,
{
    prepare_owned_withdrawal(current, layer, operation, budget(units), primary, secondary, check)
}
fn old_prefix(current: &ProjectSnapshot, layer: &Digest, operation: &str) -> u64 {
    let mut observed = None;
    let result = old_preparation(current, layer, operation, budget(MAX), token(), None, |execution, meter| {
        assert_eq!(execution.current(), current);
        observed = Some(meter.remaining_units_for_test());
        Err(invalid(SENTINEL))
    });
    assert_invalid(&result, SENTINEL);
    MAX - observed.expect("the genuine old callback must be reached")
}

impl ProductStore {
    pub(crate) fn assert_owned_policy_context_exact_costs_for_test(&self) {
        let current = self.load().unwrap();
        let layer = current.owned_scope().unwrap().layers.keys().next().unwrap();
        let operation = "context-fixed-cost";
        let spent = old_prefix(&current, layer, operation);
        assert!(spent + 8 < MAX);
        let mut reached = false;
        let result = selected(&current, layer, operation, MAX, token(), None, |check| {
            reached = true;
            assert_eq!(MAX - check.remaining_units_for_test(), spent + 8);
            let before = check.remaining_units_for_test();
            let execution = check.execution()?;
            assert_eq!(before - check.remaining_units_for_test(), 3);
            let retained_current = execution.current();
            check.charge(7)?;
            assert_eq!(before - check.remaining_units_for_test(), 10);
            assert_eq!(retained_current, &current); // Borrow survives mutable context use.
            let mut foreign = current.program()?.clone();
            foreign.binding.program_path.push_str("-foreign-policy-context");
            let before = check.remaining_units_for_test();
            let failure = check.surface(&foreign);
            assert_invalid(&failure, FOREIGN);
            assert_eq!(before - check.remaining_units_for_test(), 5);
            Err(invalid(SENTINEL))
        });
        assert!(reached);
        assert_invalid(&result, SENTINEL);
        assert_eq!(self.load().unwrap(), current);

        let prepared = selected(&current, layer, "context-fallback", MAX, token(), None, |_| Ok(())).unwrap();
        let mut meter = ProofMeter::new(budget(MAX), token()).unwrap();
        let before = meter.remaining_units_for_test();
        let mut reached = false;
        let result = prepared.check_policy(|check| {
            reached = true;
            assert_eq!(before - check.remaining_units_for_test(), 6);
            let execution = check.execution()?;
            assert_eq!(before - check.remaining_units_for_test(), 9);
            check.charge(11)?;
            assert_eq!(before - check.remaining_units_for_test(), 20);
            assert_eq!(execution.current(), &current);
            Err::<(), _>(invalid(SENTINEL))
        }, &mut meter);
        assert!(reached);
        assert_invalid(&result, SENTINEL);
        assert_eq!(before - meter.remaining_units_for_test(), 20);
        assert_eq!(self.load().unwrap(), current);
    }

    pub(crate) fn assert_owned_policy_context_constructor_fuel_for_test(&self) {
        let current = self.load().unwrap();
        let layer = current.owned_scope().unwrap().layers.keys().next().unwrap();
        let operation = "context-entry-fuel";
        let spent = old_prefix(&current, layer, operation);
        assert!(spent + 8 <= MAX);
        for available in [7, 8] {
            let mut reached = false;
            let result = selected(&current, layer, operation, spent + available, token(), None, |check| {
                reached = true;
                assert_eq!(check.remaining_units_for_test(), 0);
                Err(invalid(SENTINEL))
            });
            assert_eq!(reached, available == 8);
            assert_invalid(&result, if available == 8 { SENTINEL } else { FUEL });
            assert_eq!(self.load().unwrap(), current);
        }
        let prepared = selected(&current, layer, "fallback-entry-fuel", MAX, token(), None, |_| Ok(())).unwrap();
        for available in [5, 6] {
            let mut meter = ProofMeter::new(budget(available + 1), token()).unwrap();
            let mut reached = false;
            let result = prepared.check_policy(|check| {
                reached = true;
                assert_eq!(check.remaining_units_for_test(), 0);
                Err::<(), _>(invalid(SENTINEL))
            }, &mut meter);
            assert_eq!(reached, available == 6);
            assert_invalid(&result, if available == 6 { SENTINEL } else { FUEL });
            assert_eq!(self.load().unwrap(), current);
        }
    }

    pub(crate) fn assert_owned_policy_context_actual_refusals_for_test(&self) {
        let current = self.load().unwrap();
        let layer = current.owned_scope().unwrap().layers.keys().next().unwrap();
        let mut foreign = current.program().unwrap().clone();
        foreign.binding.program_path.push_str("-foreign-policy-context");
        for (method, available) in [("execution", 2), ("surface", 4), ("forward", 0)] {
            let mut reached = false;
            let result = selected(&current, layer, "context-method-fuel", MAX, token(), None, |check| {
                reached = true;
                let left = check.remaining_units_for_test();
                assert!(left >= available);
                check.charge(left - available)?; // Real gate debit only; never raise/refund.
                assert_eq!(check.remaining_units_for_test(), available);
                let failure = match method {
                    "execution" => check.execution().map(|_| ()),
                    "surface" => check.surface(&foreign).map(|_| ()),
                    "forward" => check.charge(1),
                    _ => unreachable!(),
                };
                assert_invalid(&failure, FUEL); // Must fail here, before finish.
                assert_eq!(check.remaining_units_for_test(), available);
                failure
            });
            assert!(reached);
            assert_invalid(&result, FUEL);
            assert_eq!(self.load().unwrap(), current);
        }
        for primary_fails in [true, false] {
            for method in ["execution", "surface"] {
                let primary = token();
                let secondary = token();
                let fail = if primary_fails { primary.clone() } else { secondary.clone() };
                let mut reached = false;
                let result = selected(&current, layer, "context-method-cancel", MAX, primary, Some(secondary), |check| {
                    reached = true;
                    check.charge(0)?; // Both tokens/deadline are still admitted before the test action.
                    let before = check.remaining_units_for_test();
                    assert!(before >= 5);
                    assert!(!fail.load(Ordering::Acquire));
                    fail.store(true, Ordering::Release);
                    let failure = if method == "execution" {
                        check.execution().map(|_| ())
                    } else {
                        check.surface(&foreign).map(|_| ())
                    };
                    assert_invalid(&failure, CANCEL); // Immediate typed callback cause.
                    assert_eq!(check.remaining_units_for_test(), before);
                    failure
                });
                assert!(reached);
                assert!(fail.load(Ordering::Acquire));
                assert_invalid(&result, CANCEL);
                assert_eq!(self.load().unwrap(), current);
            }
        }
    }
}

#[test]
fn policy_context_api_keeps_internal_borrow_lifetimes() {
    let _: for<'gate, 'borrow> fn(&'borrow mut OwnedPolicyCheck<'gate>) -> Result<&'gate VerifiedOwnedTransitionExecution> = |check| check.execution();
    let _: for<'gate, 'borrow, 'source> fn(&'borrow mut OwnedPolicyCheck<'gate>, &'source CapturedProgram) -> Result<owned_continuation::VerifiedOwnedPolicySurface> = |check, source| check.surface(source);
    let _: for<'gate, 'borrow> fn(&'borrow mut OwnedPolicyCheck<'gate>, u64) -> Result<()> = |check, units| check.charge(units);
}

impl ProductStore {
    // Only a structurally validated plan is needed to reach this private writer
    // callback. No policy evidence is invented; every callback returns an error
    // before next.validate or persistence and no preparation escapes this helper.
    fn assert_locked_adoption_policy_callback_for_test(
        &self,
        operation: &str,
        primary: Arc<AtomicBool>,
        secondary: Arc<AtomicBool>,
        expected_error: &str,
        verify: impl for<'gate> FnOnce(&mut OwnedPolicyCheck<'gate>) -> Result<()>,
    ) {
        let current = self.load().unwrap();
        let pointer = self.root.read_and_sync("CURRENT", MAX_STORE_BYTES).unwrap();
        let state = current.owned_scope().unwrap();
        assert_eq!(state.layers.len(), 1);
        let (layer_id, layer) = state.layers.iter().next().unwrap();
        let prepared = selected(
            &current, layer_id, operation, MAX, primary, Some(secondary.clone()), |_| Ok(()),
        ).unwrap();
        let original = current.adoptions.iter().find(|a| a.plan.id == layer.operation).unwrap();
        let receipt = state.adoptions.iter().find(|a| a.revision == original.revision).unwrap();
        let mut retire: Vec<_> = receipt.decisions.iter().filter(|id| {
            current.decisions.decisions.iter().any(|d| &d.id == *id && d.status == DecisionStatus::Active)
        }).cloned().collect();
        retire.sort();
        assert!(!retire.is_empty());
        let mut graph = current.decisions.clone();
        graph.revision = graph.revision.checked_add(1).unwrap();
        for decision in &mut graph.decisions {
            if retire.contains(&decision.id) {
                decision.revision = decision.revision.checked_add(1).unwrap();
                decision.status = DecisionStatus::Withdrawn { adoption: operation.into() };
            }
        }
        let plan = AdoptionPlan {
            version: CONTRACT_VERSION,
            id: operation.into(),
            project_id: current.data.project_id.clone(),
            expected_generation: current.data.generation,
            expected_data: current.data.identity().unwrap(),
            expected_decisions: current.decisions.identity().unwrap(),
            expected_session: current.session.identity().unwrap(),
            current_source: current.program().unwrap().binding.clone(),
            target: prepared.target().artifact.clone(),
            scope: prepared.scope(),
            compatibility: prepared.compatibility.clone(),
            required_decisions: graph.decisions.iter()
                .filter(|d| d.status == DecisionStatus::Active).map(|d| d.id.clone()).collect(),
            checks: vec![],
            evidence: vec![],
            retire_decisions: retire,
        };
        let adoption = self.prepare_owned_transition_adoption(plan, &prepared, &graph).unwrap();
        let mut reached = false;
        let result = self.adopt_owned_transition_verified(
            current.revision, &prepared, &adoption, &graph, Some(secondary),
            |locked_current, fresh, _, _, _, check| {
                reached = true;
                assert_eq!(locked_current, &current);
                assert_eq!(fresh.basis(), locked_current);
                assert!(!Arc::ptr_eq(&fresh.execution, &prepared.execution));
                assert!(std::ptr::eq(check.execution, fresh.execution.as_ref()));
                let registry = check.registry.expect("locked adoption must retain its regenerated registry");
                registry.check_snapshot(locked_current).unwrap();
                registry.check_snapshot(fresh.basis()).unwrap();
                let recipe = registry.resolve(locked_current.program().unwrap()).unwrap();
                assert!(matches!(recipe, owned_persist::OwnedExecutionRecipe::Initial(_)));
                assert!(std::ptr::eq(recipe.initial(), fresh.execution.original.as_ref()));
                verify(check)
            },
            |_| panic!("new operation must reach the locked callback, not retry"),
            None,
        );
        assert!(reached, "the actual locked adoption callback must be reached");
        assert_invalid(&result, expected_error);
        assert_eq!(self.root.read_and_sync("CURRENT", MAX_STORE_BYTES).unwrap(), pointer);
        assert_eq!(self.load().unwrap(), current);
    }

    pub(crate) fn assert_owned_adoption_policy_context_exact_dispatch_for_test(&self) {
        let current = self.load().unwrap();
        let layer = current.owned_scope().unwrap().layers.keys().next().unwrap();
        let operation = "adoption-context-dispatch";
        let spent = old_prefix(&current, layer, operation);
        let (old_before, old_after, old_sources) =
            owned_persist::effective_pair(&current, &current.active_revision).unwrap();
        self.assert_locked_adoption_policy_callback_for_test(
            operation, token(), token(), SENTINEL, |check| {
                // Frozen old prefix + unchanged finish1 + retained owner2 + selected context8.
                assert_eq!(MAX - check.remaining_units_for_test(), spent + 11);
                let before = check.remaining_units_for_test();
                let execution = check.execution()?;
                assert_eq!(before - check.remaining_units_for_test(), 3);
                assert_eq!(execution.current(), &current);
                assert_eq!(execution.manifest.operation, operation);
                assert_eq!(execution.continuation.original_layer(), layer);
                assert_eq!(execution.continuation.before(), old_before);
                assert_eq!(execution.continuation.after(), old_after);
                assert_eq!(&execution.continuation.request().inventory.sources, old_sources);
                let mut foreign = current.program()?.clone();
                foreign.binding.program_path.push_str("-foreign-adoption-policy-context");
                let before = check.remaining_units_for_test();
                let failure = check.surface(&foreign);
                assert_invalid(&failure, FOREIGN);
                assert_eq!(before - check.remaining_units_for_test(), 5);
                assert_eq!(execution.current(), &current); // Execution borrow survives dispatch.
                Err(invalid(SENTINEL))
            },
        );
    }

    pub(crate) fn assert_owned_adoption_policy_context_actual_refusals_for_test(&self) {
        for mode in ["fuel", "primary", "secondary"] {
            let primary = token();
            let secondary = token();
            let fail = if mode == "primary" { primary.clone() } else { secondary.clone() };
            let expected_error = if mode == "fuel" { FUEL } else { CANCEL };
            self.assert_locked_adoption_policy_callback_for_test(
                "adoption-context-refusal", primary, secondary, expected_error, |check| {
                    let mut foreign = check.execution()?.current().program()?.clone();
                    foreign.binding.program_path.push_str("-foreign-adoption-policy-context");
                    check.charge(0)?; // Both actual adoption tokens/deadline are admitted first.
                    if mode == "fuel" {
                        let left = check.remaining_units_for_test();
                        assert!(left >= 4);
                        check.charge(left - 4)?; // Consume only the real adoption gate; never refund.
                        assert_eq!(check.remaining_units_for_test(), 4);
                    } else {
                        assert!(!fail.load(Ordering::Acquire));
                        fail.store(true, Ordering::Release);
                    }
                    let before = check.remaining_units_for_test();
                    let failure = check.surface(&foreign).map(|_| ());
                    assert_invalid(&failure, expected_error); // Immediate surface cause, not finish.
                    assert_eq!(check.remaining_units_for_test(), before);
                    failure
                },
            );
            if mode != "fuel" { assert!(fail.load(Ordering::Acquire)); }
        }
    }
}
