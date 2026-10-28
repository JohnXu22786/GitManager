//! Independent controls for amendment 1; unchanged whole journey remains fixed.
#[path = "fixtures/common_fact/mod.rs"]
mod common;
#[path = "fixtures/owned_recovery/mod.rs"]
mod fixture;
#[path = "../src/product_backup.rs"]
mod product_backup;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_decisions/mod.rs"]
mod product_decisions;
#[path = "../src/product_locations.rs"]
mod product_locations;
#[path = "../src/product_protocol.rs"]
mod product_protocol;
#[path = "../src/product_runtime/mod.rs"]
mod product_runtime;
#[path = "../src/product_store/mod.rs"]
mod product_store;
use fixture::*;
use product_contract::*;
use product_decisions::*;
use product_store::scope::*;
use product_store::ProductStore;
#[test]
fn checked_continuation_accepts_changed_disjoint_final_common_marker() {
    let (_dir, store, a0, b0) = setup();
    for source in [&a0, &b0] {
        let marker = source.program.state.iter().find(|s| s.id == MARKER).unwrap();
        assert_eq!(marker.value_type, Type::Text);
        assert_eq!(marker.initial, value(""));
        // The sole occurrence is its declaration ID, not a semantic consumer.
        let bytes = canonical_bytes(&source.program).unwrap();
        let quoted = format!("\"{MARKER}\"");
        assert_eq!(bytes.windows(quoted.len()).filter(|w| *w == quoted.as_bytes()).count(), 1);
    }
    adopt(&store, &a0, &b0);
    let before = store.load().unwrap();
    let layer = before.owned_scope().unwrap().layers.keys().next().unwrap();
    let (a, b) = evolved(&a0, &b0, "independent-marker");
    let (prepared, report) = engine(&store)
        .prepare_owned_evolution(
            &store,
            &a,
            &b,
            &sources(&a, &b),
            layer,
            "marker-positive",
            ProofBudget::default(),
            cancel(),
        )
        .unwrap();
    assert_eq!(report.disposition, CheckDisposition::Ready);
    assert_eq!(prepared.seed(), &before.data);
    assert_eq!(store.load().unwrap(), before);
}
#[test]
fn first_entry_keeps_literal_empty_reset_grammar() {
    let (a0, b0) = pair();
    let (a, b) = evolved(&a0, &b0, "not-a-first-entry-reset");
    let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let store = ProductStore::create(dir.path().join("store"), &a, 20000).unwrap();
    add(&store, "add-live", "live");
    add(&store, "add-finished", "finished");
    job_action(&store, "finish", "complete", "finished");
    let before = store.load().unwrap();
    let request = request(&store, &a, &b);
    let witnesses: Vec<_> = [OwnedSourceSide::Before, OwnedSourceSide::After]
        .into_iter()
        .map(|side| LifecycleWitness {
            side,
            scenario: scenario(&store, "lifecycle", vec![]),
            completed: row(&before.data, "finished"),
            unfinished: row(&before.data, "live"),
        })
        .collect();
    let accepted = LifecycleAcceptanceHost::prepare(
        &store,
        &b,
        &request,
        &witnesses,
        ProofBudget::default(),
        cancel(),
    )
    .unwrap()
    .synthetic_operator_accept_for_test(&store)
    .unwrap();
    let error = store
        .verify_owned_preview(&b, &request, &accepted, ProofBudget::default(), cancel())
        .err()
        .expect("nonempty first-entry marker must refuse");
    assert!(
        error
            .to_string()
            .contains("input reset must be literal empty, common, top-level and final"),
        "unexpected refusal: {error}"
    );
    assert_eq!(store.load().unwrap(), before);
}
#[test]
fn continuation_does_not_admit_a_disjoint_state_expression_as_a_literal() {
    let (_dir, store, a0, b0) = setup();
    adopt(&store, &a0, &b0);
    let before = store.load().unwrap();
    let layer = before.owned_scope().unwrap().layers.keys().next().unwrap();
    let (a, b) = evolved(&a0, &b0, "placeholder");
    let (mut a, mut b) = (a.program, b.program);
    for app in [&mut a, &mut b] {
        let Statement::SetState { value, .. } = app
            .actions
            .iter_mut()
            .find(|a| a.id == "add")
            .unwrap()
            .steps
            .last_mut()
            .unwrap()
        else {
            panic!()
        };
        *value = Expr::State {
            state: MARKER.into(),
        };
    }
    let (a, b) = (common::capture(&a), common::capture(&b));
    let error = engine(&store)
        .prepare_owned_evolution(
            &store,
            &a,
            &b,
            &sources(&a, &b),
            layer,
            "marker-expression-negative",
            ProofBudget::default(),
            cancel(),
        )
        .err()
        .expect("state expression marker must refuse");
    assert!(
        error
            .to_string()
            .contains("input reset must be literal empty, common, top-level and final"),
        "unexpected refusal: {error}"
    );
    assert_eq!(store.load().unwrap(), before);
}

#[test]
fn continuation_rejects_ui_consumed_marker_with_or_without_reset() {
    for remove_reset in [false, true] {
        let (a0, b0) = pair();
        let (mut a0, mut b0) = (a0.program, b0.program);
        for app in [&mut a0, &mut b0] {
            let binding = app.views.iter().find(|v| v.id == "jobs").unwrap()
                .actions.iter().find(|a| a.id == "register").unwrap();
            assert_eq!(binding.arguments["name"], Expr::State { state: "name_input".into() });
            let register = app.actions.iter_mut().find(|a| a.id == "register").unwrap();
            assert!(matches!(register.steps.last(), Some(Statement::SetState { state, value: Expr::Literal { value_type: Type::Text, value: DataValue::Text { value } } }) if state == "name_input" && value.is_empty()));
            if remove_reset { register.steps.pop().unwrap(); }
        }
        let (a0, b0) = (common::capture(&a0), common::capture(&b0));
        let (_dir, store, a0, b0) = setup_pair(a0, b0);
        adopt(&store, &a0, &b0);
        let before = store.load().unwrap();
        let layer = before.owned_scope().unwrap().layers.keys().next().unwrap();
        let (a, b) = evolved(&a0, &b0, "implementation-one");
        let (mut a, mut b) = (a.program, b.program);
        for app in [&mut a, &mut b] {
            let Statement::SetState { state, .. } = app.actions.iter_mut()
                .find(|a| a.id == "add").unwrap().steps.last_mut().unwrap() else { panic!() };
            *state = "name_input".into();
        }
        let (a, b) = (common::capture(&a), common::capture(&b));
        let result = engine(&store).prepare_owned_evolution(
            &store, &a, &b, &sources(&a, &b), layer, "consumed-marker-negative",
            ProofBudget::default(), cancel(),
        );
        let error = result.err().expect("UI-consumed state is not a disjoint continuation marker");
        let DecisionError::Store(product_store::StoreError::Invalid(message)) = error else {
            panic!("unexpected error mode: {error:?}")
        };
        let expected = if remove_reset {
            "Common factual admission: add [1]: input reset must be literal empty, common, top-level and final"
        } else {
            "Common factual admission: add: original writer/dependency/barrier sequence changed"
        };
        assert_eq!(message, expected, "remove_reset={remove_reset}");
        assert_eq!(store.load().unwrap(), before);
    }
}
