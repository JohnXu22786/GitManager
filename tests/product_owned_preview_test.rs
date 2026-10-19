//! Host acceptance is synthetic here, through the production controller boundary.
#[path = "fixtures/common_fact/mod.rs"]
mod fixture;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_decisions/mod.rs"]
mod product_decisions;
#[path = "../src/product_protocol.rs"]
mod product_protocol;
#[path = "../src/product_runtime/mod.rs"]
mod product_runtime;
#[path = "../src/product_store/mod.rs"]
mod product_store;
use fixture::*;
use product_contract::*;
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
fn acceptance(
    store: &ProductStore,
    b: &CapturedProgram,
    r: &OwnedPreviewRequest,
) -> AcceptedPreviewLifecycle {
    let mut host = LifecycleAcceptanceHost::prepare(
        store,
        b,
        r,
        &witnesses(store),
        ProofBudget::default(),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    host.synthetic_operator_accept_for_test(store).unwrap()
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
fn scenario(s: &ProductStore, inputs: Vec<SemanticInput>) -> ScenarioSpec {
    let x = s.load().unwrap();
    ScenarioSpec {
        version: 1,
        id: "preview-scene".into(),
        label: "Exact original frame".into(),
        seed: x.data,
        session: x.session,
        clock_day: x.clock_day,
        random_seed: 0,
        inputs,
        validity: vec![],
    }
}
#[test]
fn executable_lowering_is_deterministic_and_initialization_is_metadata_only() {
    let (a, b) = pair();
    let (_d, s) = setup(&a);
    let r = request(&s, &a, &b);
    let original = s.load().unwrap();
    let p = prepare(&s, &b, &r);
    let q = prepare(&s, &b, &r);
    assert_eq!(p.target(), q.target());
    assert_eq!(p.initialized(), q.initialized());
    assert_eq!(p.receipt(), q.receipt());
    assert_ne!(p.target(), &b);
    assert_eq!(p.initialized().events, original.data.events);
    for old in &original.data.records {
        let mut new = p
            .initialized()
            .records
            .iter()
            .find(|n| n.entity == old.entity && n.id == old.id)
            .unwrap()
            .clone();
        new.values.retain(|k, _| !k.starts_with("gm_scope_"));
        assert_eq!(&new, old);
    }
    assert!(!p.initialized().records.iter().any(|r| r.entity == "pause"));
    assert_eq!(s.load().unwrap(), original);
}
#[test]
fn selected_excluded_future_and_completed_projections_are_real_typed_values() {
    let (a, b) = pair();
    let (_d, s) = setup(&a);
    s.apply(
        s.load().unwrap().revision,
        "excluded",
        &invoke(
            "add",
            [(
                "title".into(),
                DataValue::Text {
                    value: "excluded".into(),
                },
            )]
            .into(),
        ),
        RuntimeLimits::default(),
    )
    .unwrap();
    let original = s.load().unwrap();
    let excluded = row(&original.data, "excluded");
    let completed = row(&original.data, "finished");
    let mut r = request(&s, &a, &b);
    r.excluded_records.push(excluded.clone());
    let p = prepare(&s, &b, &r);
    let mut run = p
        .start(
            &s,
            RuntimeLimits::default(),
            ProofBudget::default(),
            cancel(),
        )
        .unwrap();
    let observed = run.observe("initial").unwrap();
    let view = observed.view;
    let cell = |id: &str| {
        view.rows
            .iter()
            .find(|r| r.record.record == id)
            .unwrap()
            .cells["pending"]
            .clone()
    };
    assert_eq!(cell(&excluded.record), DataValue::Null);
    assert_eq!(cell(&completed.record), DataValue::Null);
    assert_eq!(
        cell(&row(&original.data, "live").record),
        DataValue::Boolean { value: false }
    );
    run.apply(&invoke(
        "add",
        [(
            "title".into(),
            DataValue::Text {
                value: "future".into(),
            },
        )]
        .into(),
    ))
    .unwrap();
    let future = row(run.data().unwrap(), "future");
    let view = run.observe("future").unwrap().view;
    assert_eq!(
        view.rows.iter().find(|r| r.record == future).unwrap().cells["pending"],
        DataValue::Boolean { value: false }
    );
    run.apply(&invoke(
        "complete",
        [("job".into(), reference(&row(&original.data, "live")))].into(),
    ))
    .unwrap();
    assert_eq!(s.load().unwrap(), original);
}
#[test]
fn replay_binds_original_namespace_and_rejects_changed_frames() {
    let (a, b) = pair();
    let (_d, s) = setup(&a);
    let r = request(&s, &a, &b);
    let p = prepare(&s, &b, &r);
    let original = scenario(
        &s,
        vec![
            invoke(
                "add",
                [(
                    "title".into(),
                    DataValue::Text {
                        value: "future".into(),
                    },
                )]
                .into(),
            ),
            SemanticInput::Observe {
                point: "seen".into(),
            },
        ],
    );
    let evidence = p
        .replay(
            &s,
            &original,
            RuntimeLimits::default(),
            ProofBudget::default(),
            cancel(),
            "real-preview",
        )
        .unwrap();
    assert_eq!(evidence.state, EvidenceState::Observed);
    assert_eq!(evidence.trace.len(), 2);
    assert_eq!(evidence.binding.source, p.target().binding);
    // The interpreter allocates from the original unprojected input namespace.
    // This does not assert equivalence with another source's create ordinals.
    let operation = original.replay_operation_ids().unwrap().remove(0);
    let identity = canonical_digest(
        IdentityDomain::Input,
        &(0_u64, operation.as_str(), "job", 1_usize),
    )
    .unwrap();
    let expected = format!("record-{}", &identity.as_str()[..32]);
    assert!(evidence.observations[0]
        .view
        .rows
        .iter()
        .any(|r| r.record.record == expected));
    for k in 0..4 {
        let mut changed = original.clone();
        match k {
            0 => changed.clock_day += 1,
            1 => changed.session.view = "history".into(),
            2 => changed.random_seed = 1,
            _ => changed.seed.generation += 1,
        };
        assert!(p
            .replay(
                &s,
                &changed,
                RuntimeLimits::default(),
                ProofBudget::default(),
                cancel(),
                "changed"
            )
            .is_err());
    }
}
#[test]
fn stale_cancelled_and_unmanaged_previews_refuse_before_execution() {
    let (a, b) = pair();
    let (_d, s) = setup(&a);
    let r = request(&s, &a, &b);
    let proof = verify(&s, &b, &r).unwrap();
    let p = prepare(&s, &b, &r);
    assert!(s
        .prepare_owned_preview(
            &proof,
            ProofBudget {
                units: 1,
                elapsed_millis: 1000
            },
            cancel()
        )
        .is_err());
    assert!(p
        .start(
            &s,
            RuntimeLimits::default(),
            ProofBudget::default(),
            Arc::new(AtomicBool::new(true))
        )
        .is_err());
    assert!(s.prepare_switch(p.target(), "forged-preview").is_err());
    let raw = scenario(
        &s,
        vec![SemanticInput::Observe {
            point: "raw".into(),
        }],
    );
    let mut protected = raw.clone();
    protected.seed = p.initialized().clone();
    assert!(product_runtime::LocalRuntime::default()
        .replay(
            p.target(),
            &protected,
            &s.load().unwrap().decisions,
            RuntimeLimits::default(),
            "raw"
        )
        .is_err());
    let mut live = p
        .start(
            &s,
            RuntimeLimits::default(),
            ProofBudget::default(),
            cancel(),
        )
        .unwrap();
    s.apply(
        s.load().unwrap().revision,
        "advance",
        &SemanticInput::AdvanceClock { days: 1 },
        RuntimeLimits::default(),
    )
    .unwrap();
    assert!(!live.basis_is_current(&s).unwrap());
    assert!(p
        .start(
            &s,
            RuntimeLimits::default(),
            ProofBudget::default(),
            cancel()
        )
        .is_err());
    live.observe("isolated-stale").unwrap();
}
#[test]
fn completion_seals_once_and_later_inputs_preserve_business_and_nullable_capture() {
    let (a, b) = pair();
    let (_d, s) = setup(&a);
    let p = prepare(&s, &b, &request(&s, &a, &b));
    let mut run = p
        .start(
            &s,
            RuntimeLimits::default(),
            ProofBudget::default(),
            cancel(),
        )
        .unwrap();
    let live = row(run.data().unwrap(), "live");
    run.apply(&invoke(
        "complete",
        [("job".into(), reference(&live))].into(),
    ))
    .unwrap();
    let sealed = run
        .data()
        .unwrap()
        .records
        .iter()
        .find(|r| r.entity == live.entity && r.id == live.record)
        .unwrap()
        .clone();
    assert_eq!(sealed.revision, 3); // Actual business completion plus actual seal Update.
    assert!(run
        .apply(&invoke(
            "complete",
            [("job".into(), reference(&live))].into()
        ))
        .is_err());
    run.apply(&invoke(
        "add",
        [(
            "title".into(),
            DataValue::Text {
                value: "later".into(),
            },
        )]
        .into(),
    ))
    .unwrap();
    assert_eq!(
        run.data()
            .unwrap()
            .records
            .iter()
            .find(|r| r.entity == live.entity && r.id == live.record),
        Some(&sealed)
    );
    let view = run.observe("completed").unwrap().view;
    assert_eq!(
        view.rows.iter().find(|r| r.record == live).unwrap().cells["pending"],
        DataValue::Boolean { value: false }
    );
}
#[test]
fn ordinary_resource_failure_rolls_back_shared_writes_and_keeps_honest_trace() {
    let (a, b) = pair();
    let (_d, s) = setup(&a);
    let p = prepare(&s, &b, &request(&s, &a, &b));
    for k in 0..4 {
        let mut limits = RuntimeLimits::default();
        match k {
            0 => limits.transaction_writes = 1,
            1 => limits.action_steps = 1,
            2 => limits.fuel = 1,
            _ => limits.collection_items = 2,
        };
        let mut run = p
            .start(&s, limits, ProofBudget::default(), cancel())
            .unwrap();
        let before = run.data().unwrap().clone();
        let session = run.session().unwrap().clone();
        let input = invoke(
            "register",
            [
                ("owner".into(), reference(&row(&before, "live"))),
                (
                    "name".into(),
                    DataValue::Text {
                        value: "Shared item".into(),
                    },
                ),
            ]
            .into(),
        );
        assert!(run.apply(&input).is_err(), "case {k}");
        assert_eq!(run.data().unwrap(), &before);
        assert_eq!(run.session().unwrap(), &session);
        assert_eq!(run.trace().unwrap().len(), 1);
        assert!(run.apply(&input).is_err());
        assert_eq!(run.data().unwrap(), &before);
        assert_eq!(run.trace().unwrap().len(), if k == 1 { 1 } else { 2 });
    }
}
#[test]
fn failed_host_validation_is_terminal_including_pre_execution_cancellation() {
    let (a, b) = pair();
    let (_d, s) = setup(&a);
    let p = prepare(&s, &b, &request(&s, &a, &b));
    let mut run = p
        .start(
            &s,
            RuntimeLimits::default(),
            ProofBudget::default(),
            cancel(),
        )
        .unwrap();
    run.fail_next_validation_for_test();
    assert!(run
        .apply(&invoke(
            "add",
            [(
                "title".into(),
                DataValue::Text {
                    value: "unpublished".into()
                }
            )]
            .into()
        ))
        .is_err());
    assert!(run.data().is_err());
    assert!(run.session().is_err());
    assert!(run.observe("invalid").is_err());
    assert!(run.apply(&SemanticInput::AdvanceClock { days: 1 }).is_err());
    let stop = cancel();
    let mut run = p
        .start(
            &s,
            RuntimeLimits::default(),
            ProofBudget::default(),
            stop.clone(),
        )
        .unwrap();
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(run.observe("cancelled").is_err());
    assert!(run.data().is_err());
}
#[test]
fn completed_null_presence_and_cohort_cells_cannot_be_forged() {
    let (a, b) = pair();
    let (_d, s) = setup(&a);
    let p = prepare(&s, &b, &request(&s, &a, &b));
    for kind in 0..6 {
        let mut data = p.initialized().clone();
        let finished = row(&data, "finished");
        let row = data
            .records
            .iter_mut()
            .find(|r| r.entity == finished.entity && r.id == finished.record)
            .unwrap();
        let saved = row
            .values
            .keys()
            .find(|k| {
                k.starts_with("gm_scope_") && row.values[*k] == DataValue::Null && k.contains("_p")
            })
            .unwrap()
            .clone();
        match kind {
            0 => {
                row.values.remove(&saved);
            }
            1 => {
                row.values
                    .insert(saved, DataValue::Boolean { value: false });
            }
            2 => {
                let key = row
                    .values
                    .keys()
                    .find(|k| k.ends_with("_sealed"))
                    .unwrap()
                    .clone();
                row.values.insert(key, DataValue::Boolean { value: false });
            }
            3 => {
                let key = row
                    .values
                    .keys()
                    .find(|k| k.ends_with("_member"))
                    .unwrap()
                    .clone();
                let DataValue::Boolean { value } = row.values[&key] else {
                    panic!()
                };
                row.values.insert(key, DataValue::Boolean { value: !value });
            }
            4 => row.revision += 1,
            _ => {
                row.values.insert(
                    "title".into(),
                    DataValue::Text {
                        value: "forged".into(),
                    },
                );
            }
        }
        assert!(p.validate_state_for_test(&data).is_err(), "case {kind}");
    }
}
#[test]
fn full_replay_capability_rejects_same_seed_under_another_frame() {
    let (a, b) = pair();
    let (_d, s) = setup(&a);
    let p = prepare(&s, &b, &request(&s, &a, &b));
    let original = scenario(
        &s,
        vec![SemanticInput::Observe {
            point: "seen".into(),
        }],
    );
    let mut projected = original.clone();
    projected.seed = p.initialized().clone();
    for kind in 0..7 {
        let mut changed = projected.clone();
        let mut target = p.target().clone();
        let mut graph = s.load().unwrap().decisions;
        match kind {
            0 => changed.id = "different-original".into(),
            1 => changed.session.focused_record = Some(row(&changed.seed, "live")),
            2 => changed.random_seed += 1,
            3 => changed.clock_day += 1,
            4 => changed.inputs.push(SemanticInput::AdvanceClock { days: 1 }),
            5 => graph.revision += 1,
            _ => target.source_bytes.push(b' '),
        }
        assert!(
            p.validate_replay_frame_for_test(&original, &changed, &graph, &target)
                .is_err(),
            "case {kind}"
        );
    }
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
fn common_effects_execute_once_and_capture_precedes_receipt_correction() {
    let (a, b) = operable_pair();
    let (_d, s) = setup(&a);
    apply_store(
        &s,
        "excluded",
        invoke(
            "add",
            [(
                "title".into(),
                DataValue::Text {
                    value: "excluded".into(),
                },
            )]
            .into(),
        ),
    );
    let initial = s.load().unwrap();
    let selected = row(&initial.data, "live");
    let excluded = row(&initial.data, "excluded");
    for (i, r) in [&selected, &excluded].iter().enumerate() {
        apply_store(
            &s,
            &format!("register-{i}"),
            invoke(
                "register",
                [
                    ("owner".into(), reference(r)),
                    (
                        "name".into(),
                        DataValue::Text {
                            value: "common".into(),
                        },
                    ),
                ]
                .into(),
            ),
        );
    }
    let material = s
        .load()
        .unwrap()
        .data
        .records
        .iter()
        .find(|r| r.entity == "material")
        .unwrap()
        .clone();
    let material = RecordRef {
        entity: material.entity,
        record: material.id,
    };
    apply_store(
        &s,
        "arrival",
        invoke(
            "arrive",
            [
                ("material".into(), reference(&material)),
                ("note".into(), DataValue::Text { value: "".into() }),
            ]
            .into(),
        ),
    );
    for (i, r) in [&selected, &excluded].iter().enumerate() {
        apply_store(
            &s,
            &format!("begin-{i}"),
            invoke("begin", [("job".into(), reference(r))].into()),
        );
    }
    let original = s.load().unwrap();
    let mut r = operable_request(&s, &a, &b);
    r.excluded_records.push(excluded.clone());
    let p = prepare(&s, &b, &r);
    let mut run = p
        .start(
            &s,
            RuntimeLimits::default(),
            ProofBudget::default(),
            cancel(),
        )
        .unwrap();
    let before_events = run
        .data()
        .unwrap()
        .records
        .iter()
        .filter(|r| r.entity == "event")
        .count();
    run.apply(&invoke(
        "withdraw",
        [
            ("material".into(), reference(&material)),
            (
                "note".into(),
                DataValue::Text {
                    value: "wrong receipt".into(),
                },
            ),
        ]
        .into(),
    ))
    .unwrap();
    let pauses: Vec<_> = run
        .data()
        .unwrap()
        .records
        .iter()
        .filter(|r| r.entity == "pause")
        .collect();
    assert_eq!(pauses.len(), 1);
    assert_eq!(pauses[0].values["job"], reference(&selected));
    assert_eq!(
        pauses[0].values["pending"],
        DataValue::Boolean { value: true }
    );
    assert_eq!(
        run.data()
            .unwrap()
            .records
            .iter()
            .filter(|r| r.entity == "event")
            .count(),
        before_events + 1
    );
    let observed = run.observe("withdrawn").unwrap();
    for r in [&selected, &excluded] {
        assert_eq!(
            observed
                .view
                .rows
                .iter()
                .find(|x| &x.record == r)
                .unwrap()
                .cells["status"],
            DataValue::Text {
                value: "waiting".into()
            }
        );
    }
    run.apply(&invoke(
        "arrive",
        [
            ("material".into(), reference(&material)),
            (
                "note".into(),
                DataValue::Text {
                    value: "correct".into(),
                },
            ),
        ]
        .into(),
    ))
    .unwrap();
    let observed = run.observe("restored").unwrap();
    assert_eq!(
        observed.values["ready_count"],
        DataValue::Integer { value: 1 }
    );
    for (r, status, pending) in [
        (&selected, "active", DataValue::Boolean { value: false }),
        (&excluded, "ready", DataValue::Null),
    ] {
        let row = observed.view.rows.iter().find(|x| &x.record == r).unwrap();
        assert_eq!(
            row.cells["status"],
            DataValue::Text {
                value: status.into()
            }
        );
        assert_eq!(row.cells["pending"], pending);
    }
    assert_eq!(
        run.data()
            .unwrap()
            .records
            .iter()
            .filter(|r| r.entity == "material")
            .count(),
        1
    );
    assert_eq!(
        run.data()
            .unwrap()
            .records
            .iter()
            .filter(|r| r.entity == "requirement")
            .count(),
        2
    );
    assert_eq!(
        run.data()
            .unwrap()
            .records
            .iter()
            .filter(|r| r.entity == "event")
            .count(),
        before_events + 2
    );
    assert_eq!(s.load().unwrap(), original);
}
#[test]
fn selected_only_and_future_only_preserve_exact_frozen_membership() {
    let (a, b) = pair();
    let (_d, s) = setup(&a);
    let selected = row(&s.load().unwrap().data, "live");
    for population in [
        ScopePopulation::SelectedUnfinished {
            records: vec![selected.clone()],
        },
        ScopePopulation::FutureWork,
    ] {
        let future_only = matches!(population, ScopePopulation::FutureWork);
        let mut r = request(&s, &a, &b);
        r.population = population;
        let p = prepare(&s, &b, &r);
        let mut run = p
            .start(
                &s,
                RuntimeLimits::default(),
                ProofBudget::default(),
                cancel(),
            )
            .unwrap();
        run.apply(&invoke(
            "add",
            [(
                "title".into(),
                DataValue::Text {
                    value: "future".into(),
                },
            )]
            .into(),
        ))
        .unwrap();
        let future = row(run.data().unwrap(), "future");
        let observed = run.observe("membership").unwrap();
        for (r, member) in [(&selected, !future_only), (&future, future_only)] {
            assert_eq!(
                observed
                    .view
                    .rows
                    .iter()
                    .find(|x| &x.record == r)
                    .unwrap()
                    .cells["pending"],
                if member {
                    DataValue::Boolean { value: false }
                } else {
                    DataValue::Null
                }
            );
        }
    }
}
#[test]
fn exported_preview_cannot_create_or_forge_a_durable_snapshot() {
    let (a, b) = pair();
    let (_d, s) = setup(&a);
    let p = prepare(&s, &b, &request(&s, &a, &b));
    let dir = tempfile::tempdir().unwrap();
    assert!(ProductStore::create(dir.path().join("forged"), p.target(), 20000).is_err());
    let mut forged = s.load().unwrap();
    forged.programs.push(p.target().clone());
    forged.active_revision = canonical_digest(IdentityDomain::Source, p.target()).unwrap();
    forged.data = p.initialized().clone();
    assert!(forged.validate().is_err());
    let mut layers = forged.scalar_scope().unwrap().clone();
    layers.version = 2;
    forged.scope = ScopeEnvelope::ScalarV1(layers);
    assert!(forged.validate().is_err());
}
#[test]
fn repeated_completed_consumers_share_only_identical_capture_recipes() {
    let (a, b) = pair();
    let (mut aa, mut bb) = (a.program, b.program);
    for i in 0..6 {
        let mut av = aa.views[0].clone();
        let mut bv = bb.views[0].clone();
        av.id = format!("same-{i}");
        bv.id = av.id.clone();
        aa.views.push(av);
        bb.views.push(bv);
    }
    let (a, b) = (capture(&aa), capture(&bb));
    let (_d, s) = setup(&a);
    let p = prepare(&s, &b, &request(&s, &a, &b));
    let mut run = p
        .start(
            &s,
            RuntimeLimits::default(),
            ProofBudget {
                units: 4_000_000,
                elapsed_millis: 60_000,
            },
            cancel(),
        )
        .unwrap();
    let root = row(run.data().unwrap(), "live");
    run.apply(&invoke(
        "complete",
        [("job".into(), reference(&root))].into(),
    ))
    .unwrap();
    let original = run.observe("first").unwrap().view.rows;
    for i in 0..6 {
        run.apply(&SemanticInput::Navigate {
            view: format!("same-{i}"),
        })
        .unwrap();
        assert_eq!(
            run.observe(&format!("copy-{i}")).unwrap().view.rows,
            original
        );
    }
}
#[test]
fn nested_absence_queries_see_prior_common_creates_in_original_root_order() {
    let (a, b) = pair();
    let (mut aa, mut bb) = (a.program, b.program);
    for app in [&mut aa, &mut bb] {
        let sync = app
            .actions
            .iter()
            .find(|a| a.id == "register")
            .unwrap()
            .steps[2]
            .clone();
        let create_material = each(
            query(
                "job",
                "gate_row",
                and(vec![
                    eq(var("gate_row"), var("owner")),
                    eq(
                        count(query(
                            "material",
                            "m",
                            eq(field("m", "name"), field("owner", "title")),
                        )),
                        int(0),
                    ),
                ]),
            ),
            "gate",
            vec![create(
                "material",
                "made",
                vec![
                    ("name", field("owner", "title")),
                    ("received", boolean(false)),
                ],
            )],
        );
        let create_link = each(
            query(
                "material",
                "m",
                eq(field("m", "name"), field("owner", "title")),
            ),
            "material",
            vec![each(
                query(
                    "job",
                    "gate_row",
                    and(vec![
                        eq(var("gate_row"), var("owner")),
                        eq(
                            count(query(
                                "requirement",
                                "q",
                                and(vec![
                                    eq(field("q", "job"), var("owner")),
                                    eq(field("q", "material"), var("material")),
                                ]),
                            )),
                            int(0),
                        ),
                    ]),
                ),
                "link_gate",
                vec![create(
                    "requirement",
                    "made_link",
                    vec![
                        ("job", var("owner")),
                        ("material", var("material")),
                        ("required", boolean(true)),
                    ],
                )],
            )],
        );
        app.actions.push(action(
            "normalize",
            vec![],
            vec![],
            vec![
                each(
                    query("job", "j", not(done("j"))),
                    "owner",
                    vec![create_material, create_link],
                ),
                sync,
            ],
        ));
    }
    let (a, b) = (capture(&aa), capture(&bb));
    let (_d, s) = setup(&a);
    apply_store(
        &s,
        "second-live",
        invoke(
            "add",
            [(
                "title".into(),
                DataValue::Text {
                    value: "live".into(),
                },
            )]
            .into(),
        ),
    );
    let mut r = request(&s, &a, &b);
    r.inventory.sources.edits.push(OwnedSpanEdit {
        id: "normalize".into(),
        before: endpoint(&a, "normalize", vec![1], 0, 1, Some("job")),
        after: endpoint(&b, "normalize", vec![1], 0, 2, Some("job")),
    });
    let p = prepare(&s, &b, &r);
    let mut run = p
        .start(
            &s,
            RuntimeLimits::default(),
            ProofBudget::default(),
            cancel(),
        )
        .unwrap();
    run.apply(&invoke("normalize", Values::new())).unwrap();
    let materials: Vec<_> = run
        .data()
        .unwrap()
        .records
        .iter()
        .filter(|r| r.entity == "material")
        .collect();
    assert_eq!(materials.len(), 1);
    let links: Vec<_> = run
        .data()
        .unwrap()
        .records
        .iter()
        .filter(|r| r.entity == "requirement")
        .collect();
    assert_eq!(links.len(), 2);
    assert_eq!(links[0].values["material"], links[1].values["material"]);
    let events = run
        .data()
        .unwrap()
        .events
        .iter()
        .filter(|e| e.action == "normalize")
        .count();
    assert_eq!(events, 1);
}

#[test]
fn retained_lifecycle_reconstruct_derive_preserves_values_and_payments() {
    let (a, b) = pair();
    let (_d, store) = setup(&a);
    let r = request(&store, &a, &b);
    let accepted = acceptance(&store, &b, &r);
    for case in [
        "selected",
        "ordinary:derive",
        "ordinary:derive_retained",
        "ordinary:derive_for_decision",
        "helper-limit:0:fuel",
        "helper-limit:0:collection_items",
        "helper-limit:0:action_steps",
        "helper-limit:0:transaction_writes",
        "helper-limit:0:output_bytes",
        "helper-limit:0:elapsed_millis",
        "helper-limit:1:fuel",
        "helper-limit:1:collection_items",
        "helper-limit:1:action_steps",
        "helper-limit:1:transaction_writes",
        "helper-limit:1:output_bytes",
        "helper-limit:1:elapsed_millis",
        "helper-cardinality:0",
        "helper-cardinality:1",
        "helper-cardinality:3",
    ] {
        store.assert_retained_lifecycle_values_for_test(&b, &r, &accepted, case);
    }
}

#[test]
fn retained_lifecycle_reconstruct_derive_preserves_errors_and_precedence() {
    let (a, b) = pair();
    let (_d, store) = setup(&a);
    let r = request(&store, &a, &b);
    let accepted = acceptance(&store, &b, &r);
    for case in [
        "version-before-invalid-request",
        "disclosure",
        "event",
        "origin",
        "evidence-binding",
        "evidence-regeneration",
        "witness-count",
        "witness-order",
        "witness-frame",
        "witness-transition",
        "witness-distinct",
        "witness-missing",
        "witness-interpretation",
        "source-binding",
        "basis-binding",
        "basis-authentication",
        "request-binding",
        "request-authentication",
        "operations-authentication",
        "inventory-basis-authentication",
        "ordinary-purpose:derive",
        "ordinary-purpose:derive_retained",
        "ordinary-purpose:derive_for_decision",
        "ordinary-source:derive",
        "ordinary-source:derive_retained",
        "ordinary-source:derive_for_decision",
    ] {
        store.assert_retained_lifecycle_error_for_test(&b, &r, &accepted, case);
    }
}

#[test]
fn retained_lifecycle_reconstruct_derive_enforces_real_resource_boundaries() {
    let (a, b) = pair();
    let (_d, store) = setup(&a);
    let r = request(&store, &a, &b);
    let accepted = acceptance(&store, &b, &r);
    for case in [
        "first-replay-fuel",
        "guard-fuel",
        "handoff-fuel",
        "primary-first-replay",
        "secondary-first-replay",
        "primary-handoff",
        "secondary-handoff",
        "short-window-fallback",
        "expired-first-replay",
    ] {
        store.assert_retained_lifecycle_resource_for_test(&b, &r, &accepted, case);
    }
}


#[test]
fn capture_row_batch_preserves_actual_frame_values_and_presence() {
    let (a, b) = pair(); let (_d, store) = setup(&a);
    let r = request(&store, &a, &b); let proof = verify(&store, &b, &r).unwrap();
    for case in ["full", "selected-live", "excluded-live", "future", "archived-unfinished",
        "zero-projections", "nullable-absence", "duplicate-location", "actual-day",
        "distinct-source", "unfinished-skips-poison"] {
        store.assert_capture_row_values_for_test(&proof, case);
    }
}
#[test]
fn capture_row_batch_preserves_first_errors_and_lazy_boundaries() {
    let (a, b) = pair(); let (_d, store) = setup(&a);
    let r = request(&store, &a, &b); let proof = verify(&store, &b, &r).unwrap();
    for case in ["source-before-data", "binding-before-expression",
        "selected-completed-before-projection", "absent-is-lazy", "no-root-is-lazy"] {
        store.assert_capture_row_order_for_test(&proof, case);
    }
}
#[test]
fn capture_row_batch_uses_actual_host_reservation_and_refusals() {
    let (a, b) = pair(); let (_d, store) = setup(&a);
    let r = request(&store, &a, &b); let proof = verify(&store, &b, &r).unwrap();
    store.assert_capture_row_host_for_test(&proof);
}
