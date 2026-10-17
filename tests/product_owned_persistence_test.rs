//! Host acceptance is synthetic here, through the production controller boundary.
#[path = "fixtures/common_fact/mod.rs"]
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
use product_store::{scope::*, ProductStore};
use std::sync::{atomic::AtomicBool, Arc};
fn setup(a: &CapturedProgram) -> (tempfile::TempDir, ProductStore) {
    let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let store = ProductStore::create(dir.path().join("store"), a, 20000).unwrap();
    for (id, title) in [("one", "live"), ("two", "finished")] {
        store
            .apply(
                store.load().unwrap().revision,
                id,
                &SemanticInput::Invoke {
                    action: "add".into(),
                    arguments: [(
                        "title".into(),
                        DataValue::Text {
                            value: title.into(),
                        },
                    )]
                    .into(),
                },
                RuntimeLimits::default(),
            )
            .unwrap();
    }
    let s = store.load().unwrap();
    let row = s
        .data
        .records
        .iter()
        .find(|x| {
            x.values.get("title")
                == Some(&DataValue::Text {
                    value: "finished".into(),
                })
        })
        .unwrap();
    store
        .apply(
            s.revision,
            "finish",
            &SemanticInput::Invoke {
                action: "complete".into(),
                arguments: [(
                    "job".into(),
                    DataValue::Reference {
                        entity: "job".into(),
                        record: row.id.clone(),
                    },
                )]
                .into(),
            },
            RuntimeLimits::default(),
        )
        .unwrap();
    (dir, store)
}
fn witnesses(store: &ProductStore) -> Vec<LifecycleWitness> {
    let s = store.load().unwrap();
    let row = |done: bool| {
        let r = s
            .data
            .records
            .iter()
            .find(|r| {
                r.entity == "job"
                    && (r.values["status"]
                        == DataValue::Text {
                            value: "done".into(),
                        })
                        == done
            })
            .unwrap();
        RecordRef {
            entity: r.entity.clone(),
            record: r.id.clone(),
        }
    };
    [OwnedSourceSide::Before, OwnedSourceSide::After]
        .into_iter()
        .map(|side| LifecycleWitness {
            side,
            scenario: ScenarioSpec {
                version: 1,
                id: "lifecycle".into(),
                label: "Synthetic lifecycle witnesses".into(),
                seed: s.data.clone(),
                session: s.session.clone(),
                clock_day: s.clock_day,
                random_seed: 7,
                inputs: vec![SemanticInput::Observe {
                    point: "witness".into(),
                }],
                validity: vec![],
            },
            completed: row(true),
            unfinished: row(false),
        })
        .collect()
}
fn verify(
    store: &ProductStore,
    b: &CapturedProgram,
    r: &OwnedPreviewRequest,
) -> Result<VerifiedOwnedPreviewPlan, product_store::StoreError> {
    let mut host = LifecycleAcceptanceHost::prepare(
        store,
        b,
        r,
        &witnesses(store),
        ProofBudget::default(),
        Arc::new(AtomicBool::new(false)),
    )?;
    let accepted = host.synthetic_operator_accept_for_test(store)?;
    store.verify_owned_preview(
        b,
        r,
        &accepted,
        ProofBudget::default(),
        Arc::new(AtomicBool::new(false)),
    )
}

fn cancel() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}
fn prepare(s: &ProductStore, b: &CapturedProgram, r: &OwnedPreviewRequest) -> PreparedOwnedPreview {
    let proof = verify(s, b, r).unwrap();
    s.prepare_owned_preview(&proof, ProofBudget::default(), cancel())
        .unwrap()
}
fn row(s: &DataSnapshot, title: &str) -> RecordRef {
    let r = s
        .records
        .iter()
        .find(|r| {
            r.entity == "job"
                && r.values.get("title")
                    == Some(&DataValue::Text {
                        value: title.into(),
                    })
        })
        .unwrap();
    RecordRef {
        entity: r.entity.clone(),
        record: r.id.clone(),
    }
}
fn invoke(action: &str, arguments: Values) -> SemanticInput {
    SemanticInput::Invoke {
        action: action.into(),
        arguments,
    }
}
fn reference(r: &RecordRef) -> DataValue {
    DataValue::Reference {
        entity: r.entity.clone(),
        record: r.record.clone(),
    }
}
#[test]
fn owned_proof_roundtrip_reconstructs_and_reopens_without_write_authority() {
    let (a, b) = pair();
    let (_dir, store) = setup(&a);
    let request = request(&store, &a, &b);
    let preview = prepare(&store, &b, &request);
    let snapshot = owned_snapshot_for_test(&preview, "owned-adoption", false).unwrap();
    assert!(matches!(&snapshot.scope, ScopeEnvelope::OwnedV2(_)));
    snapshot.validate().unwrap();
    let bytes = canonical_bytes(&snapshot).unwrap();
    let decoded: product_store::ProjectSnapshot = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(snapshot, decoded);
    decoded.validate().unwrap();
    let root = tempfile::tempdir().unwrap();
    let reopened =
        ProductStore::create_recovered_with(root.path().join("restored"), &decoded, |_| Ok(()))
            .unwrap();
    assert_eq!(reopened.load().unwrap(), decoded);
    assert!(!reopened.runtime_view().unwrap().history.results.is_empty());
    let context = ScopedExecutionContext::committed(&decoded).unwrap();
    assert!(context.contains_managed_source(decoded.program().unwrap()));
    assert!(reopened.prepare_switch(&b, "ordinary-bypass").is_err());
}
#[test]
fn naked_owned_state_is_not_an_entry_authority() {
    let (a, _) = pair();
    let (_dir, store) = setup(&a);
    let mut snapshot = store.load().unwrap();
    snapshot.scope = ScopeEnvelope::OwnedV2(OwnedScopeStateV2::default());
    assert!(snapshot.validate().is_err());
}

fn owned_fixture() -> (
    tempfile::TempDir,
    ProductStore,
    product_store::ProjectSnapshot,
) {
    let (a, b) = pair();
    let (dir, store) = setup(&a);
    let r = request(&store, &a, &b);
    let prepared = prepare(&store, &b, &r);
    let snapshot = owned_snapshot_for_test(&prepared, "owned-adoption", false).unwrap();
    (dir, store, snapshot)
}
#[test]
fn protected_replay_binds_full_frame_and_original_ids() {
    let (_dir, _store, snapshot) = owned_fixture();
    let context = ScopedExecutionContext::committed(&snapshot).unwrap();
    let mut scene = ScenarioSpec {
        version: 1,
        id: "owned-replay".into(),
        label: "Protected owned replay".into(),
        seed: snapshot.data.clone(),
        session: snapshot.session.clone(),
        clock_day: snapshot.clock_day,
        random_seed: 0,
        inputs: vec![SemanticInput::Observe {
            point: "result".into(),
        }],
        validity: vec![],
    };
    let runtime = product_runtime::LocalRuntime::default().with_admission(Arc::new(context));
    let result = runtime
        .replay(
            snapshot.program().unwrap(),
            &scene,
            &snapshot.decisions,
            RuntimeLimits::default(),
            "owned-run",
        )
        .unwrap();
    assert_eq!(result.state, EvidenceState::Observed);
    let mut forged_graph = snapshot.decisions.clone();
    forged_graph.revision += 1;
    assert!(
        runtime
            .replay(
                snapshot.program().unwrap(),
                &scene,
                &forged_graph,
                RuntimeLimits::default(),
                "wrong-graph"
            )
            .is_err(),
        "a different decision frame cannot borrow owned replay authority"
    );
    scene.random_seed = 17;
    assert!(runtime
        .replay(
            snapshot.program().unwrap(),
            &scene,
            &snapshot.decisions,
            RuntimeLimits::default(),
            "wrong-random"
        )
        .is_err());
}
#[test]
fn retained_prospective_recipe_reconstructs_without_live_initialization() {
    let (a, b) = pair();
    let (_dir, store) = setup(&a);
    let r = request(&store, &a, &b);
    let before = store.load().unwrap();
    let prepared = prepare(&store, &b, &r);
    let snapshot = owned_snapshot_for_test(&prepared, "owned-pending", true).unwrap();
    snapshot.validate().unwrap();
    assert_eq!(snapshot.data, before.data);
    assert_eq!(snapshot.session, before.session);
    assert_eq!(snapshot.active_revision, before.active_revision);
    assert!(snapshot.owned_scope().unwrap().layers.is_empty());
    let context = ScopedExecutionContext::committed(&snapshot).unwrap();
    let proof = snapshot
        .owned_scope()
        .unwrap()
        .rehearsals
        .values()
        .next()
        .unwrap();
    let target = snapshot
        .programs
        .iter()
        .find(|p| canonical_digest(IdentityDomain::Source, *p).unwrap() == proof.manifest.output)
        .unwrap();
    let mapped = before.data.clone();
    let (_, seed) = context
        .project_seed(
            before.program().unwrap(),
            &before.data,
            target,
            &mapped,
            before.clock_day,
        )
        .unwrap();
    assert_eq!(seed.identity().unwrap(), proof.seed);
    let mut leaked = snapshot.clone();
    leaked.data = seed;
    assert!(leaked.validate().is_err());
}

