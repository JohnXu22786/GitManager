//! Completed predicate and paid route controls. No synthetic receipt is acceptance authority.
use super::*;

// The factory invariant supports completed-predicate reuse, not tuple equality
// or a prediction that a freshly scheduled evaluation would finish.
#[test]
fn policy_frame_dominance_factory_returns_positive_bounded_window() {
    let defaults = RuntimeLimits::default();
    for elapsed_millis in [60_000, 4999] {
        let mut meter = ProofMeter::new(ProofBudget { units: 200_000, elapsed_millis },
            Arc::new(AtomicBool::new(false))).unwrap();
        let before = meter.remaining_units_for_test();
        let actual = meter.runtime().unwrap();
        assert_eq!(before - meter.remaining_units_for_test(), defaults.fuel);
        assert_eq!(actual.fuel, defaults.fuel);
        assert_eq!(actual.collection_items, defaults.collection_items);
        assert_eq!(actual.action_steps, defaults.action_steps);
        assert_eq!(actual.transaction_writes, defaults.transaction_writes);
        assert_eq!(actual.output_bytes, defaults.output_bytes);
        assert!(actual.elapsed_millis > 0 && actual.elapsed_millis <= defaults.elapsed_millis);
        if elapsed_millis < defaults.elapsed_millis { assert!(actual.elapsed_millis < defaults.elapsed_millis); }
        println!("POLICY_FRAME_FACTORY_WINDOW configured={elapsed_millis} actual={actual:?}");
    }
}

