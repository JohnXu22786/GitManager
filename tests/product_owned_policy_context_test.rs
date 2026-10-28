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

#[test]
fn policy_frame_receipt_real_origin_and_parity() {
    let (dir, store, a, b) = archive_policy_small_basis();
    archive_policy_adopt_small(&store, &a, &b, false);
    let current = store.load().unwrap();
    let pointer = std::fs::read(dir.path().join("store/CURRENT")).unwrap();
    engine(&store).assert_policy_frame_locked_for_test(&store);
    assert_eq!(store.load().unwrap(), current);
    assert_eq!(std::fs::read(dir.path().join("store/CURRENT")).unwrap(), pointer);
}

#[test]
fn policy_frame_dominance_completed_default_donor() {
    let (dir, store, a, b) = archive_policy_small_basis();
    let initial = archive_policy_adopt_small(&store, &a, &b, false);
    job_action(&store, "frame-complete-original", "complete", "reserve");
    let completed_original = store.load().unwrap();
    assert_ne!(completed_original.data, initial.data);
    let layer = initial.owned_scope().unwrap().layers.keys().next().unwrap();
    let (a, b) = evolved(&a, &b, "frame-second-producer");
    let (prepared, report) = engine(&store).prepare_owned_evolution(&store, &a, &b, &sources(&a, &b), layer,
        "frame-evolution", ProofBudget::default(), cancel()).unwrap();
    assert_eq!(report.disposition, CheckDisposition::Ready);
    let change = engine(&store).prepare_owned_implementation_evolution(&store, prepared, "frame-evolution").unwrap();
    let evolved = engine(&store).adopt(&store, &change).unwrap();
    assert_eq!(evolved.data, completed_original.data);
    job_action(&store, "frame-complete-evolved", "complete", "live");
    let current = store.load().unwrap();
    assert_ne!(current.data, evolved.data);
    assert_eq!(current.decisions, initial.decisions);
    let pointer = std::fs::read(dir.path().join("store/CURRENT")).unwrap();
    engine(&store).assert_policy_frame_locked_for_test(&store);
    assert_eq!(store.load().unwrap(), current);
    assert_eq!(std::fs::read(dir.path().join("store/CURRENT")).unwrap(), pointer);
}

#[test]
fn policy_frame_receipt_actual_early_packet_boundaries() {
    let (dir, store, a, b) = archive_policy_small_basis();
    archive_policy_adopt_small(&store, &a, &b, false);
    let current = store.load().unwrap();
    let pointer = std::fs::read(dir.path().join("store/CURRENT")).unwrap();
    engine(&store).assert_policy_frame_control_for_test(&store, "early-payments");
    assert_eq!(store.load().unwrap(), current);
    assert_eq!(std::fs::read(dir.path().join("store/CURRENT")).unwrap(), pointer);
}
#[test]
fn policy_frame_receipt_repeated_decision_calls_and_early_errors() {
    let (dir, store, a, b) = archive_policy_small_basis();
    archive_policy_adopt_small(&store, &a, &b, false);
    let current = store.load().unwrap();
    let pointer = std::fs::read(dir.path().join("store/CURRENT")).unwrap();
    engine(&store).assert_policy_frame_control_for_test(&store, "decision-calls");
    assert_eq!(store.load().unwrap(), current);
    assert_eq!(std::fs::read(dir.path().join("store/CURRENT")).unwrap(), pointer);
}

