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
use product_store::ProjectSnapshot;
use std::collections::{BTreeMap, BTreeSet};

fn witnesses(s: &ProductStore) -> BTreeMap<Id, Vec<u8>> {
    s.load().unwrap().decisions.decisions.iter().map(|d| (d.id.clone(), s.read_extension(&d.witness).unwrap())).collect()
}
fn assert_transition(before: &ProjectSnapshot, after: &ProjectSnapshot, operation: &str, transition: ScopeTransition, active: bool) {
    // Independent expected whole frame comes from the committed predecessor.
    assert_eq!(after.data, before.data);
    assert_eq!(after.session, before.session);
    assert_eq!(after.clock_day, before.clock_day);
    assert_eq!(after.artifacts, before.artifacts);
    assert_eq!(after.revision, before.revision + 1);
    assert_ne!(after.active_revision, before.active_revision);
    assert_eq!(&after.programs[..before.programs.len()], before.programs.as_slice());
    assert_eq!(after.adoptions.len(), before.adoptions.len() + 1);
    assert_eq!(&after.adoptions[..before.adoptions.len()], before.adoptions.as_slice());
    assert_eq!(after.operations.len(), before.operations.len() + 1);
    for (id, receipt) in &before.operations { assert_eq!(after.operations.get(id), Some(receipt)); }
    assert_eq!(after.operations[operation].revision, after.revision);
    let (old, new) = (before.owned_scope().unwrap(), after.owned_scope().unwrap());
    assert_eq!(canonical_bytes(&new.layers).unwrap(), canonical_bytes(&old.layers).unwrap());
    assert_eq!(canonical_bytes(&new.initializations).unwrap(), canonical_bytes(&old.initializations).unwrap());
    assert_eq!(new.rehearsals, old.rehearsals);
    assert_eq!(new.correspondences, old.correspondences);
    assert_eq!(new.compositions.len(), old.compositions.len() + 1);
    for (id, composition) in &old.compositions { assert_eq!(new.compositions.get(id), Some(composition)); }
    assert_eq!(new.adoptions.len(), old.adoptions.len() + 1);
    assert_eq!(&new.adoptions[..old.adoptions.len()], old.adoptions.as_slice());
    let receipt = new.adoptions.last().unwrap();
    assert!(receipt.initialization.is_none());
    assert!(receipt.decisions.is_empty());
    let composition = &new.compositions[&receipt.composition];
    assert_eq!(composition.operation, operation);
    assert_eq!(composition.transition, transition);
    assert_eq!(composition.lowering, 3);
    assert_eq!(composition.previous.as_ref(), Some(&before.active_revision));
    assert_eq!(composition.layers, old.layers.keys().cloned().collect::<Vec<_>>());
    let expected: BTreeSet<_> = if active { old.layers.keys().cloned().collect() } else { BTreeSet::new() };
    assert_eq!(composition.active, expected);
}
fn assert_original_history(initial: &ProjectSnapshot, current: &ProjectSnapshot, key: &str, completed: &[Record]) {
    let (old, new) = (initial.owned_scope().unwrap(), current.owned_scope().unwrap());
    assert_eq!(canonical_bytes(&new.layers).unwrap(), canonical_bytes(&old.layers).unwrap());
    assert_eq!(canonical_bytes(&new.initializations).unwrap(), canonical_bytes(&old.initializations).unwrap());
    // Original cohort expectation is captured before any transition, and never
    // reconstructed from the new manifest or from today's population.
    for original in initial.data.records.iter().filter(|r| r.entity == "job") {
        assert_eq!(current.data.records.iter().find(|r| r.id == original.id).unwrap().values[key], original.values[key]);
    }
    for sealed in completed { assert_eq!(current.data.records.iter().find(|r| r.id == sealed.id), Some(sealed)); }
    assert_eq!(&current.data.events[..initial.data.events.len()], initial.data.events.as_slice());
    assert_eq!(current.artifacts, initial.artifacts);
}
fn completed_record(s: &ProjectSnapshot, title: &str) -> Record {
    let id = row(&s.data, title).record;
    let record = s.data.records.iter().find(|r| r.id == id).unwrap();
    assert_eq!(record.values["status"], value("done"));
    record.clone()
}