fn rehash_owned(snapshot: &mut product_store::ProjectSnapshot) {
    let ScopeEnvelope::OwnedV2(state) = &mut snapshot.scope else {
        panic!()
    };
    if state.layers.len() == 1 {
        let layer = state.layers.values().next().unwrap().clone();
        let layer_id = layer.identity().unwrap();
        state.layers = [(layer_id.clone(), layer)].into();
        for init in &mut state.initializations {
            init.layer = layer_id.clone();
        }
        let mut manifests = std::collections::BTreeMap::new();
        for mut manifest in state.compositions.values().cloned() {
            manifest.layers = vec![layer_id.clone()];
            manifest.active = [layer_id.clone()].into();
            manifests.insert(manifest.identity().unwrap(), manifest);
        }
        state.compositions = manifests;
        for receipt in &mut state.adoptions {
            let (id, manifest) = state.compositions.iter().next().unwrap();
            let init = &state.initializations[0];
            receipt.composition = id.clone();
            receipt.initialization = Some(init.identity().unwrap());
            let a = snapshot
                .adoptions
                .iter()
                .find(|a| a.revision == receipt.revision)
                .unwrap();
            snapshot.operations.get_mut(&a.plan.id).unwrap().request = canonical_digest(
                IdentityDomain::Adoption,
                &(
                    "owned-adoption-recording/2",
                    &(&a.plan, &snapshot.decisions, manifest, init),
                ),
            )
            .unwrap();
        }
    }
    state.rehearsals = state
        .rehearsals
        .values()
        .cloned()
        .map(|p| (p.identity().unwrap(), p))
        .collect();
}
#[test]
fn rehash_forged_recipe_fields_reject() {
    let (_dir, _store, snapshot) = owned_fixture();
    let cases: Vec<(&str, Box<dyn Fn(&mut OwnedScopeStateV2)>)> = vec![
        (
            "inventory",
            Box::new(|s| {
                s.layers
                    .values_mut()
                    .next()
                    .unwrap()
                    .inventory
                    .certificates
                    .pop();
            }),
        ),
        (
            "projection",
            Box::new(|s| {
                s.layers
                    .values_mut()
                    .next()
                    .unwrap()
                    .inventory
                    .projections
                    .pop();
            }),
        ),
        (
            "capture",
            Box::new(|s| {
                s.layers
                    .values_mut()
                    .next()
                    .unwrap()
                    .inventory
                    .captured
                    .clear();
            }),
        ),
        (
            "lowering",
            Box::new(|s| {
                s.layers.values_mut().next().unwrap().lowering = 3;
            }),
        ),
        (
            "scope version",
            Box::new(|s| {
                s.layers.values_mut().next().unwrap().version = 1;
            }),
        ),
        (
            "initialization",
            Box::new(|s| {
                s.initializations[0].original_event_count += 1;
            }),
        ),
        (
            "additions",
            Box::new(|s| {
                s.initializations[0].additions.clear();
            }),
        ),
        (
            "auxiliary",
            Box::new(|s| {
                s.initializations[0].auxiliary.clear();
            }),
        ),
        (
            "runtime",
            Box::new(|s| {
                s.compositions.values_mut().next().unwrap().runtime = "forged-runtime".into();
            }),
        ),
        (
            "source inventory",
            Box::new(|s| {
                s.layers.values_mut().next().unwrap().basis.sources.clear();
            }),
        ),
        (
            "lifecycle witnesses",
            Box::new(|s| {
                s.layers
                    .values_mut()
                    .next()
                    .unwrap()
                    .lifecycle
                    .witnesses
                    .pop();
            }),
        ),
    ];
    for (label, mutate) in cases {
        let mut bad = snapshot.clone();
        let ScopeEnvelope::OwnedV2(state) = &mut bad.scope else {
            panic!()
        };
        mutate(state);
        rehash_owned(&mut bad);
        assert!(
            bad.validate().is_err(),
            "rehashed {label} must be reconstructed, not trusted"
        );
    }
}
#[test]
fn unknown_inactive_proof_and_second_layer_fail_the_whole_family() {
    let (_dir, _store, snapshot) = owned_fixture();
    let mut bad = snapshot.clone();
    let ScopeEnvelope::OwnedV2(s) = &mut bad.scope else {
        panic!()
    };
    let mut second = s.layers.values().next().unwrap().clone();
    second.operation = "second-layer".into();
    s.layers.insert(second.identity().unwrap(), second);
    assert!(bad.validate().is_err());
    let (a, b) = pair();
    let (_d, store) = setup(&a);
    let r = request(&store, &a, &b);
    let preview = prepare(&store, &b, &r);
    let prospective = owned_snapshot_for_test(&preview, "prospective", true).unwrap();
    let mut inactive = prospective
        .owned_scope()
        .unwrap()
        .rehearsals
        .values()
        .next()
        .unwrap()
        .clone();
    inactive.version = 99;
    let mut bad = snapshot.clone();
    let ScopeEnvelope::OwnedV2(s) = &mut bad.scope else {
        panic!()
    };
    s.rehearsals.insert(inactive.identity().unwrap(), inactive);
    assert!(bad.validate().is_err());
    let mut naked = snapshot.clone();
    let ScopeEnvelope::OwnedV2(s) = &mut naked.scope else {
        panic!()
    };
    s.adoptions.clear();
    assert!(naked.validate().is_err());
    let mut missing = snapshot.clone();
    let ScopeEnvelope::OwnedV2(s) = &mut missing.scope else {
        panic!()
    };
    s.initializations.clear();
    assert!(missing.validate().is_err());
}
#[test]
fn owned_closed_wire_and_cross_family_sources_reject() {
    let (_dir, _store, snapshot) = owned_fixture();
    let mut value = serde_json::to_value(&snapshot).unwrap();
    value["scope"]["state"]["verified"] = serde_json::json!(true);
    assert!(serde_json::from_value::<product_store::ProjectSnapshot>(value).is_err());
    let mut value = serde_json::to_value(&snapshot).unwrap();
    value["scope"]["family"] = serde_json::json!("scalar_v1");
    assert!(serde_json::from_value::<product_store::ProjectSnapshot>(value).is_err());
    let mut bad = snapshot.clone();
    bad.scope = ScopeEnvelope::ScalarV1(ScopeState::default());
    assert!(bad.validate().is_err());
    let mut bad = snapshot.clone();
    bad.active_revision = bad.programs[0].clone().binding.source_digest;
    assert!(bad.validate().is_err());
}
#[test]
fn completed_rows_and_present_null_cells_cannot_be_forged() {
    let (_dir, _store, snapshot) = owned_fixture();
    for alter in 0..3 {
        let mut bad = snapshot.clone();
        let row = bad
            .data
            .records
            .iter_mut()
            .find(|r| {
                r.entity == "job"
                    && r.values.get("status")
                        == Some(&DataValue::Text {
                            value: "done".into(),
                        })
            })
            .unwrap();
        let key = row
            .values
            .keys()
            .find(|k| k.starts_with("gm_scope_") && k.ends_with("sealed"))
            .unwrap()
            .clone();
        match alter {
            0 => {
                row.values.insert(key, DataValue::Boolean { value: false });
            }
            1 => {
                let key = row
                    .values
                    .iter()
                    .find(|(k, v)| k.starts_with("gm_scope_") && **v == DataValue::Null)
                    .unwrap()
                    .0
                    .clone();
                row.values.remove(&key);
            }
            _ => {
                row.revision += 1;
            }
        }
        assert!(bad.validate().is_err());
    }
}
#[test]
fn completion_history_rejects_unrecorded_later_revision_changes() {
    let (_dir, _store, snapshot) = owned_fixture();
    let dir = tempfile::tempdir().unwrap();
    let store = ProductStore::create_recovered(dir.path().join("owned"), &snapshot).unwrap();
    let live = row(&snapshot.data, "live");
    let after = store
        .apply(
            snapshot.revision,
            "complete-owned",
            &invoke("complete", [("job".into(), reference(&live))].into()),
            RuntimeLimits::default(),
        )
        .unwrap();
    let mut forged = after.clone();
    forged
        .data
        .records
        .iter_mut()
        .find(|r| r.id == live.record)
        .unwrap()
        .revision += 1;
    assert!(
        forged.validate().is_err(),
        "later completion revision needs authenticated capture history"
    );
}