#[test]
fn policy_frame_dominance_completed_clipped_donor_is_reusable() {
    let (dir, store, a, b) = archive_policy_small_basis();
    let initial = archive_policy_adopt_small(&store, &a, &b, false);
    job_action(&store, "frame-complete-original", "complete", "reserve");
    let completed_original = store.load().unwrap();
    assert_ne!(completed_original.data, initial.data);
    let layer = initial.owned_scope().unwrap().layers.keys().next().unwrap();
    let (a, b) = evolved(&a, &b, "frame-second-producer");
    let (prepared, report) = engine(&store).prepare_owned_evolution(&store, &a, &b, &sources(&a, &b), layer,
        "frame-evolution", ProofBudget::default(), cancel()).unwrap();
    assert_eq!(report.disposition, CheckDisposition::Ready);
    let change = engine(&store).prepare_owned_implementation_evolution(&store, prepared, "frame-evolution").unwrap();
    let evolved = engine(&store).adopt(&store, &change).unwrap();
    assert_eq!(evolved.data, completed_original.data);
    job_action(&store, "frame-complete-evolved", "complete", "live");
    let current = store.load().unwrap();
    assert_ne!(current.data, evolved.data);
    assert_eq!(current.decisions, initial.decisions);
    let pointer = std::fs::read(dir.path().join("store/CURRENT")).unwrap();
    engine(&store).assert_policy_frame_control_for_test(&store, "clipped-profile");
    assert_eq!(store.load().unwrap(), current);
    assert_eq!(std::fs::read(dir.path().join("store/CURRENT")).unwrap(), pointer);
}

#[test]
fn policy_frame_receipt_empty_and_surface_refusals() {
    let (dir, store, a, b) = archive_policy_small_basis();
    archive_policy_adopt_small(&store, &a, &b, false);
    let current = store.load().unwrap();
    let pointer = std::fs::read(dir.path().join("store/CURRENT")).unwrap();
    engine(&store).assert_policy_frame_control_for_test(&store, "empty-refusals");
    assert_eq!(store.load().unwrap(), current);
    assert_eq!(std::fs::read(dir.path().join("store/CURRENT")).unwrap(), pointer);
}

#[test]
fn policy_frame_receipt_actual_donor_cancellation() {
    let (dir, store, a, b) = archive_policy_small_basis();
    archive_policy_adopt_small(&store, &a, &b, false);
    let current = store.load().unwrap();
    let pointer = std::fs::read(dir.path().join("store/CURRENT")).unwrap();
    engine(&store).assert_policy_frame_control_for_test(&store, "donor-cancellation");
    assert_eq!(store.load().unwrap(), current);
    assert_eq!(std::fs::read(dir.path().join("store/CURRENT")).unwrap(), pointer);
}

#[test]
fn policy_frame_seed_actual_packet_boundaries() {
    let (dir, store, a, b) = archive_policy_small_basis();
    archive_policy_adopt_small(&store, &a, &b, false);
    let current = store.load().unwrap();
    let pointer = std::fs::read(dir.path().join("store/CURRENT")).unwrap();
    engine(&store).assert_policy_frame_control_for_test(&store, "seed-payments");
    assert_eq!(store.load().unwrap(), current);
    assert_eq!(std::fs::read(dir.path().join("store/CURRENT")).unwrap(), pointer);
}

// Final-predicate controls reuse exactly the admitted initial/evolved actions.
#[test]
fn policy_frame_final_predicate_initial() {
    let (dir, store, a, b) = archive_policy_small_basis();
    archive_policy_adopt_small(&store, &a, &b, false);
    let current = store.load().unwrap();
    let pointer = std::fs::read(dir.path().join("store/CURRENT")).unwrap();
    engine(&store).assert_policy_frame_control_for_test(&store, "final-predicate");
    assert_eq!(store.load().unwrap(), current);
    assert_eq!(std::fs::read(dir.path().join("store/CURRENT")).unwrap(), pointer);
}

#[test]
fn policy_frame_final_predicate_evolved() {
    let (dir, store, a, b) = archive_policy_small_basis();
    let initial = archive_policy_adopt_small(&store, &a, &b, false);
    job_action(&store, "frame-complete-original", "complete", "reserve");
    let completed_original = store.load().unwrap();
    assert_ne!(completed_original.data, initial.data);
    let layer = initial.owned_scope().unwrap().layers.keys().next().unwrap();
    let (a, b) = evolved(&a, &b, "frame-second-producer");
    let (prepared, report) = engine(&store).prepare_owned_evolution(&store, &a, &b, &sources(&a, &b), layer,
        "frame-evolution", ProofBudget::default(), cancel()).unwrap();
    assert_eq!(report.disposition, CheckDisposition::Ready);
    let change = engine(&store).prepare_owned_implementation_evolution(&store, prepared, "frame-evolution").unwrap();
    let evolved = engine(&store).adopt(&store, &change).unwrap();
    assert_eq!(evolved.data, completed_original.data);
    job_action(&store, "frame-complete-evolved", "complete", "live");
    let current = store.load().unwrap();
    assert_ne!(current.data, evolved.data);
    assert_eq!(current.decisions, initial.decisions);
    let pointer = std::fs::read(dir.path().join("store/CURRENT")).unwrap();
    engine(&store).assert_policy_frame_control_for_test(&store, "final-predicate");
    assert_eq!(store.load().unwrap(), current);
    assert_eq!(std::fs::read(dir.path().join("store/CURRENT")).unwrap(), pointer);
}


