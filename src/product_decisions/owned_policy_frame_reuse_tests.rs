//! Genuine locked-writer controls; publication is stopped at an assertion boundary.
use super::*;
const FRAME_SENTINEL: &str = "policy frame assertion boundary";

#[test]
fn policy_frame_export_lexical_packets_are_exact() {
    // Print the producing owner decomposition even if an exact packet check fails.
    println!("POLICY_FRAME_DECISION_LAYOUTS {}", serde_json::json!({
        "pointer_width": usize::BITS,
        "os_string": std::mem::size_of::<std::ffi::OsString>(),
        "os_string_align": std::mem::align_of::<std::ffi::OsString>(),
        "product_store": std::mem::size_of::<ProductStore>(),
        "product_store_align": std::mem::align_of::<ProductStore>(),
        "archive_owner": std::mem::size_of::<IntentArchive>(),
        "archive_owner_align": std::mem::align_of::<IntentArchive>(),
        "recorder_borrow": std::mem::size_of::<&crate::product_store::scope::PolicyFrameRecorder<'_>>(),
        "bundle_borrow": std::mem::size_of::<&IntentionBundle>(),
        "record_decision": std::mem::size_of::<super::super::owned_policy::RecordDecisionCall<'_, '_>>(),
        "receipt_decision": std::mem::size_of::<super::super::owned_policy::ReceiptDecisionCall<'_, '_, '_>>(),
        "continuation_capture": std::mem::size_of::<RecordContinuationCapture<'_, '_, LocalRuntime>>(),
        "continuation_dispatch": std::mem::size_of::<crate::product_store::scope::RecordContinuationDispatch<'_, RecordContinuationCapture<'_, '_, LocalRuntime>>>(),
        "f0": policy_frame_continuation_payment::<LocalRuntime>(),
        "fp": super::super::owned_policy::POLICY_FRAME_DECISION_RECORD_PAYMENT,
        "fa": super::super::owned_policy::POLICY_FRAME_DECISION_CONSUME_PAYMENT,
        "f5": super::super::bundle::POLICY_FRAME_EXPORT_PAYMENT,
        "decision_result": std::mem::size_of::<Result<()>>(),
        "store_result": std::mem::size_of::<std::result::Result<(), StoreError>>(),
    }));
    assert_eq!(usize::BITS, 64, "the independent subtotal pins the supported 64-bit release targets");
    // Independent non-archive pins: base 64 + recorder borrow 8*16 + bundle
    // borrow 8*4 + retained capture 16*4 + retained dispatch 24*4 + full
    // decision/store results 152*8 each = 2816. Do not derive this subtotal
    // from the production payment: it must detect changed terms/multipliers.
    // The owning archive embeds ProductStore/OsString and varies by target;
    // 3040 is the recorded-layout floor, not a target-independent exact cost.
    let expected = super::super::bundle::archive_payment(
        2816, [(std::mem::size_of::<IntentArchive>(), 4)]).max(3040);
    assert_eq!(super::super::bundle::POLICY_FRAME_EXPORT_PAYMENT, expected,
        "the reviewed lexical owner formula replaces the removed export/validation transport");
    super::super::bundle::assert_policy_frame_export_layouts_for_test();
    for cost in [policy_frame_continuation_payment::<LocalRuntime>(),
        super::super::owned_policy::POLICY_FRAME_DECISION_RECORD_PAYMENT,
        super::super::owned_policy::POLICY_FRAME_DECISION_CONSUME_PAYMENT,
        super::super::bundle::POLICY_FRAME_EXPORT_PAYMENT] {
        crate::product_store::scope::OwnedPolicyCheck::assert_archive_packet_for_test(cost);
    }
}
impl<'gate> RecordContinuationCapture<'_, 'gate, LocalRuntime> {
    fn assert_for_test(self, check: &mut crate::product_store::scope::OwnedPolicyCheck<'gate>, recorder: &mut crate::product_store::scope::PolicyFrameRecorder<'gate>) -> std::result::Result<(), StoreError> {
        let capsule = self.capsule;
        let current = self.current;
        let candidate = self.candidate;
        let engine = self.engine;
        let decision = &current.decisions.decisions[0];
        let scenes = engine.archive.accepted_scenes(decision).map_err(into_store_error)?;
        let source = &scenes[0].program;
        let evolved = source != current.program()?;
        if evolved { check.assert_fused_row_continuation_for_test(recorder, source)?; }
        // This is the exact concrete production continuation, including history,
        // all obligations, package/replay, independent reconstruction and export.
        self.verify(check, recorder).map_err(into_store_error)?;
        check.assert_policy_frame_receipt_for_test(capsule, recorder, source, candidate, evolved, decision, &scenes, false);
        // The deleted production export wrapper no longer returns a bundle.
        // Actual pre-discard bundle parity is checked only in the separately
        // authorized private sidecar, not via a replacement output helper.
        assert_eq!(candidate.data, current.data);
        assert_eq!(candidate.session, current.session);
        assert_eq!(candidate.clock_day, current.clock_day);
        assert_eq!(candidate.artifacts, current.artifacts);
        Err(StoreError::Invalid(FRAME_SENTINEL.into()))
    }
}
impl DecisionEngine<LocalRuntime> {
    pub(crate) fn assert_policy_frame_locked_for_test(&self, store: &ProductStore) {
        self.assert_policy_frame_control_for_test(store, "parity");
    }
    pub(crate) fn assert_policy_frame_control_for_test(&self, store: &ProductStore, mode: &str) {
        let current = store.load().unwrap();
        let layer = current.owned_scope().unwrap().layers.keys().next().unwrap();
        let operation = "frame-receipt-withdrawal";
        let (prepared, summary, report) = self.preview_owned_withdrawal(store, layer, operation,
            ProofBudget::default(), Arc::new(AtomicBool::new(false))).unwrap();
        assert_eq!(report.disposition, CheckDisposition::Ready);
        let change = self.prepare_owned_withdrawal(store, prepared.clone(), &summary.retire_decisions, operation).unwrap();
        let variants: &[&str] = if mode == "early-payments" {
            &["f0-short", "f0-exact", "fp-short", "fp-exact", "f1-short", "f1-exact"]
        } else if mode == "seed-payments" {
            &["chronology-open-short", "chronology-open-exact",
              "chronology-epoch-short", "chronology-epoch-exact", "chronology-seed-short", "chronology-seed-exact"]
        } else if mode == "donor-cancellation" {
            &["cancel-secondary", "cancel-primary"]
        } else { &[mode] };
        for variant in variants {
        let secondary = (*variant == "cancel-secondary").then(|| Arc::new(AtomicBool::new(false)));
        let mut reached = false;
        let ChangeAuthority::OwnedTransition(context) = &change.authority else { panic!("owned transition authority"); };
        let mut result = None;
        self.with_owned_transition(context.clone(), || {
        result = Some(store.adopt_owned_transition_verified(change.revision, &prepared,
            &change.prepared, &change.decisions, secondary.clone(),
            |current, fresh, next, plan, candidate, check, capsule| {
                reached = true;
                let capsule = capsule.expect("actual locked writer");
                if *variant == "clipped-profile" {
                    let decision = &current.decisions.decisions[0];
                    let scenes = self.archive.accepted_scenes(decision).map_err(into_store_error)?;
                    check.assert_policy_frame_clipped_for_test(capsule, &scenes[0].program,
                        candidate, decision, &scenes)?;
                    return Err(StoreError::Invalid(FRAME_SENTINEL.into()));
                }

                if *variant == "f0-short" || *variant == "f0-exact" {
                    let cost = policy_frame_continuation_payment::<LocalRuntime>();
                    check.leave_policy_frame_units_for_test(cost - u64::from(*variant == "f0-short"));
                }
                check.charge(policy_frame_continuation_payment::<LocalRuntime>())?;
                if *variant == "cancel-primary" {
                    prepared.cancellation().store(true, std::sync::atomic::Ordering::Release);
                } else if let Some(secondary) = &secondary {
                    secondary.store(true, std::sync::atomic::Ordering::Release);
                }

                let capture = RecordContinuationCapture { engine: self, store, current, fresh, next, plan, candidate, change: &change, capsule };
                let run = match *variant {
                    "f0-short" | "f0-exact" => RecordContinuationCapture::assert_f0_exact_for_test,
                    "fp-short" => RecordContinuationCapture::assert_fp_short_for_test,
                    "fp-exact" => RecordContinuationCapture::assert_fp_exact_for_test,
                    "f1-short" => RecordContinuationCapture::assert_f1_short_for_test,
                    "f1-exact" => RecordContinuationCapture::assert_f1_exact_for_test,
                    "chronology-open-short" => RecordContinuationCapture::assert_chronology_open_short_for_test,
                    "chronology-open-exact" => RecordContinuationCapture::assert_chronology_open_exact_for_test,
                    "chronology-epoch-short" => RecordContinuationCapture::assert_chronology_epoch_short_for_test,
                    "chronology-epoch-exact" => RecordContinuationCapture::assert_chronology_epoch_exact_for_test,
                    "chronology-seed-short" => RecordContinuationCapture::assert_chronology_seed_short_for_test,
                    "chronology-seed-exact" => RecordContinuationCapture::assert_chronology_seed_exact_for_test,
                    "chronology-zero" => RecordContinuationCapture::assert_chronology_zero_for_test,
                    "final-predicate" => RecordContinuationCapture::assert_final_predicate_for_test,
                    "decision-calls" => RecordContinuationCapture::assert_decision_calls_for_test,
                    "empty-refusals" => RecordContinuationCapture::assert_empty_refusals_for_test,
                    "cancel-primary" | "cancel-secondary" => RecordContinuationCapture::assert_cancelled_for_test,
                    _ => RecordContinuationCapture::assert_for_test,
                };
                let dispatch = crate::product_store::scope::RecordContinuationDispatch { capture, run };
                capsule.with_policy_frame_recorder(check, dispatch)
            }, |_| panic!("fresh operation cannot enter retry"), None));
            Ok(())
        }).unwrap();
        let result = result.unwrap();
        assert_eq!(store.load().unwrap(), current);
        assert!(reached);
        let expected = if variant.starts_with("f0-") { FRAME_FUEL } else { FRAME_SENTINEL };
        assert!(matches!(result, Err(StoreError::Invalid(ref text)) if text == expected), "{variant}: {result:?}");
        assert_eq!(store.load().unwrap(), current);
        println!("POLICY_FRAME_ACTUAL_CALL_BOUNDARY {variant}");
        }
    }
}