#[test]
fn prospective_recipes_stay_isolated_through_fresh_first_adoption() {
    let (a, b) = pair();
    let (_dir, store) = setup(&a);
    let r = request(&store, &a, &b);
    let preview = prepare(&store, &b, &r);
    let first = owned_snapshot_for_test(&preview, "first-choice", true).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let pending =
        ProductStore::create_recovered_with(dir.path().join("pending"), &first, |_| Ok(()))
            .unwrap();
    let mut r = request(&pending, &a, &b);
    r.population = ScopePopulation::FutureWork;
    let second_preview = prepare(&pending, &b, &r);
    let second = owned_snapshot_for_test(&second_preview, "second-choice", true).unwrap();
    second.validate().unwrap();
    assert_eq!(second.owned_scope().unwrap().rehearsals.len(), 2);
    let dir2 = tempfile::tempdir().unwrap();
    let pending2 =
        ProductStore::create_recovered_with(dir2.path().join("pending"), &second, |_| Ok(()))
            .unwrap();
    let fresh = prepare(&pending2, &b, &request(&pending2, &a, &b));
    let committed = owned_snapshot_for_test(&fresh, "fresh-first-adoption", false).unwrap();
    committed.validate().unwrap();
    assert_eq!(committed.owned_scope().unwrap().layers.len(), 1);
    assert_eq!(
        committed.owned_scope().unwrap().rehearsals,
        second.owned_scope().unwrap().rehearsals
    );
    assert_eq!(committed.decisions, second.decisions);
    let context = ScopedExecutionContext::committed(&committed).unwrap();
    let proofs: Vec<_> = committed
        .owned_scope()
        .unwrap()
        .rehearsals
        .values()
        .collect();
    let mut seeds = vec![];
    for p in &proofs {
        let source = committed
            .programs
            .iter()
            .find(|s| {
                canonical_digest(IdentityDomain::Source, *s).unwrap()
                    == p.manifest.basis.frozen.active
            })
            .unwrap();
        let target = committed
            .programs
            .iter()
            .find(|s| canonical_digest(IdentityDomain::Source, *s).unwrap() == p.manifest.output)
            .unwrap();
        let (_, seed) = context
            .project_seed(
                source,
                &p.manifest.basis.frozen.data,
                target,
                &p.manifest.basis.frozen.data,
                p.manifest.basis.frozen.day,
            )
            .unwrap();
        assert_eq!(seed.identity().unwrap(), p.seed);
        seeds.push((target, seed));
    }
    assert_ne!(seeds[0].1, seeds[1].1);
    assert!(context
        .verify_seed(seeds[0].0, &seeds[1].1, committed.clock_day)
        .is_err());
}
#[test]
fn owned_family_limits_and_cancellation_are_cumulative() {
    let (_dir, _store, snapshot) = owned_fixture();
    let scene = ScenarioSpec {
        version: 1,
        id: "metered".into(),
        label: "Cumulative host meter".into(),
        seed: snapshot.data.clone(),
        session: snapshot.session.clone(),
        clock_day: snapshot.clock_day,
        random_seed: 0,
        inputs: (0..30)
            .map(|i| SemanticInput::Observe {
                point: format!("p{i}"),
            })
            .collect(),
        validity: vec![],
    };
    let bytes = canonical_bytes(&scene.seed).unwrap().len() as u64;
    let context = ScopedExecutionContext::committed(&snapshot)
        .unwrap()
        .owned_replay_budget_for_test(ProofBudget {
            units: bytes * 4 + 10_000,
            elapsed_millis: 60_000,
        });
    let runtime = product_runtime::LocalRuntime::default().with_admission(Arc::new(context));
    let run = runtime
        .replay(
            snapshot.program().unwrap(),
            &scene,
            &snapshot.decisions,
            RuntimeLimits::default(),
            "cumulative",
        )
        .unwrap();
    assert_ne!(run.state, EvidenceState::Observed);
    assert!(run.errors.iter().any(|e| e.contains("cumulative")));
    assert!(run.trace.len() < scene.inputs.len());
    let context = ScopedExecutionContext::committed(&snapshot).unwrap();
    let cancelled = Arc::new(AtomicBool::new(true));
    let runtime = product_runtime::LocalRuntime::with_cancellation(cancelled)
        .with_admission(Arc::new(context));
    assert!(runtime
        .replay(
            snapshot.program().unwrap(),
            &scene,
            &snapshot.decisions,
            RuntimeLimits::default(),
            "cancelled"
        )
        .is_err());
}
#[test]
fn owned_backup_roundtrip_and_rehashed_invalid_intake_fail_closed() {
    let (_dir, _store, snapshot) = owned_fixture();
    let dir = tempfile::tempdir().unwrap();
    let store = ProductStore::create_recovered(dir.path().join("owned"), &snapshot).unwrap();
    let backup = product_backup::VerifiedBackup::capture(&store).unwrap();
    let bytes = backup.to_bytes().unwrap();
    assert_eq!(
        product_backup::VerifiedBackup::from_bytes(&bytes)
            .unwrap()
            .snapshot(),
        &snapshot
    );
    let mut raw: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    raw["payload"]["snapshot"]["scope"]["state"]["version"] = serde_json::json!(99);
    let digest = canonical_digest(IdentityDomain::Data, &raw["payload"]["snapshot"]).unwrap();
    raw["payload"]["intentions"]["snapshot"] = serde_json::to_value(digest).unwrap();
    raw["digest"] =
        serde_json::to_value(canonical_digest(IdentityDomain::Evidence, &raw["payload"]).unwrap())
            .unwrap();
    assert!(product_backup::VerifiedBackup::from_bytes(&canonical_bytes(&raw).unwrap()).is_err());
    assert_eq!(store.load().unwrap(), snapshot);
}