// The admitted evolved helper with only its two completion actions omitted.
#[test]
fn policy_frame_chronology_consecutive_zero_event_epochs() {
    let (dir, store, a, b) = archive_policy_small_basis();
    let initial = archive_policy_adopt_small(&store, &a, &b, false);
    let layer = initial.owned_scope().unwrap().layers.keys().next().unwrap();
    let (a, b) = evolved(&a, &b, "frame-second-producer");
    let (prepared, report) = engine(&store).prepare_owned_evolution(&store, &a, &b, &sources(&a, &b), layer,
        "frame-evolution", ProofBudget::default(), cancel()).unwrap();
    assert_eq!(report.disposition, CheckDisposition::Ready);
    let change = engine(&store).prepare_owned_implementation_evolution(&store, prepared, "frame-evolution").unwrap();
    let current = engine(&store).adopt(&store, &change).unwrap();
    assert_eq!(current.data, initial.data);
    assert_eq!(current.decisions, initial.decisions);
    let pointer = std::fs::read(dir.path().join("store/CURRENT")).unwrap();
    engine(&store).assert_policy_frame_control_for_test(&store, "chronology-zero");
    assert_eq!(store.load().unwrap(), current);
    assert_eq!(std::fs::read(dir.path().join("store/CURRENT")).unwrap(), pointer);
}