const FRAME_FUEL: &str = "Common factual admission: budget: cumulative proof/initialization budget exhausted";
impl<'gate> RecordContinuationCapture<'_, 'gate, LocalRuntime> {
    fn assert_f0_exact_for_test(self, check: &mut crate::product_store::scope::OwnedPolicyCheck<'gate>, recorder: &mut crate::product_store::scope::PolicyFrameRecorder<'gate>) -> std::result::Result<(), StoreError> {
        recorder.assert_policy_frame_empty_for_test();
        check.assert_policy_frame_units_for_test(0);
        check.execution()?;
        panic!("exact F0 must stop at the next unchanged execution debit")
    }
    fn assert_fp_short_for_test(self, check: &mut crate::product_store::scope::OwnedPolicyCheck<'gate>, recorder: &mut crate::product_store::scope::PolicyFrameRecorder<'gate>) -> std::result::Result<(), StoreError> { self.assert_early_for_test(check, recorder, false, false) }
    fn assert_fp_exact_for_test(self, check: &mut crate::product_store::scope::OwnedPolicyCheck<'gate>, recorder: &mut crate::product_store::scope::PolicyFrameRecorder<'gate>) -> std::result::Result<(), StoreError> { self.assert_early_for_test(check, recorder, false, true) }
    fn assert_f1_short_for_test(self, check: &mut crate::product_store::scope::OwnedPolicyCheck<'gate>, recorder: &mut crate::product_store::scope::PolicyFrameRecorder<'gate>) -> std::result::Result<(), StoreError> { self.assert_early_for_test(check, recorder, true, false) }
    fn assert_f1_exact_for_test(self, check: &mut crate::product_store::scope::OwnedPolicyCheck<'gate>, recorder: &mut crate::product_store::scope::PolicyFrameRecorder<'gate>) -> std::result::Result<(), StoreError> { self.assert_early_for_test(check, recorder, true, true) }
    fn assert_early_for_test(self, check: &mut crate::product_store::scope::OwnedPolicyCheck<'gate>, recorder: &mut crate::product_store::scope::PolicyFrameRecorder<'gate>, surface: bool, exact: bool) -> std::result::Result<(), StoreError> {
        use super::super::owned_policy::*;
        let packet = if surface { crate::product_store::scope::POLICY_FRAME_RECORD_PAYMENT } else { POLICY_FRAME_DECISION_RECORD_PAYMENT };
        // Original execution + retirement-ID prefix; then, only for F1,
        // original decision execution + decision + first-scene debits.
        let prefix = 4 + if surface { POLICY_FRAME_DECISION_RECORD_PAYMENT + 5 } else { 0 };
        check.leave_policy_frame_units_for_test(prefix + packet - u64::from(!exact));
        let error = check_withdrawal_recorded(&self.engine.archive, check, recorder).unwrap_err();
        assert!(matches!(error, DecisionError::Store(StoreError::Invalid(ref text)) if text == FRAME_FUEL), "{error:?}");
        check.assert_policy_frame_units_for_test(if exact { 0 } else { packet - 1 });
        recorder.assert_policy_frame_empty_for_test();
        Err(StoreError::Invalid(FRAME_SENTINEL.into()))
    }
    fn assert_decision_calls_for_test(self, check: &mut crate::product_store::scope::OwnedPolicyCheck<'gate>, recorder: &mut crate::product_store::scope::PolicyFrameRecorder<'gate>) -> std::result::Result<(), StoreError> {
        use super::super::owned_policy::*;
        let decision = &self.current.decisions.decisions[0];
        let scenes = self.engine.archive.accepted_scenes(decision).map_err(into_store_error)?;
        recorder.assert_policy_frame_empty_for_test();
        let ids = check_withdrawal_recorded(&self.engine.archive, check, recorder).map_err(into_store_error)?;
        assert_eq!(ids, self.plan.retire_decisions);
        for _ in 0..2 {
            let before = check.policy_frame_units_for_test();
            let ordinary = check_withdrawal(&self.engine.archive, check).map_err(into_store_error)?;
            let ordinary_cost = before - check.policy_frame_units_for_test();
            let before = check.policy_frame_units_for_test();
            let selected = check_withdrawal_recorded(&self.engine.archive, check, recorder).map_err(into_store_error)?;
            assert_eq!(selected, ordinary);
            assert_eq!(before - check.policy_frame_units_for_test(), ordinary_cost + POLICY_FRAME_DECISION_RECORD_PAYMENT + crate::product_store::scope::POLICY_FRAME_RECORD_PAYMENT,
                "filled slot still pays per decision and one locally deduplicated source call");
        }
        assert_decoded_early_calls(check, recorder, decision, &scenes, true)?;
        // The full original continuation still performs every original semantic
        // boundary before the independent genuine recipient is inspected.
        let capsule = self.capsule;
        let candidate = self.candidate;
        self.verify(check, recorder).map_err(into_store_error)?;
        check.assert_policy_frame_receipt_for_test(capsule, recorder, &scenes[0].program, candidate, false, decision, &scenes, true);
        Err(StoreError::Invalid(FRAME_SENTINEL.into()))
    }
}
fn assert_decoded_early_calls<'gate>(check: &mut crate::product_store::scope::OwnedPolicyCheck<'gate>, recorder: &mut crate::product_store::scope::PolicyFrameRecorder<'gate>, decision: &ScopedDecision, scenes: &[AcceptedScene], producer: bool) -> std::result::Result<(), StoreError> {
    use super::super::owned_policy::*;
    for kind in ["decision", "binding", "scenario", "scene"] {
        let mut changed = decision.clone();
        let mut decoded = scenes.to_vec();
        match kind {
            "decision" => changed.id.clear(),
            "binding" => decoded[0].bind_outcome = true,
            "scenario" => decoded[0].scenario.random_seed += 1,
            "scene" => decoded[0].program.binding.program_path.push_str("-changed-frame-source"),
            _ => unreachable!(),
        }
        let before = check.policy_frame_units_for_test();
        let ordinary_error = check_decision(check, &changed, &decoded).unwrap_err();
        let prefix = before - check.policy_frame_units_for_test();
        assert_eq!(prefix, if kind == "scene" { 5 } else { 4 });
        let before = check.policy_frame_units_for_test();
        let packet = if producer { POLICY_FRAME_DECISION_RECORD_PAYMENT } else { POLICY_FRAME_DECISION_CONSUME_PAYMENT };
        check.charge(packet)?;
        let selected_error = if producer {
            RecordDecisionCall { check, recorder, decision: &changed, scenes: &decoded }.run().unwrap_err()
        } else {
            ReceiptDecisionCall { check, recorder, decision: &changed, scenes: &decoded }.run().unwrap_err()
        };
        assert_eq!(format!("{selected_error:?}"), format!("{ordinary_error:?}"));
        assert_eq!(before - check.policy_frame_units_for_test(), prefix + packet, "no F1/FS/F7 before original {kind} error");
        println!("POLICY_FRAME_EARLY_ERROR producer={producer} kind={kind} original_prefix={prefix} packet={packet}");
    }
    Ok(())
}

