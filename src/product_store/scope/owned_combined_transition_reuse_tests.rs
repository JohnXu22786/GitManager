//! Bounded accounting and exact-object reuse controls for reconstructed transitions.
use super::*;

#[test]
fn combined_transition_reuse_payment_boundaries() {
    let payments = [
        owned_continuation::RECONSTRUCTED_STRUCTURAL_ENTRY_PAYMENT,
        RECONSTRUCTED_ORIGIN_PAYMENT,
        HISTORICAL_TRANSITION_FIRST_SELECT_PAYMENT,
        HISTORICAL_TRANSITION_FIRST_MATCH_PAYMENT,
        HISTORICAL_TRANSITION_HIT_PAYMENT,
    ];
    for payment in payments {
        assert!(payment > 0);
        for exact in [false, true] {
            let units = payment - u64::from(!exact);
            let mut meter = ProofMeter::new(ProofBudget { units: units + 1, elapsed_millis: 60_000 },
                Arc::new(AtomicBool::new(false))).unwrap();
            let result = meter.charge(payment);
            if exact {
                result.unwrap();
                assert_eq!(meter.remaining_units_for_test(), 0);
            } else {
                assert!(matches!(result, Err(StoreError::Invalid(text)) if text == "Common factual admission: budget: cumulative proof/initialization budget exhausted"));
                assert_eq!(meter.remaining_units_for_test(), units);
            }
        }
    }
    // The original live-constructor floor is unchanged; only five full lexical
    // continuation transport slots disappear from reconstructed constructors.
    assert_eq!(owned_continuation::STRUCTURAL_ENTRY_PAYMENT
        - owned_continuation::RECONSTRUCTED_STRUCTURAL_ENTRY_PAYMENT,
        5 * 4 * size_of::<owned_continuation::VerifiedOwnedContinuation>() as u64);
    assert!(std::panic::catch_unwind(|| comparison_payment(u64::MAX, [(1, 1)])).is_err());
    assert!(std::panic::catch_unwind(|| comparison_payment(0, [(2, u64::MAX)])).is_err());
    println!("COMBINED_TRANSITION_PAYMENTS {payments:?}");
}

