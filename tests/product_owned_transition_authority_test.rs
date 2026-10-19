//! Synthetic negative controls for the real transition decision/writer boundary.
#[path = "fixtures/common_fact/mod.rs"] mod common;
#[path = "fixtures/owned_recovery/mod.rs"] mod fixture;
#[path = "../src/product_backup.rs"] mod product_backup;
#[path = "../src/product_contract.rs"] mod product_contract;
#[path = "../src/product_decisions/mod.rs"] mod product_decisions;
#[path = "../src/product_locations.rs"] mod product_locations;
#[path = "../src/product_protocol.rs"] mod product_protocol;
#[path = "../src/product_runtime/mod.rs"] mod product_runtime;
#[path = "../src/product_store/mod.rs"] mod product_store;
use fixture::*;
use product_contract::*;
use product_decisions::*;
use product_store::{scope::*, ProductStore, ProjectSnapshot};

fn prepared(s: &ProductStore, a: &CapturedProgram, b: &CapturedProgram, op: &str)
    -> PreparedOwnedTransitionV2
{
    let current = s.load().unwrap();
    let layer = current.owned_scope().unwrap().layers.keys().next().unwrap().clone();
    let (a, b) = evolved(a, b, op);
    let (prepared, report) = engine(s).prepare_owned_evolution(s, &a, &b, &sources(&a, &b), &layer, op, ProofBudget::default(), cancel()).unwrap();
    assert_eq!(report.disposition, CheckDisposition::Ready);
    assert_eq!(prepared.seed(), &current.data);
    assert_eq!(s.load().unwrap(), current);
    prepared
}
fn unchanged(s: &ProductStore, expected: &ProjectSnapshot) {
    assert_eq!(s.load().unwrap(), *expected);
    assert_eq!(product_backup::VerifiedBackup::capture(s).unwrap().snapshot(), expected);
}
#[test]
fn transition_operation_and_exact_frame_are_bound_before_authority() {
    for kind in ["operation", "data", "session", "day"] {
        let (_dir, s, a, b) = setup();
        adopt(&s, &a, &b);
        let p = prepared(&s, &a, &b, "prepared-transition");
        match kind {
            "data" => add(&s, "intervening-data", "fresh-business-row"),
            "session" => apply(&s, "intervening-session", SemanticInput::Control { view: "jobs".into(), control: "name".into(), value: value("new draft") }),
            "day" => apply(&s, "intervening-day", SemanticInput::AdvanceClock { days: 1 }),
            _ => {},
        }
        let current = s.load().unwrap();
        let op = if kind == "operation" { "different-operation" } else { "prepared-transition" };
        assert!(engine(&s).prepare_owned_implementation_evolution(&s, p, op).is_err(), "{kind}");
        unchanged(&s, &current);
    }
}
#[test]
fn withdrawal_requires_the_complete_distinct_displayed_union() {
    let (_dir, s, a, b) = setup();
    let current = adopt(&s, &a, &b);
    let layer = current.owned_scope().unwrap().layers.keys().next().unwrap().clone();
    let (p, summary, report) = engine(&s).preview_owned_withdrawal(&s, &layer, "withdraw-exact", ProofBudget::default(), cancel()).unwrap();
    assert_eq!(summary.retire_decisions, vec!["owned-policy"]);
    assert_eq!(report.disposition, CheckDisposition::Ready);
    for ids in [vec![], vec!["independent-add".into()], vec!["owned-policy".into(), "independent-add".into()], vec!["owned-policy".into(), "owned-policy".into()]] {
        assert!(engine(&s).prepare_owned_withdrawal(&s, p.clone(), &ids, "withdraw-exact").is_err());
        unchanged(&s, &current);
    }
    assert!(engine(&s).prepare_owned_withdrawal(&s, p, &["owned-policy".into()], "different-operation").is_err());
    unchanged(&s, &current);
}
fn literal(n: i64) -> PropertyTerm { PropertyTerm::Literal { value_type: Type::Integer, value: DataValue::Integer { value: n } } }
fn observed(id: &str) -> PropertyTerm { PropertyTerm::Observed { point: "result".into(), observable: id.into(), value_type: Type::Integer } }
fn less(id: &str, n: i64) -> PropertyPredicate { PropertyPredicate::Less { left: observed(id), right: literal(n) } }
#[test]
fn withdrawal_rejects_whole_raw_mixed_common_and_vacuous_obligations() {
    for kind in ["raw", "mixed", "common", "tautology", "hidden-common"] {
        let (_dir, s, a, b) = setup();
        let p = prepare(&s, &a, &b);
        let mut choice = policy_choice(&p);
        match kind {
            "raw" => choice.binding = IntentionBinding::ObservedOutcome,
            "mixed" => choice.obligations.push(AcceptedProperty { id: "common-too".into(), description: "A separate common promise".into(), predicate: less("ready_count", MAX_COLLECTION as i64 + 1) }),
            "common" => choice.obligations[0].predicate = less("ready_count", MAX_COLLECTION as i64 + 1),
            "tautology" => choice.obligations[0].predicate = less(COUNT, MAX_COLLECTION as i64 + 1),
            "hidden-common" => choice.obligations[0].predicate = PropertyPredicate::Or { values: vec![less(COUNT, 2), less("ready_count", MAX_COLLECTION as i64 + 1)] },
            _ => unreachable!(),
        }
        let change = engine(&s).prepare_owned_choice(&s, p.clone(), choice, action_scenes(&s, &p), p.operation_id()).unwrap();
        let current = engine(&s).adopt(&s, &change).unwrap();
        let layer = current.owned_scope().unwrap().layers.keys().next().unwrap().clone();
        // These remain valid P4 stores. New retirement authority is narrower.
        unchanged(&s, &current);
        assert!(engine(&s).preview_owned_withdrawal(&s, &layer, "unsupported-withdrawal", ProofBudget::default(), cancel()).is_err(), "{kind}");
        unchanged(&s, &current);
    }
}
#[test]
fn exact_transition_retry_preserves_later_work_and_changed_request_conflicts() {
    let (_dir, s, a, b) = setup();
    adopt(&s, &a, &b);
    let p = prepared(&s, &a, &b, "same-operation");
    let change = engine(&s).prepare_owned_implementation_evolution(&s, p, "same-operation").unwrap();
    engine(&s).adopt(&s, &change).unwrap();
    add(&s, "work-after-transition", "must-survive-retry");
    let later = s.load().unwrap();
    assert_eq!(engine(&s).adopt(&s, &change).unwrap(), later);
    unchanged(&s, &later);
    let (a1, b1) = evolved(&a, &b, "same-operation");
    let layer = later.owned_scope().unwrap().layers.keys().next().unwrap().clone();
    let (a2, b2) = evolved(&a1, &b1, "different-implementation");
    let result = engine(&s).prepare_owned_evolution(&s, &a2, &b2, &sources(&a2, &b2), &layer, "same-operation", ProofBudget::default(), cancel())
        .and_then(|(p, _)| engine(&s).prepare_owned_implementation_evolution(&s, p, "same-operation"))
        .and_then(|other| engine(&s).adopt(&s, &other));
    assert!(matches!(result, Err(DecisionError::Store(product_store::StoreError::Conflict(_)))));
    unchanged(&s, &later);
}
#[test]
fn transition_pointer_faults_preserve_old_or_exact_once_new_current() {
    for fault in [product_store::FaultPoint::AfterObject, product_store::FaultPoint::BeforePointer, product_store::FaultPoint::AfterPointer] {
        let (_dir, s, a, b) = setup();
        let before = adopt(&s, &a, &b);
        let p = prepared(&s, &a, &b, "fault-transition");
        let change = engine(&s).prepare_owned_implementation_evolution(&s, p, "fault-transition").unwrap();
        assert!(engine(&s).adopt_owned_transition_with_fault(&s, &change, fault).is_err());
        let reached = s.load().unwrap();
        if fault == product_store::FaultPoint::AfterPointer {
            assert_eq!(reached.revision, before.revision + 1);
            assert_eq!(reached.data, before.data);
            assert_eq!(reached.operations["fault-transition"].revision, reached.revision);
        } else { assert_eq!(reached, before); }
        let once = engine(&s).adopt(&s, &change).unwrap();
        assert_eq!(once.revision, before.revision + 1);
        assert_eq!(once.data, before.data);
        assert_eq!(engine(&s).adopt(&s, &change).unwrap(), once);
        unchanged(&s, &once);
    }
}
#[test]
fn both_real_tokens_cancel_at_final_pointer_checkpoint() {
    for secondary in [false, true] {
        let (_dir, s, a, b) = setup();
        let before = adopt(&s, &a, &b);
        let primary = cancel();
        let secondary_token = cancel();
        let runtime = product_runtime::LocalRuntime::with_cancellation(secondary_token.clone());
        let e = DecisionEngine::new(runtime, IntentArchive::new(s.clone()));
        let layer = before.owned_scope().unwrap().layers.keys().next().unwrap().clone();
        let (a1, b1) = evolved(&a, &b, "cancel-transition");
        let (p, report) = e.prepare_owned_evolution(&s, &a1, &b1, &sources(&a1, &b1), &layer, "cancel-transition", ProofBudget::default(), primary.clone()).unwrap();
        assert_eq!(report.disposition, CheckDisposition::Ready);
        let change = e.prepare_owned_implementation_evolution(&s, p, "cancel-transition").unwrap();
        let fault = if secondary { product_store::FaultPoint::CancelSecondaryBeforePointer } else { product_store::FaultPoint::CancelPrimaryBeforePointer };
        let error = e.adopt_owned_transition_with_fault(&s, &change, fault).unwrap_err();
        assert!(matches!(error, DecisionError::Store(product_store::StoreError::Invalid(ref text)) if text == "Common factual admission: budget: cancelled or cumulative elapsed budget exhausted"));
        assert!(if secondary { &secondary_token } else { &primary }.load(std::sync::atomic::Ordering::Acquire));
        unchanged(&s, &before);
        primary.store(false, std::sync::atomic::Ordering::Release);
        secondary_token.store(false, std::sync::atomic::Ordering::Release);
        let after = e.adopt(&s, &change).unwrap();
        assert_eq!(after.revision, before.revision + 1);
        assert_eq!(after.data, before.data);
    }
}


#[test]
fn real_withdrawal_eligibility_preserves_the_actual_meter_refusal() {
    let (_dir, store, a, b) = setup();
    let before = adopt(&store, &a, &b);
    engine(&store).assert_owned_retirement_callback_cancelled_for_test(&before);
    unchanged(&store, &before);
}
