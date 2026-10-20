//! Assertion-only controls for the closed retained constructor and real meter.
//! No acceptance, proof, meter, inventory, observer or callback escapes this child.
use super::*;
use std::time::Duration;

// Frozen original verification plus the new positively paid None-profile path:
// full verify frame384 + replay root(4*(56+48+64+32)) + dispatch32 + ten branch2.
const OLD_VERIFY_UNITS: u64 = 600_002;
const PROFILE_VERIFY_UNITS: u64 = 384 + 800 + 32 + 20;
const VERIFY_UNITS: u64 = OLD_VERIFY_UNITS + PROFILE_VERIFY_UNITS;
// The frozen reference calls capture_frame directly; the new private suffix
// adds its complete native-sized wrapper envelope. Keep it separate from verification credit.
const SUFFIX_TRANSPORT_UNITS: u64 = FINISH_PROFILE_FRAME;
const GUARD_UNITS: u64 = 32;
const SELECTED_HANDOFF_UNITS: u64 = 5;
const SAVED_UNITS: u64 = 599_965 + PROFILE_VERIFY_UNITS - SUFFIX_TRANSPORT_UNITS;
const EXHAUSTED: &str =
    "Common factual admission: budget: cumulative proof/initialization budget exhausted";
const CANCELLED: &str =
    "Common factual admission: budget: cancelled or cumulative elapsed budget exhausted";
const REGENERATION_DIFFERS: &str =
    "Common factual admission: lifecycle acceptance: independent supporting scene regeneration differs";

fn meter() -> ProofMeter {
    ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false))).unwrap()
}
fn assert_invalid<T>(result: AdmissionResult<T>, expected: &str) -> String {
    match result {
        Err(StoreError::Invalid(message)) => {
            assert_eq!(message, expected);
            format!("{:?}", StoreError::Invalid(message))
        }
        Err(error) => panic!("wrong typed error: {error:?}; expected Invalid({expected:?})"),
        Ok(_) => panic!("unexpected output; expected Invalid({expected:?})"),
    }
}
fn assert_proof_eq(left: &VerifiedOwnedPreviewPlan, right: &VerifiedOwnedPreviewPlan) {
    assert_eq!(left.snapshot, right.snapshot);
    assert_eq!(left.candidate, right.candidate);
    assert_eq!(left.request, right.request);
    assert_eq!(left.accepted.recipe(), right.accepted.recipe());
    assert_eq!(left.report, right.report);
    assert_eq!(left.captured, right.captured);
}
fn assert_defaults(lifecycle: &OwnedLifecycleV2) {
    assert_eq!(lifecycle.evidence.len(), 2);
    for evidence in &lifecycle.evidence {
        assert_eq!(evidence.limits, RuntimeLimits::default());
    }
}
// Only a read of the original clock. Never changes started/millis or replaces a meter.
fn remaining_millis(meter: &ProofMeter) -> u64 {
    meter.millis.saturating_sub(
        meter.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
    )
}
fn old_selected(
    snapshot: &ProjectSnapshot,
    candidate: &CapturedProgram,
    request: &OwnedPreviewRequest,
    lifecycle: &OwnedLifecycleV2,
    meter: &mut ProofMeter,
) -> AdmissionResult<(AcceptedPreviewLifecycle, VerifiedOwnedPreviewPlan)> {
    let accepted = AcceptedPreviewLifecycle::reconstruct(
        snapshot, candidate, request, lifecycle, meter,
    )?;
    let proof = frozen_derive_inner_before_retained_reuse(
        snapshot, candidate, request, &accepted, meter, false,
    )?;
    Ok((accepted, proof))
}
// This edits only an untrusted serialized recipe to reach deeper negative guards.
// It never constructs an AcceptedPreviewLifecycle or asserts successful authority.
fn rebind_negative_recipe(
    snapshot: &ProjectSnapshot,
    candidate: &CapturedProgram,
    request: &OwnedPreviewRequest,
    lifecycle: &mut OwnedLifecycleV2,
) {
    lifecycle.disclosure = canonical_digest(
        IdentityDomain::Evidence,
        &(
            "preview-lifecycle-disclosure/1",
            snapshot,
            candidate,
            request,
            &lifecycle.witnesses,
            &lifecycle.evidence,
        ),
    )
    .unwrap();
    let origin = match lifecycle.origin {
        OwnedAcceptanceOriginV2::ExplicitHostUi => "explicit-host-ui",
        OwnedAcceptanceOriginV2::SyntheticOperatorFixture => "synthetic-operator-fixture",
    };
    lifecycle.event = canonical_digest(
        IdentityDomain::Evidence,
        &("host-lifecycle-acceptance/1", &lifecycle.disclosure, origin),
    )
    .unwrap();
}
fn alter_helper_limits(lifecycle: &mut OwnedLifecycleV2, record: usize, field: &str) {
    // Exhaustive destructuring makes every current field explicit, including
    // compile-time review if RuntimeLimits later acquires another field.
    let RuntimeLimits {
        fuel, collection_items, action_steps, transaction_writes, output_bytes, elapsed_millis,
    } = &mut lifecycle.evidence[record].limits;
    match field {
        "fuel" => *fuel -= 1,
        "collection_items" => *collection_items -= 1,
        "action_steps" => *action_steps -= 1,
        "transaction_writes" => *transaction_writes -= 1,
        "output_bytes" => *output_bytes -= 1,
        "elapsed_millis" => *elapsed_millis -= 1,
        _ => panic!("unknown limit field: {field}"),
    }
}