// Bounded ordinary-reconstruction controls. Existing policy helpers above are unchanged.
// Safe fixture assertions use the actual retained auxiliary schema identities.
fn assert_ordinary_prefix_material_state(
    current: &product_store::ProjectSnapshot, original: &product_store::ProjectSnapshot,
    received: bool, unfinished_status: &str,
) {
    let identities = |snapshot: &product_store::ProjectSnapshot, entity: &str| {
        snapshot.data.records.iter().filter(|r| r.entity == entity)
            .map(|r| (r.entity.clone(), r.id.clone())).collect::<Vec<_>>()
    };
    for (entity, count) in [("job", 3), ("material", 1), ("requirement", 2)] {
        assert_eq!(identities(current, entity), identities(original, entity));
        assert_eq!(identities(current, entity).len(), count);
    }
    for title in ["live", "reserve", "finished"] {
        let id = row(&current.data, title);
        let record = current.data.records.iter().find(|r| r.entity == id.entity && r.id == id.record).unwrap();
        assert!(!record.archived);
        assert_eq!(record.values["status"], value(if title == "finished" { "done" } else { unfinished_status }));
        if title == "finished" {
            assert_eq!(Some(record), original.data.records.iter().find(|r| r.entity == id.entity && r.id == id.record));
        } else {
            assert!(!flag(current, title));
        }
    }
    let material_ref = material(&current.data);
    let material_row = current.data.records.iter().find(|r| r.entity == material_ref.entity && r.id == material_ref.record).unwrap();
    assert!(!material_row.archived);
    assert_eq!(material_row.values["received"], DataValue::Boolean { value: received });
    for title in ["live", "reserve"] {
        let owner = reference(&row(&current.data, title));
        let links: Vec<_> = current.data.records.iter().filter(|r| r.entity == "requirement" && r.values["job"] == owner).collect();
        assert_eq!(links.len(), 1);
        assert!(!links[0].archived);
        assert_eq!(links[0].values["material"], reference(&material_ref));
        assert_eq!(links[0].values["required"], DataValue::Boolean { value: true });
        assert_eq!(Some(links[0]), original.data.records.iter().find(|r| r.entity == "requirement" && r.id == links[0].id));
    }
    let state = current.owned_scope().unwrap();
    assert_eq!(state.initializations.len(), 1);
    let auxiliary = &state.initializations[0].auxiliary;
    assert!(!auxiliary.is_empty());
    for entity in auxiliary {
        assert!(current.data.schema.iter().any(|e| e.id == entity.id));
        assert!(!current.data.records.iter().any(|r| r.entity == entity.id),
            "no auxiliary capture was created without an active root");
    }
}
fn ordinary_prefix_material_cycle(
    store: &ProductStore, initial: &product_store::ProjectSnapshot,
    operations: [(&str, &str); 2],
) {
    let before = store.load().unwrap();
    assert_ordinary_prefix_material_state(&before, initial, true, "ready");
    assert_eq!(before.decisions, initial.decisions);
    let state = before.owned_scope().unwrap();
    assert_eq!(state.compositions.values().find(|m| m.output == before.active_revision).unwrap().active.len(), 1);
    let ids: Vec<_> = before.data.records.iter().map(|r| (r.entity.clone(), r.id.clone())).collect();
    for (index, ((operation, note), action)) in operations.into_iter().zip(["withdraw", "arrive"]).enumerate() {
        assert!(!before.operations.contains_key(operation));
        apply(store, operation, material_input(store, action, note));
        let current = store.load().unwrap();
        assert_ordinary_prefix_material_state(&current, initial, index == 1,
            if index == 0 { "waiting" } else { "ready" });
        assert_eq!(current.active_revision, before.active_revision);
        assert_eq!(current.decisions, before.decisions);
        assert_eq!(current.data.schema, before.data.schema);
        assert_eq!(current.clock_day, before.clock_day);
        assert!(current.data.events.starts_with(&before.data.events));
        assert_eq!(current.data.events.len(), before.data.events.len() + index + 1);
        let event = current.data.events.last().unwrap();
        assert_eq!(event.operation_id, operation);
        assert_eq!(event.action, action);
        assert_eq!(event.program, before.program().unwrap().artifact.program_digest);
        assert_eq!(event.day, before.clock_day);
        let current_ids: Vec<_> = current.data.records.iter().map(|r| (r.entity.clone(), r.id.clone())).collect();
        assert!(current_ids.starts_with(&ids));
        assert_eq!(current_ids.len(), ids.len() + index + 1);
        let new_rows = &current.data.records[ids.len()..];
        assert!(new_rows.iter().all(|r| r.entity == "event" && !r.archived));
        let added = new_rows.last().unwrap();
        assert_eq!(added.values["material"], reference(&material(&before.data)));
        assert_eq!(added.values["note"], value(note));
        assert_eq!(current.revision, before.revision + (index as u64) + 1);
    }
}