#[test]
fn policy_frame_packets_are_positive_exact_checked_layouts() {
    // Producing accounting regression: every full transport/query type,
    // including residual/error paths, is checked before a binary can run.
    const _: () = {
        assert!(size_of::<Result<bool>>() == 152);
        assert!(size_of::<&PolicyFrameRecorder<'_>>() == 8);
        assert!(size_of::<SelectedFrameCall<'_>>() == 48);
        assert!(size_of::<std::ops::ControlFlow<Result<std::convert::Infallible>, ()>>() == 152);
        assert!(size_of::<Result<()>>() == 152);
        assert!(size_of::<isize>() == 8);
        assert!(size_of::<Result<std::convert::Infallible>>() == 152);
        assert!(size_of::<bool>() == 1);
        assert!(size_of::<Option<&CompletedPolicyFrames<'_>>>() == 8);
        assert!(size_of::<&Option<CompletedPolicyFrames<'_>>>() == 8);
        assert!(size_of::<&CompletedPolicyFrames<'_>>() == 8);
        assert!(size_of::<*const owned_persist::ReconstructedOwned>() == 8);
        assert!(size_of::<*const owned_persist::OwnedExecutionRecipe>() == 8);
        assert!(size_of::<*const ProjectSnapshot>() == 8);
        assert!(size_of::<&ProjectSnapshot>() == 8);
        assert!(size_of::<&&Digest>() == 8);
        assert!(size_of::<&Digest>() == 8);
        assert!(size_of::<std::ops::ControlFlow<Result<std::convert::Infallible>, bool>>() == 152);
        assert!(size_of::<&mut ProofMeter>() == 8);
        assert!(size_of::<&owned_persist::ReconstructedOwned>() == 8);
        assert!(size_of::<&owned_persist::OwnedExecutionRecipe>() == 8);
        assert!(size_of::<&VerifiedOwnedTransitionExecution>() == 8);
        assert!(size_of::<&owned_continuation::VerifiedOwnedPolicySurface>() == 8);
        assert!(size_of::<&OwnedLayerV2>() == 8);
        assert!(size_of::<Option<u128>>() == 32);
        assert!(size_of::<u128>() == 16);
        assert!(size_of::<u64>() == 8);
        assert!(size_of::<&std::time::Duration>() == 8);
        assert!(size_of::<std::time::Duration>() == 16);
        assert!(size_of::<&std::time::Instant>() == 8);
        assert!(size_of::<RuntimeLimits>() == 48);
        assert!(POLICY_FRAME_SELECT_PAYMENT == 18220);
    };
    println!("POLICY_FRAME_F7_LAYOUTS {}", serde_json::json!({
        "Result<bool>": size_of::<Result<bool>>(),
        "&PolicyFrameRecorder<'_>": size_of::<&PolicyFrameRecorder<'_>>(),
        "SelectedFrameCall<'_>": size_of::<SelectedFrameCall<'_>>(),
        "std::ops::ControlFlow<Result<std::convert::Infallible>, ()>": size_of::<std::ops::ControlFlow<Result<std::convert::Infallible>, ()>>(),
        "Result<()>": size_of::<Result<()>>(),
        "isize": size_of::<isize>(),
        "Result<std::convert::Infallible>": size_of::<Result<std::convert::Infallible>>(),
        "bool": size_of::<bool>(),
        "Option<&CompletedPolicyFrames<'_>>": size_of::<Option<&CompletedPolicyFrames<'_>>>(),
        "&Option<CompletedPolicyFrames<'_>>": size_of::<&Option<CompletedPolicyFrames<'_>>>(),
        "&CompletedPolicyFrames<'_>": size_of::<&CompletedPolicyFrames<'_>>(),
        "*const owned_persist::ReconstructedOwned": size_of::<*const owned_persist::ReconstructedOwned>(),
        "*const owned_persist::OwnedExecutionRecipe": size_of::<*const owned_persist::OwnedExecutionRecipe>(),
        "*const ProjectSnapshot": size_of::<*const ProjectSnapshot>(),
        "&ProjectSnapshot": size_of::<&ProjectSnapshot>(),
        "&&Digest": size_of::<&&Digest>(),
        "&Digest": size_of::<&Digest>(),
        "std::ops::ControlFlow<Result<std::convert::Infallible>, bool>": size_of::<std::ops::ControlFlow<Result<std::convert::Infallible>, bool>>(),
        "&mut ProofMeter": size_of::<&mut ProofMeter>(),
        "&owned_persist::ReconstructedOwned": size_of::<&owned_persist::ReconstructedOwned>(),
        "&owned_persist::OwnedExecutionRecipe": size_of::<&owned_persist::OwnedExecutionRecipe>(),
        "&VerifiedOwnedTransitionExecution": size_of::<&VerifiedOwnedTransitionExecution>(),
        "&owned_continuation::VerifiedOwnedPolicySurface": size_of::<&owned_continuation::VerifiedOwnedPolicySurface>(),
        "&OwnedLayerV2": size_of::<&OwnedLayerV2>(),
        "Option<u128>": size_of::<Option<u128>>(),
        "u128": size_of::<u128>(),
        "u64": size_of::<u64>(),
        "&std::time::Duration": size_of::<&std::time::Duration>(),
        "std::time::Duration": size_of::<std::time::Duration>(),
        "&std::time::Instant": size_of::<&std::time::Instant>(),
        "RuntimeLimits": size_of::<RuntimeLimits>(),
        "f7": POLICY_FRAME_SELECT_PAYMENT,
        "retained_floor": 2984,
    }));
    println!("POLICY_FRAME_SCOPE_LAYOUTS {}", serde_json::json!({
        "capsule": size_of::<LockedArchivePolicy<'_>>(),
        "optional_capsule": size_of::<Option<LockedArchivePolicy<'_>>>(),
        "optional_capsule_borrow": size_of::<Option<&LockedArchivePolicy<'_>>>(),
        "recorder": size_of::<PolicyFrameRecorder<'_>>(),
        "receipt": size_of::<CompletedPolicyFrames<'_>>(),
        "optional_receipt": size_of::<Option<CompletedPolicyFrames<'_>>>(),
        "record_call": size_of::<SurfaceRecordCall<'_, '_>>(),
        "consume_call": size_of::<SurfaceConsumeCall<'_, '_, '_>>(),
        "policy_frame_scope": size_of::<PolicyFrameScope<'_>>(),
        "epoch_inputs": size_of::<EpochSeedInputs<'_>>(),
        "selected_frame_call": size_of::<SelectedFrameCall<'_>>(),
        "retained_frame_capture": size_of::<RetainedFrameReuseCapture<'_, '_, '_, ()>>(),
        "runtime_limits": size_of::<RuntimeLimits>(),
        "runtime_limits_result": size_of::<Result<RuntimeLimits>>(),
        "surface_result": size_of::<Result<owned_continuation::VerifiedOwnedPolicySurface>>(),
        "adapter_value_result": size_of::<std::result::Result<DataValue, AdapterError>>(),
        "store_result": size_of::<Result<()>>(),
        "bool_result": size_of::<Result<bool>>(),
        "d0": LOCKED_ARCHIVE_TRANSPORT_PAYMENT,
        "d2": LOCKED_ARCHIVE_SELECTOR_PAYMENT,
    }));
    let packets = [policy_frame_open_payment::<()>(), POLICY_FRAME_RECORD_PAYMENT,
        POLICY_FRAME_SCOPE_PAYMENT,
        POLICY_FRAME_MINT_PAYMENT, policy_frame_retained_payment::<()>(),
        POLICY_FRAME_SELECT_PAYMENT, POLICY_FRAME_CONSUME_PAYMENT,
        POLICY_FRAME_EPOCH_PAYMENT];
    for packet in packets {
        assert!(packet > 0);
        OwnedPolicyCheck::assert_archive_packet_for_test(packet);
    }
    println!("POLICY_FRAME_PACKETS {packets:?}");
    assert_eq!(std::mem::size_of::<OwnedPolicyCheck<'_>>(), 3 * std::mem::size_of::<usize>());
}

struct AssertReceiptCapture<'a> {
    source: &'a CapturedProgram,
    other: &'a CapturedProgram,
    decision: &'a ScopedDecision,
    scenes: &'a [crate::product_decisions::AcceptedScene],
    calls: bool,
}
impl OwnedPolicyCheck<'_> {
    pub(crate) fn assert_policy_frame_receipt_for_test(
        &mut self, capsule: &LockedArchivePolicy<'_>, recorder: &PolicyFrameRecorder<'_>,
        source: &CapturedProgram, candidate: &ProjectSnapshot, require_calls: bool,
        decision: &ScopedDecision, scenes: &[crate::product_decisions::AcceptedScene], calls: bool,
    ) {
        let receipt = recorder.completed.as_ref().expect("complete real source policy loop");
        assert!(std::ptr::eq(self.execution, capsule.execution));
        assert!(std::ptr::eq(recorder.execution, self.execution));
        assert!(std::ptr::eq(receipt.registry, capsule.registry));
        assert!(std::ptr::eq(receipt.snapshot, self.execution.current()));
        assert!(std::ptr::eq(receipt.recipe, capsule.registry.resolve(source).unwrap()));
        println!("POLICY_FRAME_COMPLETED_PREDICATE source_bound=true");
        assert_completed_epoch_payments(receipt, require_calls);
        let capture = AssertReceiptCapture { source, other: self.execution.current().program().unwrap(), decision, scenes, calls };
        let dispatch = RetainedPolicyDispatch { capture, run: assert_receipt_consumer };
        check_retained_transition_policies_with_frames(candidate, capsule, recorder, dispatch).unwrap();
        if calls {
            for mode in 0..4 {
                let capture = AssertRetainedBoundary { decision, scenes, mode };
                let dispatch = RetainedPolicyDispatch { capture, run: assert_retained_boundary };
                check_retained_transition_policies_with_frames(candidate, capsule, recorder, dispatch).unwrap();
            }
        }
    }
}
fn assert_receipt_consumer(
    capture: &AssertReceiptCapture<'_>, check: &mut OwnedPolicyCheck<'_>, recorder: &PolicyFrameRecorder<'_>,
) -> Result<()> {
    for (index, source) in [capture.source, capture.other].into_iter().enumerate() {
        let mut reference_meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
        let mut reference = OwnedPolicyCheck { execution: check.execution, registry: check.registry, meter: &mut reference_meter };
        let before = reference.meter.remaining_units_for_test();
        let expected = reference.surface(source)?;
        let ordinary_cost = before - reference.meter.remaining_units_for_test();
        let registry = check.registry.expect("actual immutable historical registry selected");
        let mut frame_meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
        let before = frame_meter.remaining_units_for_test();
        registry.validate_policy_frames(check.execution.current(), &expected, &mut frame_meter)?;
        let loop_cost = before - frame_meter.remaining_units_for_test();
        let receipt = recorder.completed.as_ref().unwrap();
        let matches = std::ptr::eq(receipt.recipe, registry.resolve(source)?);
        let expected_cost = ordinary_cost + POLICY_FRAME_CONSUME_PAYMENT + POLICY_FRAME_SELECT_PAYMENT
            - if matches { loop_cost } else { 0 };
        let before = check.meter.remaining_units_for_test();
        check.charge(POLICY_FRAME_CONSUME_PAYMENT)?;
        let actual = SurfaceConsumeCall { check, recorder, source }.run();
        if expected_cost > before {
            assert!(!matches, "the proven source must fit this component recipient");
            let error = actual.err().expect("unfunded full fallback must fail");
            assert!(matches!(&error, StoreError::Invalid(text) if text ==
                "Common factual admission: budget: cumulative proof/initialization budget exhausted"), "{error:?}");
            // Reproduce the same ordinary fallback on exactly the units left
            // after both added packets. This is a typed failure, not parity or
            // a successful public/whole archive reference.
            let mut bounded_meter = ProofMeter::new(ProofBudget {
                units: before - POLICY_FRAME_CONSUME_PAYMENT - POLICY_FRAME_SELECT_PAYMENT + 1,
                elapsed_millis: 60_000,
            }, Arc::new(AtomicBool::new(false)))?;
            let mut bounded = OwnedPolicyCheck { execution: check.execution, registry: check.registry, meter: &mut bounded_meter };
            let ordinary_error = bounded.surface(source).err().expect("same unfunded ordinary loop");
            assert_eq!(format!("{error:?}"), format!("{ordinary_error:?}"));
            assert_eq!(check.meter.remaining_units_for_test(), bounded.meter.remaining_units_for_test());
            println!("POLICY_FRAME_FALLBACK_TYPED_FUEL needed={expected_cost} available={before}");
            return Ok(());
        }
        let actual = actual?;
        actual.assert_same_value_for_test(&expected);
        let selected_cost = before - check.meter.remaining_units_for_test();
        assert_eq!(selected_cost, expected_cost, "complete loop only, never derivation/header/scene work");
        println!("POLICY_FRAME_CONSUMER matched={matches} selected={selected_cost} ordinary={ordinary_cost} loop={loop_cost}");
        if matches && index == 0 {
            let mut local_meter = ProofMeter::new(ProofBudget {
                units: ProofBudget::default().units, elapsed_millis: 4999,
            }, Arc::new(AtomicBool::new(false)))?;
            let mut local = OwnedPolicyCheck { execution: check.execution,
                registry: check.registry, meter: &mut local_meter };
            let before = local.meter.remaining_units_for_test();
            local.charge(POLICY_FRAME_CONSUME_PAYMENT)?;
            let fallback = SurfaceConsumeCall { check: &mut local, recorder, source }.run()?;
            fallback.assert_same_value_for_test(&expected);
            let cost = before - local.meter.remaining_units_for_test();
            assert_eq!(cost, ordinary_cost + POLICY_FRAME_CONSUME_PAYMENT + POLICY_FRAME_SELECT_PAYMENT);
            println!("POLICY_FRAME_BELOW_WINDOW_FULL_FALLBACK cost={cost}");
        }
        if capture.calls && index == 0 {
            assert!(matches, "the first actual source must be the completed receipt target");
            let prefix = ordinary_cost.checked_sub(loop_cost).unwrap()
                .checked_add(POLICY_FRAME_CONSUME_PAYMENT).unwrap();
            for short in [1_u64, 0] {
                let units = prefix.checked_add(POLICY_FRAME_SELECT_PAYMENT).unwrap()
                    .checked_sub(short).unwrap().checked_add(1).unwrap();
                assert!(units <= ProofBudget::default().units, "assertion-local boundary must fit the unchanged cap");
                let mut boundary_meter = ProofMeter::new(ProofBudget {
                    units, elapsed_millis: ProofBudget::default().elapsed_millis,
                }, check.meter.cancelled())?;
                let mut boundary = OwnedPolicyCheck { execution: check.execution,
                    registry: check.registry, meter: &mut boundary_meter };
                boundary.charge(POLICY_FRAME_CONSUME_PAYMENT)?;
                let result = SurfaceConsumeCall { check: &mut boundary, recorder, source }.run();
                let exact = short == 0;
                if exact {
                    result?.assert_same_value_for_test(&expected);
                    assert_eq!(boundary.meter.remaining_units_for_test(), 0);
                } else {
                    let error = result.err().expect("one unit short must refuse the original F7 debit");
                    assert!(matches!(&error, StoreError::Invalid(text) if text ==
                        "Common factual admission: budget: cumulative proof/initialization budget exhausted"), "{error:?}");
                    assert_eq!(boundary.meter.remaining_units_for_test(), POLICY_FRAME_SELECT_PAYMENT - 1);
                }
                assert!(std::ptr::eq(recorder.completed.as_ref().unwrap(), receipt));
                println!("POLICY_FRAME_F7_ASSERTION_LOCAL exact={exact}");
            }
            crate::product_decisions::DecisionEngine::<LocalRuntime>::assert_policy_frame_recipient_calls_for_test(
                check, recorder, capture.decision, capture.scenes, loop_cost, POLICY_FRAME_SELECT_PAYMENT)?;
        }
        let mut foreign = source.clone();
        foreign.binding.program_path.push_str("-foreign-frame-proof");
        let before = check.meter.remaining_units_for_test();
        check.charge(POLICY_FRAME_CONSUME_PAYMENT)?;
        let result = SurfaceConsumeCall { check, recorder, source: &foreign }.run().err();
        assert_eq!(foreign.artifact.program_digest, source.artifact.program_digest);
        assert!(matches!(result, Some(StoreError::Invalid(ref text)) if
            text == "Common factual admission: owned persisted proof: source is not a checked owned recipe"), "{result:?}");
        assert_eq!(before - check.meter.remaining_units_for_test(), POLICY_FRAME_CONSUME_PAYMENT + 5);
    }
    Ok(())
}

// Independent bounded replay references retain every seed, post-event and exact
// endpoint visit. Counts are derived from actual authenticated epoch events,
// not inferred from an end-to-end debit or added production observer.
fn assert_completed_epoch_payments(receipt: &CompletedPolicyFrames<'_>, require_calls: bool) {
    let fresh = || ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false))).unwrap();
    let mut derive_meter = fresh();
    let surface = match receipt.recipe {
        owned_persist::OwnedExecutionRecipe::Initial(original) =>
            owned_continuation::derive_policy_surface(original, receipt.layer, None, &mut derive_meter),
        owned_persist::OwnedExecutionRecipe::Transition(prior) =>
            owned_continuation::derive_policy_surface(&prior.original, receipt.layer,
                Some((&prior.continuation, &prior.target)), &mut derive_meter),
    }.unwrap();
    assert_seed_identity_and_slots(receipt, &surface);
    let selected_costs = run_chronological_frames_for_test(receipt.registry, receipt.snapshot,
        receipt.recipe, &surface, &mut fresh()).unwrap();
    let mut total_expected_calls = 0;
    for (index, epoch) in receipt.registry.epochs.iter().enumerate() {
        let recipe = &receipt.registry.recipes[&epoch.output];
        let (endpoint, day) = if let Some(next) = receipt.registry.epochs.get(index + 1) {
            let next = &receipt.registry.recipes[&next.output];
            (next.seed(), next.basis().clock_day)
        } else { (&receipt.snapshot.data, receipt.snapshot.clock_day) };
        let changes: Vec<_> = endpoint.events[recipe.seed().events.len()..].iter()
            .flat_map(|event| &event.changes).filter(|change| change.entity == recipe.metadata().root).collect();
        let sealed = changes.iter().filter(|change|
            change.after.get(&recipe.metadata().sealed) == Some(&DataValue::Boolean { value: true })).count();
        let saved: BTreeSet<_> = recipe.metadata().saved.iter().collect();
        let expected_calls = (sealed * saved.len()) as u64;
        let mut ordinary = fresh();
        let before = ordinary.remaining_units_for_test();
        let mut frames = 0;
        owned_replay::validate_recipe_history_with(recipe, recipe.seed(), recipe.basis().clock_day,
            endpoint, day, &mut ordinary, &mut |frame, meter| {
                frames += 1;
                meter.charge(canonical_bytes(frame)?.len() as u64)?;
                surface.validate_frame(frame)
            }).unwrap();
        assert_eq!(frames, endpoint.events.len() - recipe.seed().events.len() + 2);
        let ordinary_cost = before - ordinary.remaining_units_for_test();
        let selected_cost = selected_costs[index];
        if require_calls { assert!(expected_calls > 0, "each genuine producer requires saved projections"); }
        let covered = index != 0 || std::ptr::eq(recipe.seed(), &receipt.recipe.initial().initialized);
        let removed_seed = if covered { canonical_bytes(recipe.seed()).unwrap().len() as u64 } else { 0 };
        let removed_final = canonical_bytes(endpoint).unwrap().len() as u64;
        let removed = removed_seed + removed_final;
        assert_eq!(selected_cost, ordinary_cost + POLICY_FRAME_EPOCH_PAYMENT
            + POLICY_CHRONOLOGY_SEED_PAYMENT
            - removed);
        println!("POLICY_SEED_ACTUAL_EPOCH index={index} covered={covered} removed={removed}");
        println!("POLICY_FRAME_EPOCH index={index} frames={frames} rows={} expected_projections={expected_calls} ordinary={ordinary_cost} selected={selected_cost}", changes.len());
        total_expected_calls += expected_calls;
    }
    assert_eq!(total_expected_calls, if require_calls { 10 } else { 0 });
    println!("POLICY_FRAME_REQUIRED_PROJECTIONS count={total_expected_calls} source=immutable_events_not_observer");
    if require_calls { assert_eq!(receipt.registry.epochs.len(), 2); }
}

impl OwnedPolicyCheck<'_> {
    pub(crate) fn leave_policy_frame_units_for_test(&mut self, units: u64) {
        let before = self.meter.remaining_units_for_test();
        assert!(before >= units);
        self.charge(before - units).unwrap();
        assert_eq!(self.meter.remaining_units_for_test(), units);
    }
    pub(crate) fn assert_policy_frame_units_for_test(&self, units: u64) {
        assert_eq!(self.meter.remaining_units_for_test(), units);
    }
    pub(crate) fn policy_frame_units_for_test(&self) -> u64 {
        self.meter.remaining_units_for_test()
    }
}
impl PolicyFrameRecorder<'_> {
    pub(crate) fn assert_policy_frame_empty_for_test(&self) { assert!(self.completed.is_none()); }
}

