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