fn rehash_recording(snapshot: &mut product_store::ProjectSnapshot) {
    let ScopeEnvelope::OwnedV2(state) = &mut snapshot.scope else {
        panic!()
    };
    state.rehearsals = state
        .rehearsals
        .values()
        .cloned()
        .map(|p| (p.identity().unwrap(), p))
        .collect();
    state.correspondences = state
        .correspondences
        .values()
        .cloned()
        .map(|p| (p.identity().unwrap(), p))
        .collect();
    for proof in state.rehearsals.values() {
        let a = snapshot
            .adoptions
            .iter()
            .find(|a| a.plan.id == proof.recorded_by)
            .unwrap();
        let correspondences: Vec<_> = state
            .correspondences
            .values()
            .filter(|r| r.operation == proof.recorded_by)
            .map(|r| r.proof.clone())
            .collect();
        let recording = canonical_digest(
            IdentityDomain::Adoption,
            &("owned-rehearsal-recording/2", &(proof, &correspondences)),
        )
        .unwrap();
        snapshot.operations.get_mut(&a.plan.id).unwrap().request = canonical_digest(
            IdentityDomain::Adoption,
            &(
                "owned-decision-recording/2",
                &(&a.plan, &snapshot.decisions, recording),
            ),
        )
        .unwrap();
    }
}
fn recorded_scene_package(
    snapshot: &mut product_store::ProjectSnapshot,
    scenes: &[product_decisions::AcceptedScene],
) -> Vec<u8> {
    let object = serde_json::json!({"version":1,"content":{"kind":"scenes","scenes":scenes}});
    let digest = canonical_digest(IdentityDomain::Evidence, &object).unwrap();
    let decision = &mut snapshot.decisions.decisions[0];
    decision.witness = digest.clone();
    decision.scenarios = scenes
        .iter()
        .map(|s| s.scenario().identity().unwrap())
        .collect();
    let ScopeEnvelope::OwnedV2(state) = &mut snapshot.scope else {
        panic!()
    };
    state
        .rehearsals
        .values_mut()
        .next()
        .unwrap()
        .witnesses
        .insert(decision.id.clone(), digest);
    rehash_recording(snapshot);
    canonical_bytes(&object).unwrap()
}
#[test]
fn owned_backups_and_archives_validate_full_family() {
    let (a, b) = pair();
    let (_dir, store) = setup(&a);
    let r = request(&store, &a, &b);
    let preview = prepare(&store, &b, &r);
    let mut snapshot = owned_snapshot_for_test(&preview, "archived-prospective", true).unwrap();
    let context = ScopedExecutionContext::committed(&snapshot).unwrap();
    let proof = snapshot
        .owned_scope()
        .unwrap()
        .rehearsals
        .values()
        .next()
        .unwrap()
        .clone();
    let basis = &proof.manifest.basis.frozen;
    let source = snapshot
        .programs
        .iter()
        .find(|p| canonical_digest(IdentityDomain::Source, *p).unwrap() == basis.active)
        .unwrap()
        .clone();
    let target = snapshot
        .programs
        .iter()
        .find(|p| canonical_digest(IdentityDomain::Source, *p).unwrap() == proof.manifest.output)
        .unwrap()
        .clone();
    let original = ScenarioSpec {
        version: 1,
        id: "archived-frame".into(),
        label: "Exact archived original frame".into(),
        seed: basis.data.clone(),
        session: basis.session.clone(),
        clock_day: basis.day,
        random_seed: 0,
        inputs: vec![SemanticInput::Observe {
            point: "result".into(),
        }],
        validity: vec![],
    };
    let (_, projected) = context
        .project_owned_scenario(&source, &original, &target)
        .unwrap();
    assert_eq!(
        context.replay_operation_ids(&target, &projected).unwrap(),
        original.replay_operation_ids().unwrap()
    );
    let runtime = product_runtime::LocalRuntime::default().with_admission(Arc::new(context));
    let mut scenes = vec![];
    for (program, scenario) in [(&source, &original), (&target, &projected)] {
        let evidence = runtime
            .replay(
                program,
                scenario,
                &proof.manifest.basis.graph,
                RuntimeLimits::default(),
                "archived-scene",
            )
            .unwrap();
        assert_eq!(evidence.state, EvidenceState::Observed);
        let scene:product_decisions::AcceptedScene=serde_json::from_value(serde_json::json!({"program":program,"scenario":scenario,"evidence":evidence,"disclosure":"synthetic","bind_outcome":true})).unwrap();
        scenes.push(scene);
    }
    let init = proof.initialization.as_ref().unwrap();
    let correspondence = OwnedCorrespondenceV2 {
        version: 2,
        source: source.clone(),
        original: original.seed.clone(),
        target: proof.manifest.output.clone(),
        day: basis.day,
        projected: proof.seed.clone(),
        operation_seed: original.seed.identity().unwrap(),
        scenario: Some(ScopeScenarioCorrespondence {
            original: original.clone(),
            projected: projected.clone(),
        }),
        layer: init.layer.clone(),
        composition: proof.manifest.identity().unwrap(),
        initialization: init.identity().unwrap(),
        basis: basis.snapshot.clone(),
        session: basis.session.clone(),
        decisions: basis.decisions.clone(),
        random_seed: 0,
    };
    let receipt = OwnedCorrespondenceReceiptV2 {
        proof: correspondence,
        operation: proof.recorded_by.clone(),
        revision: proof.recorded_revision,
    };
    let ScopeEnvelope::OwnedV2(state) = &mut snapshot.scope else {
        panic!()
    };
    state
        .correspondences
        .insert(receipt.identity().unwrap(), receipt);
    let bytes = recorded_scene_package(&mut snapshot, &scenes);
    snapshot.validate().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let restored =
        ProductStore::create_recovered_with(dir.path().join("prospective"), &snapshot, |store| {
            store.stage_extension(&bytes)?;
            Ok(())
        })
        .unwrap();
    let archive = product_decisions::IntentArchive::new(restored.clone());
    assert_eq!(
        archive
            .accepted_scenes(&snapshot.decisions.decisions[0])
            .unwrap(),
        scenes
    );
    let bundle = archive.export_for(&snapshot).unwrap();
    product_decisions::validate_bundle(&snapshot, &bundle).unwrap();
    let engine = product_decisions::DecisionEngine::new(
        product_runtime::LocalRuntime::default(),
        archive.clone(),
    );
    assert_eq!(
        engine.check_current(&snapshot).unwrap().disposition,
        product_decisions::CheckDisposition::Ready
    );
    let backup = product_backup::VerifiedBackup::capture(&restored).unwrap();
    assert_eq!(
        product_backup::VerifiedBackup::from_bytes(&backup.to_bytes().unwrap())
            .unwrap()
            .snapshot(),
        &snapshot
    );
    for changed in 0..5 {
        let mut bad = snapshot.clone();
        let ScopeEnvelope::OwnedV2(state) = &mut bad.scope else {
            panic!()
        };
        let proof = &mut state.correspondences.values_mut().next().unwrap().proof;
        match changed {
            0 => proof.day += 1,
            1 => proof.random_seed = 7,
            2 => proof.source = b.clone(),
            3 => proof
                .session
                .values
                .insert("forged".into(), DataValue::Null)
                .map(|_| ())
                .unwrap_or(()),
            _ => proof
                .scenario
                .as_mut()
                .unwrap()
                .projected
                .inputs
                .push(SemanticInput::Observe {
                    point: "unrecorded".into(),
                }),
        };
        rehash_recording(&mut bad);
        assert!(
            bad.validate().is_err(),
            "rehashed correspondence frame {changed}"
        );
    }
    let mut wrong = snapshot.clone();
    let bytes = recorded_scene_package(&mut wrong, &scenes[..1]);
    wrong.validate().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let wrong_store =
        ProductStore::create_recovered_with(dir.path().join("missing-pair"), &wrong, |store| {
            store.stage_extension(&bytes)?;
            Ok(())
        })
        .unwrap();
    assert!(product_decisions::IntentArchive::new(wrong_store.clone())
        .accepted_scenes(&wrong.decisions.decisions[0])
        .is_err());
    assert!(product_backup::VerifiedBackup::capture(&wrong_store).is_err());
}
#[test]
fn completion_history_reconstructs_from_original_event_frames() {
    let (_dir, _store, snapshot) = owned_fixture();
    let dir = tempfile::tempdir().unwrap();
    let store = ProductStore::create_recovered(dir.path().join("owned"), &snapshot).unwrap();
    let live = row(&snapshot.data, "live");
    let after = store
        .apply(
            snapshot.revision,
            "complete-owned",
            &invoke("complete", [("job".into(), reference(&live))].into()),
            RuntimeLimits::default(),
        )
        .unwrap();
    let history = store.runtime_view().unwrap().history;
    assert!(history.results.iter().any(|r| r.record == live
        && r.origin == product_protocol::HistoryOrigin::ObservedAtCompletion
        && r.event == Some(after.data.events.last().unwrap().id.clone())));
    let later = store
        .apply(
            after.revision,
            "later-job",
            &invoke(
                "add",
                [(
                    "title".into(),
                    DataValue::Text {
                        value: "later".into(),
                    },
                )]
                .into(),
            ),
            RuntimeLimits::default(),
        )
        .unwrap();
    later.validate().unwrap();
    assert_eq!(
        store.runtime_view().unwrap().history.results,
        history.results
    );
    for mutation in 0..3 {
        let mut forged = later.clone();
        let event = forged.data.events.last_mut().unwrap();
        match mutation {
            0 => {
                forged.data.generation += 1;
                event.sequence += 1;
            }
            1 => event.day = snapshot.clock_day - 1,
            _ => event.id = "forged-event".into(),
        }
        assert!(forged.validate().is_err(),"event history must preserve exact sequence, monotonic day and event identity: {mutation}");
    }
}
#[test]
fn decision_replay_driver_retains_full_frame_and_one_host_meter() {
    let (_dir, _store, snapshot) = owned_fixture();
    let scene = ScenarioSpec {
        version: 1,
        id: "decision-meter".into(),
        label: "Decision replay meter".into(),
        seed: snapshot.data.clone(),
        session: snapshot.session.clone(),
        clock_day: snapshot.clock_day,
        random_seed: 0,
        inputs: (0..30)
            .map(|i| SemanticInput::Observe {
                point: format!("p{i}"),
            })
            .collect(),
        validity: vec![],
    };
    let bytes = canonical_bytes(&scene.seed).unwrap().len() as u64;
    let context = ScopedExecutionContext::committed(&snapshot)
        .unwrap()
        .owned_replay_budget_for_test(ProofBudget {
            units: bytes * 4 + 10_000,
            elapsed_millis: 60_000,
        });
    let runtime = product_runtime::LocalRuntime::default();
    assert!(
        product_decisions::execute_admitted(
            &runtime,
            snapshot.program().unwrap(),
            &scene,
            &snapshot.decisions,
            RuntimeLimits::default(),
            "cumulative-decisions",
            Some(&context)
        )
        .is_err(),
        "decision replay must not replenish its host meter per input"
    );
    let context = ScopedExecutionContext::committed(&snapshot).unwrap();
    let mut wrong = snapshot.decisions.clone();
    wrong.revision += 1;
    assert!(
        product_decisions::execute_admitted(
            &runtime,
            snapshot.program().unwrap(),
            &scene,
            &wrong,
            RuntimeLimits::default(),
            "wrong-decisions",
            Some(&context)
        )
        .is_err(),
        "decision replay must authenticate its graph"
    );
}

