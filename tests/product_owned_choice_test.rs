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
use product_decisions::*;
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
fn first_owned_preparation_preserves_independent_intention() {
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
    let request = request(&store, &a, &b);
    // Red baseline: the public ephemeral preview cannot prepare this required
    // decision-owned comparison. P4 adds a distinct private-authority route;
    // the conservative public-preview refusal must remain unchanged.
    assert!(
        verify(&store, &b, &request).is_err(),
        "public preview remains conservative"
    );
    let lifecycle = lifecycle(&store, &b, &request);
    let (prepared, report) = engine
        .prepare_owned_change(
            &store,
            &b,
            &request,
            &lifecycle,
            "first-owned",
            ProofBudget::default(),
            cancel(),
        )
        .unwrap();
    assert_eq!(
        report.disposition,
        CheckDisposition::Ready,
        "{:?}",
        report.checks
    );
    assert_ne!(prepared.target(), &b);
    assert_eq!(store.load().unwrap(), before);
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

fn lifecycle(
    store: &ProductStore,
    b: &CapturedProgram,
    request: &OwnedPreviewRequest,
) -> AcceptedPreviewLifecycle {
    LifecycleAcceptanceHost::prepare(
        store,
        b,
        request,
        &witnesses(store),
        ProofBudget::default(),
        cancel(),
    )
    .unwrap()
    .synthetic_operator_accept_for_test(store)
    .unwrap()
}
fn engine(store: &ProductStore) -> DecisionEngine<product_runtime::LocalRuntime> {
    DecisionEngine::new(
        product_runtime::LocalRuntime::default(),
        IntentArchive::new(store.clone()),
    )
}
fn setup_operable() -> (
    tempfile::TempDir,
    ProductStore,
    CapturedProgram,
    CapturedProgram,
) {
    let (a, b) = operable_pair();
    let (dir, store) = setup(&a);
    let live = row(&store.load().unwrap().data, "live");
    apply_store(
        &store,
        "register-basis",
        invoke(
            "register",
            [
                ("owner".into(), reference(&live)),
                (
                    "name".into(),
                    DataValue::Text {
                        value: "material".into(),
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
        .unwrap()
        .clone();
    apply_store(
        &store,
        "arrive-basis",
        invoke(
            "arrive",
            [
                (
                    "material".into(),
                    reference(&RecordRef {
                        entity: material.entity,
                        record: material.id,
                    }),
                ),
                (
                    "note".into(),
                    DataValue::Text {
                        value: "basis".into(),
                    },
                ),
            ]
            .into(),
        ),
    );
    apply_store(
        &store,
        "add-completable",
        invoke(
            "add",
            [(
                "title".into(),
                DataValue::Text {
                    value: "complete-me".into(),
                },
            )]
            .into(),
        ),
    );
    (dir, store, a, b)
}
fn prepared(
    store: &ProductStore,
    a: &CapturedProgram,
    b: &CapturedProgram,
    operation: &str,
) -> PreparedOwnedScopedChangeV2 {
    let r = operable_request(store, a, b);
    engine(store)
        .prepare_owned_change(
            store,
            b,
            &r,
            &lifecycle(store, b, &r),
            operation,
            ProofBudget::default(),
            cancel(),
        )
        .unwrap()
        .0
}
fn scenario(store: &ProductStore, id: &str, inputs: Vec<SemanticInput>) -> ScenarioSpec {
    let s = store.load().unwrap();
    ScenarioSpec {
        version: 1,
        id: id.into(),
        label: format!("Synthetic owned choice {id}"),
        seed: s.data,
        session: s.session,
        clock_day: s.clock_day,
        random_seed: 0,
        inputs,
        validity: vec![],
    }
}
fn action_scenes(store: &ProductStore, p: &PreparedOwnedScopedChangeV2) -> Vec<AcceptedScene> {
    let s = store.load().unwrap();
    let live = row(&s.data, "live");
    let complete = row(&s.data, "complete-me");
    let entity_ref = |entity: &str| {
        let r = s.data.records.iter().find(|r| r.entity == entity).unwrap();
        reference(&RecordRef {
            entity: r.entity.clone(),
            record: r.id.clone(),
        })
    };
    let material = entity_ref("material");
    let note = DataValue::Text {
        value: "scene".into(),
    };
    let withdraw = invoke(
        "withdraw",
        [
            ("material".into(), material.clone()),
            ("note".into(), note.clone()),
        ]
        .into(),
    );
    let actions = vec![
        (
            "add",
            vec![invoke(
                "add",
                [(
                    "title".into(),
                    DataValue::Text {
                        value: "new".into(),
                    },
                )]
                .into(),
            )],
        ),
        (
            "complete",
            vec![invoke(
                "complete",
                [("job".into(), reference(&complete))].into(),
            )],
        ),
        (
            "register",
            vec![invoke(
                "register",
                [
                    ("owner".into(), reference(&live)),
                    (
                        "name".into(),
                        DataValue::Text {
                            value: "another".into(),
                        },
                    ),
                ]
                .into(),
            )],
        ),
        (
            "remove",
            vec![invoke(
                "remove",
                [("link".into(), entity_ref("requirement"))].into(),
            )],
        ),
        ("withdraw", vec![withdraw.clone()]),
        (
            "begin",
            vec![invoke("begin", [("job".into(), reference(&live))].into())],
        ),
        (
            "arrive",
            vec![
                withdraw,
                invoke(
                    "arrive",
                    [("material".into(), material), ("note".into(), note)].into(),
                ),
            ],
        ),
    ];
    let e = engine(store);
    actions
        .into_iter()
        .map(|(id, mut inputs)| {
            inputs.push(SemanticInput::Observe {
                point: "result".into(),
            });
            e.accept_owned_scene(
                store,
                p,
                &scenario(store, id, inputs),
                Disclosure::Synthetic,
            )
            .unwrap()
        })
        .collect()
}
fn choice(p: &PreparedOwnedScopedChangeV2, id: &str, outcome: DecisionOutcome) -> Choice {
    Choice {
        id: id.into(),
        request: "Synthetic operator business choice, separate from lifecycle meaning".into(),
        rationale: None,
        scope: p.scope(),
        outcome,
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    }
}
fn accept_choice(
    store: &ProductStore,
    p: &PreparedOwnedScopedChangeV2,
    id: &str,
) -> VerifiedChange {
    engine(store)
        .prepare_owned_choice(
            store,
            p.clone(),
            choice(
                p,
                id,
                DecisionOutcome::Accept {
                    artifact: p.target().artifact.program_digest.clone(),
                },
            ),
            action_scenes(store, p),
            p.operation_id(),
        )
        .unwrap()
}
fn pending_choice(
    store: &ProductStore,
    p: &PreparedOwnedScopedChangeV2,
    id: &str,
    operation: &str,
    outcome: DecisionOutcome,
) -> VerifiedChange {
    let e = engine(store);
    let scene = scenario(
        store,
        "paired",
        vec![SemanticInput::Observe {
            point: "result".into(),
        }],
    );
    let scenes = vec![
        e.accept_owned_current_scene(store, p, &scene, Disclosure::Synthetic)
            .unwrap(),
        e.accept_owned_scene(store, p, &scene, Disclosure::Synthetic)
            .unwrap(),
    ];
    e.prepare_owned_rehearsed_choice(
        store,
        p.clone(),
        choice(p, id, outcome),
        scenes,
        &[],
        operation,
    )
    .unwrap()
}
#[test]
fn first_owned_adoption_reopens_preserves_frame_and_accepts_later_work() {
    let (_dir, store, a, b) = setup_operable();
    let before = store.load().unwrap();
    let p = prepared(&store, &a, &b, "adopt-first");
    assert_eq!(store.load().unwrap(), before);
    let change = accept_choice(&store, &p, "owned-accept");
    assert_eq!(store.load().unwrap(), before);
    let after = engine(&store).adopt(&store, &change).unwrap();
    assert_eq!(after.revision, before.revision + 1);
    assert_eq!(after.session, before.session);
    assert_eq!(after.clock_day, before.clock_day);
    assert_eq!(after.artifacts, before.artifacts);
    assert_eq!(after.data.events, before.data.events);
    assert_eq!(after.data.generation, before.data.generation);
    for old in &before.data.records {
        let now = after.data.records.iter().find(|r| r.id == old.id).unwrap();
        assert_eq!(now.revision, old.revision);
        for (key, value) in &old.values {
            assert_eq!(now.values.get(key), Some(value));
        }
    }
    let state = after.owned_scope().unwrap();
    assert_eq!(state.layers.len(), 1);
    assert_eq!(state.compositions.len(), 1);
    assert_eq!(state.initializations.len(), 1);
    assert_eq!(state.adoptions.len(), 1);
    let reopened = ProductStore::open(_dir.path().join("store")).unwrap();
    assert_eq!(reopened.load().unwrap(), after);
    assert_eq!(
        engine(&reopened).check_current(&after).unwrap().disposition,
        CheckDisposition::Ready
    );
    apply_store(
        &reopened,
        "later-add",
        invoke(
            "add",
            [(
                "title".into(),
                DataValue::Text {
                    value: "later".into(),
                },
            )]
            .into(),
        ),
    );
    let later = reopened.load().unwrap();
    assert_eq!(engine(&reopened).adopt(&reopened, &change).unwrap(), later);
    assert!(LifecycleAcceptanceHost::prepare(
        &reopened,
        &b,
        &operable_request(&reopened, &a, &b),
        &witnesses(&reopened),
        ProofBudget::default(),
        cancel()
    )
    .is_err());
}
#[test]
fn four_nonbinary_choices_preserve_live_frame_and_exact_retry() {
    for outcome in [
        DecisionOutcome::EitherAcceptable,
        DecisionOutcome::BothNeeded,
        DecisionOutcome::NeitherFits,
        DecisionOutcome::Deferred,
    ] {
        let (_dir, store, a, b) = setup_operable();
        let before = store.load().unwrap();
        let p = prepared(&store, &a, &b, "recipe");
        let change = pending_choice(&store, &p, "pending", "record", outcome.clone());
        let after = engine(&store).adopt(&store, &change).unwrap();
        assert_eq!(after.active_revision, before.active_revision);
        assert_eq!(after.data, before.data);
        assert_eq!(after.session, before.session);
        assert_eq!(after.clock_day, before.clock_day);
        assert_eq!(after.artifacts, before.artifacts);
        assert!(after.owned_scope().unwrap().layers.is_empty());
        assert_eq!(after.owned_scope().unwrap().rehearsals.len(), 1);
        assert_eq!(after.decisions.decisions.last().unwrap().outcome, outcome);
        assert_eq!(
            after.decisions.decisions.last().unwrap().status,
            DecisionStatus::Pending
        );
        assert_eq!(engine(&store).adopt(&store, &change).unwrap(), after);
        let reopened = ProductStore::open(_dir.path().join("store")).unwrap();
        assert_eq!(reopened.load().unwrap(), after);
        apply_store(
            &reopened,
            "later-add",
            invoke(
                "add",
                [(
                    "title".into(),
                    DataValue::Text {
                        value: "later".into(),
                    },
                )]
                .into(),
            ),
        );
        let later = reopened.load().unwrap();
        assert_eq!(engine(&reopened).adopt(&reopened, &change).unwrap(), later);
        assert!(engine(&reopened)
            .accept_owned_scene(
                &reopened,
                &p,
                &scenario(&reopened, "stale", vec![]),
                Disclosure::Synthetic
            )
            .is_err());
    }
}
fn ordinary_choice(store: &ProductStore, id: &str, outcome: DecisionOutcome, resolves: &[Id]) {
    let current = store.load().unwrap();
    let original = scenario(
        store,
        id,
        vec![
            invoke(
                "add",
                [("title".into(), DataValue::Text { value: id.into() })].into(),
            ),
            SemanticInput::Observe {
                point: "independent".into(),
            },
        ],
    );
    let e = engine(store);
    let accepted = e
        .accept_current_scene(&current, &original, Disclosure::Synthetic)
        .unwrap();
    let expected = accepted
        .observations()
        .iter()
        .find(|o| o.point == "independent")
        .unwrap()
        .values["ready_count"]
        .clone();
    let choice = Choice {
        id: id.into(),
        request: "Keep this independent ready-count property".into(),
        rationale: None,
        scope: DecisionScope {
            operations: ["add".into()].into(),
            population: Population::All,
            conditions: Values::new(),
            excluded_records: vec![],
            unknowns: vec![],
        },
        outcome,
        obligations: vec![AcceptedProperty {
            id: "count".into(),
            description: "The real historical count after adding".into(),
            predicate: PropertyPredicate::Equal {
                left: PropertyTerm::Observed {
                    point: "independent".into(),
                    observable: "ready_count".into(),
                    value_type: Type::Integer,
                },
                right: PropertyTerm::Literal {
                    value_type: Type::Integer,
                    value: expected,
                },
            },
        }],
        binding: IntentionBinding::PropertiesOnly,
    };
    let change = if resolves.is_empty() {
        e.prepare_choice(
            store,
            current.program().unwrap(),
            choice,
            vec![accepted],
            &format!("op-{id}"),
        )
    } else {
        e.prepare_resolution(
            store,
            current.program().unwrap(),
            choice,
            vec![accepted],
            resolves,
            &format!("op-{id}"),
        )
    }
    .unwrap();
    e.adopt(store, &change).unwrap();
}
fn recover(store: &ProductStore) -> (tempfile::TempDir, ProductStore) {
    let snapshot = store.load().unwrap();
    let bundle = IntentArchive::new(store.clone())
        .export_for(&snapshot)
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let restored =
        ProductStore::create_recovered_with(dir.path().join("restored"), &snapshot, |s| {
            IntentArchive::new(s.clone())
                .restore_for(&snapshot, &bundle)
                .map_err(|e| product_store::StoreError::Invalid(e.to_string()))
        })
        .unwrap();
    assert_eq!(restored.load().unwrap(), snapshot);
    (dir, restored)
}
#[test]
fn distinct_historical_intentions_and_pending_terminal_history_survive_real_commit() {
    let (_dir, store, a, b) = setup_operable();
    ordinary_choice(&store, "first-promise", DecisionOutcome::KeepCurrent, &[]);
    apply_store(
        &store,
        "between-promises",
        invoke(
            "add",
            [(
                "title".into(),
                DataValue::Text {
                    value: "between".into(),
                },
            )]
            .into(),
        ),
    );
    apply_store(&store, "new-day", SemanticInput::AdvanceClock { days: 2 });
    apply_store(
        &store,
        "session-work",
        SemanticInput::Control {
            view: "jobs".into(),
            control: "name".into(),
            value: DataValue::Text {
                value: "unfinished entry".into(),
            },
        },
    );
    ordinary_choice(&store, "second-promise", DecisionOutcome::KeepCurrent, &[]);
    ordinary_choice(&store, "old-pending", DecisionOutcome::Deferred, &[]);
    ordinary_choice(
        &store,
        "terminal-successor",
        DecisionOutcome::Deferred,
        &["old-pending".into()],
    );
    ordinary_choice(&store, "other-pending", DecisionOutcome::NeitherFits, &[]);
    apply_store(
        &store,
        "after-promises",
        invoke(
            "add",
            [(
                "title".into(),
                DataValue::Text {
                    value: "after".into(),
                },
            )]
            .into(),
        ),
    );
    let before = store.load().unwrap();
    let p = prepared(&store, &a, &b, "adopt-with-promises");
    let change = accept_choice(&store, &p, "owned-promise");
    let after = engine(&store).adopt(&store, &change).unwrap();
    for d in &before.decisions.decisions {
        assert!(after.decisions.decisions.contains(d));
    }
    assert_eq!(after.session, before.session);
    assert_eq!(after.clock_day, before.clock_day);
    assert_eq!(
        engine(&store).check_current(&after).unwrap().disposition,
        CheckDisposition::Ready
    );
    let (_backup, restored) = recover(&store);
    assert_eq!(
        engine(&restored)
            .check_current(&restored.load().unwrap())
            .unwrap()
            .disposition,
        CheckDisposition::Ready
    );
}
#[test]
fn two_prospectives_survive_later_work_and_exact_pending_resolution() {
    let (_dir, store, a, b) = setup_operable();
    let p = prepared(&store, &a, &b, "recipe-one");
    let first = pending_choice(
        &store,
        &p,
        "pending-one",
        "record-one",
        DecisionOutcome::Deferred,
    );
    let once = engine(&store).adopt(&store, &first).unwrap();
    let p2 = prepared(&store, &a, &b, "recipe-two");
    assert_ne!(p.target(), p2.target());
    let second = pending_choice(
        &store,
        &p2,
        "pending-two",
        "record-two",
        DecisionOutcome::BothNeeded,
    );
    engine(&store).adopt(&store, &second).unwrap();
    apply_store(
        &store,
        "between-record-and-adopt",
        invoke(
            "add",
            [(
                "title".into(),
                DataValue::Text {
                    value: "later".into(),
                },
            )]
            .into(),
        ),
    );
    let before = store.load().unwrap();
    let old_state = before.owned_scope().unwrap().clone();
    let p3 = prepared(&store, &a, &b, "fresh-adopt");
    let scenes = action_scenes(&store, &p3);
    let e = engine(&store);
    let c = choice(
        &p3,
        "resolve-one",
        DecisionOutcome::Accept {
            artifact: p3.target().artifact.program_digest.clone(),
        },
    );
    for ids in [
        vec!["missing".into()],
        vec!["pending-one".into(), "pending-one".into()],
    ] {
        assert!(e
            .prepare_owned_resolution(
                &store,
                p3.clone(),
                c.clone(),
                scenes.clone(),
                &ids,
                "fresh-adopt"
            )
            .is_err());
    }
    let change = e
        .prepare_owned_resolution(
            &store,
            p3.clone(),
            c,
            scenes,
            &["pending-one".into()],
            "fresh-adopt",
        )
        .unwrap();
    let after = e.adopt(&store, &change).unwrap();
    assert_eq!(
        after.owned_scope().unwrap().rehearsals,
        old_state.rehearsals
    );
    let mut expected = once
        .decisions
        .decisions
        .iter()
        .find(|d| d.id == "pending-one")
        .unwrap()
        .clone();
    expected.status = DecisionStatus::Superseded {
        by: "resolve-one".into(),
    };
    expected.revision += 1;
    assert!(after.decisions.decisions.contains(&expected));
    assert!(after.decisions.decisions.contains(
        before
            .decisions
            .decisions
            .iter()
            .find(|d| d.id == "pending-two")
            .unwrap()
    ));
    assert!(after.data.records.iter().any(|r| r.values.get("title")
        == Some(&DataValue::Text {
            value: "later".into()
        })));
    assert_eq!(e.adopt(&store, &first).unwrap(), after);
    let (_backup, restored) = recover(&store);
    assert_eq!(
        engine(&restored)
            .check_current(&restored.load().unwrap())
            .unwrap()
            .disposition,
        CheckDisposition::Ready
    );
}
#[test]
fn wrong_durable_target_frame_scope_and_missing_operation_coverage_refuse() {
    let (_dir, store, a, b) = setup_operable();
    let before = store.load().unwrap();
    let p = prepared(&store, &a, &b, "first");
    let e = engine(&store);
    let scene = scenario(
        &store,
        "experience",
        vec![SemanticInput::Observe {
            point: "result".into(),
        }],
    );
    for field in 0..4 {
        let mut wrong = scene.clone();
        match field {
            0 => wrong.clock_day += 1,
            1 => wrong.random_seed = 1,
            2 => wrong
                .session
                .values
                .insert(
                    "name_input".into(),
                    DataValue::Text {
                        value: "other".into(),
                    },
                )
                .map(|_| ())
                .unwrap_or(()),
            _ => wrong.seed.generation += 1,
        };
        assert!(e
            .accept_owned_scene(&store, &p, &wrong, Disclosure::Synthetic)
            .is_err());
    }
    let accepted = e
        .accept_owned_scene(&store, &p, &scene, Disclosure::Synthetic)
        .unwrap();
    let mut c = choice(
        &p,
        "accept",
        DecisionOutcome::Accept {
            artifact: p.target().artifact.program_digest.clone(),
        },
    );
    assert!(
        e.prepare_owned_choice(
            &store,
            p.clone(),
            c.clone(),
            vec![accepted.clone()],
            "first"
        )
        .is_err(),
        "Observe is not all-action coverage"
    );
    c.outcome = DecisionOutcome::KeepCurrent;
    assert!(e
        .prepare_owned_choice(
            &store,
            p.clone(),
            c.clone(),
            vec![accepted.clone()],
            "first"
        )
        .is_err());
    c.outcome = DecisionOutcome::Accept {
        artifact: b.artifact.program_digest.clone(),
    };
    assert!(e
        .prepare_owned_choice(
            &store,
            p.clone(),
            c.clone(),
            vec![accepted.clone()],
            "first"
        )
        .is_err());
    c.outcome = DecisionOutcome::Deferred;
    assert!(e
        .prepare_owned_rehearsed_choice(
            &store,
            p.clone(),
            c.clone(),
            vec![accepted.clone()],
            &[],
            "record"
        )
        .is_err());
    let current = e
        .accept_owned_current_scene(&store, &p, &scene, Disclosure::Synthetic)
        .unwrap();
    c.scope.excluded_records.push(row(&before.data, "live"));
    assert!(e
        .prepare_owned_rehearsed_choice(
            &store,
            p.clone(),
            c,
            vec![current, accepted],
            &[],
            "record"
        )
        .is_err());
    assert_eq!(store.load().unwrap(), before);
}
#[test]
fn first_owned_faults_and_concurrent_writer_never_double_initialize() {
    use product_store::FaultPoint;
    for recording in [false, true] {
        for fault in [
            FaultPoint::AfterObject,
            FaultPoint::BeforePointer,
            FaultPoint::AfterPointer,
        ] {
            let (_dir, store, a, b) = setup_operable();
            let before = store.load().unwrap();
            let p = prepared(&store, &a, &b, "faulted");
            let change = if recording {
                pending_choice(&store, &p, "pending", "record", DecisionOutcome::Deferred)
            } else {
                accept_choice(&store, &p, "accepted")
            };
            let e = engine(&store);
            assert!(e.adopt_owned_with_fault(&store, &change, fault).is_err());
            let reopened = ProductStore::open(_dir.path().join("store")).unwrap();
            if fault == FaultPoint::AfterPointer {
                assert_eq!(reopened.load().unwrap().revision, before.revision + 1);
            } else {
                assert_eq!(reopened.load().unwrap(), before);
            }
            let after = e.adopt(&reopened, &change).unwrap();
            assert_eq!(after.revision, before.revision + 1);
            assert_eq!(
                after.owned_scope().unwrap().initializations.len(),
                if recording { 0 } else { 1 }
            );
            assert_eq!(e.adopt(&reopened, &change).unwrap(), after);
        }
    }
    let (_dir, store, a, b) = setup_operable();
    let p = prepared(&store, &a, &b, "stale");
    let change = accept_choice(&store, &p, "accepted");
    let other = ProductStore::open(_dir.path().join("store")).unwrap();
    apply_store(
        &other,
        "concurrent",
        SemanticInput::AdvanceClock { days: 1 },
    );
    let changed = other.load().unwrap();
    assert!(engine(&store).adopt(&store, &change).is_err());
    assert_eq!(store.load().unwrap(), changed);
}
#[test]
fn packages_are_checked_before_publication_and_on_exact_retry() {
    for recording in [false, true] {
        let (dir, store, a, b) = setup_operable();
        let p = prepared(&store, &a, &b, "packages");
        let change = if recording {
            pending_choice(&store, &p, "pending", "record", DecisionOutcome::Deferred)
        } else {
            accept_choice(&store, &p, "accepted")
        };
        let before = store.load().unwrap();
        let mapping = change.plan().evidence[0].clone();
        let path = dir
            .path()
            .join("store")
            .join(format!("extension-{}.json", mapping.as_str()));
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, b"corrupt package").unwrap();
        assert!(engine(&store).adopt(&store, &change).is_err());
        assert_eq!(store.load().unwrap(), before);
        std::fs::write(&path, &bytes).unwrap();
        let after = engine(&store).adopt(&store, &change).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(engine(&store).adopt(&store, &change).is_err());
        assert_eq!(store.load().unwrap(), after);
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(engine(&store).adopt(&store, &change).unwrap(), after);
        let bundle = IntentArchive::new(store.clone())
            .export_for(&after)
            .unwrap();
        let raw = serde_json::to_value(&bundle).unwrap();
        for kind in 0..3 {
            let mut wrong = raw.clone();
            let objects = wrong["objects"].as_array_mut().unwrap();
            if kind == 0 {
                objects.pop();
            } else if kind == 1 {
                objects.push(objects[0].clone());
            } else {
                objects[0]["bytes"] = serde_json::json!([123, 125]);
            }
            let wrong: IntentionBundle = serde_json::from_value(wrong).unwrap();
            assert!(validate_bundle(&after, &wrong).is_err());
        }
    }
}
#[test]
fn failed_candidate_intention_blocks_activation_but_honest_pending_recording_checks_current() {
    let (_dir, store, a, b) = setup_operable();
    let e = engine(&store);
    let before = store.load().unwrap();
    let live = row(&before.data, "live");
    let material = before
        .data
        .records
        .iter()
        .find(|r| r.entity == "material")
        .unwrap();
    let material = reference(&RecordRef {
        entity: material.entity.clone(),
        record: material.id.clone(),
    });
    let args = || {
        [
            ("material".into(), material.clone()),
            (
                "note".into(),
                DataValue::Text {
                    value: "observed correction".into(),
                },
            ),
        ]
        .into()
    };
    let input = scenario(
        &store,
        "ordinary-observed",
        vec![
            invoke("begin", [("job".into(), reference(&live))].into()),
            invoke("withdraw", args()),
            invoke("arrive", args()),
            SemanticInput::Observe {
                point: "result".into(),
            },
        ],
    );
    let scene = e
        .accept_current_scene(&before, &input, Disclosure::Synthetic)
        .unwrap();
    let ordinary = Choice {
        id: "observed-promise".into(),
        request: "Preserve this complete observed result".into(),
        rationale: None,
        scope: DecisionScope {
            operations: ["begin".into(), "withdraw".into(), "arrive".into()].into(),
            population: Population::All,
            conditions: Values::new(),
            excluded_records: vec![],
            unknowns: vec![],
        },
        outcome: DecisionOutcome::KeepCurrent,
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    };
    let c = e
        .prepare_choice(&store, &a, ordinary, vec![scene], "keep-observed")
        .unwrap();
    e.adopt(&store, &c).unwrap();
    let request = operable_request(&store, &a, &b);
    let accepted = lifecycle(&store, &b, &request);
    let (p, report) = e
        .prepare_owned_change(
            &store,
            &b,
            &request,
            &accepted,
            "diagnostic",
            ProofBudget::default(),
            cancel(),
        )
        .unwrap();
    assert_ne!(
        report.disposition,
        CheckDisposition::Ready,
        "the candidate resumes active work while the retained baseline outcome is ready"
    );
    let before = store.load().unwrap();
    assert!(e
        .prepare_owned_choice(
            &store,
            p.clone(),
            choice(
                &p,
                "accept",
                DecisionOutcome::Accept {
                    artifact: p.target().artifact.program_digest.clone()
                }
            ),
            action_scenes(&store, &p),
            "diagnostic"
        )
        .is_err());
    assert_eq!(store.load().unwrap(), before);
    let record = pending_choice(
        &store,
        &p,
        "honest-pending",
        "record-diagnostic",
        DecisionOutcome::NeitherFits,
    );
    let after = e.adopt(&store, &record).unwrap();
    assert_eq!(after.data, before.data);
    assert_eq!(after.active_revision, before.active_revision);
    assert_eq!(
        e.check_current(&after).unwrap().disposition,
        CheckDisposition::Ready
    );
    assert!(after
        .decisions
        .decisions
        .contains(&before.decisions.decisions[0]));
}
#[test]
fn same_operation_changed_choice_conflicts_and_cancelled_preparation_cannot_save() {
    for recording in [false, true] {
        let (_dir, store, a, b) = setup_operable();
        let p = prepared(&store, &a, &b, "same-operation");
        let first = if recording {
            pending_choice(
                &store,
                &p,
                "first",
                "same-record",
                DecisionOutcome::Deferred,
            )
        } else {
            accept_choice(&store, &p, "first")
        };
        let second = if recording {
            pending_choice(
                &store,
                &p,
                "second",
                "same-record",
                DecisionOutcome::NeitherFits,
            )
        } else {
            accept_choice(&store, &p, "second")
        };
        let after = engine(&store).adopt(&store, &first).unwrap();
        assert!(engine(&store).adopt(&store, &second).is_err());
        assert_eq!(store.load().unwrap(), after);
    }
    let (_dir, store, a, b) = setup_operable();
    let before = store.load().unwrap();
    let r = operable_request(&store, &a, &b);
    let accepted = lifecycle(&store, &b, &r);
    let token = cancel();
    token.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(engine(&store)
        .prepare_owned_change(
            &store,
            &b,
            &r,
            &accepted,
            "cancelled",
            ProofBudget::default(),
            token
        )
        .is_err());
    assert!(engine(&store)
        .prepare_owned_change(
            &store,
            &b,
            &r,
            &accepted,
            "small",
            ProofBudget {
                units: 1,
                elapsed_millis: 60_000
            },
            cancel()
        )
        .is_err());
    let token = cancel();
    let (p, _) = engine(&store)
        .prepare_owned_change(
            &store,
            &b,
            &r,
            &accepted,
            "cancel-later",
            ProofBudget::default(),
            token.clone(),
        )
        .unwrap();
    let change = accept_choice(&store, &p, "accept");
    token.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(engine(&store).adopt(&store, &change).is_err());
    assert_eq!(store.load().unwrap(), before);
}
#[test]
fn retry_finishes_required_sync_barriers_before_reporting_success() {
    let (dir, store, a, b) = setup_operable();
    let p = prepared(&store, &a, &b, "sync");
    let change = pending_choice(&store, &p, "pending", "record", DecisionOutcome::Deferred);
    assert!(engine(&store)
        .adopt_owned_with_fault(&store, &change, product_store::FaultPoint::AfterPointer)
        .is_err());
    let visible = store.load().unwrap();
    let fault = product_store::file_sync_testing::fail(dir.path().join("store").join("CURRENT"));
    assert!(engine(&store).adopt(&store, &change).is_err());
    assert_eq!(fault.attempts(), 1);
    assert_eq!(store.load().unwrap(), visible);
    drop(fault);
    assert_eq!(engine(&store).adopt(&store, &change).unwrap(), visible);
}
#[test]
fn prepared_context_rejects_unbound_graph_target_and_inflight_cancellation() {
    let (_dir, store, a, b) = setup_operable();
    let request = operable_request(&store, &a, &b);
    let life = lifecycle(&store, &b, &request);
    let token = cancel();
    let (p, _) = engine(&store)
        .prepare_owned_change(
            &store,
            &b,
            &request,
            &life,
            "graph",
            ProofBudget::default(),
            token.clone(),
        )
        .unwrap();
    let change = pending_choice(&store, &p, "pending", "record", DecisionOutcome::Deferred);
    let (context, graph) = engine(&store).owned_context_for_test(&change).unwrap();
    let original = scenario(
        &store,
        "context",
        vec![SemanticInput::Observe {
            point: "result".into(),
        }],
    );
    let (_, projected) = context
        .project_owned_scenario(&a, &original, p.target())
        .unwrap();
    let runtime = product_runtime::LocalRuntime::default();
    let mut wrong = graph.clone();
    wrong.revision += 1;
    assert!(product_decisions::execute_admitted(
        &runtime,
        p.target(),
        &projected,
        &wrong,
        RuntimeLimits::default(),
        "wrong-graph",
        Some(&context)
    )
    .is_err());
    assert!(
        context.admit_target(p.target()).is_err(),
        "recording may only save unchanged current"
    );
    assert!(context.admit_target(&b).is_err());
    let admission = context
        .begin_replay(p.target(), &projected, &graph, cancel())
        .unwrap()
        .unwrap();
    token.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(
        admission
            .validate_seed(p.target(), &projected.seed, projected.clock_day)
            .is_err(),
        "preparation cancellation stays live inside the same run meter"
    );
    assert_eq!(store.load().unwrap(), *p.basis());
}
#[test]
fn simultaneous_owned_recordings_publish_at_most_one_checkpoint() {
    let (_dir, store, a, b) = setup_operable();
    let before = store.load().unwrap();
    let p = prepared(&store, &a, &b, "concurrent-recipe");
    let a = pending_choice(&store, &p, "one", "record-one", DecisionOutcome::Deferred);
    let b = pending_choice(
        &store,
        &p,
        "two",
        "record-two",
        DecisionOutcome::NeitherFits,
    );
    let barrier = std::sync::Barrier::new(3);
    let results = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            barrier.wait();
            engine(&store).adopt(&store, &a)
        });
        let second = scope.spawn(|| {
            barrier.wait();
            engine(&store).adopt(&store, &b)
        });
        barrier.wait();
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(
        results.iter().filter(|r| r.is_ok()).count(),
        1,
        "{results:?}"
    );
    let after = store.load().unwrap();
    assert_eq!(after.revision, before.revision + 1);
    assert_eq!(after.data, before.data);
    assert_eq!(after.owned_scope().unwrap().rehearsals.len(), 1);
    IntentArchive::new(store.clone())
        .export_for(&after)
        .unwrap();
}
#[test]
fn independently_real_but_mismatched_pairs_and_other_recipe_scenes_refuse() {
    let (_dir, store, a, b) = setup_operable();
    let before = store.load().unwrap();
    let request = operable_request(&store, &a, &b);
    assert!(engine(&store)
        .prepare_owned_change(
            &store,
            &b,
            &request,
            &lifecycle(&store, &b, &request),
            "one", // already used by a real business operation
            ProofBudget::default(),
            cancel(),
        )
        .is_err());
    assert_eq!(store.load().unwrap(), before);
    let p = prepared(&store, &a, &b, "recipe-one");
    let other = prepared(&store, &a, &b, "recipe-two");
    let e = engine(&store);
    let first = scenario(
        &store,
        "first-scene",
        vec![SemanticInput::Observe {
            point: "result".into(),
        }],
    );
    let second = scenario(
        &store,
        "second-scene",
        vec![SemanticInput::Observe {
            point: "result".into(),
        }],
    );
    let left = e
        .accept_owned_current_scene(&store, &p, &first, Disclosure::Synthetic)
        .unwrap();
    let right = e
        .accept_owned_scene(&store, &p, &second, Disclosure::Synthetic)
        .unwrap();
    assert!(e
        .prepare_owned_rehearsed_choice(
            &store,
            p.clone(),
            choice(&p, "pending", DecisionOutcome::Deferred),
            vec![left, right],
            &[],
            "record"
        )
        .is_err());
    let foreign = e
        .accept_owned_scene(&store, &other, &first, Disclosure::Synthetic)
        .unwrap();
    let accepted = choice(
        &p,
        "accept",
        DecisionOutcome::Accept {
            artifact: p.target().artifact.program_digest.clone(),
        },
    );
    let mut scenes = action_scenes(&store, &p);
    assert!(e
        .prepare_owned_choice(
            &store,
            p.clone(),
            accepted.clone(),
            scenes.clone(),
            p.operation_id(),
        )
        .is_ok());
    scenes.push(foreign);
    assert!(e
        .prepare_owned_choice(&store, p.clone(), accepted, scenes, p.operation_id(),)
        .is_err());
    assert_eq!(store.load().unwrap(), before);
}
#[cfg(unix)]
#[test]
fn replaced_store_path_cannot_receive_an_old_verified_choice() {
    let (dir, store, a, b) = setup_operable();
    let p = prepared(&store, &a, &b, "replace");
    let change = pending_choice(&store, &p, "pending", "record", DecisionOutcome::Deferred);
    let before = store.load().unwrap();
    std::fs::rename(dir.path().join("store"), dir.path().join("old-store")).unwrap();
    let replacement = ProductStore::create(dir.path().join("store"), &a, 20000).unwrap();
    let fresh = replacement.load().unwrap();
    assert!(engine(&store).adopt(&store, &change).is_err());
    assert_eq!(replacement.load().unwrap(), fresh);
    assert_eq!(
        ProductStore::open(dir.path().join("old-store"))
            .unwrap()
            .load()
            .unwrap(),
        before
    );
}