impl DecisionEngine<LocalRuntime> {
    pub(crate) fn assert_policy_frame_recipient_calls_for_test(
        check: &mut crate::product_store::scope::OwnedPolicyCheck<'_>,
        recorder: &crate::product_store::scope::PolicyFrameRecorder<'_>,
        decision: &ScopedDecision, scenes: &[AcceptedScene], loop_cost: u64, selector: u64,
    ) -> std::result::Result<(), StoreError> {
        use super::super::owned_policy::*;
        for _ in 0..2 {
            let before = check.policy_frame_units_for_test();
            check_decision(check, decision, scenes).map_err(into_store_error)?;
            let ordinary_cost = before - check.policy_frame_units_for_test();
            let before = check.policy_frame_units_for_test();
            check.charge(POLICY_FRAME_DECISION_CONSUME_PAYMENT)?;
            ReceiptDecisionCall { check, recorder, decision, scenes }.run().map_err(into_store_error)?;
            assert_eq!(before - check.policy_frame_units_for_test(), ordinary_cost + POLICY_FRAME_DECISION_CONSUME_PAYMENT
                + crate::product_store::scope::POLICY_FRAME_CONSUME_PAYMENT + selector - loop_cost,
                "each invocation pays FA even when all seven scenes share one surface");
        }
        for kind in ["decision", "binding", "scenario", "scene"] {
            let mut changed = decision.clone();
            let mut decoded = scenes.to_vec();
            match kind {
                "decision" => changed.id.clear(),
                "binding" => decoded[0].bind_outcome = true,
                "scenario" => decoded[0].scenario.random_seed += 1,
                "scene" => decoded[0].program.binding.program_path.push_str("-changed-frame-source"),
                _ => unreachable!(),
            }
            let before = check.policy_frame_units_for_test();
            let ordinary_error = check_decision(check, &changed, &decoded).unwrap_err();
            let prefix = before - check.policy_frame_units_for_test();
            assert_eq!(prefix, if kind == "scene" { 5 } else { 4 });
            let before = check.policy_frame_units_for_test();
            check.charge(POLICY_FRAME_DECISION_CONSUME_PAYMENT)?;
            let selected_error = ReceiptDecisionCall { check, recorder, decision: &changed, scenes: &decoded }.run().unwrap_err();
            assert_eq!(format!("{selected_error:?}"), format!("{ordinary_error:?}"));
            assert_eq!(before - check.policy_frame_units_for_test(), prefix + POLICY_FRAME_DECISION_CONSUME_PAYMENT);
            println!("POLICY_FRAME_EARLY_ERROR producer=false kind={kind} original_prefix={prefix} packet={POLICY_FRAME_DECISION_CONSUME_PAYMENT}");
        }
        Ok(())
    }
}