fn operable_pair() -> (CapturedProgram, CapturedProgram) {
    let (a, b) = pair();
    let mut a = a.program;
    let mut b = b.program;
    // This independent synthetic fixture retains pending memory while material
    // is unavailable, and exposes ordinary begin/arrival actions on both sides.
    for action in &mut b.actions {
        for step in &mut action.steps {
            if let Statement::ForEach { binding, steps, .. } = step {
                if binding == "job" {
                    if let Statement::ForEach { items, .. } = &mut steps[1] {
                        if let Expr::Query { predicate, .. } = items {
                            **predicate = and(vec![
                                *predicate.clone(),
                                not(eq(field("job", "status"), text("waiting"))),
                            ]);
                        }
                    }
                }
            }
        }
    }
    for app in [&mut a, &mut b] {
        app.actions.push(action(
            "begin",
            vec![("job", Type::reference("job"))],
            vec![eq(field("job", "status"), text("ready"))],
            vec![update(var("job"), vec![("status", text("active"))])],
        ));
    }
    let mut arrival_a = a
        .actions
        .iter()
        .find(|x| x.id == "withdraw")
        .unwrap()
        .clone();
    arrival_a.id = "arrive".into();
    arrival_a.label = "Arrive".into();
    arrival_a.guards = vec![not(field("material", "received"))];
    arrival_a.steps[0] = update(var("material"), vec![("received", boolean(true))]);
    let mut arrival_b = arrival_a.clone();
    arrival_b.steps[2] = b.actions.iter().find(|x| x.id == "withdraw").unwrap().steps[3].clone();
    a.actions.push(arrival_a);
    b.actions.push(arrival_b);
    (capture(&a), capture(&b))
}
fn operable_request(
    s: &ProductStore,
    a: &CapturedProgram,
    b: &CapturedProgram,
) -> OwnedPreviewRequest {
    let mut r = request(s, a, b);
    r.inventory.sources.edits.push(OwnedSpanEdit {
        id: "arrive".into(),
        before: endpoint(a, "arrive", vec![2], 0, 1, Some("job")),
        after: endpoint(b, "arrive", vec![2], 0, 2, Some("job")),
    });
    r
}
fn apply_store(s: &ProductStore, id: &str, input: SemanticInput) {
    s.apply(
        s.load().unwrap().revision,
        id,
        &input,
        RuntimeLimits::default(),
    )
    .unwrap();
}
#[test]
fn owned_auxiliary_revisions_are_bound_to_real_event_changes() {
    let (a, b) = operable_pair();
    let (_dir, store) = setup(&a);
    let live = row(&store.load().unwrap().data, "live");
    apply_store(
        &store,
        "register-material",
        invoke(
            "register",
            [
                ("owner".into(), reference(&live)),
                (
                    "name".into(),
                    DataValue::Text {
                        value: "paper".into(),
                    },
                ),
            ]
            .into(),
        ),
    );
    let material = store
        .load()
        .unwrap()
        .data
        .records
        .iter()
        .find(|r| r.entity == "material")
        .map(|r| RecordRef {
            entity: r.entity.clone(),
            record: r.id.clone(),
        })
        .unwrap();
    let receipt = || {
        [
            ("material".into(), reference(&material)),
            ("note".into(), DataValue::Text { value: "".into() }),
        ]
        .into()
    };
    apply_store(&store, "arrive-material", invoke("arrive", receipt()));
    apply_store(
        &store,
        "begin-live",
        invoke("begin", [("job".into(), reference(&live))].into()),
    );
    let prepared = prepare(&store, &b, &operable_request(&store, &a, &b));
    let snapshot = owned_snapshot_for_test(&prepared, "auxiliary-proof", false).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let owned = ProductStore::create_recovered(dir.path().join("owned"), &snapshot).unwrap();
    apply_store(&owned, "withdraw-material", invoke("withdraw", receipt()));
    let after = owned.load().unwrap();
    assert_eq!(
        after
            .data
            .records
            .iter()
            .filter(|r| r.entity == "pause")
            .count(),
        1
    );
    let mut bad = after.clone();
    bad.data
        .records
        .iter_mut()
        .find(|r| r.entity == "pause")
        .unwrap()
        .revision += 1;
    assert!(
        bad.validate().is_err(),
        "owned auxiliary revision cannot change outside actual recorded writes"
    );
    apply_store(&owned, "arrive-again", invoke("arrive", receipt()));
    owned.load().unwrap().validate().unwrap();
}
#[test]
fn correspondence_names_are_isolated_by_owned_recipe() {
    let (a, b) = pair();
    let (_dir, store) = setup(&a);
    let preview = prepare(&store, &b, &request(&store, &a, &b));
    let mut first = owned_snapshot_for_test(&preview, "first-recipe", true).unwrap();
    let proof = first
        .owned_scope()
        .unwrap()
        .rehearsals
        .values()
        .next()
        .unwrap()
        .clone();
    let basis = &proof.manifest.basis.frozen;
    let source = first.program().unwrap().clone();
    let target = first
        .programs
        .iter()
        .find(|p| canonical_digest(IdentityDomain::Source, *p).unwrap() == proof.manifest.output)
        .unwrap();
    let original = ScenarioSpec {
        version: 1,
        id: "same-comparison".into(),
        label: "Same original comparison".into(),
        seed: basis.data.clone(),
        session: basis.session.clone(),
        clock_day: basis.day,
        random_seed: 0,
        inputs: vec![SemanticInput::Observe {
            point: "result".into(),
        }],
        validity: vec![],
    };
    let (_, projected) = ScopedExecutionContext::committed(&first)
        .unwrap()
        .project_owned_scenario(&source, &original, target)
        .unwrap();
    let init = proof.initialization.as_ref().unwrap();
    let receipt = OwnedCorrespondenceReceiptV2 {
        operation: proof.recorded_by.clone(),
        revision: proof.recorded_revision,
        proof: OwnedCorrespondenceV2 {
            version: 2,
            source: source.clone(),
            original: original.seed.clone(),
            target: proof.manifest.output.clone(),
            day: basis.day,
            projected: proof.seed.clone(),
            operation_seed: original.seed.identity().unwrap(),
            scenario: Some(ScopeScenarioCorrespondence {
                original: original.clone(),
                projected,
            }),
            layer: init.layer.clone(),
            composition: proof.manifest.identity().unwrap(),
            initialization: init.identity().unwrap(),
            basis: basis.snapshot.clone(),
            session: basis.session.clone(),
            decisions: basis.decisions.clone(),
            random_seed: 0,
        },
    };
    let ScopeEnvelope::OwnedV2(state) = &mut first.scope else {
        panic!()
    };
    state
        .correspondences
        .insert(receipt.identity().unwrap(), receipt);
    rehash_recording(&mut first);
    first.validate().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let pending =
        ProductStore::create_recovered_with(dir.path().join("pending"), &first, |_| Ok(()))
            .unwrap();
    let mut r = request(&pending, &a, &b);
    r.population = ScopePopulation::FutureWork;
    let second =
        owned_snapshot_for_test(&prepare(&pending, &b, &r), "second-recipe", true).unwrap();
    second.validate().unwrap();
    let proof = second
        .owned_scope()
        .unwrap()
        .rehearsals
        .values()
        .find(|p| p.recorded_by == "second-recipe")
        .unwrap();
    let target = second
        .programs
        .iter()
        .find(|p| canonical_digest(IdentityDomain::Source, *p).unwrap() == proof.manifest.output)
        .unwrap();
    let context = ScopedExecutionContext::committed(&second).unwrap();
    let (_, projected) = context
        .project_owned_scenario(&source, &original, target)
        .unwrap();
    assert_eq!(
        context.replay_operation_ids(target, &projected).unwrap(),
        original.replay_operation_ids().unwrap(),
        "another recipe's retained correspondence cannot shadow this one"
    );
}

