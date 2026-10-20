//! Fresh synthetic acceptance. No native GUI/operator evidence is claimed.
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
use product_store::{scope::*, ProductStore};

#[test]
fn policy_context_fixed_debits_keep_real_execution_borrows() {
    let (_dir, store, a, b) = setup();
    adopt(&store, &a, &b);
    store.assert_owned_policy_context_exact_costs_for_test();
}
#[test]
fn policy_context_constructors_refuse_before_callback_at_real_fuel_boundary() {
    let (_dir, store, a, b) = setup();
    adopt(&store, &a, &b);
    store.assert_owned_policy_context_constructor_fuel_for_test();
}
#[test]
fn policy_context_methods_preserve_both_tokens_and_real_fuel_refusal() {
    let (_dir, store, a, b) = setup();
    adopt(&store, &a, &b);
    store.assert_owned_policy_context_actual_refusals_for_test();
}

#[test]
fn policy_context_locked_adoption_keeps_fresh_registry_and_exact_dispatch() {
    let (_dir, store, a, b) = setup();
    adopt(&store, &a, &b);
    store.assert_owned_adoption_policy_context_exact_dispatch_for_test();
}
#[test]
fn policy_context_locked_adoption_preserves_actual_fuel_and_both_tokens() {
    let (_dir, store, a, b) = setup();
    adopt(&store, &a, &b);
    store.assert_owned_adoption_policy_context_actual_refusals_for_test();
}

// Bounded genuine policy fixture: three roots, seven independently accepted scenes.
fn archive_policy_small_basis() -> (tempfile::TempDir, ProductStore, CapturedProgram, CapturedProgram) {
    let (a, b) = pair();
    let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let store = ProductStore::create(dir.path().join("store"), &a, 20000).unwrap();
    for title in ["live", "reserve", "finished"] {
        add(&store, &format!("add-{title}"), title);
    }
    job_action(&store, "finish-before-policy", "complete", "finished");
    for title in ["live", "reserve"] {
        let owner = row(&store.load().unwrap().data, title);
        apply(&store, &format!("register-{title}"), invoke("register", [
            ("owner".into(), reference(&owner)), ("name".into(), value("shared-material")),
        ].into()));
    }
    apply(&store, "material-arrived", material_input(&store, "arrive", "before policy"));
    let current = store.load().unwrap();
    assert_eq!(current.data.records.iter().filter(|r| r.entity == "job").count(), 3);
    assert!(current.decisions.decisions.is_empty());
    (dir, store, a, b)
}
fn archive_policy_seven_specs(store: &ProductStore, prefix: &str) -> Vec<ScenarioSpec> {
    let current = store.load().unwrap();
    let live = row(&current.data, "live");
    let reserve = row(&current.data, "reserve");
    let link = current.data.records.iter().find(|r| r.entity == "requirement").unwrap();
    let material = reference(&material(&current.data));
    let withdrawal = invoke("withdraw", [
        ("material".into(), material.clone()), ("note".into(), value("scene")),
    ].into());
    vec![
        ("add", vec![invoke("add", [("title".into(), value("scene-new"))].into())]),
        ("complete", vec![invoke("complete", [("job".into(), reference(&reserve))].into())]),
        ("register", vec![invoke("register", [("owner".into(), reference(&live)), ("name".into(), value("scene-material"))].into())]),
        ("remove", vec![invoke("remove", [("link".into(), DataValue::Reference { entity: "requirement".into(), record: link.id.clone() })].into())]),
        ("withdraw", vec![invoke("begin", [("job".into(), reference(&live))].into()), withdrawal.clone()]),
        ("begin", vec![invoke("begin", [("job".into(), reference(&live))].into())]),
        ("arrive", vec![invoke("begin", [("job".into(), reference(&live))].into()), withdrawal, invoke("arrive", [("material".into(), material), ("note".into(), value("scene"))].into())]),
    ].into_iter().map(|(id, inputs)| scenario(store, &format!("{prefix}-{id}"), inputs)).collect()
}
fn archive_policy_adopt_small(store: &ProductStore, a: &CapturedProgram, b: &CapturedProgram, hidden_common: bool) -> product_store::ProjectSnapshot {
    let before = store.load().unwrap();
    let prepared = prepare(store, a, b);
    assert_nonvacuity(store, &prepared);
    let mut choice = policy_choice(&prepared);
    if hidden_common {
        choice.obligations[0].predicate = PropertyPredicate::Or { values: vec![
            choice.obligations[0].predicate.clone(),
            PropertyPredicate::Less {
                left: PropertyTerm::Observed { point: "result".into(), observable: "ready_count".into(), value_type: Type::Integer },
                right: PropertyTerm::Literal { value_type: Type::Integer, value: DataValue::Integer { value: MAX_COLLECTION as i64 + 1 } },
            },
        ] };
    }
    let scenes: Vec<_> = archive_policy_seven_specs(store, "initial").iter().map(|spec| {
        let scene = engine(store).accept_owned_scene(store, &prepared, spec, Disclosure::Synthetic).unwrap();
        assert!(observed_count(&scene) < 2);
        scene
    }).collect();
    let change = engine(store).prepare_owned_choice(store, prepared.clone(), choice, scenes, prepared.operation_id()).unwrap();
    let adopted = engine(store).adopt(store, &change).unwrap();
    assert_eq!(adopted.data.events, before.data.events);
    assert_eq!(adopted.owned_scope().unwrap().adoptions[0].decisions, vec!["owned-policy"]);
    assert_eq!(engine(store).check_current(&adopted).unwrap().disposition, CheckDisposition::Ready);
    adopted
}