impl<'gate> RecordContinuationCapture<'_, 'gate, LocalRuntime> {
    fn assert_empty_refusals_for_test(self, check: &mut crate::product_store::scope::OwnedPolicyCheck<'gate>, recorder: &mut crate::product_store::scope::PolicyFrameRecorder<'gate>) -> std::result::Result<(), StoreError> {
        let decision = &self.current.decisions.decisions[0];
        let scenes = self.engine.archive.accepted_scenes(decision).map_err(into_store_error)?;
        let ChangeAuthority::OwnedTransition(context) = &self.change.authority else { panic!("owned context"); };
        check.assert_policy_frame_empty_and_refusals_for_test(self.capsule, recorder,
            &scenes[0].program, self.candidate, &context.prepared)?;
        self.verify(check, recorder).map_err(into_store_error)?;
        Err(StoreError::Invalid(FRAME_SENTINEL.into()))
    }
}

impl<'gate> RecordContinuationCapture<'_, 'gate, LocalRuntime> {
    fn assert_cancelled_for_test(self, check: &mut crate::product_store::scope::OwnedPolicyCheck<'gate>, recorder: &mut crate::product_store::scope::PolicyFrameRecorder<'gate>) -> std::result::Result<(), StoreError> {
        recorder.assert_policy_frame_empty_for_test();
        let before = check.policy_frame_units_for_test();
        let error = self.verify(check, recorder).unwrap_err();
        assert!(matches!(error, DecisionError::Store(StoreError::Invalid(ref text)) if text == "Common factual admission: budget: cancelled or cumulative elapsed budget exhausted"), "{error:?}");
        check.assert_policy_frame_units_for_test(before);
        recorder.assert_policy_frame_empty_for_test();
        Err(StoreError::Invalid(FRAME_SENTINEL.into()))
    }
}