struct AssertRetainedBoundary<'a> {
    decision: &'a ScopedDecision,
    scenes: &'a [crate::product_decisions::AcceptedScene],
    mode: u8,
}
fn assert_retained_boundary(capture: &AssertRetainedBoundary<'_>, check: &mut OwnedPolicyCheck<'_>, recorder: &PolicyFrameRecorder<'_>) -> Result<()> {
    crate::product_decisions::DecisionEngine::<LocalRuntime>::assert_policy_frame_retained_boundary_for_test(
        check, recorder, capture.decision, capture.scenes, capture.mode)
}

#[test]
fn policy_frame_dominance_recipient_live_window_and_errors() {
    let fresh_token = || Arc::new(AtomicBool::new(false));
    let mut default = ProofMeter::new(ProofBudget::default(), fresh_token()).unwrap();
    let limits = default.runtime().unwrap();
    assert_eq!(limits, RuntimeLimits::default());
    println!("POLICY_FRAME_ACTUAL_DEFAULT_TUPLE {limits:?}");
    for (millis, expected) in [(60_000, true), (4_999, false)] {
        let mut meter = ProofMeter::new(ProofBudget { units: POLICY_FRAME_SELECT_PAYMENT + (10_000 - 2984), elapsed_millis: millis }, fresh_token()).unwrap();
        meter.charge(POLICY_FRAME_SELECT_PAYMENT).unwrap();
        let before = meter.remaining_units_for_test();
        assert_eq!(before, 7015, "preserve the original synthetic post-constructor/F7 balance");
        assert_eq!(meter.policy_frame_default_window().unwrap(), expected);
        assert_eq!(meter.remaining_units_for_test(), before, "no hypothetical runtime reservation or refund");
    }
    for secondary in [false, true] {
        let cancelled = fresh_token();
        let mut meter = ProofMeter::new(ProofBudget::default(), if secondary { fresh_token() } else { cancelled.clone() }).unwrap();
        if secondary { meter.watch_cancellation(&cancelled); }
        meter.charge(POLICY_FRAME_SELECT_PAYMENT).unwrap();
        cancelled.store(true, Ordering::Release);
        let error = meter.policy_frame_default_window().unwrap_err();
        assert!(error.to_string().contains("cancelled or cumulative elapsed budget exhausted"));
    }
    let mut meter = ProofMeter::new(ProofBudget { units: POLICY_FRAME_SELECT_PAYMENT + (10_000 - 2984), elapsed_millis: 1 }, fresh_token()).unwrap();
    meter.charge(POLICY_FRAME_SELECT_PAYMENT).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    assert!(meter.policy_frame_default_window().unwrap_err().to_string().contains("elapsed"));
}