#[test]
fn prospective_recording_preserves_exact_frame_and_later_work() {
    let (a, b) = pair();
    let (_dir, store) = setup(&a);
    let prepared = prepare(&store, &b, &request(&store, &a, &b));
    let pending = owned_snapshot_for_test(&prepared, "preserve-recording", true).unwrap();
    pending.validate().unwrap();
    for change in 0..5 {
        let mut forged = pending.clone();
        match change {
            0 => {
                forged.data.records[0].values.insert(
                    "title".into(),
                    DataValue::Text {
                        value: "unrecorded".into(),
                    },
                );
            }
            1 => forged.clock_day += 1,
            2 => {
                forged.session.values.insert(
                    "name_input".into(),
                    DataValue::Text {
                        value: "unrecorded".into(),
                    },
                );
            }
            3 => forged.data.generation += 1,
            _ => forged.data.records[0].revision += 1,
        }
        assert!(
            forged.validate().is_err(),
            "recording changed live field {change}"
        );
    }
    let dir = tempfile::tempdir().unwrap();
    let store =
        ProductStore::create_recovered_with(dir.path().join("pending"), &pending, |_| Ok(()))
            .unwrap();
    let later = store
        .apply(
            pending.revision,
            "legitimate-later-work",
            &invoke(
                "add",
                [(
                    "title".into(),
                    DataValue::Text {
                        value: "later".into(),
                    },
                )]
                .into(),
            ),
            RuntimeLimits::default(),
        )
        .unwrap();
    let later = store
        .apply(
            later.revision,
            "later-clock",
            &SemanticInput::AdvanceClock { days: 1 },
            RuntimeLimits::default(),
        )
        .unwrap();
    let later = store
        .apply(
            later.revision,
            "later-session",
            &SemanticInput::Control {
                view: "jobs".into(),
                control: "name".into(),
                value: DataValue::Text {
                    value: "ordinary edit".into(),
                },
            },
            RuntimeLimits::default(),
        )
        .unwrap();
    later.validate().unwrap();
    assert_eq!(later.data.records.len(), pending.data.records.len() + 1);
    assert_ne!(later.session, pending.session);
    assert_eq!(later.clock_day, pending.clock_day + 1);
    let mut forged = later.clone();
    forged.data.records[0].values.insert(
        "title".into(),
        DataValue::Text {
            value: "outside-event".into(),
        },
    );
    assert!(
        forged.validate().is_err(),
        "later work must continue the preserved frame"
    );
    let mut forged = later.clone();
    forged
        .operations
        .get_mut("legitimate-later-work")
        .unwrap()
        .revision = pending.revision;
    assert!(
        forged.validate().is_err(),
        "a recording cannot own a later business event"
    );
    let mut forged = later.clone();
    forged.data.records[0].revision += 1;
    assert!(
        forged.validate().is_err(),
        "an untouched earlier row cannot acquire an unrecorded revision"
    );
    let fresh = prepare(&store, &b, &request(&store, &a, &b));
    let committed = owned_snapshot_for_test(&fresh, "adopt-after-work", false).unwrap();
    committed.validate().unwrap();
}

#[test]
fn historical_owned_basis_requires_complete_container() {
    let (a, b) = pair();
    let mut old = a.program.clone();
    old.label = "Earlier ordinary source".into();
    let old = capture(&old);
    let (_dir, store) = setup(&old);
    let prior = store.load().unwrap();
    let switch = store.prepare_switch(&a, "ordinary-switch").unwrap();
    store
        .adopt(prior.revision, &switch, &a, &prior.decisions)
        .unwrap();
    let before = store.load().unwrap();
    let prepared = prepare(&store, &b, &request(&store, &a, &b));
    let pending = owned_snapshot_for_test(&prepared, "retained-container", true).unwrap();
    let mut basis = pending
        .owned_scope()
        .unwrap()
        .rehearsals
        .values()
        .next()
        .unwrap()
        .manifest
        .basis
        .clone();
    owned_historical_basis_for_test(&pending, &basis).unwrap();
    let old_id = canonical_digest(IdentityDomain::Source, &old).unwrap();
    basis.sources.retain(|id| id != &old_id);
    let mut forged_history = before.clone();
    forged_history.programs.retain(|p| p != &old);
    basis.frozen.snapshot = canonical_digest(IdentityDomain::Data, &forged_history).unwrap();
    assert!(forged_history.validate().is_err());
    assert!(
        owned_historical_basis_for_test(&pending, &basis).is_err(),
        "self-consistent hashes cannot replace historical producer/source closure"
    );
}

#[test]
fn current_owned_read_check_does_not_grant_write_authority() {
    let (a, b) = pair();
    let (_dir, store) = setup(&a);
    let prepared = prepare(&store, &b, &request(&store, &a, &b));
    let snapshot = owned_snapshot_for_test(&prepared, "current-read", false).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store =
        ProductStore::create_recovered_with(dir.path().join("owned"), &snapshot, |_| Ok(()))
            .unwrap();
    let engine = product_decisions::DecisionEngine::new(
        product_runtime::LocalRuntime::default(),
        product_decisions::IntentArchive::new(store.clone()),
    );
    assert_eq!(
        engine.check_current(&snapshot).unwrap().disposition,
        product_decisions::CheckDisposition::Ready
    );
    assert!(store
        .prepare_switch(snapshot.program().unwrap(), "cannot-write")
        .is_err());
    assert!(ScopedExecutionContext::committed(&snapshot)
        .unwrap()
        .admit_target(&b)
        .is_err());
}