impl<'gate> RecordContinuationCapture<'_, 'gate, LocalRuntime> {
    fn assert_chronology_open_short_for_test(self, check: &mut crate::product_store::scope::OwnedPolicyCheck<'gate>, recorder: &mut crate::product_store::scope::PolicyFrameRecorder<'gate>) -> std::result::Result<(), StoreError> { self.assert_seed_boundary_for_test(check, recorder, 0, false) }
    fn assert_chronology_open_exact_for_test(self, check: &mut crate::product_store::scope::OwnedPolicyCheck<'gate>, recorder: &mut crate::product_store::scope::PolicyFrameRecorder<'gate>) -> std::result::Result<(), StoreError> { self.assert_seed_boundary_for_test(check, recorder, 0, true) }
    fn assert_chronology_epoch_short_for_test(self, check: &mut crate::product_store::scope::OwnedPolicyCheck<'gate>, recorder: &mut crate::product_store::scope::PolicyFrameRecorder<'gate>) -> std::result::Result<(), StoreError> { self.assert_seed_boundary_for_test(check, recorder, 1, false) }
    fn assert_chronology_epoch_exact_for_test(self, check: &mut crate::product_store::scope::OwnedPolicyCheck<'gate>, recorder: &mut crate::product_store::scope::PolicyFrameRecorder<'gate>) -> std::result::Result<(), StoreError> { self.assert_seed_boundary_for_test(check, recorder, 1, true) }
    fn assert_chronology_seed_short_for_test(self, check: &mut crate::product_store::scope::OwnedPolicyCheck<'gate>, recorder: &mut crate::product_store::scope::PolicyFrameRecorder<'gate>) -> std::result::Result<(), StoreError> { self.assert_seed_boundary_for_test(check, recorder, 2, false) }
    fn assert_chronology_seed_exact_for_test(self, check: &mut crate::product_store::scope::OwnedPolicyCheck<'gate>, recorder: &mut crate::product_store::scope::PolicyFrameRecorder<'gate>) -> std::result::Result<(), StoreError> { self.assert_seed_boundary_for_test(check, recorder, 2, true) }
    fn assert_seed_boundary_for_test(self, check: &mut crate::product_store::scope::OwnedPolicyCheck<'gate>, recorder: &mut crate::product_store::scope::PolicyFrameRecorder<'gate>, boundary: u8, exact: bool) -> std::result::Result<(), StoreError> {
        let scenes = self.engine.archive.accepted_scenes(&self.current.decisions.decisions[0]).map_err(into_store_error)?;
        check.assert_seed_packet_for_test(recorder, &scenes[0].program, boundary, exact)?;
        Err(StoreError::Invalid(FRAME_SENTINEL.into()))
    }
}