fn assert_replay(report: &CheckReport, expected: &[&str]) {
    assert_eq!(report.disposition, CheckDisposition::Ready);
    assert_eq!(report.checks.iter().map(|c| c.decision.as_str()).collect::<BTreeSet<_>>(), expected.iter().copied().collect());
    for id in expected {
        assert!(report.checks.iter().any(|c| c.decision == *id && c.state == CheckState::Satisfied && c.evidence.is_some()));
    }
}
fn complete_with_actual_producer(s: &ProductStore, operation: &str, title: &str) -> Record {
    let before = s.load().unwrap();
    let producer = before.program().unwrap().artifact.program_digest.clone();
    job_action(s, operation, "complete", title);
    let after = s.load().unwrap();
    assert_eq!(after.data.events.len(), before.data.events.len() + 1);
    let event = after.data.events.last().unwrap();
    assert_eq!(event.operation_id, operation);
    assert_eq!(event.action, "complete");
    assert_eq!(event.program, producer);
    assert_eq!(event.day, before.clock_day);
    let record = completed_record(&after, title);
    assert!(event.changes.iter().any(|change| change.record == record.id && change.after == record.values));
    record
}

#[test]
fn all_policy_count_current_data_survives_evolution_withdrawal_and_fresh_recovery() {
    let (dir, store, a0, b0) = setup();
    let initial = adopt(&store, &a0, &b0);
    let original_graph = initial.decisions.clone();
    let original_witnesses = witnesses(&store);
    let key = member_key(&initial);
    let layer = initial.owned_scope().unwrap().layers.keys().next().unwrap().clone();
    let mut completed = vec![completed_record(&initial, "finished")];
    for title in ["live", "reserve", "complete-original", "finished"] { assert_member(&initial, title, &key, true); }

    completed.push(complete_with_actual_producer(&store, "complete-under-original", "complete-original"));
    job_action(&store, "begin-live", "begin", "live");
    apply(&store, "later-material-withdrawal", material_input(&store, "withdraw", "later shared fact"));
    apply(&store, "later-day", SemanticInput::AdvanceClock { days: 2 });
    apply(&store, "later-session", SemanticInput::Control { view: "jobs".into(), control: "name".into(), value: value("unfinished draft") });
    let before_evolution = store.load().unwrap();
    assert!(flag(&before_evolution, "live"));
    assert!(!flag(&before_evolution, "reserve"));
    assert!(before_evolution.data.events.len() > initial.data.events.len());
    assert_original_history(&initial, &before_evolution, &key, &completed);

    let (a1, b1) = evolved(&a0, &b0, "implementation-one");
    assert_ne!(a1.program, a0.program);
    assert_ne!(b1.program, b0.program);
    let (prepared, report) = engine(&store).prepare_owned_evolution(&store, &a1, &b1, &sources(&a1, &b1), &layer, "evolve-active", ProofBudget::default(), cancel()).unwrap();
    assert_replay(&report, &["independent-add", "owned-policy"]);
    assert_eq!(prepared.seed(), &before_evolution.data);
    assert_eq!(store.load().unwrap(), before_evolution);
    // This is actual changed variant behavior under the two real executables.
    let comparison = scenario(&store, "executable-evolution", vec![
        material_input(&store, "arrive", "comparison restore"),
        invoke("begin", [("job".into(), reference(&row(&before_evolution.data, "reserve")))].into()),
        material_input(&store, "withdraw", "comparison withdraw"),
    ]);
    let old_scene = engine(&store).accept_owned_transition_current_scene(&store, &prepared, &comparison, Disclosure::Synthetic).unwrap();
    let new_scene = engine(&store).accept_owned_transition_scene(&store, &prepared, &comparison, Disclosure::Synthetic).unwrap();
    assert_eq!(observed_count(&old_scene), 2);
    assert_eq!(observed_count(&new_scene), 1);
    assert_eq!(store.load().unwrap(), before_evolution);
    let install = engine(&store).prepare_owned_implementation_evolution(&store, prepared, "evolve-active").unwrap();
    assert_eq!(store.load().unwrap(), before_evolution);
    let evolved = engine(&store).adopt(&store, &install).unwrap();
    assert_transition(&before_evolution, &evolved, "evolve-active", ScopeTransition::Evolution, true);
    assert_eq!(evolved.decisions, original_graph);
    assert_eq!(witnesses(&store), original_witnesses);
    assert_original_history(&initial, &evolved, &key, &completed);

    apply(&store, "evolved-arrival", material_input(&store, "arrive", "actual restore"));
    job_action(&store, "evolved-begin-reserve", "begin", "reserve");
    apply(&store, "evolved-material-withdrawal", material_input(&store, "withdraw", "actual evolved memory"));
    add(&store, "add-evolved-completion", "evolved-completion");
    assert_eq!(store.load().unwrap().session.values[MARKER], value("implementation-one"));
    assert_member(&store.load().unwrap(), "evolved-completion", &key, true);
    completed.push(complete_with_actual_producer(&store, "complete-under-evolved", "evolved-completion"));
    add(&store, "later-unfinished-record", "later-unfinished");
    let before_withdrawal = store.load().unwrap();
    assert!(flag(&before_withdrawal, "live"));
    assert!(!flag(&before_withdrawal, "reserve"));
    assert_original_history(&initial, &before_withdrawal, &key, &completed);
    let (prepared, summary, report) = engine(&store).preview_owned_withdrawal(&store, &layer, "withdraw-layer", ProofBudget::default(), cancel()).unwrap();
    assert_replay(&report, &["independent-add"]);
    assert_eq!(prepared.seed(), &before_withdrawal.data);
    assert_eq!(summary.retire_decisions, vec!["owned-policy"]);
    assert_eq!(store.load().unwrap(), before_withdrawal);
    let change = engine(&store).prepare_owned_withdrawal(&store, prepared, &["owned-policy".into()], "withdraw-layer").unwrap();
    assert_eq!(store.load().unwrap(), before_withdrawal);
    let withdrawn = engine(&store).adopt(&store, &change).unwrap();
    assert_transition(&before_withdrawal, &withdrawn, "withdraw-layer", ScopeTransition::Withdrawal, false);
    assert!(flag(&withdrawn, "live")); // Legitimate later fact is NOT reset by withdrawal.
    let mut expected_graph = before_withdrawal.decisions.clone();
    expected_graph.revision += 1;
    let decision = expected_graph.decisions.iter_mut().find(|d| d.id == "owned-policy").unwrap();
    decision.status = DecisionStatus::Withdrawn { adoption: "withdraw-layer".into() };
    decision.revision += 1;
    assert_eq!(withdrawn.decisions, expected_graph);
    assert_eq!(witnesses(&store), original_witnesses);
    assert_replay(&engine(&store).check_current(&withdrawn).unwrap(), &["independent-add"]);
    assert_original_history(&initial, &withdrawn, &key, &completed);

    let dormant_aux: Vec<_> = withdrawn.data.records.iter().filter(|r| r.entity == "pause").cloned().collect();
    assert_eq!(dormant_aux.len(), 2);
    apply(&store, "inactive-arrival", material_input(&store, "arrive", "baseline restore"));
    assert!(!flag(&store.load().unwrap(), "live")); // A real baseline action now changes F.
    completed.push(complete_with_actual_producer(&store, "complete-after-withdrawal", "live"));
    add(&store, "inactive-add", "inactive-before-evolution");
    assert_member(&store.load().unwrap(), "inactive-before-evolution", &key, false);
    assert_eq!(store.load().unwrap().session.values[MARKER], value("implementation-one"));
    assert_eq!(store.load().unwrap().data.records.iter().filter(|r| r.entity == "pause").cloned().collect::<Vec<_>>(), dormant_aux);
    let before_inactive = store.load().unwrap();
    let (a2, b2) = fixture::evolved(&a1, &b1, "implementation-two");
    let (prepared, report) = engine(&store).prepare_owned_evolution(&store, &a2, &b2, &sources(&a2, &b2), &layer, "evolve-inactive", ProofBudget::default(), cancel()).unwrap();
    assert_replay(&report, &["independent-add"]);
    assert_eq!(store.load().unwrap(), before_inactive);
    let change = engine(&store).prepare_owned_implementation_evolution(&store, prepared, "evolve-inactive").unwrap();
    assert_eq!(store.load().unwrap(), before_inactive);
    let inactive_evolved = engine(&store).adopt(&store, &change).unwrap();
    assert_transition(&before_inactive, &inactive_evolved, "evolve-inactive", ScopeTransition::Evolution, false);
    assert_eq!(inactive_evolved.decisions, expected_graph);
    assert_eq!(witnesses(&store), original_witnesses);
    assert_original_history(&initial, &inactive_evolved, &key, &completed);
    add(&store, "inactive-evolved-add", "inactive-completion");
    assert_eq!(store.load().unwrap().session.values[MARKER], value("implementation-two"));
    assert_member(&store.load().unwrap(), "inactive-completion", &key, false);
    completed.push(complete_with_actual_producer(&store, "inactive-evolved-complete", "inactive-completion"));
    let saved = store.load().unwrap();
    assert_eq!(saved.data.records.iter().filter(|r| r.entity == "pause").cloned().collect::<Vec<_>>(), dormant_aux);
    drop(store);

    let reopened = product_backup::open_verified(&dir.path().join("store"), None).unwrap().store;
    assert_eq!(reopened.load().unwrap(), saved);
    assert_replay(&engine(&reopened).check_current(&saved).unwrap(), &["independent-add"]);
    let bytes = product_backup::VerifiedBackup::capture(&reopened).unwrap().to_bytes().unwrap();
    let decoded = product_backup::VerifiedBackup::from_bytes(&bytes).unwrap();
    assert_eq!(decoded.snapshot(), &saved);
    let recovery_dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let recent = product_locations::RecentTools::open(recovery_dir.path()).unwrap();
    let created = decoded.recover_tool(&recovery_dir.path().join("recovered"), &recent, 1).unwrap();
    assert!(created.registration.is_ok());
    drop(created);
    let recovered = product_backup::open_verified(&recovery_dir.path().join("recovered"), None).unwrap().store;
    assert_eq!(recovered.load().unwrap(), saved);
    assert_eq!(witnesses(&recovered), original_witnesses);
    assert_replay(&engine(&recovered).check_current(&saved).unwrap(), &["independent-add"]);
    assert_original_history(&initial, &saved, &key, &completed);
    add(&recovered, "recovered-normal-add", "recovered-normal");
    assert_member(&recovered.load().unwrap(), "recovered-normal", &key, false);
    assert_eq!(recovered.load().unwrap().session.values[MARKER], value("implementation-two"));
    job_action(&recovered, "recovered-normal-complete", "complete", "recovered-normal");
    let final_state = recovered.load().unwrap();
    assert_eq!(final_state.decisions, expected_graph);
    assert_eq!(final_state.data.records.iter().filter(|r| r.entity == "pause").cloned().collect::<Vec<_>>(), dormant_aux);
    assert_original_history(&initial, &final_state, &key, &completed);
    assert_replay(&engine(&recovered).check_current(&final_state).unwrap(), &["independent-add"]);
    assert_eq!(reopened.load().unwrap(), saved); // Recovery never overwrites the original tool.
}