use std::cell::RefCell;
#[derive(Default)]
struct Observation {
    enabled: bool,
    accounting: bool,
    boundary: &'static str,
    boundary_exact: bool,
    boundary_seen: usize,
    boundary_entered: usize,
    boundary_next: Option<(&'static str, u64)>,
    boundary_hit: bool,
    cancel_at: &'static str,
    ordinary: Vec<(bool, ScopeTransition)>,
    hits: usize,
    misses: usize,
    negatives_checked: bool,
    selection_start: u64,
    selection_cost: u64,
    refcount: usize,
}
thread_local! { static OBSERVATION: RefCell<Observation> = RefCell::new(Observation::default()); }
struct Reset;
impl Drop for Reset {
    fn drop(&mut self) { OBSERVATION.with(|s| *s.borrow_mut() = Observation::default()); }
}
// Test-only paid depletion at the actual boundary, never a refill or new cap.
// The ordinary production charge decides whether payload entry is allowed.
pub(crate) fn boundary_before(stage: &'static str, payment: u64, meter: &mut ProofMeter) {
    boundary_next(stage, payment, meter);
    let available = OBSERVATION.with(|s| {
        let mut s = s.borrow_mut();
        if s.boundary != stage || s.boundary_seen != 0 { return None; }
        s.boundary_seen += 1;
        Some(payment - u64::from(!s.boundary_exact))
    });
    if let Some(available) = available {
        let remaining = meter.remaining_units_for_test();
        assert!(remaining >= available);
        meter.charge(remaining - available).unwrap();
    }
}
pub(crate) fn boundary_after(stage: &'static str, meter: &ProofMeter) {
    OBSERVATION.with(|s| {
        let mut s = s.borrow_mut();
        if s.boundary == stage && s.boundary_seen == 1 {
            assert!(s.boundary_exact);
            assert_eq!(meter.remaining_units_for_test(), 0);
            s.boundary_entered += 1;
        }
    });
}
pub(crate) fn boundary_next(stage: &'static str, payment: u64, meter: &ProofMeter) {
    OBSERVATION.with(|s| {
        let mut s = s.borrow_mut();
        if s.boundary_entered == 1 && s.boundary_next.is_none() && s.boundary != stage {
            assert!(payment > 0);
            assert_eq!(meter.remaining_units_for_test(), 0);
            s.boundary_next = Some((stage, payment));
        }
    });
}
pub(crate) fn ordinary(fresh: bool, kind: ScopeTransition) {
    OBSERVATION.with(|s| { let mut s = s.borrow_mut(); if s.enabled { s.ordinary.push((fresh, kind)); } });
}
pub(crate) fn poll(stage: &str, meter: &ProofMeter) {
    if OBSERVATION.with(|s| s.borrow().cancel_at == stage) {
        meter.cancelled().store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
fn meter(millis: u64) -> ProofMeter {
    ProofMeter::new(ProofBudget { elapsed_millis: millis, ..ProofBudget::default() },
        Arc::new(AtomicBool::new(false))).unwrap()
}
fn copy_registry(r: &ReconstructedOwned) -> ReconstructedOwned {
    ReconstructedOwned { snapshot: r.snapshot.clone(), recipes: r.recipes.clone(),
        epochs: r.epochs.clone(), proofs: r.proofs.clone(), manifests: r.manifests.clone(),
        initializations: r.initializations.clone() }
}
pub(crate) fn selected(loan: &HistoricalInitialLoan<'_>, inner: &ReconstructedOwned,
    manifest: &OwnedCompositionV2, completed: Option<&CompletedInitialOrigin<'_, '_>>,
    donor: Option<&Arc<VerifiedOwnedTransitionExecution>>,
    live_meter: &ProofMeter,
) {
    let run = OBSERVATION.with(|s| {
        let mut s = s.borrow_mut();
        if !s.enabled { return false; }
        s.selection_cost = s.selection_start - live_meter.remaining_units_for_test();
        if let Some(donor) = donor { s.hits += 1; s.refcount = Arc::strong_count(donor); }
        else { s.misses += 1; }
        if s.negatives_checked || donor.is_none() { return false; }
        s.negatives_checked = true;
        true
    });
    if !run { return; }
    let donor = donor.unwrap();
    assert!(std::ptr::eq(donor, match &loan.registry.recipes[&manifest.output] {
        OwnedExecutionRecipe::Transition(value) => value, _ => panic!("actual committed donor"),
    }));
    assert_eq!(donor.manifest, *manifest);
    // False live window and both actual registered cancellation tokens are
    // refusals/misses on genuine carriers, never synthetic positive authority.
    let mut clipped = meter(RuntimeLimits::default().elapsed_millis - 1);
    assert!(loan.transition_donor(inner, manifest, completed, &mut clipped).unwrap().is_none());
    for secondary in [false, true] {
        let token = Arc::new(AtomicBool::new(false));
        let mut cancelled = ProofMeter::new(ProofBudget::default(), token.clone()).unwrap();
        if secondary {
            let primary = Arc::new(AtomicBool::new(false));
            cancelled = ProofMeter::new(ProofBudget::default(), primary).unwrap();
            cancelled.watch_cancellation(&token);
        }
        token.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(matches!(loan.transition_donor(inner, manifest, completed, &mut cancelled),
            Err(StoreError::Invalid(text)) if text ==
            "Common factual admission: budget: cancelled or cumulative elapsed budget exhausted"));
    }
    for exact in [false, true] {
        let available = HISTORICAL_TRANSITION_FIRST_MATCH_PAYMENT - u64::from(!exact);
        let mut bounded = ProofMeter::new(ProofBudget { units: available + 1,
            ..ProofBudget::default() }, Arc::new(AtomicBool::new(false))).unwrap();
        let before = Arc::strong_count(donor);
        let result = loan.transition_donor(inner, manifest, completed, &mut bounded);
        if exact {
            assert!(std::ptr::eq(result.unwrap().unwrap(), donor));
            assert_eq!(bounded.remaining_units_for_test(), 0);
        } else {
            assert!(matches!(result, Err(StoreError::Invalid(text)) if text ==
                "Common factual admission: budget: cumulative proof/initialization budget exhausted"));
            assert_eq!(bounded.remaining_units_for_test(), available);
        }
        assert_eq!(Arc::strong_count(donor), before, "selection must only borrow");
    }
    // A separately regenerated genuine Initial is value-equivalent but not the
    // exact Arc. Only negative tests copy the registry; no copy mints a hit.
    OBSERVATION.with(|s| s.borrow_mut().enabled = false);
    let native = reconstruct(loan.execution.current(), &mut meter(60_000)).unwrap();
    OBSERVATION.with(|s| s.borrow_mut().enabled = true);
    let first = &inner.epochs[0].output;
    let native_initial = match &native.recipes[first] { OwnedExecutionRecipe::Initial(p) => p, _ => panic!() };
    assert!(!Arc::ptr_eq(&donor.original, native_initial));
    assert_eq!(donor.original.target, native_initial.target);
    let native_origin = CompletedInitialOrigin { retained: loan.execution.current(), original: native_initial };
    assert!(loan.transition_donor(inner, manifest, Some(&native_origin), &mut meter(60_000)).unwrap().is_none());
    assert!(loan.transition_donor(inner, manifest, None, &mut meter(60_000)).unwrap().is_none());
    let mut later = copy_registry(inner);
    later.epochs.push(OwnedProducerEpoch { output: manifest.output.clone(), adoption_revision: 0 });
    assert!(loan.transition_donor(&later, manifest, completed, &mut meter(60_000)).unwrap().is_none());
    let mut missing_donor = copy_registry(loan.registry);
    missing_donor.recipes.remove(&manifest.output);
    let missing_loan = HistoricalInitialLoan { registry: &missing_donor, execution: loan.execution };
    assert!(missing_loan.transition_donor(inner, manifest, completed, &mut meter(60_000)).unwrap().is_none());
    let mut wrong_donor = copy_registry(loan.registry);
    wrong_donor.recipes.insert(manifest.output.clone(), OwnedExecutionRecipe::Initial(donor.original.clone()));
    let wrong_loan = HistoricalInitialLoan { registry: &wrong_donor, execution: loan.execution };
    assert!(wrong_loan.transition_donor(inner, manifest, completed, &mut meter(60_000)).unwrap().is_none());
    for index in [0, 1] {
        let mut changed = copy_registry(loan.registry);
        changed.epochs[index].output = Digest::try_from("0".repeat(64)).unwrap();
        let changed_loan = HistoricalInitialLoan { registry: &changed, execution: loan.execution };
        assert!(changed_loan.transition_donor(inner, manifest, completed, &mut meter(60_000)).unwrap().is_none());
    }
    let mut wrong_previous = manifest.clone();
    wrong_previous.previous = Some(Digest::try_from("0".repeat(64)).unwrap());
    assert!(loan.transition_donor(inner, &wrong_previous, completed, &mut meter(60_000)).unwrap().is_none());
    let mut wrong_output = manifest.clone();
    wrong_output.output = Digest::try_from("0".repeat(64)).unwrap();
    assert!(loan.transition_donor(inner, &wrong_output, completed, &mut meter(60_000)).unwrap().is_none());
}
pub(crate) fn accounting(stage: &str, meter: &ProofMeter) {
    if OBSERVATION.with(|s| s.borrow().accounting) {
        println!("COMBINED_METER stage={stage} left={}", meter.remaining_units_for_test());
    }
}
pub(crate) fn start_selection(meter: &ProofMeter) {
    accounting("before-selector", meter);
    poll("select", meter);
    OBSERVATION.with(|s| s.borrow_mut().selection_start = meter.remaining_units_for_test());
}
pub(crate) fn force_native(donor: Option<&Arc<VerifiedOwnedTransitionExecution>>)
    -> Option<&Arc<VerifiedOwnedTransitionExecution>> {
    if OBSERVATION.with(|s| s.borrow().cancel_at == "native-miss") { None } else { donor }
}
pub(crate) fn hit(donor: &Arc<VerifiedOwnedTransitionExecution>, recipe: &OwnedExecutionRecipe,
    meter: &ProofMeter) {
    accounting("after-hit", meter);
    OBSERVATION.with(|s| {
        let mut s = s.borrow_mut();
        if s.boundary == "hit" && s.boundary_entered == 1 {
            let OwnedExecutionRecipe::Transition(actual) = recipe else { panic!("hit recipe"); };
            assert!(Arc::ptr_eq(donor, actual));
            assert_eq!(meter.remaining_units_for_test(), 0);
            s.boundary_hit = true;
        }
    });
    OBSERVATION.with(|s| {
        let s = s.borrow();
        if !s.enabled { return; }
        let OwnedExecutionRecipe::Transition(actual) = recipe else { panic!("Transition join"); };
        assert!(Arc::ptr_eq(donor, actual));
        assert_eq!(Arc::strong_count(donor), s.refcount + 1);
        let total = s.selection_start - meter.remaining_units_for_test();
        assert_eq!(total - s.selection_cost, HISTORICAL_TRANSITION_HIT_PAYMENT);
        println!("COMBINED_ACTUAL_A select={} hit={} total={total}", s.selection_cost, HISTORICAL_TRANSITION_HIT_PAYMENT);
    });
}
impl ProjectSnapshot {
    pub(crate) fn with_combined_accounting_for_test<T>(action: impl FnOnce() -> T) -> T {
        let _reset = Reset;
        OBSERVATION.with(|s| s.borrow_mut().accounting = true);
        action()
    }
    pub(crate) fn assert_combined_transition_reuse_for_test(&self, require_general_evolution: bool) {
        let _reset = Reset;
        OBSERVATION.with(|s| s.borrow_mut().enabled = true);
        let mut surfaces = 0;
        check_retained_transition_policies_with_initials(self, |check, loan| {
            let source = loan.execution.current().program()?;
            check.charge(HISTORICAL_INITIAL_SURFACE_PAYMENT)?;
            let surface = check.surface_with_initial_loan(loan, source)?;
            assert_eq!(surface.source(), &revision(source)?);
            surfaces += 1;
            Ok(())
        }).unwrap();
        assert_eq!(surfaces, 1);
        OBSERVATION.with(|s| {
            let s = s.borrow();
            assert!(s.ordinary.contains(&(true, ScopeTransition::Evolution)));
            assert!(s.ordinary.contains(&(false, ScopeTransition::Withdrawal)));
            if require_general_evolution { assert!(s.ordinary.contains(&(false, ScopeTransition::Evolution))); }
            assert!(s.hits > 0);
            assert!(s.negatives_checked);
            println!("COMBINED_GENUINE hits={} misses={} ordinary={:?}", s.hits, s.misses, s.ordinary);
        });
        OBSERVATION.with(|s| { let mut s = s.borrow_mut(); s.cancel_at = "native-miss"; });
        check_retained_transition_policies_with_initials(self, |check, loan| {
            check.charge(HISTORICAL_INITIAL_SURFACE_PAYMENT)?;
            check.surface_with_initial_loan(loan, loan.execution.current().program()?).map(|_| ())
        }).unwrap();
        OBSERVATION.with(|s| { assert!(s.borrow().misses > 0); });
        println!("COMBINED_NATIVE_FALLBACK completed=true");
        for stage in ["closed", "before-lower", "select", "pre-hit", "post-hit"] {
            OBSERVATION.with(|s| { let mut s = s.borrow_mut(); s.enabled = false; s.cancel_at = stage; });
            let result = check_retained_transition_policies_with_initials(self, |check, loan| {
                check.charge(HISTORICAL_INITIAL_SURFACE_PAYMENT)?;
                check.surface_with_initial_loan(loan, loan.execution.current().program()?).map(|_| ())
            });
            assert!(matches!(result, Err(StoreError::Invalid(text)) if text ==
                "Common factual admission: budget: cancelled or cumulative elapsed budget exhausted"), "{stage}");
            println!("COMBINED_CANCELLATION stage={stage} original_error=true");
        }
        for (stage, next) in [("origin", "structural"), ("structural", "dispatch"),
            ("select", "match"), ("hit", "tail-scope")] {
            for exact in [false, true] {
                OBSERVATION.with(|s| *s.borrow_mut() = Observation {
                    boundary: stage, boundary_exact: exact, ..Observation::default()
                });
                let result = check_retained_transition_policies_with_initials(self, |check, loan| {
                    check.charge(HISTORICAL_INITIAL_SURFACE_PAYMENT)?;
                    check.surface_with_initial_loan(loan, loan.execution.current().program()?).map(|_| ())
                });
                assert!(matches!(result, Err(StoreError::Invalid(text)) if text ==
                    "Common factual admission: budget: cumulative proof/initialization budget exhausted"));
                OBSERVATION.with(|s| {
                    let s = s.borrow();
                    assert_eq!(s.boundary_seen, 1, "{stage}: genuine production boundary");
                    assert_eq!(s.boundary_entered, usize::from(exact), "{stage}: paid payload entry");
                    assert_eq!(s.boundary_next.map(|(stage, _)| stage), exact.then_some(next));
                    assert_eq!(s.boundary_hit, stage == "hit" && exact);
                    println!("COMBINED_REAL_BOUNDARY stage={stage} exact={exact} entered={} next={:?} hit={}",
                        s.boundary_entered, s.boundary_next, s.boundary_hit);
                });
            }
        }
    }
}


#[test]
fn first_transition_lookup_bound_and_key_oracle() {
    assert_eq!(MAX_ITEMS, 256, "rederive completed registry cap if changed");
    assert_eq!(HISTORICAL_TRANSITION_LOOKUP_NATIVE_PAYMENT, 6_012);
    assert_eq!(HISTORICAL_TRANSITION_GUARD_BYTES_PAYMENT, 399);
    for len in [0, 1, 10, 11, 70, 71, 430, 431, 512] {
        let map: BTreeMap<Digest, usize> = (0..len).map(|i| {
            (Digest::try_from(format!("{i:064x}")).unwrap(), i)
        }).collect();
        for index in [0, len.saturating_sub(1), len, len + 1] {
            let key = Digest::try_from(format!("{index:064x}")).unwrap();
            assert_eq!(map.get(&key).copied(), map.iter().find(|(k, _)| *k == &key).map(|(_, v)| *v));
        }
        let mut quotient = (len + 1) / 2;
        let mut levels = 1u64;
        while quotient >= 6 { quotient /= 6; levels += 1; }
        let native = 4 + levels * (6 + 11 * (3 + 133));
        assert!(native <= HISTORICAL_TRANSITION_LOOKUP_NATIVE_PAYMENT);
    }
    assert!(Digest::try_from("0".repeat(63)).is_err());
    assert!(Digest::try_from("0".repeat(65)).is_err());
    assert!(Digest::try_from("G".repeat(64)).is_err());
    for payment in [HISTORICAL_TRANSITION_FIRST_SELECT_PAYMENT,
        HISTORICAL_TRANSITION_FIRST_MATCH_PAYMENT] {
        for exact in [false, true] {
            let available = payment - u64::from(!exact);
            let mut meter = ProofMeter::new(ProofBudget { units: available + 1, elapsed_millis: 60_000 },
                Arc::new(AtomicBool::new(false))).unwrap();
            assert_eq!(meter.charge(payment).is_ok(), exact);
            assert_eq!(meter.remaining_units_for_test(), if exact { 0 } else { available });
        }
    }
}