#[test]
fn retained_active_ordinary_intention_replays_through_owned_initializer() {
    retained_intention_case(false);
}
#[test]
fn older_retained_intentions_replay_isolated_historical_frames() {
    retained_intention_case(true);
}
fn retained_intention_case(historical: bool) {
    use product_decisions::*;
    let (a, b) = pair();
    let (_dir, store) = setup(&a);
    let before = store.load().unwrap();
    let scene = ScenarioSpec {
        version: 1,
        id: "independent-add".into(),
        label: "Independent ordinary property".into(),
        seed: before.data.clone(),
        session: before.session.clone(),
        clock_day: before.clock_day,
        random_seed: 0,
        inputs: vec![
            invoke(
                "add",
                [(
                    "title".into(),
                    DataValue::Text {
                        value: "independent".into(),
                    },
                )]
                .into(),
            ),
            SemanticInput::Observe {
                point: "independent".into(),
            },
        ],
        validity: vec![],
    };
    let accepted = accept_scene(
        &product_runtime::LocalRuntime::default(),
        &a,
        &scene,
        Disclosure::Synthetic,
        RuntimeLimits::default(),
    )
    .unwrap();
    let engine = DecisionEngine::new(
        product_runtime::LocalRuntime::default(),
        IntentArchive::new(store.clone()),
    );
    let choice = Choice {
        id: "ordinary-property".into(),
        request: "Keep adding ordinary ready jobs".into(),
        rationale: None,
        scope: DecisionScope {
            operations: ["add".into()].into(),
            population: Population::All,
            conditions: Values::new(),
            excluded_records: vec![],
            unknowns: vec![],
        },
        outcome: DecisionOutcome::KeepCurrent,
        binding: IntentionBinding::PropertiesOnly,
        obligations: vec![AcceptedProperty {
            id: "ready-count".into(),
            description: "Adding one job retains the prior ready job".into(),
            predicate: PropertyPredicate::Equal {
                left: PropertyTerm::Observed {
                    point: "independent".into(),
                    observable: "ready_count".into(),
                    value_type: Type::Integer,
                },
                right: PropertyTerm::Literal {
                    value_type: Type::Integer,
                    value: DataValue::Integer { value: 2 },
                },
            },
        }],
    };
    let change = engine
        .prepare_choice(
            &store,
            &a,
            choice.clone(),
            vec![accepted],
            "keep-ordinary-property",
        )
        .unwrap();
    let before = engine.adopt(&store, &change).unwrap();
    assert_eq!(
        engine.check_current(&before).unwrap().disposition,
        CheckDisposition::Ready
    );
    if historical {
        for step in 0..2 {
            apply_store(
                &store,
                &format!("ordinary-after-acceptance-{step}"),
                invoke(
                    "add",
                    [(
                        "title".into(),
                        DataValue::Text {
                            value: format!("later-{step}"),
                        },
                    )]
                    .into(),
                ),
            );
            apply_store(
                &store,
                &format!("clock-after-acceptance-{step}"),
                SemanticInput::AdvanceClock { days: 1 },
            );
            apply_store(
                &store,
                &format!("session-after-acceptance-{step}"),
                SemanticInput::Control {
                    view: "jobs".into(),
                    control: "name".into(),
                    value: DataValue::Text {
                        value: format!("later-{step}"),
                    },
                },
            );
            if step == 0 {
                let frame = store.load().unwrap();
                let mut second_scene = scene.clone();
                second_scene.id = "second-independent-add".into();
                second_scene.seed = frame.data;
                second_scene.session = frame.session;
                second_scene.clock_day = frame.clock_day;
                let accepted = accept_scene(
                    &product_runtime::LocalRuntime::default(),
                    &a,
                    &second_scene,
                    Disclosure::Synthetic,
                    RuntimeLimits::default(),
                )
                .unwrap();
                let mut second_choice = choice.clone();
                second_choice.id = "second-ordinary-property".into();
                if let PropertyPredicate::Equal { right, .. } =
                    &mut second_choice.obligations[0].predicate
                {
                    *right = PropertyTerm::Literal {
                        value_type: Type::Integer,
                        value: DataValue::Integer { value: 3 },
                    };
                }
                let change = engine
                    .prepare_choice(
                        &store,
                        &a,
                        second_choice,
                        vec![accepted],
                        "keep-second-ordinary-property",
                    )
                    .unwrap();
                engine.adopt(&store, &change).unwrap();
            }
        }
    }
    let before = store.load().unwrap();
    let r = request(&store, &a, &b);
    let mut host = LifecycleAcceptanceHost::prepare(
        &store,
        &b,
        &r,
        &witnesses(&store),
        ProofBudget::default(),
        cancel(),
    )
    .unwrap();
    let lifecycle = host.synthetic_operator_accept_for_test(&store).unwrap();
    assert!(
        store
            .verify_owned_preview(&b, &r, &lifecycle, ProofBudget::default(), cancel())
            .is_err(),
        "public active-decision preview stays unsupported until P4"
    );
    let preview = owned_retained_preview_for_test(&store, &b, &r, &lifecycle).unwrap();
    let snapshot = owned_snapshot_for_test(&preview, "retained-active-adoption", false).unwrap();
    snapshot.validate().unwrap();
    assert_eq!(snapshot.decisions, before.decisions);
    let objects: Vec<_> = before
        .decisions
        .decisions
        .iter()
        .map(|d| d.witness.clone())
        .chain(
            before
                .adoptions
                .iter()
                .filter_map(|a| a.plan.evidence.first().cloned()),
        )
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|id| store.read_extension(&id).unwrap())
        .collect();
    let dir = tempfile::tempdir().unwrap();
    let recovered =
        ProductStore::create_recovered_with(dir.path().join("active-owned"), &snapshot, |s| {
            for bytes in &objects {
                s.stage_extension(bytes)?;
            }
            Ok(())
        })
        .unwrap();
    let archive = IntentArchive::new(recovered.clone());
    archive.export_for(&snapshot).unwrap();
    let engine = DecisionEngine::new(product_runtime::LocalRuntime::default(), archive);
    let report = engine.check_current(&snapshot).unwrap();
    assert_eq!(report.disposition, CheckDisposition::Ready, "{report:?}");
    assert_eq!(report.checks.len(), if historical { 2 } else { 1 });
    assert_eq!(
        recovered.load().unwrap(),
        snapshot,
        "read-only historical initialization cannot mutate current work"
    );
    assert!(report
        .checks
        .iter()
        .all(|c| matches!(c.state, CheckState::Satisfied | CheckState::Outside)));
    assert_eq!(
        engine
            .check_all(&snapshot.decisions, snapshot.program().unwrap(), &[])
            .unwrap()
            .disposition,
        CheckDisposition::Ready
    );
    assert!(engine.check_all(&snapshot.decisions, &b, &[]).is_err());
    assert!(recovered
        .prepare_switch(snapshot.program().unwrap(), "still-no-write")
        .is_err());
    let mut forged = snapshot.clone();
    forged.decisions.decisions[0].request.push_str(" altered");
    assert!(forged.validate().is_err());
    let mut forged = snapshot.clone();
    forged
        .adoptions
        .last_mut()
        .unwrap()
        .plan
        .expected_generation += 1;
    assert!(forged.validate().is_err());
    if historical {
        let archive = IntentArchive::new(recovered.clone());
        let decision = &snapshot.decisions.decisions[0];
        let accepted = archive.accepted_scenes(decision).unwrap().remove(0);
        let token = archive
            .retained_scene(&snapshot, decision, &accepted)
            .unwrap();
        let context = ScopedExecutionContext::committed(&snapshot).unwrap();
        let target = snapshot.program().unwrap();
        assert!(
            context
                .project_owned_scenario(accepted.program(), accepted.scenario(), target)
                .is_err(),
            "the public arbitrary-scene projection must remain exact-basis"
        );
        assert!(context
            .project_retained_scene(&token, &b, &snapshot.decisions, cancel())
            .is_err());
        let mut changed_graph = snapshot.decisions.clone();
        changed_graph.decisions[0].request.push_str(" substituted");
        assert!(context
            .project_retained_scene(&token, target, &changed_graph, cancel())
            .is_err());
        let mut changed_decision = decision.clone();
        changed_decision.request.push_str(" substituted");
        assert!(archive
            .retained_scene(&snapshot, &changed_decision, &accepted)
            .is_err());
        changed_decision = decision.clone();
        changed_decision.witness = snapshot.decisions.decisions[1].witness.clone();
        assert!(archive
            .retained_scene(&snapshot, &changed_decision, &accepted)
            .is_err());
        // These are genuinely executed valid scenes, but they are not the exact
        // bytes in this retained witness package.
        for wrong_source in [false, true] {
            let mut changed_scenario = accepted.scenario().clone();
            let mut changed_program = accepted.program().program.clone();
            if wrong_source {
                changed_program.label.push_str(" changed source");
            } else {
                changed_scenario.clock_day += 1;
            }
            let mut changed = serde_json::to_value(
                accept_scene(
                    &product_runtime::LocalRuntime::default(),
                    &capture(&changed_program),
                    &changed_scenario,
                    Disclosure::Synthetic,
                    RuntimeLimits::default(),
                )
                .unwrap(),
            )
            .unwrap();
            changed["bind_outcome"] =
                serde_json::to_value(&accepted).unwrap()["bind_outcome"].clone();
            let changed: AcceptedScene = serde_json::from_value(changed).unwrap();
            assert!(archive
                .retained_scene(&snapshot, decision, &changed)
                .is_err());
        }
        let cancelled = Arc::new(AtomicBool::new(true));
        assert!(context
            .project_retained_scene(&token, target, &snapshot.decisions, cancelled)
            .is_err());
        let cancelled = cancel();
        let (actual, admission) = context
            .project_retained_scene(&token, target, &snapshot.decisions, cancelled.clone())
            .unwrap();
        assert_eq!(
            admission.replay_operation_ids(target, &actual).unwrap(),
            accepted.scenario().replay_operation_ids().unwrap()
        );
        let mut changed = actual.clone();
        changed.random_seed += 1;
        assert!(admission
            .begin_replay(target, &changed, &snapshot.decisions, cancel())
            .is_err());
        assert!(admission
            .begin_replay(target, &actual, &changed_graph, cancel())
            .is_err());
        assert!(admission
            .begin_replay(&b, &actual, &snapshot.decisions, cancel())
            .is_err());
        cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(product_decisions::execute_admitted(
            &product_runtime::LocalRuntime::with_cancellation(cancelled),
            target,
            &actual,
            &snapshot.decisions,
            RuntimeLimits::default(),
            "cancelled-retained",
            Some(admission.as_ref()),
        )
        .is_err());
        // Find the deterministic initialization boundary, then execute using
        // exactly that allowance. A replenished per-input meter would pass.
        let (mut low, mut high) = (1, ProofBudget::default().units);
        while low < high {
            let mid = low + (high - low) / 2;
            let bounded = context.clone().owned_replay_budget_for_test(ProofBudget {
                units: mid,
                elapsed_millis: 60_000,
            });
            if bounded
                .project_retained_scene(&token, target, &snapshot.decisions, cancel())
                .is_ok()
            {
                high = mid;
            } else {
                low = mid + 1;
            }
        }
        let bounded = context.clone().owned_replay_budget_for_test(ProofBudget {
            units: high,
            elapsed_millis: 60_000,
        });
        let (actual, admission) = bounded
            .project_retained_scene(&token, target, &snapshot.decisions, cancel())
            .unwrap();
        let error = product_decisions::execute_admitted(
            &product_runtime::LocalRuntime::default(),
            target,
            &actual,
            &snapshot.decisions,
            RuntimeLimits::default(),
            "cumulative-retained",
            Some(admission.as_ref()),
        )
        .unwrap_err();
        assert!(error.to_string().contains("budget"), "{error}");
        assert_eq!(recovered.load().unwrap(), snapshot);
        apply_store(
            &recovered,
            "later-clock-for-token-staleness",
            SemanticInput::AdvanceClock { days: 1 },
        );
        let later = recovered.load().unwrap();
        assert!(archive
            .retained_scene(&snapshot, decision, &accepted)
            .is_err());
        assert!(ScopedExecutionContext::committed(&later)
            .unwrap()
            .project_retained_scene(&token, later.program().unwrap(), &later.decisions, cancel())
            .is_err());
    }
}