#[derive(Clone, Copy)]
enum OrdinaryPrefixHistory {
    One, TwoZero, ThreeZero, MiddleZero, MiddleEvents,
}
struct OrdinaryPrefixFixture {
    dir: tempfile::TempDir,
    store: ProductStore,
    initial: product_store::ProjectSnapshot,
    historical: Vec<product_store::ProjectSnapshot>,
    current: product_store::ProjectSnapshot,
    pointer: Vec<u8>,
}
fn ordinary_prefix_history_fixture(mode: OrdinaryPrefixHistory) -> OrdinaryPrefixFixture {
    let (dir, store, a, b) = archive_policy_small_basis();
    let initial = archive_policy_adopt_small(&store, &a, &b, false);
    let layer = initial.owned_scope().unwrap().layers.keys().next().unwrap().clone();
    let mut historical = Vec::new();
    if matches!(mode, OrdinaryPrefixHistory::MiddleZero) {
        ordinary_prefix_material_cycle(&store, &initial, [
            ("prefix-first-withdraw", "ordinary-prefix-first-withdraw"),
            ("prefix-first-arrive", "ordinary-prefix-first-arrive"),
        ]);
    }
    if !matches!(mode, OrdinaryPrefixHistory::One) {
        let before_evolution = store.load().unwrap();
        historical.push(before_evolution.clone());
        let (next_a, next_b) = evolved(&a, &b, "ordinary-prefix-evolution");
        let (prepared, report) = engine(&store).prepare_owned_evolution(&store, &next_a, &next_b,
            &sources(&next_a, &next_b), &layer, "prefix-evolution", ProofBudget::default(), cancel()).unwrap();
        assert_eq!(report.disposition, CheckDisposition::Ready);
        let change = engine(&store).prepare_owned_implementation_evolution(&store, prepared,
            "prefix-evolution").unwrap();
        let evolved = engine(&store).adopt(&store, &change).unwrap();
        assert_eq!(evolved.data, before_evolution.data);
        assert_eq!(evolved.decisions, initial.decisions);
    }
    if matches!(mode, OrdinaryPrefixHistory::MiddleEvents) {
        ordinary_prefix_material_cycle(&store, &initial, [
            ("prefix-middle-withdraw", "ordinary-prefix-middle-withdraw"),
            ("prefix-middle-arrive", "ordinary-prefix-middle-arrive"),
        ]);
    }
    if matches!(mode, OrdinaryPrefixHistory::ThreeZero | OrdinaryPrefixHistory::MiddleZero | OrdinaryPrefixHistory::MiddleEvents) {
        let before_withdrawal = store.load().unwrap();
        historical.push(before_withdrawal.clone());
        let (prepared, summary, report) = engine(&store).preview_owned_withdrawal(&store, &layer,
            "prefix-withdrawal", ProofBudget::default(), cancel()).unwrap();
        assert_eq!(report.disposition, CheckDisposition::Ready);
        assert_eq!(summary.retire_decisions, vec!["owned-policy"]);
        assert_eq!(prepared.seed(), &before_withdrawal.data);
        assert_eq!(store.load().unwrap(), before_withdrawal);
        let change = engine(&store).prepare_owned_withdrawal(&store, prepared,
            &["owned-policy".into()], "prefix-withdrawal").unwrap();
        assert_eq!(store.load().unwrap(), before_withdrawal);
        let withdrawn = engine(&store).adopt(&store, &change).unwrap();
        assert_eq!(withdrawn.data, before_withdrawal.data);
    }
    if matches!(mode, OrdinaryPrefixHistory::MiddleZero) {
        let before_tail = store.load().unwrap();
        assert_ordinary_prefix_material_state(&before_tail, &initial, true, "ready");
        assert!(before_tail.owned_scope().unwrap().compositions.values()
            .find(|m| m.output == before_tail.active_revision).unwrap().active.is_empty());
        assert!(matches!(&before_tail.decisions.decisions.iter().find(|d| d.id == "owned-policy").unwrap().status,
            DecisionStatus::Withdrawn { .. }));
        job_action(&store, "prefix-tail-complete", "complete", "reserve");
    }
    let current = store.load().unwrap();
    assert_eq!(current.data.records.iter().filter(|r| r.entity == "job").count(), 3);
    assert!(current.data.events.starts_with(&initial.data.events));
    let pointer = std::fs::read(dir.path().join("store/CURRENT")).unwrap();
    OrdinaryPrefixFixture { dir, store, initial, historical, current, pointer }
}

#[test]
fn ordinary_prefix_genuine_one_epoch() {
    let fixture = ordinary_prefix_history_fixture(OrdinaryPrefixHistory::One);
    fixture.current.assert_ordinary_prefix_for_test(&fixture.initial, &fixture.historical, &[0]);
    assert_eq!(fixture.store.load().unwrap(), fixture.current);
    assert_eq!(std::fs::read(fixture.dir.path().join("store/CURRENT")).unwrap(), fixture.pointer);
}

#[test]
fn ordinary_prefix_genuine_two_zero_epochs() {
    let fixture = ordinary_prefix_history_fixture(OrdinaryPrefixHistory::TwoZero);
    fixture.current.assert_ordinary_prefix_for_test(&fixture.initial, &fixture.historical, &[0, 0]);
    assert_eq!(fixture.store.load().unwrap(), fixture.current);
    assert_eq!(std::fs::read(fixture.dir.path().join("store/CURRENT")).unwrap(), fixture.pointer);
}