// These controls operate only inside the real locked capsule. Test-local
// reference meters never replace, refill or alter the writer/recipient meters.
struct ClippedCompletionCapture<'a, 'gate> {
    capsule: &'a LockedArchivePolicy<'gate>,
    source: &'a CapturedProgram,
    candidate: &'a ProjectSnapshot,
    decision: &'a ScopedDecision,
    scenes: &'a [crate::product_decisions::AcceptedScene],
}
impl OwnedPolicyCheck<'_> {
    pub(crate) fn assert_policy_frame_clipped_for_test(
        &mut self, capsule: &LockedArchivePolicy<'_>, source: &CapturedProgram,
        candidate: &ProjectSnapshot, decision: &ScopedDecision,
        scenes: &[crate::product_decisions::AcceptedScene],
    ) -> Result<()> {
        let before = self.meter.remaining_units_for_test();
        let mut local_meter = ProofMeter::new(ProofBudget {
            units: ProofBudget::default().units, elapsed_millis: 4999,
        }, Arc::new(AtomicBool::new(false)))?;
        let mut local = OwnedPolicyCheck { execution: self.execution,
            registry: self.registry, meter: &mut local_meter };
        local.charge(const { policy_frame_open_payment::<ClippedCompletionCapture<'_, '_>>() })?;
        let capture = ClippedCompletionCapture { capsule, source, candidate, decision, scenes };
        let dispatch = RecordContinuationDispatch { capture, run: assert_clipped_completion };
        let result = capsule.with_policy_frame_recorder(&mut local, dispatch);
        assert_eq!(self.meter.remaining_units_for_test(), before, "production meter untouched");
        result
    }
}
fn assert_clipped_completion<'gate>(
    capture: ClippedCompletionCapture<'_, 'gate>, check: &mut OwnedPolicyCheck<'gate>,
    recorder: &mut PolicyFrameRecorder<'gate>,
) -> Result<()> {
    assert!(recorder.completed.is_none());
    check.charge(POLICY_FRAME_RECORD_PAYMENT)?;
    SurfaceRecordCall { check, recorder, source: capture.source }.run()?;
    let receipt = recorder.completed.as_ref().expect("whole source loop actually completed");
    assert!(std::ptr::eq(receipt.registry, capture.capsule.registry));
    assert!(std::ptr::eq(recorder.execution, capture.capsule.execution));
    // This actual local meter is configured4999; the pinned factory can only
    // return positive elapsed windows below5000. Completion is the witness.
    println!("POLICY_FRAME_CLIPPED_COMPLETE donor_window_bound=4999 authority=completed_predicate");
    // A different real source must take the unchanged donor route and cannot
    // overwrite the first slot, including a completed clipped donor.
    let source = check.execution.current().program()?;
    assert_ne!(source, capture.source);
    let recipe = receipt.recipe;
    let before = check.meter.remaining_units_for_test();
    check.charge(POLICY_FRAME_RECORD_PAYMENT)?;
    let actual = SurfaceRecordCall { check, recorder, source }.run()?;
    let selected_cost = before - check.meter.remaining_units_for_test();
    let mut reference_meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
    let mut reference = OwnedPolicyCheck { execution: check.execution, registry: check.registry, meter: &mut reference_meter };
    let before = reference.meter.remaining_units_for_test();
    let expected = reference.surface(source)?;
    let ordinary = before - reference.meter.remaining_units_for_test();
    actual.assert_same_value_for_test(&expected);
    assert_eq!(selected_cost, ordinary + POLICY_FRAME_RECORD_PAYMENT);
    let receipt = recorder.completed.as_ref().unwrap();
    assert!(std::ptr::eq(receipt.recipe, recipe));
    println!("POLICY_FRAME_SECOND_SOURCE_NO_OVERWRITE ordinary={ordinary}");
    // Independently reconstructed recipient may omit this completed exact loop.
    // Other-source ordinary typed fuel refusal remains an honest refusal.
    check.assert_policy_frame_receipt_for_test(capture.capsule, recorder, capture.source,
        capture.candidate, true, capture.decision, capture.scenes, false);
    Ok(())
}
struct EmptyFrameCapture<'a> { source: &'a CapturedProgram }
fn assert_empty_frame_consumer(
    capture: &EmptyFrameCapture<'_>, check: &mut OwnedPolicyCheck<'_>, recorder: &PolicyFrameRecorder<'_>,
) -> Result<()> {
    assert!(recorder.completed.is_none());
    let mut reference_meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
    let mut reference = OwnedPolicyCheck { execution: check.execution, registry: check.registry, meter: &mut reference_meter };
    let before = reference.meter.remaining_units_for_test();
    let expected = reference.surface(capture.source)?;
    let ordinary = before - reference.meter.remaining_units_for_test();
    let before = check.meter.remaining_units_for_test();
    check.charge(POLICY_FRAME_CONSUME_PAYMENT)?;
    let actual = SurfaceConsumeCall { check, recorder, source: capture.source }.run()?;
    actual.assert_same_value_for_test(&expected);
    assert_eq!(before - check.meter.remaining_units_for_test(), ordinary + POLICY_FRAME_CONSUME_PAYMENT + POLICY_FRAME_SELECT_PAYMENT);
    assert!(recorder.completed.is_none());
    println!("POLICY_FRAME_EMPTY_RECIPIENT_FULL_LOOP ordinary={ordinary}");
    Ok(())
}
impl<'gate> OwnedPolicyCheck<'gate> {
    pub(crate) fn assert_policy_frame_empty_and_refusals_for_test(
        &mut self, capsule: &LockedArchivePolicy<'_>, recorder: &mut PolicyFrameRecorder<'gate>,
        source: &CapturedProgram, candidate: &ProjectSnapshot, prepared: &PreparedOwnedTransitionV2,
    ) -> Result<()> {
        assert!(recorder.completed.is_none());
        let mut reconstruction_meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
        let other_registry = owned_persist::reconstruct(self.execution.current(), &mut reconstruction_meter)?;
        assert!(!std::ptr::eq(self.execution, prepared.execution.as_ref()));
        for kind in ["execution", "registry", "none"] {
            let mut local_meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
            let mut wrong = OwnedPolicyCheck {
                execution: if kind == "execution" { &prepared.execution } else { self.execution },
                registry: match kind { "registry" => Some(&other_registry), "none" => None, _ => self.registry },
                meter: &mut local_meter,
            };
            wrong.charge(const { policy_frame_open_payment::<()>() })?;
            let dispatch = RecordContinuationDispatch { capture: (), run: |_, _, _| panic!("wrong origin cannot open recorder") };
            let before = wrong.meter.remaining_units_for_test();
            let error = capsule.with_policy_frame_recorder(&mut wrong, dispatch).unwrap_err();
            assert!(matches!(error, StoreError::Invalid(ref text) if text == "Common factual admission: owned policy frames: recorder is not the fresh locked check"));
            assert_eq!(wrong.meter.remaining_units_for_test(), before);
            println!("POLICY_FRAME_ORIGIN_REFUSAL {kind}");
        }
        // A missing hint is only a negative recording-origin control. Restore
        // the identical immutable borrow before continuing; never alter fuel.
        let original_registry = self.registry.take();
        let before = self.meter.remaining_units_for_test();
        self.charge(POLICY_FRAME_RECORD_PAYMENT)?;
        let refusal = SurfaceRecordCall { check: self, recorder, source }.run();
        self.registry = original_registry;
        let error = refusal.err().expect("missing recording registry");
        assert!(matches!(error, StoreError::Invalid(ref text) if text == "Common factual admission: owned policy frames: recording check changed"));
        assert_eq!(before - self.meter.remaining_units_for_test(), POLICY_FRAME_RECORD_PAYMENT);
        assert!(recorder.completed.is_none());
        let dispatch = RetainedPolicyDispatch { capture: EmptyFrameCapture { source }, run: assert_empty_frame_consumer };
        check_retained_transition_policies_with_frames(candidate, capsule, recorder, dispatch)?;
        for kind in ["snapshot", "derive", "header"] {
            // An independently reconstructed registry is corrupted only as a
            // refusal input. It is never a recorder mint or positive authority.
            let mut reconstruction_meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
            let mut registry = owned_persist::reconstruct(self.execution.current(), &mut reconstruction_meter)?;
            let expected = match kind {
                "snapshot" => {
                    let mut changed = self.execution.current().clone(); changed.clock_day += 1;
                    registry.snapshot = canonical_digest(IdentityDomain::Data, &changed)?;
                    "Common factual admission: owned persisted proof: execution registry belongs to another complete snapshot"
                }
                "derive" => {
                    registry.proofs.clear();
                    let owned_persist::OwnedExecutionRecipe::Initial(original) = registry.recipes.get_mut(&revision(source)?).unwrap() else { panic!("initial-only fixture"); };
                    let original = Arc::get_mut(original).expect("isolated initial reconstruction");
                    let proof = Arc::get_mut(&mut original.proof).expect("isolated reconstructed plan");
                    proof.candidate.binding.program_path.push_str("-refusal-only");
                    "Common factual admission: policy ownership: wrong reconstructed original layer"
                }
                "header" => {
                    registry.epochs.clear();
                    "Common factual admission: owned persisted proof: policy surface is not bound to this committed original layer"
                }
                _ => unreachable!(),
            };
            let mut ordinary_meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
            let mut ordinary = OwnedPolicyCheck { execution: self.execution, registry: Some(&registry), meter: &mut ordinary_meter };
            let before = ordinary.meter.remaining_units_for_test();
            let old_error = ordinary.surface(source).err().expect("ordinary refusal");
            let old_cost = before - ordinary.meter.remaining_units_for_test();
            let mut selected_meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
            let mut selected = OwnedPolicyCheck { execution: self.execution, registry: Some(&registry), meter: &mut selected_meter };
            let before = selected.meter.remaining_units_for_test();
            selected.charge(POLICY_FRAME_CONSUME_PAYMENT)?;
            let error = SurfaceConsumeCall { check: &mut selected, recorder, source }.run().err().expect("selected original refusal");
            assert!(matches!(&error, StoreError::Invalid(text) if text == expected), "{error:?}");
            assert_eq!(format!("{error:?}"), format!("{old_error:?}"));
            assert_eq!(before - selected.meter.remaining_units_for_test(), old_cost + POLICY_FRAME_CONSUME_PAYMENT, "no F7 before {kind} refusal");
            assert!(recorder.completed.is_none());
            println!("POLICY_FRAME_SURFACE_REFUSAL kind={kind} ordinary={old_cost}");
        }
        Ok(())
    }
}