impl ProductStore {
    pub(crate) fn assert_retained_lifecycle_values_for_test(
        &self,
        candidate: &CapturedProgram,
        request: &OwnedPreviewRequest,
        accepted: &AcceptedPreviewLifecycle,
        case: &str,
    ) {
        let snapshot = self.load().unwrap();
        let current = self.root.read("CURRENT", 16 * 1024).unwrap();
        let lifecycle = accepted.recipe();
        let inputs = (snapshot.clone(), candidate.clone(), request.clone(), lifecycle.clone());
        assert_defaults(&lifecycle);
        let mut old_meter = meter();
        let mut new_meter = meter();
        assert_eq!(old_meter.remaining_units_for_test(), new_meter.remaining_units_for_test());
        let start = old_meter.remaining_units_for_test();
        if let Some(route) = case.strip_prefix("ordinary:") {
            let verify_snapshot = route == "derive";
            if route == "derive_for_decision" {
                // The old wrapper's unchanged validation has no meter charge.
                snapshot.validate().unwrap();
                assert_eq!(old_meter.remaining_units_for_test(), start);
            }
            let old = frozen_derive_inner_before_retained_reuse(
                &snapshot, candidate, request, accepted, &mut old_meter, verify_snapshot,
            ).unwrap();
            let result: AdmissionResult<VerifiedOwnedPreviewPlan> = match route {
                "derive" => derive(&snapshot, candidate, request, accepted, &mut new_meter),
                "derive_retained" => derive_retained(&snapshot, candidate, request, accepted, &mut new_meter),
                "derive_for_decision" => derive_for_decision(&snapshot, candidate, request, accepted, &mut new_meter),
                _ => panic!("unknown ordinary route: {route}"),
            };
            let new = result.unwrap();
            assert_proof_eq(&old, &new);
            assert_eq!(old.snapshot, snapshot);
            assert_eq!(old.candidate, *candidate);
            assert_eq!(old.request, *request);
            assert_eq!(old.accepted.recipe(), lifecycle);
            assert_eq!(new_meter.remaining_units_for_test() + 2 + SUFFIX_TRANSPORT_UNITS, old_meter.remaining_units_for_test());
            assert_eq!(start - new_meter.remaining_units_for_test(), start - old_meter.remaining_units_for_test() + 2 + SUFFIX_TRANSPORT_UNITS);
        } else {
            let (old_accepted, old): (AcceptedPreviewLifecycle, VerifiedOwnedPreviewPlan) =
                old_selected(&snapshot, candidate, request, &lifecycle, &mut old_meter).unwrap();
            let (new_accepted, new): (AcceptedPreviewLifecycle, VerifiedOwnedPreviewPlan) = if case == "selected" {
                let result: AdmissionResult<(AcceptedPreviewLifecycle, VerifiedOwnedPreviewPlan)> =
                    super::reconstruct_and_derive_retained(
                        &snapshot, candidate, request, &lifecycle, &mut new_meter,
                    );
                result.unwrap()
            } else {
                // Helper-only eligibility controls. The modified borrowed recipe
                // is NOT submitted to the constructor as authentic altered evidence.
                let reconstructed = AcceptedPreviewLifecycle::reconstruct(
                    &snapshot, candidate, request, &lifecycle, &mut new_meter,
                ).unwrap();
                let inventory = authenticate_inner(&snapshot, candidate, request, &mut new_meter, false).unwrap();
                let mut guard_recipe = lifecycle.clone();
                if let Some(parts) = case.strip_prefix("helper-limit:") {
                    let (record, field) = parts.split_once(':').unwrap();
                    alter_helper_limits(&mut guard_recipe, record.parse().unwrap(), field);
                } else if let Some(length) = case.strip_prefix("helper-cardinality:") {
                    match length {
                        "0" => guard_recipe.evidence.clear(),
                        "1" => guard_recipe.evidence.truncate(1),
                        "3" => guard_recipe.evidence.push(lifecycle.evidence[0].clone()),
                        _ => panic!("unknown helper cardinality: {length}"),
                    }
                } else {
                    panic!("unknown value case: {case}");
                }
                let guard_input = guard_recipe.clone();
                assert!(remaining_millis(&new_meter) >= 5_000, "helper prerequisites exhausted the default window");
                new_meter.charge(GUARD_UNITS).unwrap();
                let result: AdmissionResult<VerifiedOwnedPreviewPlan> = super::finish_reconstructed_plan(
                    &snapshot, candidate, request, &guard_recipe, &reconstructed, inventory, &mut new_meter,
                );
                let proof = result.unwrap();
                assert_eq!(guard_recipe, guard_input);
                (reconstructed, proof)
            };
            assert_eq!(old_accepted.recipe(), lifecycle);
            assert_eq!(new_accepted.recipe(), old_accepted.recipe());
            assert_proof_eq(&old, &new);
            assert_eq!(new.snapshot, snapshot);
            assert_eq!(new.candidate, *candidate);
            assert_eq!(new.request, *request);
            assert_eq!(new.accepted.recipe(), lifecycle);
            let old_left = old_meter.remaining_units_for_test();
            let new_left = new_meter.remaining_units_for_test();
            if case == "selected" {
                assert_eq!(new_left, old_left + SAVED_UNITS);
                assert_eq!(start - new_left, (start - old_left) - SAVED_UNITS);
            } else {
                // Full verification remains on both sides. The new guard32,
                // handoff5 and complete private suffix transport are positively paid.
                assert_eq!(new_left + GUARD_UNITS + SELECTED_HANDOFF_UNITS + SUFFIX_TRANSPORT_UNITS, old_left);
                assert_eq!(start - new_left, start - old_left + 37 + SUFFIX_TRANSPORT_UNITS);
            }
        }
        assert_eq!((snapshot, candidate.clone(), request.clone(), accepted.recipe()), inputs);
        assert_eq!(self.load().unwrap(), inputs.0);
        assert_eq!(self.root.read("CURRENT", 16 * 1024).unwrap(), current);
    }