#[test]
fn ordinary_prefix_genuine_three_zero_epochs() {
    let fixture = ordinary_prefix_history_fixture(OrdinaryPrefixHistory::ThreeZero);
    fixture.current.assert_ordinary_prefix_for_test(&fixture.initial, &fixture.historical, &[0, 0, 0]);
    assert_eq!(fixture.store.load().unwrap(), fixture.current);
    assert_eq!(std::fs::read(fixture.dir.path().join("store/CURRENT")).unwrap(), fixture.pointer);
}

#[test]
fn ordinary_prefix_genuine_middle_zero_material_cycle() {
    let fixture = ordinary_prefix_history_fixture(OrdinaryPrefixHistory::MiddleZero);
    fixture.current.assert_ordinary_prefix_for_test(&fixture.initial, &fixture.historical, &[2, 0, 1]);
    assert_eq!(fixture.store.load().unwrap(), fixture.current);
    assert_eq!(std::fs::read(fixture.dir.path().join("store/CURRENT")).unwrap(), fixture.pointer);
}

#[test]
fn ordinary_prefix_genuine_middle_event_material_cycle() {
    let fixture = ordinary_prefix_history_fixture(OrdinaryPrefixHistory::MiddleEvents);
    fixture.current.assert_ordinary_prefix_for_test(&fixture.initial, &fixture.historical, &[0, 2, 0]);
    assert_eq!(fixture.store.load().unwrap(), fixture.current);
    assert_eq!(std::fs::read(fixture.dir.path().join("store/CURRENT")).unwrap(), fixture.pointer);
}

#[test]
fn historical_initial_loan_genuine_three_zero() {
    let fixture = ordinary_prefix_history_fixture(OrdinaryPrefixHistory::ThreeZero);
    fixture.current.assert_ordinary_prefix_for_test(&fixture.initial, &fixture.historical, &[0, 0, 0]);
    assert_eq!(fixture.historical.len(), 2);
    fixture.current.assert_historical_initial_loan_for_test(fixture.historical.last().unwrap());
    assert_eq!(fixture.store.load().unwrap(), fixture.current);
    assert_eq!(std::fs::read(fixture.dir.path().join("store/CURRENT")).unwrap(), fixture.pointer);
}

#[test]
fn historical_initial_loan_genuine_middle_zero() {
    let fixture = ordinary_prefix_history_fixture(OrdinaryPrefixHistory::MiddleZero);
    fixture.current.assert_ordinary_prefix_for_test(&fixture.initial, &fixture.historical, &[2, 0, 1]);
    assert_eq!(fixture.historical.len(), 2);
    fixture.current.assert_historical_initial_loan_for_test(fixture.historical.last().unwrap());
    assert_eq!(fixture.store.load().unwrap(), fixture.current);
    assert_eq!(std::fs::read(fixture.dir.path().join("store/CURRENT")).unwrap(), fixture.pointer);
}

#[test]
fn historical_initial_loan_genuine_middle_events() {
    let fixture = ordinary_prefix_history_fixture(OrdinaryPrefixHistory::MiddleEvents);
    fixture.current.assert_ordinary_prefix_for_test(&fixture.initial, &fixture.historical, &[0, 2, 0]);
    assert_eq!(fixture.historical.len(), 2);
    fixture.current.assert_historical_initial_loan_for_test(fixture.historical.last().unwrap());
    assert_eq!(fixture.store.load().unwrap(), fixture.current);
    assert_eq!(std::fs::read(fixture.dir.path().join("store/CURRENT")).unwrap(), fixture.pointer);
}

#[test]
fn combined_transition_reuse_genuine_route() {
    let fixture = ordinary_prefix_history_fixture(OrdinaryPrefixHistory::ThreeZero);
    fixture.current.assert_combined_transition_reuse_for_test(false);
    assert_eq!(fixture.store.load().unwrap(), fixture.current);
    assert_eq!(std::fs::read(fixture.dir.path().join("store/CURRENT")).unwrap(), fixture.pointer);
}