#[test]
fn policy_frame_seed_packets_cover_actual_owners() {
    // N0/N1 and their owners are retired. These are the surviving actual
    // lexical scope/epoch owners and the direct C-seed operation.
    for payment in [POLICY_CHRONOLOGY_SEED_PAYMENT, POLICY_FRAME_SCOPE_PAYMENT, POLICY_FRAME_EPOCH_PAYMENT] {
        OwnedPolicyCheck::assert_archive_packet_for_test(payment);
    }
    assert_eq!(size_of::<OwnedPolicyCheck<'_>>(), 3 * size_of::<usize>());
    println!("POLICY_SEED_LAYOUTS {}", serde_json::json!({
        "scope":size_of::<PolicyFrameScope<'_>>(),
        "epoch_inputs":size_of::<EpochSeedInputs<'_>>(),
        "store_result":size_of::<Result<()>>(),
        "c_seed":POLICY_CHRONOLOGY_SEED_PAYMENT, "scope_payment":POLICY_FRAME_SCOPE_PAYMENT,
        "epoch":POLICY_FRAME_EPOCH_PAYMENT,
    }));
}

// Assertion-only source/first-slot controls. No isolated epoch can fill a
// recorder. Later seed authority is exercised only by the complete loop below.
fn assert_seed_identity_and_slots(
    receipt: &CompletedPolicyFrames<'_>, surface: &owned_continuation::VerifiedOwnedPolicySurface,
) {
    let fresh = || ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false))).unwrap();
    let initialized = &receipt.recipe.initial().initialized;
    let equal = initialized.clone();
    assert_eq!(&equal, initialized);
    assert!(!std::ptr::eq(&equal, initialized));
    surface.validate_frame(initialized).unwrap();
    // Only actual committed recipes are used as positive source references.
    // Prospective non-epoch recipes are not assumed to share this Initial owner.
    for epoch in &receipt.registry.epochs {
        let recipe = &receipt.registry.recipes[&epoch.output];
        assert!(std::ptr::eq(&recipe.initial().initialized,
            receipt.registry.recipes[&receipt.registry.epochs[0].output].seed()));
        let mut meter = fresh();
        let other = match recipe {
            owned_persist::OwnedExecutionRecipe::Initial(original) =>
                owned_continuation::derive_policy_surface(original, receipt.layer, None, &mut meter),
            owned_persist::OwnedExecutionRecipe::Transition(prior) =>
                owned_continuation::derive_policy_surface(&prior.original, receipt.layer,
                    Some((&prior.continuation, &prior.target)), &mut meter),
        }.unwrap();
        run_chronological_frames_for_test(receipt.registry, receipt.snapshot,
            recipe, &other, &mut fresh()).unwrap();
        let first_hit = std::ptr::eq(recipe.seed(), &recipe.initial().initialized);
        // Ordinary zero-event history still invokes seed and final callbacks; only
        // the exact selected pure final predicate is implied by retained checks.
        let mut ordinary = fresh(); let before = ordinary.remaining_units_for_test(); let mut visits = 0;
        owned_replay::validate_recipe_history_with(recipe, recipe.seed(), recipe.basis().clock_day,
            recipe.seed(), recipe.basis().clock_day, &mut ordinary, &mut |frame, meter| {
                visits += 1; meter.charge(canonical_bytes(frame)?.len() as u64)?; other.validate_frame(frame)
            }).unwrap();
        assert_eq!(visits, 2);
        let mut generic = fresh(); let mut generic_visits = 0;
        let error = owned_replay::validate_recipe_history_with(recipe, recipe.seed(), recipe.basis().clock_day,
            recipe.seed(), recipe.basis().clock_day, &mut generic, &mut |_, _| {
                generic_visits += 1;
                if generic_visits == 2 { return Err(StoreError::Invalid("generic final-only sentinel".into())); }
                Ok(())
            }).unwrap_err();
        assert_eq!(generic_visits, 2);
        assert!(matches!(error, StoreError::Invalid(ref text) if text == "generic final-only sentinel"));
        let old_cost = before - ordinary.remaining_units_for_test();
        let mut selected = fresh(); let before = selected.remaining_units_for_test();
        selected.charge(POLICY_FRAME_EPOCH_PAYMENT).unwrap();
        selected.charge(POLICY_CHRONOLOGY_SEED_PAYMENT).unwrap();
        run_first_epoch_for_test(recipe, recipe, recipe.seed(), recipe.basis().clock_day,
            recipe.seed(), recipe.basis().clock_day, &mut selected, &other).unwrap();
        assert_eq!(before - selected.remaining_units_for_test(), old_cost + POLICY_FRAME_EPOCH_PAYMENT
            + POLICY_CHRONOLOGY_SEED_PAYMENT - (1 + u64::from(first_hit)) * canonical_bytes(recipe.seed()).unwrap().len() as u64);
        // A value-equal distinct seed retains the complete seed visitor; the
        // selected final pure predicate is independently implied after checks.
        let cloned = recipe.seed().clone(); let mut selected = fresh();
        let before = selected.remaining_units_for_test();
        selected.charge(POLICY_FRAME_EPOCH_PAYMENT).unwrap(); selected.charge(POLICY_CHRONOLOGY_SEED_PAYMENT).unwrap();
        run_first_epoch_for_test(recipe, recipe, &cloned, recipe.basis().clock_day,
            recipe.seed(), recipe.basis().clock_day, &mut selected, &other).unwrap();
        assert_eq!(before - selected.remaining_units_for_test(), old_cost + POLICY_FRAME_EPOCH_PAYMENT
            + POLICY_CHRONOLOGY_SEED_PAYMENT - canonical_bytes(recipe.seed()).unwrap().len() as u64);
        // An original endpoint/header error must still occur before the seed slot.
        let mut bad = recipe.seed().clone(); bad.project_id.push_str("-wrong-seed-header");
        let mut old = fresh(); let old_before = old.remaining_units_for_test(); let mut visited = false;
        let error = owned_replay::validate_recipe_history_with(recipe, recipe.seed(), recipe.basis().clock_day,
            &bad, recipe.basis().clock_day, &mut old, &mut |_, _| { visited = true; Ok(()) }).unwrap_err();
        assert!(!visited);
        let old_cost = old_before - old.remaining_units_for_test();
        let mut selected = fresh(); let before = selected.remaining_units_for_test();
        selected.charge(POLICY_FRAME_EPOCH_PAYMENT).unwrap(); selected.charge(POLICY_CHRONOLOGY_SEED_PAYMENT).unwrap();
        let selected_error = run_first_epoch_for_test(recipe, recipe, recipe.seed(), recipe.basis().clock_day,
            &bad, recipe.basis().clock_day, &mut selected, &other).unwrap_err();
        assert_eq!(format!("{selected_error:?}"), format!("{error:?}"));
        assert_eq!(before - selected.remaining_units_for_test(), old_cost + POLICY_FRAME_EPOCH_PAYMENT + POLICY_CHRONOLOGY_SEED_PAYMENT);
        println!("POLICY_SEED_IDENTITY_AND_SLOTS transition={}", matches!(recipe, owned_persist::OwnedExecutionRecipe::Transition(_)));
    }
}
impl<'gate> OwnedPolicyCheck<'gate> {
    pub(crate) fn assert_seed_packet_for_test(
        &mut self, recorder: &mut PolicyFrameRecorder<'gate>, source: &CapturedProgram, boundary: u8, exact: bool,
    ) -> Result<()> {
        assert!(recorder.completed.is_none());
        let registry = self.registry.unwrap(); let recipe = registry.resolve(source)?;
        let layer = &self.execution.current().owned_scope()?.layers[self.execution.continuation.original_layer()];
        let mut reference = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
        let before = reference.remaining_units_for_test();
        let _surface = match recipe {
            owned_persist::OwnedExecutionRecipe::Initial(original) => owned_continuation::derive_policy_surface(original, layer, None, &mut reference)?,
            owned_persist::OwnedExecutionRecipe::Transition(prior) => owned_continuation::derive_policy_surface(&prior.original, layer,
                Some((&prior.continuation, &prior.target)), &mut reference)?,
        };
        let derive_cost = before - reference.remaining_units_for_test();
        let packets = [POLICY_FRAME_SCOPE_PAYMENT, POLICY_FRAME_EPOCH_PAYMENT, POLICY_CHRONOLOGY_SEED_PAYMENT];
        let prefix = POLICY_FRAME_RECORD_PAYMENT + 5 + derive_cost + packets[..boundary as usize].iter().sum::<u64>();
        let packet = packets[boundary as usize];
        self.leave_policy_frame_units_for_test(prefix + packet - u64::from(!exact));
        self.charge(POLICY_FRAME_RECORD_PAYMENT)?;
        let error = SurfaceRecordCall { check: self, recorder, source }.run().err().expect("actual paid boundary must stop");
        assert!(matches!(error, StoreError::Invalid(ref text) if text == "Common factual admission: budget: cumulative proof/initialization budget exhausted"), "{error:?}");
        self.assert_policy_frame_units_for_test(if exact { 0 } else { packet - 1 });
        assert!(recorder.completed.is_none());
        println!("POLICY_SEED_ACTUAL_BOUNDARY boundary={boundary} exact={exact} packet={packet} derive={derive_cost}");
        Ok(())
    }
}