#[test]
fn archive_policy_locked_origin_and_selector() {
    let (_dir, store, a, b) = archive_policy_small_basis();
    archive_policy_adopt_small(&store, &a, &b, false);
    let pointer = std::fs::read(_dir.path().join("store/CURRENT")).unwrap();
    engine(&store).assert_locked_archive_policy_for_test(&store, "origin");
    assert_eq!(std::fs::read(_dir.path().join("store/CURRENT")).unwrap(), pointer);
}
#[test]
fn archive_policy_locked_payments_and_current() {
    let (_dir, store, a, b) = archive_policy_small_basis();
    archive_policy_adopt_small(&store, &a, &b, false);
    let pointer = std::fs::read(_dir.path().join("store/CURRENT")).unwrap();
    engine(&store).assert_locked_archive_policy_for_test(&store, "payments");
    assert_eq!(std::fs::read(_dir.path().join("store/CURRENT")).unwrap(), pointer);
}
#[test]
fn archive_policy_bundle_parity_and_error_order() {
    let (_dir, store, a, b) = archive_policy_small_basis();
    archive_policy_adopt_small(&store, &a, &b, false);
    let pointer = std::fs::read(_dir.path().join("store/CURRENT")).unwrap();
    engine(&store).assert_locked_archive_policy_for_test(&store, "bundle");
    assert_eq!(std::fs::read(_dir.path().join("store/CURRENT")).unwrap(), pointer);
}
#[test]
fn archive_policy_sources_and_history() {
    let (_dir, store, a, b) = archive_policy_small_basis();
    let initial = archive_policy_adopt_small(&store, &a, &b, false);
    let layer = initial.owned_scope().unwrap().layers.keys().next().unwrap();
    let (a, b) = evolved(&a, &b, "archive-second-source");
    let (prepared, report) = engine(&store).prepare_owned_evolution(&store, &a, &b, &sources(&a, &b), layer,
        "archive-evolution", ProofBudget::default(), cancel()).unwrap();
    assert_eq!(report.disposition, CheckDisposition::Ready);
    let change = engine(&store).prepare_owned_implementation_evolution(&store, prepared, "archive-evolution").unwrap();
    let evolved = engine(&store).adopt(&store, &change).unwrap();
    assert_eq!(evolved.decisions, initial.decisions);
    job_action(&store, "archive-live-begin", "begin", "live");
    assert_ne!(store.load().unwrap().data, evolved.data);
    let pointer = std::fs::read(_dir.path().join("store/CURRENT")).unwrap();
    engine(&store).assert_locked_archive_policy_for_test(&store, "history");
    assert_eq!(std::fs::read(_dir.path().join("store/CURRENT")).unwrap(), pointer);
}