    pub(crate) fn assert_retained_lifecycle_error_for_test(
        &self,
        candidate: &CapturedProgram,
        request: &OwnedPreviewRequest,
        accepted: &AcceptedPreviewLifecycle,
        case: &str,
    ) {
        let original_snapshot = self.load().unwrap();
        let current = self.root.read("CURRENT", 16 * 1024).unwrap();
        let original = (candidate.clone(), request.clone(), accepted.recipe());
        let mut snapshot = original_snapshot.clone();
        let mut candidate = candidate.clone();
        let mut request = request.clone();
        let mut lifecycle = accepted.recipe();
        let binding = "Common factual admission: lifecycle: retained disclosure/event binding differs";
        let frame = "Common factual admission: lifecycle witness: supporting scene is not the exact current frame/observation";
        let stale = "Common factual admission: lifecycle acceptance: stale source, snapshot, policy, meaning or decision basis";
        let mut rebind = false;
        let expected = match case {
            "version-before-invalid-request" => {
                lifecycle.version += 1;
                request.purpose.clear();
                "Common factual admission: lifecycle: unsupported retained acceptance version"
            }
            "disclosure" => {
                lifecycle.disclosure = canonical_digest(IdentityDomain::Evidence, &"different disclosure").unwrap();
                binding
            }
            "event" => {
                lifecycle.event = canonical_digest(IdentityDomain::Evidence, &"different event").unwrap();
                binding
            }
            "origin" => {
                lifecycle.origin = OwnedAcceptanceOriginV2::ExplicitHostUi;
                binding
            }
            "evidence-binding" => {
                lifecycle.evidence[0].limits.fuel -= 1;
                binding
            }
            "evidence-regeneration" => {
                lifecycle.evidence[0].limits.fuel -= 1;
                rebind = true;
                REGENERATION_DIFFERS
            }
            "witness-count" => {
                lifecycle.witnesses.pop();
                rebind = true;
                "Common factual admission: lifecycle witnesses: exact before and after supporting scenes required"
            }
            "witness-order" => {
                lifecycle.witnesses.swap(0, 1);
                rebind = true;
                "Common factual admission: lifecycle witnesses: exact before and after supporting scenes required"
            }
            "witness-frame" => {
                lifecycle.witnesses[0].scenario.clock_day += 1;
                rebind = true;
                frame
            }
            "witness-transition" => {
                lifecycle.witnesses[0].scenario.inputs.clear();
                rebind = true;
                frame
            }
            "witness-distinct" => {
                lifecycle.witnesses[0].unfinished = lifecycle.witnesses[0].completed.clone();
                rebind = true;
                "Common factual admission: lifecycle witness: distinct exact-root completed and unfinished witnesses required"
            }
            "witness-missing" => {
                lifecycle.witnesses[0].completed.record = "absent-control-row".into();
                rebind = true;
                "Common factual admission: lifecycle witness: active witness row missing"
            }
            "witness-interpretation" => {
                let witness = &mut lifecycle.witnesses[0];
                std::mem::swap(&mut witness.completed, &mut witness.unfinished);
                rebind = true;
                "Common factual admission: lifecycle witness: replayed completed/unfinished interpretation disagrees"
            }
            "source-binding" => {
                candidate.source_bytes.push(b' ');
                binding
            }
            "basis-binding" => {
                snapshot.clock_day += 1;
                binding
            }
            "basis-authentication" => {
                let ScopeEnvelope::ScalarV1(state) = &mut snapshot.scope else { panic!("fixture is not scalar"); };
                state.version += 1;
                rebind = true;
                "Common factual admission: basis: existing scoped lineage is unsupported"
            }
            "request-binding" => {
                request.purpose.push_str(" changed");
                binding
            }
            "request-authentication" => {
                request.purpose.clear();
                rebind = true;
                "Common factual admission: request: bounded explicit preview purpose and operations required"
            }
            "operations-authentication" => {
                let first = request.operations.iter().next().unwrap().clone();
                request.operations.remove(&first);
                rebind = true;
                "Common factual admission: operations: all source writers must be explicitly covered"
            }
            "inventory-basis-authentication" => {
                request.inventory.snapshot = canonical_digest(IdentityDomain::Data, &"different basis").unwrap();
                rebind = true;
                "Common fact inventory: stale authenticated snapshot/source basis"
            }
            ordinary if ordinary.starts_with("ordinary-purpose:") => {
                request.purpose.push_str(" changed but still valid");
                stale
            }
            ordinary if ordinary.starts_with("ordinary-source:") => {
                candidate.source_bytes.push(b' ');
                "raw source digest mismatch"
            }
            _ => panic!("unknown error case: {case}"),
        };
        if rebind {
            rebind_negative_recipe(&snapshot, &candidate, &request, &mut lifecycle);
        }
        let inputs = (snapshot.clone(), candidate.clone(), request.clone(), lifecycle.clone());
        let mut old_meter = meter();
        let mut new_meter = meter();
        let (old_error, new_error) = if case.starts_with("ordinary-") {
            let route = case.split_once(':').unwrap().1;
            if case.starts_with("ordinary-purpose:") {
                // Demonstrate that the changed purpose passes authentication;
                // the existing opaque acceptance must reject the exact stale basis.
                let mut auth_meter = meter();
                authenticate_inner(&snapshot, &candidate, &request, &mut auth_meter, route == "derive").unwrap();
            }
            if route == "derive_for_decision" { snapshot.validate().unwrap(); }
            let old: AdmissionResult<VerifiedOwnedPreviewPlan> = frozen_derive_inner_before_retained_reuse(
                &snapshot, &candidate, &request, accepted, &mut old_meter, route == "derive",
            );
            let new: AdmissionResult<VerifiedOwnedPreviewPlan> = match route {
                "derive" => derive(&snapshot, &candidate, &request, accepted, &mut new_meter),
                "derive_retained" => derive_retained(&snapshot, &candidate, &request, accepted, &mut new_meter),
                "derive_for_decision" => derive_for_decision(&snapshot, &candidate, &request, accepted, &mut new_meter),
                _ => panic!("unknown ordinary route: {route}"),
            };
            (assert_invalid(old, expected), assert_invalid(new, expected))
        } else {
            let old: AdmissionResult<(AcceptedPreviewLifecycle, VerifiedOwnedPreviewPlan)> =
                old_selected(&snapshot, &candidate, &request, &lifecycle, &mut old_meter);
            let new: AdmissionResult<(AcceptedPreviewLifecycle, VerifiedOwnedPreviewPlan)> =
                super::reconstruct_and_derive_retained(&snapshot, &candidate, &request, &lifecycle, &mut new_meter);
            (assert_invalid(old, expected), assert_invalid(new, expected))
        };
        assert_eq!(old_error, new_error, "{case}");
        // Every negative case returns before the new guard/handoff, so no added debit.
        assert_eq!(old_meter.remaining_units_for_test(), new_meter.remaining_units_for_test(), "{case}");
        assert_eq!((snapshot, candidate, request, lifecycle), inputs);
        assert_eq!(accepted.recipe(), original.2);
        assert_eq!(self.load().unwrap(), original_snapshot);
        assert_eq!(self.root.read("CURRENT", 16 * 1024).unwrap(), current);
    }