#[test]
fn combined_transition_reuse_fourth_epoch_route() {
    let fixture = ordinary_prefix_history_fixture(OrdinaryPrefixHistory::ThreeZero);
    let (a, b) = pair();
    let (a1, b1) = evolved(&a, &b, "ordinary-prefix-evolution");
    let (a2, b2) = evolved(&a1, &b1, "combined-inactive-evolution");
    let layer = fixture.current.owned_scope().unwrap().layers.keys().next().unwrap().clone();
    let before = fixture.store.load().unwrap();
    let (prepared, report) = engine(&fixture.store).prepare_owned_evolution(&fixture.store,
        &a2, &b2, &sources(&a2, &b2), &layer, "combined-inactive", ProofBudget::default(), cancel()).unwrap();
    assert_eq!(report.disposition, CheckDisposition::Ready);
    let change = engine(&fixture.store).prepare_owned_implementation_evolution(&fixture.store,
        prepared, "combined-inactive").unwrap();
    assert_eq!(fixture.store.load().unwrap(), before);
    let diagnostic = product_store::scope::adoption_diagnostic::start();
    let result = product_store::ProjectSnapshot::with_combined_accounting_for_test(||
        engine(&fixture.store).adopt(&fixture.store, &change));
    drop(diagnostic);
    if result.is_err() {
        assert_eq!(fixture.store.load().unwrap(), before);
        assert_eq!(std::fs::read(fixture.dir.path().join("store/CURRENT")).unwrap(), fixture.pointer);
    }
    let current = result.unwrap();
    assert_eq!(current.data, before.data);
    assert_eq!(current.decisions, before.decisions);
    assert_eq!(current.owned_scope().unwrap().adoptions.len(), 4);
    let pointer = std::fs::read(fixture.dir.path().join("store/CURRENT")).unwrap();
    current.assert_combined_transition_reuse_for_test(true);
    assert_eq!(fixture.store.load().unwrap(), current);
    assert_eq!(std::fs::read(fixture.dir.path().join("store/CURRENT")).unwrap(), pointer);
}

#[test]
fn completed_intervals_genuine_middle_events() {
    let fixture = ordinary_prefix_history_fixture(OrdinaryPrefixHistory::MiddleEvents);
    fixture.current.assert_completed_intervals_for_test();
    assert_eq!(fixture.store.load().unwrap(), fixture.current);
    assert_eq!(std::fs::read(fixture.dir.path().join("store/CURRENT")).unwrap(), fixture.pointer);
}

#[test]
fn completed_intervals_exact_original_source_derive_cost() {
    let (dir,store,a0,b0)=setup();
    let initial=adopt(&store,&a0,&b0);
    let layer=initial.owned_scope().unwrap().layers.keys().next().unwrap().clone();
    let (a1,b1)=evolved(&a0,&b0,"implementation-one");
    let (prepared,report)=engine(&store).prepare_owned_evolution(&store,&a1,&b1,
        &sources(&a1,&b1),&layer,"evolve-active",ProofBudget::default(),cancel()).unwrap();
    assert_eq!(report.disposition,CheckDisposition::Ready);
    let install=engine(&store).prepare_owned_implementation_evolution(&store,prepared,"evolve-active").unwrap();
    let current=engine(&store).adopt(&store,&install).unwrap();
    let pointer=std::fs::read(dir.path().join("store/CURRENT")).unwrap();
    current.assert_completed_source_derive_for_test();
    assert_eq!(store.load().unwrap(),current);
    assert_eq!(std::fs::read(dir.path().join("store/CURRENT")).unwrap(),pointer);
}

