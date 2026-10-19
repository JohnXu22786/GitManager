//! Additional small-fixture controls; the strict whole recovery journey is unchanged.
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

// Reuse the actual owned_recovery pair, lifecycle, source edits and policy.
// Three roots retain two real unfinished contributors plus the completed
// lifecycle witness. This adds a focused fixture and never edits the full one.
fn small_basis() -> (tempfile::TempDir, ProductStore, CapturedProgram, CapturedProgram) {
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
fn seven_specs(store: &ProductStore, prefix: &str) -> Vec<ScenarioSpec> {
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
fn adopt_small(store: &ProductStore, a: &CapturedProgram, b: &CapturedProgram, hidden_common: bool) -> ProjectSnapshot {
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
    let scenes: Vec<_> = seven_specs(store, "initial").iter().map(|spec| {
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
fn registry_public_retirement_matches_old_reference_and_survives_fresh_archive() {
    let (dir, store, a, b) = small_basis();
    let current = adopt_small(&store, &a, &b, false);
    let layer = current.owned_scope().unwrap().layers.keys().next().unwrap().clone();
    engine(&store).assert_registry_public_withdrawal_parity_for_test(&store, &layer);
    let (prepared, summary, report) = engine(&store).preview_owned_withdrawal(
        &store, &layer, "small-withdrawal", ProofBudget::default(), cancel(),
    ).unwrap();
    assert_eq!(report.disposition, CheckDisposition::Ready);
    assert_eq!(summary.retire_decisions, vec!["owned-policy"]);
    let change = engine(&store).prepare_owned_withdrawal(
        &store, prepared, &summary.retire_decisions, "small-withdrawal",
    ).unwrap();
    let withdrawn = engine(&store).adopt(&store, &change).unwrap();
    assert_eq!(withdrawn.data, current.data);
    assert_eq!(withdrawn.decisions.decisions[0].status, DecisionStatus::Withdrawn { adoption: "small-withdrawal".into() });
    IntentArchive::new(store.clone()).export_for(&withdrawn).unwrap();
    drop(store);
    let reopened = product_backup::open_verified(&dir.path().join("store"), None).unwrap().store;
    assert_eq!(reopened.load().unwrap(), withdrawn);
    assert_eq!(engine(&reopened).check_current(&withdrawn).unwrap().disposition, CheckDisposition::Ready);
}

#[test]
fn registry_distinct_initial_and_evolved_sources_preserve_complete_surfaces() {
    let (_dir, store, a, b) = small_basis();
    let initial = adopt_small(&store, &a, &b, false);
    let layer = initial.owned_scope().unwrap().layers.keys().next().unwrap().clone();
    let (next_a, next_b) = evolved(&a, &b, "second-source");
    let (prepared, report) = engine(&store).prepare_owned_evolution(
        &store, &next_a, &next_b, &sources(&next_a, &next_b), &layer,
        "small-evolution", ProofBudget::default(), cancel(),
    ).unwrap();
    assert_eq!(report.disposition, CheckDisposition::Ready);
    let change = engine(&store).prepare_owned_implementation_evolution(&store, prepared, "small-evolution").unwrap();
    let evolved = engine(&store).adopt(&store, &change).unwrap();
    assert_eq!(evolved.decisions, initial.decisions);
    let scenes: Vec<_> = seven_specs(&store, "evolved").iter().map(|spec| {
        let scene = engine(&store).accept_current_scene(&evolved, spec, Disclosure::Synthetic).unwrap();
        assert!(observed_count(&scene) < 2);
        scene
    }).collect();
    // These are genuine observed scenes for the second source, not a forged
    // second retirement decision. Existing writers cannot add that decision.
    assert_eq!(store.load().unwrap(), evolved);
    engine(&store).assert_registry_distinct_sources_for_test(&store, &layer, &scenes);
}

#[test]
fn registry_rejects_full_snapshot_and_foreign_capture_at_exact_guards() {
    let (_dir, store, a, b) = small_basis();
    let current = adopt_small(&store, &a, &b, false);
    let layer = current.owned_scope().unwrap().layers.keys().next().unwrap().clone();
    current.assert_registry_complete_association_for_test();
    engine(&store).assert_registry_surface_rejects_foreign_source_for_test(&store, &layer);
    assert_eq!(store.load().unwrap(), current);
}

#[test]
fn registry_and_old_reference_both_reject_hidden_unowned_obligation() {
    let (_dir, store, a, b) = small_basis();
    let current = adopt_small(&store, &a, &b, true);
    let layer = current.owned_scope().unwrap().layers.keys().next().unwrap().clone();
    engine(&store).assert_registry_hidden_unowned_refusal_for_test(&store, &layer);
}
