//! Fresh bounded P5 history negatives. Positive frames come from real writers.
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
use product_runtime::ReplayAdmission;
use product_store::{scope::*, ProductStore, ProjectSnapshot};

fn evolved_store() -> (tempfile::TempDir, ProductStore, ProjectSnapshot) {
    let (dir, store, before, after) = setup();
    adopt(&store, &before, &after);
    job_action(&store, "old-complete", "complete", "complete-original");
    let original = store.load().unwrap();
    let layer = original
        .owned_scope()
        .unwrap()
        .layers
        .keys()
        .next()
        .unwrap()
        .clone();
    let (next_before, next_after) = evolved(&before, &after, "history-epoch");
    let (prepared, _) = engine(&store)
        .prepare_owned_evolution(
            &store,
            &next_before,
            &next_after,
            &sources(&next_before, &next_after),
            &layer,
            "history-evolve",
            ProofBudget::default(),
            cancel(),
        )
        .unwrap();
    let change = engine(&store)
        .prepare_owned_implementation_evolution(&store, prepared, "history-evolve")
        .unwrap();
    engine(&store).adopt(&store, &change).unwrap();
    add(&store, "new-epoch-add", "new-epoch");
    (dir, store, original)
}
fn state(snapshot: &mut ProjectSnapshot) -> &mut OwnedScopeStateV2 {
    match &mut snapshot.scope {
        ScopeEnvelope::OwnedV2(s) => s,
        _ => panic!("owned fixture"),
    }
}
fn reject(snapshot: &ProjectSnapshot) {
    assert!(snapshot.validate().is_err());
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("recovered");
    assert!(ProductStore::create_recovered_with(&destination, snapshot, |_| Ok(())).is_err());
    assert!(!destination.join("CURRENT").exists());
}
// Rehash the manifest, its ordinary plan linkage and exact transition request.
// This does not forge opaque execution authority: admission must still refuse.
fn alter_last_manifest(
    snapshot: &mut ProjectSnapshot,
    change: impl FnOnce(&mut OwnedCompositionV2),
) {
    let (old_id, mut manifest) = {
        let owned = snapshot.owned_scope().unwrap();
        let id = owned.adoptions.last().unwrap().composition.clone();
        (id.clone(), owned.compositions[&id].clone())
    };
    change(&mut manifest);
    let new_id = manifest.identity().unwrap();
    let owned = state(snapshot);
    owned.compositions.remove(&old_id);
    owned.compositions.insert(new_id.clone(), manifest.clone());
    owned.adoptions.last_mut().unwrap().composition = new_id;
    let adoption = snapshot
        .adoptions
        .iter()
        .find(|a| a.plan.id == manifest.operation)
        .unwrap();
    let request = canonical_digest(
        IdentityDomain::Adoption,
        &(
            "owned-transition-recording/2",
            (&adoption.plan, &snapshot.decisions, &manifest),
        ),
    )
    .unwrap();
    snapshot
        .operations
        .get_mut(&manifest.operation)
        .unwrap()
        .request = request;
}
#[test]
fn rehashed_transition_dispatch_and_predecessor_forgeries_are_rejected() {
    let (_dir, store, original) = evolved_store();
    let valid = store.load().unwrap();
    valid.validate().unwrap();
    for case in 0..7 {
        let mut bad = valid.clone();
        match case {
            0 => alter_last_manifest(&mut bad, |m| m.lowering = 99),
            1 => alter_last_manifest(&mut bad, |m| m.lowering = 2),
            2 => {
                let init = bad.owned_scope().unwrap().initializations[0]
                    .identity()
                    .unwrap();
                state(&mut bad).adoptions.last_mut().unwrap().initialization = Some(init);
            }
            3 => alter_last_manifest(&mut bad, |m| {
                m.previous = Some(
                    original
                        .owned_scope()
                        .unwrap()
                        .layers
                        .values()
                        .next()
                        .unwrap()
                        .basis
                        .frozen
                        .active
                        .clone(),
                )
            }),
            4 => state(&mut bad).adoptions.reverse(),
            5 => alter_last_manifest(&mut bad, |m| {
                m.basis.sources.pop();
            }),
            6 => {
                let duplicate = bad.owned_scope().unwrap().adoptions[0].clone();
                state(&mut bad).adoptions.push(duplicate);
            }
            _ => unreachable!(),
        }
        reject(&bad);
        assert_eq!(store.load().unwrap(), valid);
    }
}
#[test]
fn event_operation_interval_rejects_retained_wrong_producer_and_adoption_revisions() {
    let (_dir, store, original) = evolved_store();
    let valid = store.load().unwrap();
    // Direct mechanism evidence: reconstruct the genuine recipe first, then
    // probe only the actual producer-interval guard with unique swapped receipts.
    valid.assert_owned_epoch_interval_rejects_unique_swapped_receipts_for_test();
    for case in 0..4 {
        let mut bad = valid.clone();
        let adoption_revision = bad.operations["history-evolve"].revision;
        match case {
            0 => {
                let old_revision = bad.operations["old-complete"].revision;
                let new_revision = bad.operations["new-epoch-add"].revision;
                bad.operations.get_mut("old-complete").unwrap().revision = new_revision;
                bad.operations.get_mut("new-epoch-add").unwrap().revision = old_revision;
                // Public intake evidence is separate: authenticating the frozen
                // historical snapshot rejects before the interval guard runs.
                let error = bad.validate().unwrap_err();
                assert!(
                    matches!(&error, product_store::StoreError::Invalid(message)
                    if message == "Common factual admission: owned persisted proof: complete historical snapshot does not reconstruct"),
                    "{error}"
                );
            }
            1 => bad.operations.get_mut("new-epoch-add").unwrap().revision = adoption_revision,
            2 => {
                let revision = bad.operations["old-complete"].revision;
                bad.operations.get_mut("new-epoch-add").unwrap().revision = revision;
            }
            3 => {
                let event = bad
                    .data
                    .events
                    .iter_mut()
                    .find(|e| e.operation_id == "new-epoch-add")
                    .unwrap();
                event.program = original.program().unwrap().artifact.program_digest.clone();
                let digest = canonical_digest(
                    IdentityDomain::Input,
                    &(&event.operation_id, &event.program),
                )
                .unwrap();
                event.id = format!("event-{}", &digest.as_str()[..32]);
            }
            _ => unreachable!(),
        }
        reject(&bad);
    }
    assert_eq!(store.load().unwrap(), valid);
}
#[test]
fn superseded_capture_and_inactive_member_forgeries_do_not_recover() {
    let (_dir, store, _) = evolved_store();
    job_action(&store, "history-begin-live", "begin", "live");
    apply(
        &store,
        "history-make-memory",
        material_input(&store, "withdraw", "genuine dormant history"),
    );
    let layer = store
        .load()
        .unwrap()
        .owned_scope()
        .unwrap()
        .layers
        .keys()
        .next()
        .unwrap()
        .clone();
    let (prepared, summary, _) = engine(&store)
        .preview_owned_withdrawal(
            &store,
            &layer,
            "history-withdraw",
            ProofBudget::default(),
            cancel(),
        )
        .unwrap();
    let change = engine(&store)
        .prepare_owned_withdrawal(
            &store,
            prepared,
            &summary.retire_decisions,
            "history-withdraw",
        )
        .unwrap();
    engine(&store).adopt(&store, &change).unwrap();
    add(&store, "inactive-birth", "inactive-root");
    let valid = store.load().unwrap();
    assert!(valid.data.records.iter().any(|r| r.entity == "pause"));
    for case in 0..5 {
        let mut bad = valid.clone();
        if case < 2 {
            let r = row(&bad.data, "complete-original");
            let root = bad
                .data
                .records
                .iter_mut()
                .find(|x| x.id == r.record)
                .unwrap();
            let key = root
                .values
                .keys()
                .find(|k| {
                    if case == 0 {
                        k.ends_with("_origin")
                    } else {
                        k.ends_with("_p0")
                    }
                })
                .unwrap()
                .clone();
            if case == 0 {
                root.values.insert(key, value("forged-producer"));
            } else {
                root.values.remove(&key);
            }
        } else if case == 2 {
            let key = member_key(&bad);
            let r = row(&bad.data, "inactive-root");
            bad.data
                .records
                .iter_mut()
                .find(|x| x.id == r.record)
                .unwrap()
                .values
                .insert(key.clone(), DataValue::Boolean { value: true });
            bad.data
                .events
                .iter_mut()
                .find(|e| e.operation_id == "inactive-birth")
                .unwrap()
                .changes
                .iter_mut()
                .find(|c| c.record == r.record)
                .unwrap()
                .after
                .insert(key, DataValue::Boolean { value: true });
        }
        if case == 3 {
            let auxiliary = bad
                .data
                .records
                .iter_mut()
                .find(|r| r.entity == "pause")
                .unwrap();
            let before = auxiliary.values.clone();
            auxiliary.revision += 1;
            let revision_key = auxiliary
                .values
                .keys()
                .find(|k| k.ends_with("_revision"))
                .unwrap()
                .clone();
            auxiliary.values.insert(
                revision_key,
                DataValue::Integer {
                    value: auxiliary.revision as i64,
                },
            );
            auxiliary
                .values
                .insert("pending".into(), DataValue::Boolean { value: false });
            let change = RecordChange {
                entity: auxiliary.entity.clone(),
                record: auxiliary.id.clone(),
                before: Some(before),
                after: auxiliary.values.clone(),
                archived: false,
            };
            bad.data
                .events
                .iter_mut()
                .find(|e| e.operation_id == "inactive-birth")
                .unwrap()
                .changes
                .push(change);
        } else if case == 4 {
            alter_last_manifest(&mut bad, |manifest| {
                manifest.correspondences[0].source = manifest.previous.clone().unwrap();
            });
        }
        reject(&bad);
    }
    assert_eq!(store.load().unwrap(), valid);
}
#[test]
fn protected_replay_rejects_unbound_historical_and_current_frames() {
    let (_dir, store, original) = evolved_store();
    let valid = store.load().unwrap();
    let context = ScopedExecutionContext::committed(&valid).unwrap();
    let target = valid.program().unwrap();
    let mut wrong = valid.data.clone();
    wrong.records[0].revision += 1;
    assert!(context
        .validate_seed(target, &wrong, valid.clock_day)
        .is_err());
    assert!(context
        .validate_seed(original.program().unwrap(), &valid.data, valid.clock_day)
        .is_err());
    let mut foreign = target.clone();
    foreign.source_bytes.push(b' ');
    assert!(!context.contains_managed_source(&foreign));
    let mut spec = scenario(&store, "bound-frame", vec![]);
    spec.random_seed = 99;
    assert!(context.replay_operation_ids(target, &spec).is_err());
}