#[test]
fn completed_intervals_genuine_two_completions_and_cancellation() {
    let (dir,store,a,b)=archive_policy_small_basis();
    let initial=archive_policy_adopt_small(&store,&a,&b,false);
    job_action(&store,"completed-pair-original-seal","complete","reserve");
    let first_end=store.load().unwrap();
    let layer=initial.owned_scope().unwrap().layers.keys().next().unwrap().clone();
    let (a,b)=evolved(&a,&b,"completed-pair-second-producer");
    let (prepared,report)=engine(&store).prepare_owned_evolution(&store,&a,&b,&sources(&a,&b),&layer,
        "completed-pair-evolution",ProofBudget::default(),cancel()).unwrap();
    assert_eq!(report.disposition,CheckDisposition::Ready);
    let change=engine(&store).prepare_owned_implementation_evolution(&store,prepared,"completed-pair-evolution").unwrap();
    let evolved=engine(&store).adopt(&store,&change).unwrap();
    assert_eq!(evolved.data,first_end.data);
    job_action(&store,"completed-pair-evolved-seal","complete","live");
    let second_end=store.load().unwrap();
    let (prepared,summary,report)=engine(&store).preview_owned_withdrawal(&store,&layer,"completed-pair-withdrawal",
        ProofBudget::default(),cancel()).unwrap();
    assert_eq!(report.disposition,CheckDisposition::Ready);
    assert_eq!(summary.retire_decisions,vec!["owned-policy"]);
    assert_eq!(prepared.seed(),&second_end.data);
    let change=engine(&store).prepare_owned_withdrawal(&store,prepared,&["owned-policy".into()],"completed-pair-withdrawal").unwrap();
    let current=engine(&store).adopt(&store,&change).unwrap();
    assert_eq!(current.data,second_end.data);
    let pointer=std::fs::read(dir.path().join("store/CURRENT")).unwrap();
    current.assert_completed_intervals_with_seals_for_test();
    assert_eq!(store.load().unwrap(),current);
    assert_eq!(std::fs::read(dir.path().join("store/CURRENT")).unwrap(),pointer);
}

#[test]
fn cold_historical_serialization_genuine_four_epochs() {
    let fixture=ordinary_prefix_history_fixture(OrdinaryPrefixHistory::ThreeZero);
    let (a,b)=pair();let (a,b)=evolved(&a,&b,"ordinary-prefix-evolution");
    let (a,b)=evolved(&a,&b,"cold-serializer-fourth-producer");
    let layer=fixture.current.owned_scope().unwrap().layers.keys().next().unwrap().clone();
    let before=fixture.store.load().unwrap();
    let (prepared,report)=engine(&fixture.store).prepare_owned_evolution(&fixture.store,&a,&b,
        &sources(&a,&b),&layer,"cold-serializer-fourth",ProofBudget::default(),cancel()).unwrap();
    assert_eq!(report.disposition,CheckDisposition::Ready);
    let change=engine(&fixture.store).prepare_owned_implementation_evolution(&fixture.store,prepared,
        "cold-serializer-fourth").unwrap();
    assert_eq!(fixture.store.load().unwrap(),before);
    let adopted=engine(&fixture.store).adopt(&fixture.store,&change).unwrap();
    assert_eq!(adopted.data,before.data);assert_eq!(adopted.decisions,before.decisions);
    assert_eq!(adopted.owned_scope().unwrap().adoptions.len(),4);
    add(&fixture.store,"cold-serializer-new-add","cold-serializer-new-job");
    job_action(&fixture.store,"cold-serializer-new-complete","complete","cold-serializer-new-job");
    let current=fixture.store.load().unwrap();
    assert_eq!(current.data.events.len(),adopted.data.events.len()+2);
    assert_eq!(current.data.events.last().unwrap().operation_id,"cold-serializer-new-complete");
    let pointer=std::fs::read(fixture.dir.path().join("store/CURRENT")).unwrap();
    current.assert_cold_historical_serialization_for_test();
    assert_eq!(fixture.store.load().unwrap(),current);
    assert_eq!(std::fs::read(fixture.dir.path().join("store/CURRENT")).unwrap(),pointer);
    drop(fixture.store);
    let reopened=product_backup::open_verified(&fixture.dir.path().join("store"),None).unwrap().store;
    assert_eq!(reopened.load().unwrap(),current);
    assert_eq!(std::fs::read(fixture.dir.path().join("store/CURRENT")).unwrap(),pointer);
}