#[test]
fn policy_frame_row_hybrid_packets_cover_actual_owners() {
    let open = comparison_payment(64, [
        (size_of::<PolicyFrameScope<'_>>(), 4),
        (size_of::<Result<()>>(), 8),
    ]);
    let epoch = comparison_payment(32, [
        (size_of::<EpochSeedInputs<'_>>(), 4),
        (size_of::<Result<()>>(), 4),
    ]);
    assert_eq!(POLICY_FRAME_SCOPE_PAYMENT, open.max(1536));
    assert_eq!(POLICY_FRAME_EPOCH_PAYMENT, epoch.max(832));
    for payment in [POLICY_FRAME_SCOPE_PAYMENT, POLICY_FRAME_EPOCH_PAYMENT] {
        OwnedPolicyCheck::assert_archive_packet_for_test(payment);
    }
    assert_eq!(POLICY_CHRONOLOGY_SEED_PAYMENT, 1744);
    println!("POLICY_FRAME_FUSION_LAYOUTS {}", serde_json::json!({
        "scope":size_of::<PolicyFrameScope<'_>>(),
        "epoch_inputs":size_of::<EpochSeedInputs<'_>>(),
        "store_result":size_of::<Result<()>>(),
        "actual_open":open,"actual_epoch":epoch,
        "paid_open":POLICY_FRAME_SCOPE_PAYMENT,"paid_epoch":POLICY_FRAME_EPOCH_PAYMENT,
    }));
}

// The complete chronological expansion uses the actual immutable registry and
// existing enumerate index. Fixed test-only observations never enter production.
fn run_chronological_frames_for_test(
    registry: &owned_persist::ReconstructedOwned, snapshot: &ProjectSnapshot,
    source_recipe: &owned_persist::OwnedExecutionRecipe,
    surface: &owned_continuation::VerifiedOwnedPolicySurface, meter: &mut ProofMeter,
) -> Result<[u64; 2]> {
    assert!(registry.epochs.len() <= 2, "bounded genuine fixture");
    let mut costs = [0; 2];
    let mut previous_endpoint: Option<&DataSnapshot> = None;
    owned_persist::policy_frames_body!(registry, snapshot, surface, meter, [], [
        if let Some(previous) = previous_endpoint { assert!(std::ptr::eq(previous, recipe.seed())); }
        let before = meter.remaining_units_for_test();
        meter.charge(POLICY_FRAME_EPOCH_PAYMENT)?;
        meter.charge(POLICY_CHRONOLOGY_SEED_PAYMENT)?;
    ], [
        policy_epoch_body!(recipe, recipe.seed(), recipe.basis().clock_day,
            endpoint, day, meter, surface, source_recipe, epoch_index);
        costs[epoch_index] = before - meter.remaining_units_for_test();
        previous_endpoint = Some(endpoint);
        println!("POLICY_CHRONOLOGY_COMPLETE_EPOCH index={epoch_index} suffix_events={} source_transition={}",
            endpoint.events.len() - recipe.seed().events.len(),
            matches!(source_recipe, owned_persist::OwnedExecutionRecipe::Transition(_)));
    ], epoch_index, recipe, endpoint, day, unused_visit, [Ok(costs)])
}

// Isolated negative/base controls are permanently first-epoch only. There is no
// arbitrary skip index parameter and no recorder/mint authority in this helper.
fn run_first_epoch_for_test(
    source_recipe: &owned_persist::OwnedExecutionRecipe,
    recipe: &owned_persist::OwnedExecutionRecipe, seed: &DataSnapshot, seed_day: i32,
    endpoint: &DataSnapshot, day: i32, meter: &mut ProofMeter,
    surface: &owned_continuation::VerifiedOwnedPolicySurface,
) -> Result<()> {
    let epoch_index = 0;
    policy_epoch_body!(recipe, seed, seed_day, endpoint, day, meter, surface, source_recipe, epoch_index);
    Ok(())
}

// The ordinary history Update guard excludes this sealed-row fast path before
// row proof. This is deliberately a direct original-row continuation control, not
// a forged full event-route history or a naturally reachable early-return case.
fn original_row_then_failure_for_test(
    recipe: &owned_persist::OwnedExecutionRecipe, endpoint: &DataSnapshot, day: i32,
    row: &Record, surface: &owned_continuation::VerifiedOwnedPolicySurface,
    source_recipe: &owned_persist::OwnedExecutionRecipe, mode: u8, reached: &mut bool,
) -> Result<()> {
    let mut meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
    let prior = Some(row);
    owned_replay::protected_row(recipe, recipe.seed(), row, prior, recipe.basis().clock_day, &mut meter)?;
    *reached = true;
    if mode == 0 {
        let mut bad = row.clone();
        let member = recipe.metadata().member.clone();
        let DataValue::Boolean { value } = bad.values.get_mut(&member).unwrap() else { panic!("member Boolean"); };
        *value = !*value;
        let bad = &bad;
        owned_replay::protected_row(recipe, recipe.seed(), bad, prior, recipe.basis().clock_day, &mut meter)?;
    } else {
        let mut bad = endpoint.clone();
        if mode == 1 {
            let event = bad.events.get_mut(recipe.seed().events.len()).expect("genuine post-adoption completion");
            event.id.push_str("-bad-later-event");
        } else {
            let root = bad.records.iter_mut().find(|r| r.entity == row.entity && r.id == row.id).unwrap();
            root.revision += 1;
        }
        meter.charge(POLICY_FRAME_EPOCH_PAYMENT)?;
        meter.charge(POLICY_CHRONOLOGY_SEED_PAYMENT)?;
        let epoch_index = 0;
        policy_epoch_body!(recipe, recipe.seed(), recipe.basis().clock_day,
            &bad, day, &mut meter, surface, source_recipe, epoch_index);
    }
    panic!("an original row return must not skip the later original refusal")
}
impl OwnedPolicyCheck<'_> {
    pub(crate) fn assert_fused_row_continuation_for_test(
        &self, recorder: &PolicyFrameRecorder<'_>, source: &CapturedProgram,
    ) -> Result<()> {
        assert!(recorder.completed.is_none());
        let registry = self.registry.unwrap();
        let source_recipe = registry.resolve(source)?;
        let layer = &self.execution.current().owned_scope()?.layers[self.execution.continuation.original_layer()];
        let mut meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
        let surface = match source_recipe {
            owned_persist::OwnedExecutionRecipe::Initial(original) =>
                owned_continuation::derive_policy_surface(original, layer, None, &mut meter)?,
            owned_persist::OwnedExecutionRecipe::Transition(prior) =>
                owned_continuation::derive_policy_surface(&prior.original, layer,
                    Some((&prior.continuation, &prior.target)), &mut meter)?,
        };
        assert_eq!(registry.epochs.len(), 2, "existing bounded evolved fixture only");
        let recipe = &registry.recipes[&registry.epochs[0].output];
        let next = &registry.recipes[&registry.epochs[1].output];
        let row = recipe.seed().records.iter().find(|row| row.entity == recipe.metadata().root
            && row.values.get(&recipe.metadata().sealed) == Some(&DataValue::Boolean { value: true }))
            .expect("genuine initially completed row");
        // G2-row short/exact controls are historical: the whole selected row
        // packet is removed. The actual original sealed-row call has no row
        // entry debit. This direct control is not a forged event-route case.
        let mut row_meter = ProofMeter::new(ProofBudget { units: 1, elapsed_millis: 60_000 },
            Arc::new(AtomicBool::new(false)))?;
        assert_eq!(row_meter.remaining_units_for_test(), 0);
        let mut returned = false;
        original_sealed_row_boundary_for_test(recipe, row, &mut row_meter, &mut returned)?;
        assert!(returned, "the original row's success must return only to its caller");
        assert_eq!(row_meter.remaining_units_for_test(), 0);
        assert!(recorder.completed.is_none());
        println!("POLICY_FRAME_DIRECT_ORIGINAL_ROW returned={returned} remaining=0");
        for (mode, message) in ["immutable owned population changed",
            "event sequence/day/identity differs from the actual runtime chain",
            "current owned revision differs from its exact protected history"].iter().enumerate() {
            let mut reached = false;
            let error = original_row_then_failure_for_test(recipe, next.seed(), next.basis().clock_day,
                row, &surface, source_recipe, mode as u8, &mut reached).unwrap_err();
            assert!(reached, "the genuine sealed row must finish locally before mode {mode}");
            assert!(matches!(&error, StoreError::Invalid(text) if text ==
                &format!("Common factual admission: owned protected history: {message}")), "{error:?}");
            assert!(recorder.completed.is_none());
            println!("POLICY_FRAME_DIRECT_ROW_CONTINUATION mode={mode} reached={reached} error={error:?}");
        }
        Ok(())
    }
}

fn original_sealed_row_boundary_for_test(
    recipe: &owned_persist::OwnedExecutionRecipe, row: &Record, meter: &mut ProofMeter,
    returned: &mut bool,
) -> Result<()> {
    let prior = Some(row);
    owned_replay::protected_row(recipe, recipe.seed(), row, prior, recipe.basis().clock_day, meter)?;
    *returned = true;
    Ok(())
}

// New admitted proposition: completion, not tuple equality, is the witness.
#[test]
fn policy_frame_dominance_layout_and_retained_floors() {
    assert_eq!(size_of::<CompletedPolicyFrames<'_>>(), 5 * size_of::<usize>());
    assert_eq!(size_of::<PolicyFrameRecorder<'_>>(), 7 * size_of::<usize>());
    assert_eq!(size_of::<PolicyFrameScope<'_>>(), 2 * size_of::<usize>());
    assert_eq!(policy_frame_open_payment::<()>(), 3232);
    assert_eq!(POLICY_FRAME_RECORD_PAYMENT, 1984);
    assert_eq!(POLICY_FRAME_SCOPE_PAYMENT, 1536);
    assert_eq!(POLICY_FRAME_EPOCH_PAYMENT, 832);
    assert_eq!(POLICY_FRAME_MINT_PAYMENT, 1856);
    assert_eq!(POLICY_FRAME_SELECT_PAYMENT, 18220);
    assert_eq!(POLICY_FRAME_CONSUME_PAYMENT, 2784);
}

// Assertion-only endpoint corruption and generic-reference controls. All
// positive frames are borrowed from the genuine registry; clones below are
// explicitly negative inputs and can never mint a receipt.
impl OwnedPolicyCheck<'_> {
    pub(crate) fn assert_final_predicate_boundaries_for_test(
        &self, recorder: &PolicyFrameRecorder<'_>, source: &CapturedProgram,
    ) -> Result<()> {
        assert!(recorder.completed.is_none());
        let registry = self.registry.unwrap();
        let source_recipe = registry.resolve(source)?;
        let layer = &self.execution.current().owned_scope()?.layers[self.execution.continuation.original_layer()];
        let fresh = || ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false))).unwrap();
        let mut derive = fresh();
        let surface = match source_recipe {
            owned_persist::OwnedExecutionRecipe::Initial(original) =>
                owned_continuation::derive_policy_surface(original, layer, None, &mut derive)?,
            owned_persist::OwnedExecutionRecipe::Transition(prior) =>
                owned_continuation::derive_policy_surface(&prior.original, layer,
                    Some((&prior.continuation, &prior.target)), &mut derive)?,
        };
        run_chronological_frames_for_test(registry, self.execution.current(),
            source_recipe, &surface, &mut fresh())?;
        let mut shared_difference = false;
        for (index, epoch) in registry.epochs.iter().enumerate() {
            let recipe = &registry.recipes[&epoch.output];
            let (endpoint, day) = if let Some(next) = registry.epochs.get(index + 1) {
                let next = &registry.recipes[&next.output];
                (next.seed(), next.basis().clock_day)
            } else { (&self.execution.current().data, self.execution.current().clock_day) };
            let expected_visits = endpoint.events.len() - recipe.seed().events.len() + 2;
            let mut visits = 0; let mut predecessor = None;
            owned_replay::validate_recipe_history_with(recipe, recipe.seed(), recipe.basis().clock_day,
                endpoint, day, &mut fresh(), &mut |frame, meter| {
                    visits += 1;
                    meter.charge(canonical_bytes(frame)?.len() as u64)?;
                    surface.validate_frame(frame)?;
                    if visits + 1 == expected_visits { predecessor = Some(frame.clone()); }
                    Ok(())
                })?;
            assert_eq!(visits, expected_visits);
            let predecessor = predecessor.unwrap();
            assert_eq!(predecessor.schema, endpoint.schema);
            assert_eq!(predecessor.records.len(), endpoint.records.len());
            for (before, after) in predecessor.records.iter().zip(&endpoint.records) {
                assert_eq!((&before.entity, &before.id, &before.values, before.archived),
                    (&after.entity, &after.id, &after.values, after.archived));
                if before.revision != after.revision {
                    assert!(!owned_replay::owned_entity(recipe, &before.entity));
                    shared_difference = true;
                }
            }
            println!("POLICY_FINAL_GENUINE_EPOCH index={index} ordinary_visits={visits}");

            let seed = recipe.seed();
            let root = seed.records.iter().position(|row| row.entity == recipe.metadata().root).unwrap();
            let mut cases = Vec::new();
            let mut bad = seed.clone(); bad.schema[0].label.push_str(" changed"); cases.push(("schema", bad));
            let mut bad = seed.clone(); bad.project_id.push_str("-changed"); cases.push(("project", bad));
            let mut bad = seed.clone(); bad.records.swap(0, 1); cases.push(("order", bad));
            let mut bad = seed.clone(); bad.records.pop(); cases.push(("count", bad));
            let mut bad = seed.clone(); bad.records[root].id.push_str("-changed"); cases.push(("identity", bad));
            let mut bad = seed.clone(); bad.records.push(seed.records[root].clone()); cases.push(("duplicate", bad));
            let mut bad = seed.clone();
            let DataValue::Boolean { value } = bad.records[root].values.get_mut(&recipe.metadata().member).unwrap() else { panic!("member Boolean"); };
            *value = !*value; cases.push(("values", bad));
            let mut bad = seed.clone(); bad.records[root].archived = !bad.records[root].archived; cases.push(("archive", bad));
            let mut bad = seed.clone(); bad.records[root].created_program = "f".repeat(64).try_into().unwrap();
            assert_ne!(bad.records[root].created_program, seed.records[root].created_program); cases.push(("created-program", bad));
            let mut bad = seed.clone(); bad.records[root].revision += 1; cases.push(("owned-revision", bad));
            if let Some(shared) = seed.records.iter().position(|row| !owned_replay::owned_entity(recipe, &row.entity)) {
                let mut bad = seed.clone(); bad.records[shared].revision -= 1; cases.push(("shared-under-minimum", bad));
                let mut bad = seed.clone(); bad.records[shared].revision += 1; cases.push(("shared-infeasible", bad));
                let mut bad = seed.clone(); bad.records[shared].revision = u64::MAX; cases.push(("shared-max-infeasible", bad));
            }
            for (name, bad) in cases {
                let mut visited = 0;
                let old = owned_replay::validate_recipe_history_with(recipe, seed, recipe.basis().clock_day,
                    &bad, recipe.basis().clock_day, &mut fresh(), &mut |frame, meter| {
                        visited += 1; meter.charge(canonical_bytes(frame)?.len() as u64)?; surface.validate_frame(frame)
                    }).unwrap_err();
                assert!(visited < 2, "{name}: original endpoint refusal must precede final callback");
                let selected = run_first_epoch_for_test(recipe, recipe, seed, recipe.basis().clock_day,
                    &bad, recipe.basis().clock_day, &mut fresh(), &surface).unwrap_err();
                assert_eq!(format!("{old:?}"), format!("{selected:?}"), "{name}");
                assert!(!old.to_string().contains("budget"), "negative semantic input must not hide a resource refusal");
                assert!(recorder.completed.is_none());
                println!("POLICY_FINAL_NEGATIVE epoch={index} case={name} visits={visited} error={old:?}");
            }
        }
        println!("POLICY_FINAL_GENUINE_SHARED_REVISION_DIFFERENCE observed={shared_difference}");
        assert!(recorder.completed.is_none());
        Ok(())
    }
}