    pub(crate) fn assert_retained_lifecycle_resource_for_test(
        &self,
        candidate: &CapturedProgram,
        request: &OwnedPreviewRequest,
        accepted: &AcceptedPreviewLifecycle,
        case: &str,
    ) {
        let snapshot = self.load().unwrap();
        let current = self.root.read("CURRENT", 16 * 1024).unwrap();
        let lifecycle = accepted.recipe();
        let inputs = (snapshot.clone(), candidate.clone(), request.clone(), lifecycle.clone());
        assert_defaults(&lifecycle);
        // new() charge1, complete first verification and exact authentication bytes.
        let authentication = (candidate.source_bytes.len() + snapshot.program().unwrap().source_bytes.len()) as u64;
        let prefix = 1 + VERIFY_UNITS + authentication;
        match case {
            "first-replay-fuel" | "guard-fuel" | "handoff-fuel" => {
                let (units, left) = match case {
                    "first-replay-fuel" => (100_000, 99_999 - 384 - 800 - 32),
                    "guard-fuel" => (prefix + GUARD_UNITS - 1, GUARD_UNITS - 1),
                    _ => (prefix + GUARD_UNITS + SELECTED_HANDOFF_UNITS - 1, SELECTED_HANDOFF_UNITS - 1),
                };
                let mut meter = ProofMeter::new(
                    ProofBudget { units, elapsed_millis: 60_000 },
                    Arc::new(AtomicBool::new(false)),
                ).unwrap();
                let result: AdmissionResult<(AcceptedPreviewLifecycle, VerifiedOwnedPreviewPlan)> =
                    super::reconstruct_and_derive_retained(&snapshot, candidate, request, &lifecycle, &mut meter);
                assert_invalid(result, EXHAUSTED);
                assert_eq!(meter.remaining_units_for_test(), left, "{case}: wrong refusal boundary");
                assert!(remaining_millis(&meter) >= 5_000, "resource fixture missed eligible window");
            }
            "primary-first-replay" | "secondary-first-replay" | "primary-handoff" | "secondary-handoff" => {
                let primary = Arc::new(AtomicBool::new(false));
                let secondary = Arc::new(AtomicBool::new(false));
                let mut meter = ProofMeter::new(ProofBudget::default(), primary.clone()).unwrap();
                meter.watch_cancellation(&secondary);
                let token = if case.starts_with("primary") { &primary } else { &secondary };
                if case.ends_with("first-replay") {
                    let before = meter.remaining_units_for_test();
                    token.store(true, Ordering::Relaxed);
                    let result: AdmissionResult<(AcceptedPreviewLifecycle, VerifiedOwnedPreviewPlan)> =
                        super::reconstruct_and_derive_retained(&snapshot, candidate, request, &lifecycle, &mut meter);
                    assert_invalid(result, CANCELLED);
                    assert_eq!(meter.remaining_units_for_test(), before);
                } else {
                    let reconstructed = AcceptedPreviewLifecycle::reconstruct(
                        &snapshot, candidate, request, &lifecycle, &mut meter,
                    ).unwrap();
                    let inventory = authenticate_inner(&snapshot, candidate, request, &mut meter, false).unwrap();
                    assert_eq!(meter.remaining_units_for_test(), 10_000_000 - prefix);
                    meter.charge(GUARD_UNITS).unwrap();
                    assert!(remaining_millis(&meter) >= 5_000, "cancellation setup missed eligible window");
                    let before = meter.remaining_units_for_test();
                    token.store(true, Ordering::Relaxed);
                    let result: AdmissionResult<VerifiedOwnedPreviewPlan> = super::finish_reconstructed_plan(
                        &snapshot, candidate, request, &lifecycle, &reconstructed, inventory, &mut meter,
                    );
                    assert_invalid(result, CANCELLED);
                    assert_eq!(meter.remaining_units_for_test(), before);
                }
            }
            "short-window-fallback" => {
                // Independent old/new originals; each first constructor and
                // authentication must finish successfully with default evidence.
                // Slow setup or a later generic timeout is a failure, never a pass.
                for selected in [false, true] {
                    let mut meter = ProofMeter::new(
                        ProofBudget { units: 10_000_000, elapsed_millis: 10_000 },
                        Arc::new(AtomicBool::new(false)),
                    ).unwrap();
                    let reconstructed = AcceptedPreviewLifecycle::reconstruct(
                        &snapshot, candidate, request, &lifecycle, &mut meter,
                    ).unwrap_or_else(|e| panic!("short-window first constructor failed: {e:?}"));
                    let inventory = authenticate_inner(&snapshot, candidate, request, &mut meter, false)
                        .unwrap_or_else(|e| panic!("short-window authentication failed: {e:?}"));
                    assert_eq!(reconstructed.recipe(), lifecycle);
                    assert_defaults(&reconstructed.recipe());
                    assert_eq!(meter.remaining_units_for_test(), 10_000_000 - prefix);
                    let remaining = remaining_millis(&meter);
                    assert!(remaining >= 5_000, "short-window prerequisites missed default window: {remaining}ms");
                    std::thread::sleep(Duration::from_millis(remaining - 3_500));
                    let remaining = remaining_millis(&meter);
                    assert!((3_000..=4_000).contains(&remaining), "short-window scheduler missed reached window: {remaining}ms");
                    let before = meter.remaining_units_for_test();
                    if selected {
                        meter.charge(GUARD_UNITS).unwrap();
                        let result: AdmissionResult<VerifiedOwnedPreviewPlan> = super::finish_reconstructed_plan(
                            &snapshot, candidate, request, &lifecycle, &reconstructed, inventory, &mut meter,
                        );
                        assert_invalid(result, REGENERATION_DIFFERS);
                        assert_eq!(before - meter.remaining_units_for_test(), GUARD_UNITS + VERIFY_UNITS);
                    } else {
                        // The actual unchanged old second verification operation,
                        // after the same genuine constructor/authentication prefix.
                        let result: AdmissionResult<()> = reconstructed.verify(&snapshot, candidate, request, &mut meter);
                        assert_invalid(result, REGENERATION_DIFFERS);
                        assert_eq!(before - meter.remaining_units_for_test(), VERIFY_UNITS);
                    }
                    // Exact mismatch above excludes elapsed/cancellation errors;
                    // no handoff5 or common suffix can be charged on that failure.
                    assert!(remaining_millis(&meter) > 0, "fallback completed outside its original deadline");
                }
            }
            "expired-first-replay" => {
                let mut meter = ProofMeter::new(
                    ProofBudget { units: 10_000_000, elapsed_millis: 20 },
                    Arc::new(AtomicBool::new(false)),
                ).unwrap();
                let before = meter.remaining_units_for_test();
                std::thread::sleep(Duration::from_millis(remaining_millis(&meter) + 1));
                assert_eq!(remaining_millis(&meter), 0);
                let result: AdmissionResult<(AcceptedPreviewLifecycle, VerifiedOwnedPreviewPlan)> =
                    super::reconstruct_and_derive_retained(&snapshot, candidate, request, &lifecycle, &mut meter);
                assert_invalid(result, CANCELLED);
                assert_eq!(meter.remaining_units_for_test(), before);
            }
            _ => panic!("unknown resource case: {case}"),
        }
        assert_eq!((snapshot, candidate.clone(), request.clone(), accepted.recipe()), inputs);
        assert_eq!(self.load().unwrap(), inputs.0);
        assert_eq!(self.root.read("CURRENT", 16 * 1024).unwrap(), current);
    }
}