#[test]
fn historical_owned_basis_retains_prior_prospective_capture() {
    let (a, b) = pair();
    let (_dir, store) = setup(&a);
    let first = owned_snapshot_for_test(
        &prepare(&store, &b, &request(&store, &a, &b)),
        "first-container",
        true,
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = ProductStore::create_recovered_with(dir.path().join("pending"), &first, |_| Ok(()))
        .unwrap();
    let mut r = request(&store, &a, &b);
    r.population = ScopePopulation::FutureWork;
    let second =
        owned_snapshot_for_test(&prepare(&store, &b, &r), "second-container", true).unwrap();
    let mut basis = second
        .owned_scope()
        .unwrap()
        .rehearsals
        .values()
        .find(|p| p.recorded_by == "second-container")
        .unwrap()
        .manifest
        .basis
        .clone();
    let output = first
        .owned_scope()
        .unwrap()
        .rehearsals
        .values()
        .next()
        .unwrap()
        .manifest
        .output
        .clone();
    owned_historical_basis_for_test(&second, &basis).unwrap();
    basis.sources.retain(|id| id != &output);
    let mut forged_history = first;
    forged_history
        .programs
        .retain(|p| canonical_digest(IdentityDomain::Source, p).unwrap() != output);
    basis.frozen.snapshot = canonical_digest(IdentityDomain::Data, &forged_history).unwrap();
    assert!(forged_history.validate().is_err());
    assert!(
        owned_historical_basis_for_test(&second, &basis).is_err(),
        "historical source inventory must retain every earlier prospective executable"
    );
}
#[test]
fn committed_shared_revisions_require_bounded_recorded_writes() {
    let (a, b) = pair();
    // The same ordinary shared update occurs twice in one real transaction.
    // Runtime coalesces its event change, but preserves both revision bumps.
    let coalesced = |source: &CapturedProgram| {
        let mut app = source.program.clone();
        let remove = app.actions.iter_mut().find(|a| a.id == "remove").unwrap();
        remove.steps.push(remove.steps[0].clone());
        capture(&app)
    };
    let a = coalesced(&a);
    let b = coalesced(&b);
    let (_dir, original) = setup(&a);
    let before = original.load().unwrap();
    let live = row(&before.data, "live");
    original
        .apply(
            before.revision,
            "register-shared",
            &invoke(
                "register",
                [
                    ("owner".into(), reference(&live)),
                    (
                        "name".into(),
                        DataValue::Text {
                            value: "shared".into(),
                        },
                    ),
                ]
                .into(),
            ),
            RuntimeLimits::default(),
        )
        .unwrap();
    let request = request(&original, &a, &b);
    let prepared = prepare(&original, &b, &request);
    let snapshot = owned_snapshot_for_test(&prepared, "shared-adoption", false).unwrap();
    let root = tempfile::tempdir().unwrap();
    let store = ProductStore::create_recovered(root.path().join("owned"), &snapshot).unwrap();
    let link = snapshot
        .data
        .records
        .iter()
        .find(|r| r.entity == "requirement")
        .unwrap();
    let link_ref = RecordRef {
        entity: link.entity.clone(),
        record: link.id.clone(),
    };
    let after = store
        .apply(
            snapshot.revision,
            "remove-shared",
            &invoke("remove", [("link".into(), reference(&link_ref))].into()),
            RuntimeLimits::default(),
        )
        .unwrap();
    let actual = after.data.records.iter().find(|r| r.id == link.id).unwrap();
    assert_eq!(actual.revision, link.revision + 2);
    assert_eq!(
        after
            .data
            .events
            .last()
            .unwrap()
            .changes
            .iter()
            .filter(|c| c.record == link.id)
            .count(),
        1
    );
    after.validate().unwrap();
    for (entity, revision) in [
        ("material", 2),
        ("material", u64::MAX),
        ("requirement", u64::MAX),
    ] {
        let mut forged = after.clone();
        forged
            .data
            .records
            .iter_mut()
            .find(|r| r.entity == entity)
            .unwrap()
            .revision = revision;
        assert!(
            forged.validate().is_err(),
            "{entity} revision {revision} lacks bounded recorded writes"
        );
    }
    assert_eq!(store.load().unwrap(), after);
}

#[test]
fn committed_owned_events_require_post_adoption_operation_receipts() {
    let (_dir, _original, snapshot) = owned_fixture();
    let root = tempfile::tempdir().unwrap();
    let store = ProductStore::create_recovered(root.path().join("owned"), &snapshot).unwrap();
    let live = row(&snapshot.data, "live");
    let after = store
        .apply(
            snapshot.revision,
            "complete-receipt",
            &invoke("complete", [("job".into(), reference(&live))].into()),
            RuntimeLimits::default(),
        )
        .unwrap();
    let later = store
        .apply(
            after.revision,
            "add-receipt",
            &invoke(
                "add",
                [(
                    "title".into(),
                    DataValue::Text {
                        value: "later receipt".into(),
                    },
                )]
                .into(),
            ),
            RuntimeLimits::default(),
        )
        .unwrap();
    later.validate().unwrap();
    for mutation in 0..4 {
        let mut forged = later.clone();
        match mutation {
            0 => {
                forged.operations.remove("complete-receipt");
            }
            1 => {
                forged
                    .operations
                    .get_mut("complete-receipt")
                    .unwrap()
                    .revision = snapshot.revision;
            }
            2 => {
                forged
                    .operations
                    .get_mut("complete-receipt")
                    .unwrap()
                    .revision = snapshot.revision - 1;
            }
            _ => {
                forged.operations.get_mut("add-receipt").unwrap().revision = after.revision;
            }
        }
        assert!(
            forged.validate().is_err(),
            "committed event receipt mutation {mutation} must fail closed"
        );
    }
    assert_eq!(store.load().unwrap(), later);
}
fn event_write_budget_case(prospective: bool) {
    let (a, b) = pair();
    let (_dir, original) = setup(&a);
    let prepared = prepare(&original, &b, &request(&original, &a, &b));
    {
        let snapshot = owned_snapshot_for_test(&prepared, "event-budget", prospective).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let store = ProductStore::create_recovered_with(dir.path().join("owned"), &snapshot, |_| Ok(())).unwrap();
        let live = row(&snapshot.data, "live");
        let after = store
            .apply(
                snapshot.revision,
                "register-budget",
                &invoke(
                    "register",
                    [
                        ("owner".into(), reference(&live)),
                        (
                            "name".into(),
                            DataValue::Text {
                                value: "budget shared".into(),
                            },
                        ),
                    ]
                    .into(),
                ),
                RuntimeLimits::default(),
            )
            .unwrap();
        after.validate().unwrap();
        let limit = RuntimeLimits::default().transaction_writes as u64;
        for shared in [vec!["material", "requirement"], vec!["material"]] {
            let mut forged = after.clone();
            let revision = if shared.len() == 1 { limit - 1 } else { limit };
            for entity in shared {
                forged
                    .data
                    .records
                    .iter_mut()
                    .find(|r| r.entity == entity)
                    .unwrap()
                    .revision = revision;
            }
            assert!(forged.validate().is_err(), "shared and owned writes must compete for one event allowance (prospective={prospective})");
        }
        assert_eq!(store.load().unwrap(), after);
    }
}

#[test]
fn committed_events_share_one_transaction_write_bound() {
    event_write_budget_case(false);
}
#[test]
fn prospective_events_share_one_transaction_write_bound() {
    event_write_budget_case(true);
}

#[test]
fn committed_checkpoint_preserves_session_day_without_blocking_later_inputs() {
    let (_dir, _original, snapshot) = owned_fixture();
    let mut changed_session = snapshot.clone();
    changed_session.session.values.insert(
        "name_input".into(),
        DataValue::Text {
            value: "unrecorded checkpoint".into(),
        },
    );
    let mut changed_day = snapshot.clone();
    changed_day.clock_day += 1;
    assert_eq!(
        (
            changed_session.validate().is_err(),
            changed_day.validate().is_err()
        ),
        (true, true),
        "the exact committed checkpoint must retain both its session and day"
    );
    let root = tempfile::tempdir().unwrap();
    assert!(
        ProductStore::create_recovered(root.path().join("changed-session"), &changed_session)
            .is_err()
    );
    assert!(ProductStore::create_recovered(root.path().join("changed-day"), &changed_day).is_err());
    let store = ProductStore::create_recovered(root.path().join("valid"), &snapshot).unwrap();
    let later = store
        .apply(
            snapshot.revision,
            "real-clock-continuation",
            &SemanticInput::AdvanceClock { days: 1 },
            RuntimeLimits::default(),
        )
        .unwrap();
    let later = store
        .apply(
            later.revision,
            "real-session-continuation",
            &SemanticInput::Control {
                view: "jobs".into(),
                control: "name".into(),
                value: DataValue::Text {
                    value: "legitimate continuation".into(),
                },
            },
            RuntimeLimits::default(),
        )
        .unwrap();
    assert_eq!(later.clock_day, snapshot.clock_day + 1);
    assert_ne!(later.session, snapshot.session);
    later.validate().unwrap();
    assert_eq!(store.load().unwrap(), later);
}