#[test]
fn policy_frame_chronology_selector_payment() {
    let expected = comparison_payment(64, [
        (size_of::<usize>(), 8),
        (size_of::<bool>(), 16),
        (size_of::<&owned_persist::OwnedExecutionRecipe>(), 8),
        (size_of::<&PreparedOwnedPreview>(), 8),
        (size_of::<&DataSnapshot>(), 16),
        (size_of::<&owned_continuation::VerifiedOwnedPolicySurface>(), 8),
        (size_of::<&mut ProofMeter>(), 8),
        (size_of::<Result<()>>(), 8),
    ]);
    assert_eq!(expected, 1744);
    assert_eq!(POLICY_CHRONOLOGY_SEED_PAYMENT, expected);
    OwnedPolicyCheck::assert_archive_packet_for_test(expected);
}

impl OwnedPolicyCheck<'_> {
    pub(crate) fn assert_zero_event_chronology_for_test(&self) {
        let registry = self.registry.unwrap();
        assert_eq!(registry.epochs.len(), 2);
        for (index, epoch) in registry.epochs.iter().enumerate() {
            let recipe = &registry.recipes[&epoch.output];
            let endpoint = match registry.epochs.get(index + 1) {
                Some(next) => registry.recipes[&next.output].seed(),
                None => &self.execution.current().data,
            };
            assert_eq!(recipe.seed().events.len(), endpoint.events.len());
            println!("POLICY_CHRONOLOGY_ZERO_EPOCH index={index} suffix_events=0");
        }
    }
}