// Frozen byte-for-byte from the pinned baseline: only this function name changed.
fn frozen_derive_inner_before_retained_reuse(
    snapshot: &ProjectSnapshot,
    candidate: &CapturedProgram,
    request: &OwnedPreviewRequest,
    accepted: &super::owned_lifecycle::AcceptedPreviewLifecycle,
    meter: &mut ProofMeter,
    verify_snapshot: bool,
) -> AdmissionResult<VerifiedOwnedPreviewPlan> {
    let inventory = authenticate_inner(snapshot, candidate, request, meter, verify_snapshot)?;
    accepted.verify(snapshot, candidate, request, meter)?;
    // Structural regeneration of retained recipes never settles independent
    // intentions. Their archive scenes are checked by the decision replay path.
    // Public preview still cannot make that comparison or issue write authority.
    if verify_snapshot
        && snapshot
            .decisions
            .decisions
            .iter()
            .any(|d| d.status == DecisionStatus::Active)
    {
        return Err(refuse("accepted decisions", "active retained outcome/property comparison is unsupported by this preview proof; no obligation is silently passed"));
    }
    let (certificates, projections, comparison) =
        super::common_fact_rules::verify(snapshot, candidate, request, &inventory, meter)?;
    let captured = capture_frame(
        snapshot.program()?,
        &snapshot.data,
        snapshot.clock_day,
        request,
        &projections,
        meter,
    )?;
    let report = CommonFactAdmissionReport {
        version: 1,
        snapshot: request.inventory.snapshot.clone(),
        before: inventory.structure.before.clone(),
        after: inventory.structure.after.clone(),
        decisions: snapshot.decisions.identity()?,
        acceptance: accepted.identity().clone(),
        certificates,
        projections,
        phases: inventory.actions,
        outcome_comparison: comparison,
        capture_presence_required: true,
        durable_adoption_supported: false,
    };
    meter.charge(1)?;
    Ok(VerifiedOwnedPreviewPlan {
        snapshot: snapshot.clone(),
        candidate: candidate.clone(),
        request: request.clone(),
        accepted: accepted.copy_internal(),
        report,
        captured,
    })
}