impl<'gate> RecordContinuationCapture<'_, 'gate, LocalRuntime> {
    fn assert_final_predicate_for_test(self, check: &mut crate::product_store::scope::OwnedPolicyCheck<'gate>, recorder: &mut crate::product_store::scope::PolicyFrameRecorder<'gate>) -> std::result::Result<(), StoreError> {
        let decision = &self.current.decisions.decisions[0];
        let scenes = self.engine.archive.accepted_scenes(decision).map_err(into_store_error)?;
        check.assert_final_predicate_boundaries_for_test(recorder, &scenes[0].program)?;
        self.assert_for_test(check, recorder)
    }
}

impl<'gate> RecordContinuationCapture<'_, 'gate, LocalRuntime> {
    fn assert_chronology_zero_for_test(self, check: &mut crate::product_store::scope::OwnedPolicyCheck<'gate>, recorder: &mut crate::product_store::scope::PolicyFrameRecorder<'gate>) -> std::result::Result<(), StoreError> {
        check.assert_zero_event_chronology_for_test();
        let capsule = self.capsule; let current = self.current; let candidate = self.candidate;
        let decision = &current.decisions.decisions[0];
        let scenes = self.engine.archive.accepted_scenes(decision).map_err(into_store_error)?;
        let source = &scenes[0].program;
        check.assert_final_predicate_boundaries_for_test(recorder, source)?;
        self.verify(check, recorder).map_err(into_store_error)?;
        check.assert_policy_frame_receipt_for_test(capsule, recorder, source, candidate, false, decision, &scenes, false);
        assert_eq!(candidate.data, current.data);
        assert_eq!(candidate.session, current.session);
        assert_eq!(candidate.clock_day, current.clock_day);
        assert_eq!(candidate.artifacts, current.artifacts);
        Err(StoreError::Invalid(FRAME_SENTINEL.into()))
    }
}