// V11 accounting controls are staged before the production route. These are
// synthetic necessary conditions, not completed-origin or archive authority.
#[test]
fn historical_initial_loan_packets_cover_full_results() {
    assert_eq!(size_of::<owned_persist::HistoricalInitialLoan<'_>>(), 2 * size_of::<usize>());
    assert_eq!(size_of::<OwnedPolicyCheck<'_>>(), 3 * size_of::<usize>());
    for packet in [owned_persist::historical_initial_callback_payment::<()>(),
        owned_persist::HISTORICAL_INITIAL_SURFACE_PAYMENT] {
        assert!(packet >= 4 * size_of::<Result<()>>() as u64);
        assert_ne!(packet, u64::MAX, "compile-only measurement scaffold cannot run");
    }
}

#[cfg(target_pointer_width = "64")]
const _: [(); 16] = [(); size_of::<(&mut OwnedPolicyCheck<'_>, &owned_persist::HistoricalInitialLoan<'_>)>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 24] = [(); size_of::<(&PreparedOwnedPreview, &CapturedProgram, &CapturedProgram)>()];

// V11 one-source component parity. No loan/registry/origin escapes the fixed
// callback; these test-only observations are not production accounting credit.
impl ProjectSnapshot {
    pub(crate) fn assert_historical_initial_loan_for_test(&self, historical: &ProjectSnapshot) {
        let mut original_surfaces = Vec::new();
        let mut original_calls = 0;
        check_retained_transition_policies(self, |check| {
            original_calls += 1;
            assert!(check.registry.is_none());
            assert_eq!(check.execution.current(), historical);
            let execution = check.execution;
            let source = execution.current().program()?;
            let surface = check.surface(source)?;
            assert_eq!(surface.source(), &revision(source)?);
            original_surfaces.push(surface);
            Ok(())
        }).unwrap();
        assert_eq!(original_calls, 1);
        assert_eq!(original_surfaces.len(), 1);

        let mut selected_surfaces = Vec::new();
        let mut selected_calls = 0;
        owned_persist::check_retained_transition_policies_with_initials(self, |check, loan| {
            selected_calls += 1;
            assert!(check.registry.is_none());
            assert_eq!(check.execution.current(), historical);
            let execution = check.execution;
            let source = execution.current().program()?;
            check.charge(owned_persist::HISTORICAL_INITIAL_SURFACE_PAYMENT)?;
            let surface = check.surface_with_initial_loan(loan, source)?;
            assert_eq!(surface.source(), &revision(source)?);
            selected_surfaces.push(surface);
            Ok(())
        }).unwrap();
        assert_eq!(selected_calls, 1);
        assert_eq!(selected_surfaces.len(), 1);
        for (original, selected) in original_surfaces.iter().zip(&selected_surfaces) {
            selected.assert_same_value_for_test(original);
            assert_eq!(selected.source(), original.source());
            assert_eq!(selected.original_layer(), original.original_layer());
            assert_eq!(selected.baseline(), original.baseline());
            assert_eq!(selected.candidate(), original.candidate());
        }
        println!("HISTORICAL_INITIAL_COMPONENT_PARITY original_callbacks=1 selected_callbacks=1 sources=1 full_surface_equal=1");
    }
}

// V12 tests-first fixed ownership and short/exact accounting. These synthetic
// controls do not create an interval capability or establish route affordability.
#[test]
fn historical_initial_interval_actual_accounting() {
    let (layouts, packets) = owned_persist::HISTORICAL_INITIAL_INTERVAL_CENSUS;
    assert_eq!(layouts[0], 2 * size_of::<usize>());
    assert_eq!(layouts[1], layouts[0]);
    assert_eq!(layouts[2], size_of::<Result<owned_persist::ReconstructedOwned>>());
    assert_eq!(layouts[3], size_of::<Result<owned_continuation::VerifiedOwnedPolicySurface>>());
    assert_eq!(layouts[4], size_of::<Result<bool>>());
    assert_eq!(layouts[5], size_of::<Result<()>>());
    assert_eq!(layouts[6], size_of::<Result<std::convert::Infallible>>());
    assert_eq!(layouts[7], size_of::<Option<u128>>());
    assert_eq!(layouts[8], size_of::<RuntimeLimits>());
    assert_eq!(layouts[9], size_of::<OwnedPolicyCheck<'_>>());
    assert_eq!(layouts[9], 3 * size_of::<usize>(), "existing check layout stays unchanged");
    for packet in packets {
        assert!(packet >= 4 * size_of::<Result<()>>() as u64);
        assert_ne!(packet, u64::MAX, "compile-only failclosed scaffold must never run");
    }
    println!("HISTORICAL_INITIAL_INTERVAL_ACCOUNTING layouts={layouts:?} packets={packets:?}");
}

#[test]
fn historical_initial_interval_entry_short_exact() {
    let cost = owned_persist::HISTORICAL_INITIAL_INTERVAL_CENSUS.1[0];
    assert!(cost > 0 && cost < u64::MAX);
    for available in [cost - 1, cost] {
        let mut meter = ProofMeter::new(ProofBudget { units: available.checked_add(1).unwrap(),
            elapsed_millis: ProofBudget::default().elapsed_millis },
            Arc::new(AtomicBool::new(false))).unwrap();
        let result = meter.charge(cost);
        assert_eq!(result.is_ok(), available == cost);
        assert_eq!(meter.remaining_units_for_test(), if available == cost { 0 } else { available });
        if available < cost {
            assert!(matches!(result, Err(StoreError::Invalid(text)) if text ==
                "Common factual admission: budget: cumulative proof/initialization budget exhausted"));
        }
        println!("HISTORICAL_INITIAL_INTERVAL_I0_BOUNDARY exact={} cost={cost}", available == cost);
    }
}

#[test]
fn historical_initial_interval_mint_short_exact() {
    let cost = owned_persist::HISTORICAL_INITIAL_INTERVAL_CENSUS.1[1];
    assert!(cost > 0 && cost < u64::MAX);
    for available in [cost - 1, cost] {
        let mut meter = ProofMeter::new(ProofBudget { units: available.checked_add(1).unwrap(),
            elapsed_millis: ProofBudget::default().elapsed_millis },
            Arc::new(AtomicBool::new(false))).unwrap();
        let result = meter.charge(cost);
        assert_eq!(result.is_ok(), available == cost);
        assert_eq!(meter.remaining_units_for_test(), if available == cost { 0 } else { available });
        if available < cost {
            assert!(matches!(result, Err(StoreError::Invalid(text)) if text ==
                "Common factual admission: budget: cumulative proof/initialization budget exhausted"));
        }
        println!("HISTORICAL_INITIAL_INTERVAL_I1_BOUNDARY exact={} cost={cost}", available == cost);
    }
}

#[test]
fn historical_initial_interval_selector_short_exact() {
    let cost = owned_persist::HISTORICAL_INITIAL_INTERVAL_CENSUS.1[2];
    assert!(cost > 0 && cost < u64::MAX);
    for available in [cost - 1, cost] {
        let mut meter = ProofMeter::new(ProofBudget { units: available.checked_add(1).unwrap(),
            elapsed_millis: ProofBudget::default().elapsed_millis },
            Arc::new(AtomicBool::new(false))).unwrap();
        let result = meter.charge(cost);
        assert_eq!(result.is_ok(), available == cost);
        assert_eq!(meter.remaining_units_for_test(), if available == cost { 0 } else { available });
        if available < cost {
            assert!(matches!(result, Err(StoreError::Invalid(text)) if text ==
                "Common factual admission: budget: cumulative proof/initialization budget exhausted"));
        }
        println!("HISTORICAL_INITIAL_INTERVAL_I2_BOUNDARY exact={} cost={cost}", available == cost);
    }
}

// Producing target values are compiler-checked, never inferred from a run.
#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(owned_persist::HISTORICAL_INITIAL_INTERVAL_CENSUS.1[0] == 14856);
    assert!(owned_persist::HISTORICAL_INITIAL_INTERVAL_CENSUS.1[1] == 3968);
    assert!(owned_persist::HISTORICAL_INITIAL_INTERVAL_CENSUS.1[2] == 12136);
};

// V16 pure serialization regression only. These deliberately unadmitted values
// are never a registry, completed constructor, loan, or positive authority.
#[test]
fn historical_state_nested_serialization_preserves_exact_values() {
    let id = canonical_digest(IdentityDomain::Evidence, &"synthetic-state-serialization").unwrap();
    for version in [0, OWNED_SCOPE_VERSION, u32::MAX] {
        let mut state = OwnedScopeStateV2::default();
        state.version = version;
        for operation in ["z\\\n\"", "a中文", "é"] {
            state.initializations.push(OwnedInitializationV2 {
                version, operation: operation.into(), layer: id.clone(),
                before_data: id.clone(), after_data: id.clone(),
                before_schema: id.clone(), after_schema: id.clone(),
                additions: vec![], auxiliary: vec![],
                business_projection: id.clone(), original_events: id.clone(),
                original_records: 7, original_event_count: 3, capture_day: -17,
                capture_program: id.clone(), prior_snapshot: id.clone(),
                adoption_revision: u64::MAX,
            });
        }
        let standalone = canonical_bytes(&state).unwrap();
        let envelope = ScopeEnvelope::OwnedV2(state.clone());
        let containing_value = serde_json::json!({"z": 1, "scope": envelope, "a": 2});
        let complete = canonical_bytes(&containing_value).unwrap();
        let decoded: serde_json::Value = serde_json::from_slice(&complete).unwrap();
        assert_eq!(decoded["scope"]["family"], "owned_v2");
        assert_eq!(standalone, canonical_bytes(&decoded["scope"]["state"]).unwrap());
        assert_eq!(decoded["scope"]["state"]["version"], version);
        assert_eq!(decoded["scope"]["state"]["initializations"][0]["operation"], "z\\\n\"");
        assert_eq!(decoded["scope"]["state"]["initializations"][1]["operation"], "a中文");
        assert_eq!(decoded["scope"]["state"]["initializations"][2]["operation"], "é");
        state.initializations.reverse();
        assert_ne!(standalone, canonical_bytes(&state).unwrap(), "array order is semantic, never sorted away");
    }
    println!("HISTORICAL_STATE_SERIALIZATION_PROJECTION versions=3 ordered_entries=3 authority=false");
}

// V17 tests-first guard-elision accounting and direct debit boundary control.
// Synthetic only: these checks do not mint a loan or establish route affordability.
#[test]
fn historical_initial_interval_guard_elision_accounting() {
    let (layouts, packets) = owned_persist::HISTORICAL_INITIAL_INTERVAL_CENSUS;
    assert_eq!(packets[0], 14856);
    assert_eq!(packets[1], 3968);
    assert_eq!(layouts[0], 2 * size_of::<usize>());
    assert_eq!(layouts[1], layouts[0]);
    assert_eq!(layouts[4], size_of::<Result<bool>>());
    assert_eq!(layouts[5], size_of::<Result<()>>());
    assert_eq!(layouts[6], size_of::<Result<std::convert::Infallible>>());
    assert_eq!(layouts[7], size_of::<Option<u128>>());
    assert_eq!(layouts[8], size_of::<RuntimeLimits>());
    assert_eq!(packets[2], 12136, "actual full-type producing formula");
    historical_initial_interval_selector_short_exact();
    assert!(packets[2] > 0 && packets[2] < 27304,
        "only compiler-proven absent guard work may leave the unchanged packet");
    println!("HISTORICAL_INITIAL_INTERVAL_GUARD_ELISION layouts={layouts:?} packets={packets:?}");
}

// V18 tests-first selected window transport census and synthetic debit boundary.
// This does not call the private observation helper or create completed authority.
#[test]
fn historical_initial_interval_window_transport_accounting() {
    let (layouts, packets) = owned_persist::HISTORICAL_INITIAL_INTERVAL_CENSUS;
    assert_eq!(packets[0], 14856);
    assert_eq!(packets[1], 3968);
    assert_eq!(layouts[4], size_of::<Result<bool>>(), "ordinary full Result remains");
    assert_eq!(layouts[5], size_of::<Result<()>>(), "relocated poll remains full");
    assert_eq!(layouts[6], size_of::<Result<std::convert::Infallible>>());
    assert_eq!(layouts[7], size_of::<Option<u128>>());
    assert_eq!(layouts[8], size_of::<RuntimeLimits>());
    assert_eq!(size_of::<&ProofMeter>(), size_of::<usize>());
    assert_eq!(size_of::<bool>(), 1);
    assert_eq!(packets[2], 12136, "actual complete selected transport formula");
    historical_initial_interval_selector_short_exact();
    println!("HISTORICAL_INITIAL_INTERVAL_WINDOW_TRANSPORT layouts={layouts:?} packets={packets:?}");
}

// V19 tests-first actual full-error transport accounting and direct debit boundary.
// Synthetic only: no reconstruction, interval authority or route-fit claim.
#[test]
fn historical_initial_interval_same_error_transport_accounting() {
    let (layouts, packets) = owned_persist::HISTORICAL_INITIAL_INTERVAL_CENSUS;
    assert_eq!(packets[0], 14856);
    assert_eq!(packets[1], 3968);
    assert_eq!(layouts[5], size_of::<Result<()>>(), "retained full callee Result");
    assert_eq!(size_of::<StoreError>(), 152, "actual complete moved error payload");
    assert_eq!(layouts[7], size_of::<Option<u128>>());
    assert_eq!(layouts[8], size_of::<RuntimeLimits>());
    assert_eq!(packets[2], 12136, "complete same-error and post-built cleanup packet");
    historical_initial_interval_selector_short_exact();
    println!("HISTORICAL_INITIAL_INTERVAL_SAME_ERROR_TRANSPORT layouts={layouts:?} packets={packets:?}");
}

// Cause-return contract: final payment follows the complete reviewed full-type census.
// Synthetic representation/error equivalence only, no route or affordability claim.
#[test]
fn historical_initial_interval_cause_return_measurement_contract() {
    use crate::product_store::scope::common_fact_admission::ProofChargeFailure;
    assert_eq!(size_of::<ProofChargeFailure>(), 1);
    assert_eq!(size_of::<std::result::Result<(), ProofChargeFailure>>(), 1);
    assert_eq!(size_of::<StoreError>(), 152);
    let (_, packets) = owned_persist::HISTORICAL_INITIAL_INTERVAL_CENSUS;
    assert_eq!(packets, [14856, 3968, 12136]);
    for (cause, expected) in [
        (ProofChargeFailure::Cancelled, "Common factual admission: budget: cancelled or cumulative elapsed budget exhausted"),
        (ProofChargeFailure::Elapsed, "Common factual admission: budget: cancelled or cumulative elapsed budget exhausted"),
        (ProofChargeFailure::Fuel, "Common factual admission: budget: cumulative proof/initialization budget exhausted"),
    ] {
        match cause.historical_initial_interval_error() {
            StoreError::Invalid(message) => assert_eq!(message, expected),
            _ => panic!("selected cause must retain the original Invalid error variant"),
        }
    }
}