#[test]
fn initial_owned_limits_profile_checks_every_field_and_original_window() {
    for field in ["full", "fuel", "collection_items", "action_steps", "transaction_writes", "output_bytes", "elapsed_millis"] {
        let mut meter = meter();
        let mut profile = InitialLimitsProfile::new(&mut meter).unwrap();
        for index in 0..6 {
            let mut limits = RuntimeLimits::default();
            if index == 3 {
                match field {
                    "full" => (),
                    "fuel" => limits.fuel -= 1,
                    "collection_items" => limits.collection_items -= 1,
                    "action_steps" => limits.action_steps -= 1,
                    "transaction_writes" => limits.transaction_writes -= 1,
                    "output_bytes" => limits.output_bytes -= 1,
                    "elapsed_millis" => limits.elapsed_millis -= 1,
                    _ => unreachable!(),
                }
            }
            profile.observe(&limits, false, &mut meter).unwrap();
        }
        for _ in 0..2 { profile.observe(&RuntimeLimits::default(), true, &mut meter).unwrap(); }
        assert_eq!(profile.eligible(&mut meter).unwrap(), field == "full");
        meter.started = Instant::now() - Duration::from_millis(55_001);
        assert!(!profile.eligible(&mut meter).unwrap());
        meter.cancelled.store(true, Ordering::Relaxed);
        assert_invalid(profile.eligible(&mut meter), CANCELLED);
    }
    let mut meter = meter();
    let mut partial = InitialLimitsProfile::new(&mut meter).unwrap();
    for _ in 0..5 { partial.observe(&RuntimeLimits::default(), false, &mut meter).unwrap(); }
    for _ in 0..2 { partial.observe(&RuntimeLimits::default(), true, &mut meter).unwrap(); }
    assert!(!partial.eligible(&mut meter).unwrap());
}

#[test]
fn initial_owned_limits_profile_packets_refuse_before_state_changes() {
    let root = InitialLimitsProfile::ROOT as u64 + 64;
    assert_eq!(INITIAL_CAPTURE_PROFILE_TRANSPORT, 496);
    assert_eq!(AcceptedPreviewLifecycle::PROFILE_VERIFY_FRAME, 384);
    eprintln!("initial frames finish={} inventory={} capture={} verify={}", FINISH_PROFILE_FRAME,
        std::mem::size_of::<CommonFactInventory>(), INITIAL_CAPTURE_PROFILE_TRANSPORT,
        AcceptedPreviewLifecycle::PROFILE_VERIFY_FRAME);
    // Concrete native widths, not an inferred native traversal credit.
    assert_eq!(std::mem::size_of::<InitialLimitsProfile>(), 24);
    assert_eq!(std::mem::size_of::<RuntimeLimits>(), 48);
    eprintln!("initial profile root={} result_width={}", root,
        std::mem::size_of::<AdmissionResult<()>>());
    for (packet, operation) in [(root, 0), (256, 1), (64, 2)] {
        for exact in [false, true] {
            let mut m = meter();
            let mut p = InitialLimitsProfile::new(&mut m).unwrap();
            let available = packet - u64::from(!exact);
            m.charge(m.remaining_units_for_test() - available).unwrap();
            let before = p.counts_for_test();
            let result = match operation {
                0 => InitialLimitsProfile::new(&mut m).map(|_| ()),
                1 => p.observe(&RuntimeLimits::default(), false, &mut m),
                _ => p.eligible(&mut m).map(|_| ()),
            };
            if exact { result.unwrap(); assert_eq!(m.remaining_units_for_test(), 0); }
            else {
                assert_invalid(result, EXHAUSTED);
                assert_eq!(p.counts_for_test(), before);
                assert_eq!(m.remaining_units_for_test(), available);
            }
        }
    }
    for row in [false, true] {
        let mut m = meter(); let mut p = InitialLimitsProfile::new(&mut m).unwrap();
        if row { p.rows = u64::MAX; } else { p.lifecycle = u64::MAX; }
        let before = p.counts_for_test(); let left = m.remaining_units_for_test();
        assert!(p.observe(&RuntimeLimits::default(), row, &mut m).is_err());
        assert_eq!(p.counts_for_test(), before);
        assert_eq!(left - m.remaining_units_for_test(), 256);
    }
    for secondary in [false, true] {
        let mut m = meter(); let mut p = InitialLimitsProfile::new(&mut m).unwrap();
        let other = Arc::new(AtomicBool::new(false)); m.watch_cancellation(&other);
        m.charge(m.remaining_units_for_test()).unwrap();
        if secondary { other.store(true, Ordering::Relaxed); }
        else { m.cancelled.store(true, Ordering::Relaxed); }
        assert_invalid(p.observe(&RuntimeLimits::default(), false, &mut m), CANCELLED);
        assert_invalid(p.eligible(&mut m), CANCELLED);
        assert_eq!(p.counts_for_test(), (0, 0, true));
    }
    let mut m = meter(); let p = InitialLimitsProfile::new(&mut m).unwrap();
    m.charge(m.remaining_units_for_test()).unwrap();
    m.started = Instant::now() - Duration::from_millis(60_001);
    assert_invalid(p.eligible(&mut m), CANCELLED);
}
