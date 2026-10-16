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
#[test]
fn generic_registry_receipt_owner_and_policy_spans_have_complete_opaque_proof() {
    let (a, b) = pair();
    let (_d, s) = setup(&a);
    let r = request(&s, &a, &b);
    let before = s.load().unwrap();
    let proof = verify(&s, &b, &r).unwrap();
    assert!(!proof.report().certificates.is_empty());
    assert!(proof
        .report()
        .projections
        .iter()
        .any(|p| p.baseline.is_none()));
    assert!(proof
        .report()
        .outcome_comparison
        .iter()
        .any(|x| x.contains("unsupported")));
    assert_eq!(s.load().unwrap(), before);
    s.revalidate_owned_preview(
        &proof,
        ProofBudget::default(),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
}
#[test]
fn host_challenge_rejects_wrong_witness_frame_and_requires_both_lifecycle_states() {
    let (a, b) = pair();
    let (_d, s) = setup(&a);
    let r = request(&s, &a, &b);
    for kind in 0..5 {
        let mut w = witnesses(&s);
        match kind {
            0 => w[0].scenario.clock_day += 1,
            1 => w[0].scenario.session.view = "history".into(),
            2 => w[0].unfinished = w[0].completed.clone(),
            3 => {
                w.remove(1);
            }
            _ => w[0].completed.entity = "material".into(),
        };
        assert!(
            LifecycleAcceptanceHost::prepare(
                &s,
                &b,
                &r,
                &w,
                ProofBudget::default(),
                Arc::new(AtomicBool::new(false))
            )
            .is_err(),
            "case {kind}"
        );
    }
}
#[test]
fn acceptance_is_bound_to_full_sources_scope_meaning_and_current_snapshot() {
    let (a, b) = pair();
    let (_d, s) = setup(&a);
    let r = request(&s, &a, &b);
    let accepted = acceptance(&s, &b, &r);
    for k in 0..4 {
        let mut changed = r.clone();
        match k {
            0 => changed.purpose.push('!'),
            1 => changed.inventory.lifecycle.meaning.push('!'),
            2 => changed.population = ScopePopulation::FutureWork,
            _ => {
                changed.operations.remove("complete");
            }
        }
        assert!(s
            .verify_owned_preview(
                &b,
                &changed,
                &accepted,
                ProofBudget::default(),
                Arc::new(AtomicBool::new(false))
            )
            .is_err());
    }
    let mut changed = b.clone();
    changed.source_bytes.push(b' ');
    assert!(s
        .verify_owned_preview(
            &changed,
            &r,
            &accepted,
            ProofBudget::default(),
            Arc::new(AtomicBool::new(false))
        )
        .is_err());
    s.apply(
        s.load().unwrap().revision,
        "advance",
        &SemanticInput::AdvanceClock { days: 1 },
        RuntimeLimits::default(),
    )
    .unwrap();
    assert!(s
        .verify_owned_preview(
            &b,
            &r,
            &accepted,
            ProofBudget::default(),
            Arc::new(AtomicBool::new(false))
        )
        .is_err());
}
#[test]
fn cumulative_proof_budget_and_cancellation_refuse_without_resetting_per_projection() {
    let (a, b) = pair();
    let (_d, s) = setup(&a);
    let r = request(&s, &a, &b);
    let accepted = acceptance(&s, &b, &r);
    assert!(s
        .verify_owned_preview(
            &b,
            &r,
            &accepted,
            ProofBudget {
                units: 1,
                elapsed_millis: 5000
            },
            Arc::new(AtomicBool::new(false))
        )
        .is_err());
    assert!(s
        .verify_owned_preview(
            &b,
            &r,
            &accepted,
            ProofBudget::default(),
            Arc::new(AtomicBool::new(true))
        )
        .is_err());
}
#[test]
fn complete_pair_rejects_policy_admission_mutation_totality_and_hidden_consumers() {
    for k in 0..12 {
        let (a, b) = pair();
        let mut aa = a.program.clone();
        let mut bb = b.program.clone();
        for app in [&mut aa, &mut bb] {
            match k {
                0 => app
                    .actions
                    .iter_mut()
                    .find(|x| x.id == "register")
                    .unwrap()
                    .guards
                    .push(eq(field("owner", "status"), text("active"))),
                1 => app
                    .actions
                    .iter_mut()
                    .find(|x| x.id == "register")
                    .unwrap()
                    .ensures
                    .push(eq(field("owner", "status"), text("active"))),
                2 => app
                    .entities
                    .iter_mut()
                    .find(|x| x.id == "material")
                    .unwrap()
                    .constraints
                    .push(eq(
                        count(query("job", "j", eq(field("j", "status"), text("active")))),
                        int(0),
                    )),
                3 => app.actions.push(action(
                    "hidden_owner_transfer",
                    vec![
                        ("link", Type::reference("requirement")),
                        ("other", Type::reference("job")),
                    ],
                    vec![],
                    vec![update(var("link"), vec![("job", var("other"))])],
                )),
                4 => app.actions.push(action(
                    "hidden_archive",
                    vec![("link", Type::reference("requirement"))],
                    vec![],
                    vec![Statement::Archive {
                        record: var("link"),
                    }],
                )),
                5 => app
                    .actions
                    .iter_mut()
                    .find(|x| x.id == "register")
                    .unwrap()
                    .steps
                    .insert(0, update(get(field("owner", "status"), "bad"), vec![])), // Invalid typed target must fail at capture.
                6 => {
                    if let ViewKind::List { .. } = &app.views[0].kind {
                        app.views[0].actions[0]
                            .arguments
                            .insert("name".into(), field("row", "status"));
                    }
                }
                7 => app.actions.push(action(
                    "launder",
                    vec![("job", Type::reference("job"))],
                    vec![],
                    vec![Statement::SetState {
                        state: "name_input".into(),
                        value: field("job", "status"),
                    }],
                )),
                8 => {
                    app.entities
                        .iter_mut()
                        .find(|x| x.id == "requirement")
                        .unwrap()
                        .unique = vec![vec!["material".into()]]
                }
                9 => app.actions.push(action(
                    "event_guard",
                    vec![],
                    vec![eq(count(query("event", "e", boolean(true))), int(0))],
                    vec![Statement::SetState {
                        state: "name_input".into(),
                        value: text(""),
                    }],
                )),
                10 => app.actions.push(action(
                    "event_fold",
                    vec![("material", Type::reference("material"))],
                    vec![],
                    vec![each(
                        query("event", "e", boolean(true)),
                        "e",
                        vec![update(
                            var("material"),
                            vec![("received", eq(field("e", "note"), text("yes")))],
                        )],
                    )],
                )),
                _ => app
                    .actions
                    .iter_mut()
                    .find(|x| x.id == "register")
                    .unwrap()
                    .steps
                    .push(Statement::Assert {
                        condition: eq(field("owner", "status"), text("active")),
                        message: "policy assertion".into(),
                    }),
            }
        }
        if k == 5 {
            assert!(aa.validate().is_err());
            continue;
        }
        let aa = capture(&aa);
        let bb = capture(&bb);
        let (_d, s) = setup(&aa);
        let r = request(&s, &aa, &bb);
        assert!(verify(&s, &bb, &r).is_err(), "unsafe case {k} admitted");
    }
}
#[test]
fn variant_overflow_and_lifecycle_transition_are_not_resource_exceptions() {
    for k in 0..3 {
        let (a, b) = pair();
        let mut app = b.program.clone();
        let act = app.actions.iter_mut().find(|x| x.id == "register").unwrap();
        let Statement::ForEach { steps, .. } = &mut act.steps[2] else {
            panic!()
        };
        let Statement::Update { values, .. } = &mut steps[0] else {
            panic!()
        };
        let bad = match k {
            0 => text("done"),
            1 => cond(
                eq(
                    Expr::Add {
                        left: Box::new(int(i64::MAX)),
                        right: Box::new(int(1)),
                    },
                    int(0),
                ),
                text("ready"),
                text("waiting"),
            ),
            _ => cond(
                eq(
                    Expr::DateAdd {
                        date: Box::new(lit(DataValue::Date { days: 0 }, Type::Date)),
                        days: Box::new(int(i64::MAX)),
                    },
                    lit(DataValue::Date { days: 0 }, Type::Date),
                ),
                text("ready"),
                text("waiting"),
            ),
        };
        let original = values["status"].clone();
        values.insert(
            "status".into(),
            cond(eq(field("job", "status"), text("active")), bad, original),
        );
        let b = capture(&app);
        let (_d, s) = setup(&a);
        let r = request(&s, &a, &b);
        assert!(verify(&s, &b, &r).is_err());
    }
}
#[test]
fn exact_sites_reject_shared_writes_laundered_keys_and_unproved_auxiliary_uniqueness() {
    for case in 0..5 {
        let (a, b) = pair();
        let mut aa = a.program.clone();
        let mut bb = b.program.clone();
        for app in [&mut aa, &mut bb] {
            match case {
                0 => {
                    let action = app.actions.iter_mut().find(|a| a.id == "withdraw").unwrap();
                    action
                        .parameters
                        .insert("owner".into(), Type::reference("job"));
                    action.guards.push(not(done("owner")));
                }
                1 => {
                    let action = app.actions.iter_mut().find(|a| a.id == "register").unwrap();
                    let Statement::ForEach { steps, .. } = &mut action.steps[0] else {
                        panic!()
                    };
                    let Statement::Create { values, .. } = &mut steps[0] else {
                        panic!()
                    };
                    values.insert("name".into(), field("owner", "status"));
                }
                2 => {
                    let ViewKind::List { rows, .. } = &mut app.views[1].kind else {
                        panic!()
                    };
                    let Expr::Query { limit, .. } = rows else {
                        panic!()
                    };
                    *limit = 1;
                }
                3 => {
                    for action in &mut app.actions {
                        for step in &mut action.steps {
                            if let Statement::ForEach { items, binding, .. } = step {
                                if binding == "job" {
                                    let Expr::Query { predicate, .. } = items else {
                                        panic!()
                                    };
                                    *predicate = Box::new(boolean(true));
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if case == 4 {
            let action = bb.actions.iter_mut().find(|a| a.id == "withdraw").unwrap();
            let Statement::ForEach { steps, .. } = &mut action.steps[0] else {
                panic!()
            };
            let Statement::ForEach { items, .. } = &mut steps[0] else {
                panic!()
            };
            let Expr::Query { predicate, .. } = items else {
                panic!()
            };
            *predicate = Box::new(eq(var("g"), var("paused_job")));
        }
        let a = capture(&aa);
        let b = capture(&bb);
        let (_dir, store) = setup(&a);
        let req = request(&store, &a, &b);
        assert!(verify(&store, &b, &req).is_err(), "case {case} admitted");
    }
}
#[test]
fn retained_sequence_tie_and_non_boolean_availability_are_refused() {
    let (a, b) = pair();
    let (_dir, store) = setup(&a);
    let mut snapshot = store.load().unwrap();
    let mut material = snapshot.data.records[0].clone();
    material.entity = "material".into();
    material.id = "retained-material".into();
    material.values = [
        (
            "name".into(),
            DataValue::Text {
                value: "paper".into(),
            },
        ),
        ("received".into(), DataValue::Boolean { value: true }),
    ]
    .into();
    snapshot.data.records.push(material);
    let mut event = snapshot.data.records[0].clone();
    event.entity = "event".into();
    event.id = "retained-event".into();
    event.values = [
        (
            "material".into(),
            DataValue::Reference {
                entity: "material".into(),
                record: "retained-material".into(),
            },
        ),
        ("sequence".into(), DataValue::Integer { value: 2 }),
        (
            "note".into(),
            DataValue::Text {
                value: "old".into(),
            },
        ),
    ]
    .into();
    snapshot.data.records.push(event);
    let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let recovered = ProductStore::create_recovered(dir.path().join("store"), &snapshot).unwrap();
    let req = request(&recovered, &a, &b);
    assert!(verify(&recovered, &b, &req).is_err());
    let mut app = b.program.clone();
    let ViewKind::List { columns, .. } = &mut app.views[0].kind else {
        panic!()
    };
    columns.last_mut().unwrap().value = text("unavailable?");
    let b = capture(&app);
    let req = request(&store, &a, &b);
    assert!(verify(&store, &b, &req).is_err());
}

#[test]
fn projection_recipe_distinguishes_nullable_column_from_added_count() {
    let (a, original) = pair();
    let mut app = original.program.clone();
    app.observables.push(ObservableDefinition {
        id: "pending_count".into(),
        label: "Pending".into(),
        value: count(query("job", "j", boolean(true))),
    });
    let b = capture(&app);
    let (_dir, store) = setup(&a);
    let req = request(&store, &a, &b);
    let proof = verify(&store, &b, &req).unwrap();
    let report = serde_json::to_value(proof.report()).unwrap();
    let ps = report["projections"].as_array().unwrap();
    let column = ps
        .iter()
        .find(|p| p["location"] == "view/jobs/pending")
        .unwrap();
    assert_eq!(
        column["host_value_type"],
        serde_json::to_value(Type::Optional {
            item: Box::new(Type::Boolean)
        })
        .unwrap()
    );
    assert_eq!(
        column["absent_value"],
        serde_json::to_value(DataValue::Null).unwrap()
    );
    let count = ps
        .iter()
        .find(|p| p["location"] == "count/pending_count")
        .unwrap();
    assert_eq!(
        count["host_value_type"],
        serde_json::to_value(Type::Boolean).unwrap()
    );
    assert_eq!(
        count["absent_value"],
        serde_json::to_value(DataValue::Boolean { value: false }).unwrap()
    );
}

#[test]
fn historical_time_and_session_reads_require_a_completed_constant_rule() {
    for expression in [
        Expr::Today,
        Expr::State {
            state: "name_input".into(),
        },
    ] {
        let (a, b) = pair();
        let mut aa = a.program.clone();
        let mut bb = b.program.clone();
        for app in [&mut aa, &mut bb] {
            let ViewKind::List { columns, .. } = &mut app.views[0].kind else {
                panic!()
            };
            columns.push(Column {
                id: "unstable".into(),
                label: "Unstable".into(),
                value: expression.clone(),
            });
        }
        let a = capture(&aa);
        let b = capture(&bb);
        let (_dir, store) = setup(&a);
        let req = request(&store, &a, &b);
        assert!(
            verify(&store, &b, &req).is_err(),
            "historical temporal/session column admitted"
        );
    }
}

#[test]
fn active_retained_outcomes_are_refused_instead_of_assuming_raw_correspondence() {
    use product_decisions::{Choice, DecisionEngine, IntentArchive, IntentionBinding};
    let (a, b) = pair();
    let (_dir, store) = setup(&a);
    let engine = DecisionEngine::new(
        product_runtime::LocalRuntime::default(),
        IntentArchive::new(store.clone()),
    );
    let mut scenario = witnesses(&store)[0].scenario.clone();
    scenario.inputs.insert(
        0,
        SemanticInput::Invoke {
            action: "add".into(),
            arguments: [(
                "title".into(),
                DataValue::Text {
                    value: "synthetic accepted scene".into(),
                },
            )]
            .into(),
        },
    );
    let scene = engine
        .accept_current_scene(&store.load().unwrap(), &scenario, Disclosure::Synthetic)
        .unwrap();
    let choice = Choice {
        id: "synthetic-retained-observation".into(),
        request: "Synthetic fixture requires exact raw row identity and Boolean schema".into(),
        rationale: None,
        scope: DecisionScope {
            operations: ["add".into()].into(),
            population: Population::All,
            conditions: Values::new(),
            excluded_records: vec![],
            unknowns: vec![],
        },
        outcome: DecisionOutcome::KeepCurrent,
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    };
    let prepared = engine
        .prepare_choice(&store, &a, choice, vec![scene], "synthetic-retention")
        .unwrap();
    engine.adopt(&store, &prepared).unwrap();
    assert_eq!(
        store.load().unwrap().decisions.decisions[0].status,
        DecisionStatus::Active
    );
    let r = request(&store, &a, &b);
    let error = verify(&store, &b, &r)
        .err()
        .expect("active raw outcome cannot be passed")
        .to_string();
    assert!(
        error.contains("active retained outcome/property comparison is unsupported"),
        "{error}"
    );
}

#[test]
fn common_factual_ui_enabled_must_not_depend_on_private_status() {
    let (a, b) = pair();
    let mut aa = a.program.clone();
    let mut bb = b.program.clone();
    for app in [&mut aa, &mut bb] {
        app.views[0].actions[0].enabled = eq(field("row", "status"), text("active"));
    }
    let a = capture(&aa);
    let b = capture(&bb);
    let (_dir, store) = setup(&a);
    let req = request(&store, &a, &b);
    assert!(
        verify(&store, &b, &req).is_err(),
        "UI policy-dependent common admission was accepted"
    );
}

#[test]
fn every_changed_observation_needs_an_exact_projection_recipe() {
    for case in 0..3 {
        let (a, b) = pair();
        let mut aa = a.program.clone();
        let mut bb = b.program.clone();
        match case {
            0 => {
                aa.observables.push(ObservableDefinition {
                    id: "raw".into(),
                    label: "Raw".into(),
                    value: int(1),
                });
                bb.observables.push(ObservableDefinition {
                    id: "raw".into(),
                    label: "Raw".into(),
                    value: int(2),
                });
            }
            1 => bb.observables.push(ObservableDefinition {
                id: "raw".into(),
                label: "Raw".into(),
                value: int(2),
            }),
            _ => {
                let ViewKind::List { columns, .. } = &mut bb.views[1].kind else {
                    panic!()
                };
                columns[0].value = field("row", "sequence");
            }
        }
        let a = capture(&aa);
        let b = capture(&bb);
        let (_dir, store) = setup(&a);
        let req = request(&store, &a, &b);
        assert!(
            verify(&store, &b, &req).is_err(),
            "unplanned passive change {case} was accepted"
        );
    }
}

#[test]
fn policy_selected_archived_root_update_cannot_reject_shared_facts() {
    let (a, b) = pair();
    let mut bb = b.program.clone();
    let action = bb.actions.iter_mut().find(|a| a.id == "withdraw").unwrap();
    let Statement::ForEach { items, steps, .. } = &mut action.steps[0] else {
        panic!()
    };
    let Expr::Query {
        include_archived, ..
    } = items
    else {
        panic!()
    };
    *include_archived = true;
    steps.push(Statement::Update {
        record: var("paused_job"),
        values: [("status".into(), text("active"))].into(),
    });
    let b = capture(&bb);
    let (_dir, store) = setup(&a);
    let mut snapshot = store.load().unwrap();
    let mut archived = snapshot
        .data
        .records
        .iter()
        .find(|r| r.entity == "job")
        .unwrap()
        .clone();
    archived.id = "archived-active".into();
    archived.archived = true;
    archived.values.insert(
        "status".into(),
        DataValue::Text {
            value: "active".into(),
        },
    );
    snapshot.data.records.push(archived);
    let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let recovered = ProductStore::create_recovered(dir.path().join("store"), &snapshot).unwrap();
    let req = request(&recovered, &a, &b);
    assert!(
        verify(&recovered, &b, &req).is_err(),
        "policy-selected archived write was accepted"
    );
}

#[test]
fn private_extra_writes_cannot_overflow_retained_revisions() {
    let (a, b) = pair();
    let mut bb = b.program.clone();
    let action = bb.actions.iter_mut().find(|a| a.id == "withdraw").unwrap();
    let Statement::ForEach { steps, .. } = &mut action.steps[0] else {
        panic!()
    };
    steps.push(Statement::Update {
        record: var("paused_job"),
        values: [("status".into(), text("active"))].into(),
    });
    let b = capture(&bb);
    let (_dir, store) = setup(&a);
    let mut snapshot = store.load().unwrap();
    let row = snapshot
        .data
        .records
        .iter_mut()
        .find(|r| {
            r.entity == "job"
                && r.values["status"]
                    != DataValue::Text {
                        value: "done".into(),
                    }
        })
        .unwrap();
    row.revision = u64::MAX - 1;
    row.values.insert(
        "status".into(),
        DataValue::Text {
            value: "active".into(),
        },
    );
    let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let recovered = ProductStore::create_recovered(dir.path().join("store"), &snapshot).unwrap();
    let req = request(&recovered, &a, &b);
    assert!(
        verify(&recovered, &b, &req).is_err(),
        "policy-selected revision overflow was accepted"
    );
}

#[test]
fn unchanged_root_selection_and_order_cannot_drift_completed_history() {
    for use_sort in [false, true] {
        let (a, b) = pair();
        let mut aa = a.program.clone();
        let mut bb = b.program.clone();
        for app in [&mut aa, &mut bb] {
            let ViewKind::List { rows, .. } = &mut app.views[0].kind else {
                panic!()
            };
            let Expr::Query {
                predicate, sort, ..
            } = rows
            else {
                panic!()
            };
            let shared = Expr::All {
                items: Box::new(query(
                    "requirement",
                    "link",
                    eq(field("link", "job"), var("j")),
                )),
                binding: "owned_link".into(),
                predicate: Box::new(Expr::Field {
                    record: Box::new(field("owned_link", "material")),
                    field: "received".into(),
                }),
            };
            if use_sort {
                sort.push(SortKey {
                    value: shared,
                    descending: false,
                });
            } else {
                *predicate = Box::new(shared);
            }
        }
        let a = capture(&aa);
        let b = capture(&bb);
        let (_dir, store) = setup(&a);
        let req = request(&store, &a, &b);
        assert!(
            verify(&store, &b, &req).is_err(),
            "completed row membership/order drift admitted: sort={use_sort}"
        );
    }
}

#[test]
fn policy_selected_unicode_lowercase_overflow_is_not_a_resource_exception() {
    let (a, b) = pair();
    let mut app = b.program.clone();
    let act = app.actions.iter_mut().find(|x| x.id == "register").unwrap();
    let Statement::ForEach { steps, .. } = &mut act.steps[2] else {
        panic!()
    };
    let Statement::Update { values, .. } = &mut steps[0] else {
        panic!()
    };
    let original = values["status"].clone();
    let bad = cond(
        eq(
            Expr::Lower {
                value: Box::new(text(&"İ".repeat(1366))),
            },
            text(""),
        ),
        text("ready"),
        text("waiting"),
    );
    values.insert(
        "status".into(),
        cond(eq(field("job", "status"), text("active")), bad, original),
    );
    let b = capture(&app);
    let (_dir, store) = setup(&a);
    let req = request(&store, &a, &b);
    assert!(
        verify(&store, &b, &req).is_err(),
        "Unicode expansion selected by private status was admitted"
    );
}

#[test]
fn proof_cancellation_is_checked_before_loading_the_store() {
    let (a, b) = pair();
    let (dir, store) = setup(&a);
    let req = request(&store, &a, &b);
    let accepted = acceptance(&store, &b, &req);
    std::fs::rename(
        dir.path().join("store/CURRENT"),
        dir.path().join("store/CURRENT.saved-for-test"),
    )
    .unwrap();
    let error = store
        .verify_owned_preview(
            &b,
            &req,
            &accepted,
            ProofBudget::default(),
            Arc::new(AtomicBool::new(true)),
        )
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("cancelled"),
        "store was loaded before cancellation: {error}"
    );
}

fn top_level_pair(
    first: &str,
    second: &str,
    declared: Option<&str>,
    extra: bool,
) -> (
    tempfile::TempDir,
    ProductStore,
    CapturedProgram,
    OwnedPreviewRequest,
) {
    let (a, b) = pair();
    let mut aa = a.program.clone();
    let mut bb = b.program.clone();
    for (app, target, label) in [(&mut aa, first, "before"), (&mut bb, second, "after")] {
        let mut steps = vec![update(var(target), vec![("title", text(label))])];
        if extra && label == "after" {
            steps.push(update(var("q"), vec![("title", text("extra"))]));
        }
        app.actions.push(action(
            "paired_roots",
            vec![("p", Type::reference("job")), ("q", Type::reference("job"))],
            vec![not(done("p")), not(done("q"))],
            steps,
        ));
    }
    let a = capture(&aa);
    let b = capture(&bb);
    let (dir, store) = setup(&a);
    store
        .apply(
            store.load().unwrap().revision,
            "second-live",
            &SemanticInput::Invoke {
                action: "add".into(),
                arguments: [(
                    "title".into(),
                    DataValue::Text {
                        value: "second live".into(),
                    },
                )]
                .into(),
            },
            RuntimeLimits::default(),
        )
        .unwrap();
    let mut req = request(&store, &a, &b);
    req.inventory.sources.edits.push(OwnedSpanEdit {
        id: "top_pair".into(),
        before: endpoint(&a, "paired_roots", vec![], 0, 1, declared),
        after: endpoint(
            &b,
            "paired_roots",
            vec![],
            0,
            if extra { 2 } else { 1 },
            declared,
        ),
    });
    (dir, store, b, req)
}
#[test]
fn paired_top_level_variant_cannot_switch_its_declared_root() {
    let (_dir, store, b, req) = top_level_pair("p", "q", Some("p"), false);
    assert!(
        verify(&store, &b, &req).is_err(),
        "paired edit changed an excluded root through another root's gate"
    );
}
#[test]
fn paired_top_level_variant_cannot_write_two_roots() {
    let (_dir, store, b, req) = top_level_pair("p", "p", Some("p"), true);
    assert!(
        verify(&store, &b, &req).is_err(),
        "single-root paired span wrote two roots"
    );
}
#[test]
fn paired_variant_without_a_single_root_decomposition_is_refused() {
    let (_dir, store, b, req) = top_level_pair("p", "q", None, false);
    assert!(
        verify(&store, &b, &req).is_err(),
        "unbound paired span gained ownership authority"
    );
}
#[test]
fn paired_top_level_variant_can_preserve_its_proved_root() {
    let (_dir, store, b, req) = top_level_pair("p", "p", Some("p"), false);
    verify(&store, &b, &req).unwrap();
}
#[test]
fn paired_unbound_traversals_cannot_choose_different_roots() {
    let (a, b) = pair();
    let mut aa = a.program.clone();
    let mut bb = b.program.clone();
    for (app, target) in [(&mut aa, "p"), (&mut bb, "q")] {
        app.actions.push(action(
            "paired_roots",
            vec![("p", Type::reference("job")), ("q", Type::reference("job"))],
            vec![not(done("p")), not(done("q"))],
            vec![Statement::ForEach {
                items: query("job", "j", eq(var("j"), var(target))),
                binding: "owned".into(),
                steps: vec![update(var("owned"), vec![("title", text("changed"))])],
            }],
        ));
    }
    let a = capture(&aa);
    let b = capture(&bb);
    let (_dir, store) = setup(&a);
    let mut req = request(&store, &a, &b);
    req.inventory.sources.edits.push(OwnedSpanEdit {
        id: "unbound_traversals".into(),
        before: endpoint(&a, "paired_roots", vec![], 0, 1, None),
        after: endpoint(&b, "paired_roots", vec![], 0, 1, None),
    });
    assert!(
        verify(&store, &b, &req).is_err(),
        "paired independent traversal frontiers were treated as the same root"
    );
}

#[test]
fn nested_auxiliary_creation_cannot_invalidate_an_observed_absence() {
    let (a, b) = pair();
    let mut app = b.program.clone();
    let withdraw = app.actions.iter_mut().find(|a| a.id == "withdraw").unwrap();
    let Statement::ForEach { steps, .. } = &mut withdraw.steps[0] else {
        panic!()
    };
    let Statement::ForEach { items, steps, .. } = &mut steps[0] else {
        panic!()
    };
    let nested = Statement::ForEach {
        items: items.clone(),
        binding: "nested_capture_gate".into(),
        steps: steps.clone(),
    };
    steps.insert(0, nested);
    let b = capture(&app);
    let (_dir, store) = setup(&a);
    let current = store.load().unwrap();
    let owner = current
        .data
        .records
        .iter()
        .find(|r| {
            r.entity == "job"
                && r.values["title"]
                    == DataValue::Text {
                        value: "live".into(),
                    }
        })
        .unwrap()
        .id
        .clone();
    store
        .apply(
            current.revision,
            "common-material",
            &SemanticInput::Invoke {
                action: "register".into(),
                arguments: [
                    (
                        "owner".into(),
                        DataValue::Reference {
                            entity: "job".into(),
                            record: owner.clone(),
                        },
                    ),
                    (
                        "name".into(),
                        DataValue::Text {
                            value: "paper".into(),
                        },
                    ),
                ]
                .into(),
            },
            RuntimeLimits::default(),
        )
        .unwrap();
    let snapshot = store.load().unwrap();
    let material = snapshot
        .data
        .records
        .iter()
        .find(|r| r.entity == "material")
        .unwrap()
        .id
        .clone();
    // These are synthetic standalone interpreter counterexample frames, not
    // accepted scenes or replacements for the authenticated admission basis.
    let mut seed = snapshot.data.clone();
    seed.records
        .iter_mut()
        .find(|r| r.id == material)
        .unwrap()
        .values
        .insert("received".into(), DataValue::Boolean { value: true });
    seed.records
        .iter_mut()
        .find(|r| r.id == owner)
        .unwrap()
        .values
        .insert(
            "status".into(),
            DataValue::Text {
                value: "active".into(),
            },
        );
    let runtime = product_runtime::LocalRuntime::default();
    let input = SemanticInput::Invoke {
        action: "withdraw".into(),
        arguments: [
            (
                "material".into(),
                DataValue::Reference {
                    entity: "material".into(),
                    record: material.clone(),
                },
            ),
            ("note".into(), DataValue::Text { value: "".into() }),
        ]
        .into(),
    };
    let mut run = runtime
        .start(
            &b,
            &seed,
            &snapshot.session,
            snapshot.clock_day,
            7,
            RuntimeLimits::default(),
        )
        .unwrap();
    let before_data = runtime.data(&run).clone();
    let before_session = runtime.session(&run).clone();
    let error = runtime
        .apply(&mut run, &input, "nested-absence")
        .unwrap_err();
    assert!(
        format!("{error:?}").to_lowercase().contains("unique"),
        "{error:?}"
    );
    assert_eq!(runtime.data(&run), &before_data);
    assert_eq!(runtime.session(&run), &before_session);
    assert!(run.artifacts().is_empty());
    let failed = run.trace().last().unwrap();
    assert_eq!(failed.before_data, failed.after_data);
    assert_eq!(failed.before_session, failed.after_session);
    // Changing only private status, with the same unfinished lifecycle facet,
    // skips the bad prelude and commits the common receipt/event transaction.
    seed.records
        .iter_mut()
        .find(|r| r.id == owner)
        .unwrap()
        .values
        .insert(
            "status".into(),
            DataValue::Text {
                value: "ready".into(),
            },
        );
    let mut control = runtime
        .start(
            &b,
            &seed,
            &snapshot.session,
            snapshot.clock_day,
            7,
            RuntimeLimits::default(),
        )
        .unwrap();
    runtime
        .apply(&mut control, &input, "nested-absence")
        .unwrap();
    assert_eq!(
        runtime
            .data(&control)
            .records
            .iter()
            .find(|r| r.id == material)
            .unwrap()
            .values["received"],
        DataValue::Boolean { value: false }
    );
    assert_eq!(
        runtime
            .data(&control)
            .records
            .iter()
            .filter(|r| r.entity == "event")
            .count(),
        1
    );
    assert_eq!(store.load().unwrap(), snapshot);
    let req = request(&store, &a, &b);
    assert!(
        verify(&store, &b, &req).is_err(),
        "nested producer invalidated the absence witness before a second Create"
    );
}

fn lifecycle_named_auxiliary_candidate(
    value: Expr,
    unrelated_alias: bool,
) -> (CapturedProgram, CapturedProgram) {
    let (a, b) = pair();
    let mut app = b.program.clone();
    app.entities
        .iter_mut()
        .find(|e| e.id == "pause")
        .unwrap()
        .fields
        .push(FieldDefinition {
            id: "status".into(),
            label: "Auxiliary status".into(),
            value_type: Type::Text,
        });
    let withdraw = app.actions.iter_mut().find(|a| a.id == "withdraw").unwrap();
    let Statement::ForEach { steps, .. } = &mut withdraw.steps[0] else {
        panic!()
    };
    let Statement::ForEach { steps, .. } = &mut steps[0] else {
        panic!()
    };
    let Statement::Create { values, .. } = &mut steps[0] else {
        panic!()
    };
    values.insert("status".into(), text("done"));
    let Statement::ForEach { steps, .. } = &mut withdraw.steps[3] else {
        panic!()
    };
    let Statement::ForEach { steps, .. } = &mut steps[1] else {
        panic!()
    };
    let assignment = update(var("job"), vec![("status", value)]);
    steps.push(if unrelated_alias {
        each(
            query("job", "foreign", boolean(true)),
            "other",
            vec![assignment],
        )
    } else {
        assignment
    });
    (a, capture(&app))
}

#[test]
fn lifecycle_named_auxiliary_field_cannot_complete_a_root_in_a_mixed_span() {
    let (a, b) = lifecycle_named_auxiliary_candidate(field("memory", "status"), false);
    let (_dir, store) = setup(&a);
    let current = store.load().unwrap();
    let owner = current
        .data
        .records
        .iter()
        .find(|r| {
            r.entity == "job"
                && r.values["title"]
                    == DataValue::Text {
                        value: "live".into(),
                    }
        })
        .unwrap()
        .id
        .clone();
    store
        .apply(
            current.revision,
            "lifecycle-material",
            &SemanticInput::Invoke {
                action: "register".into(),
                arguments: [
                    (
                        "owner".into(),
                        DataValue::Reference {
                            entity: "job".into(),
                            record: owner.clone(),
                        },
                    ),
                    (
                        "name".into(),
                        DataValue::Text {
                            value: "paper".into(),
                        },
                    ),
                ]
                .into(),
            },
            RuntimeLimits::default(),
        )
        .unwrap();
    let snapshot = store.load().unwrap();
    let material = snapshot
        .data
        .records
        .iter()
        .find(|r| r.entity == "material")
        .unwrap()
        .id
        .clone();
    // Standalone synthetic runtime frame demonstrates the incorrectly admitted
    // lifecycle transition; the authenticated live basis is never altered.
    let mut seed = snapshot.data.clone();
    seed.records
        .iter_mut()
        .find(|r| r.id == owner)
        .unwrap()
        .values
        .insert(
            "status".into(),
            DataValue::Text {
                value: "active".into(),
            },
        );
    seed.records
        .iter_mut()
        .find(|r| r.id == material)
        .unwrap()
        .values
        .insert("received".into(), DataValue::Boolean { value: true });
    let input = SemanticInput::Invoke {
        action: "withdraw".into(),
        arguments: [
            (
                "material".into(),
                DataValue::Reference {
                    entity: "material".into(),
                    record: material.clone(),
                },
            ),
            ("note".into(), DataValue::Text { value: "".into() }),
        ]
        .into(),
    };
    let runtime = product_runtime::LocalRuntime::default();
    for (program, expected_status) in [(&a, "waiting"), (&b, "done")] {
        let mut run = runtime
            .start(
                program,
                &seed,
                &snapshot.session,
                snapshot.clock_day,
                7,
                RuntimeLimits::default(),
            )
            .unwrap();
        runtime.apply(&mut run, &input, "lifecycle-alias").unwrap();
        let data = runtime.data(&run);
        assert_eq!(
            data.records.iter().find(|r| r.id == owner).unwrap().values["status"],
            DataValue::Text {
                value: expected_status.into()
            }
        );
        assert_eq!(
            data.records
                .iter()
                .find(|r| r.id == material)
                .unwrap()
                .values["received"],
            DataValue::Boolean { value: false }
        );
        assert_eq!(
            data.records.iter().filter(|r| r.entity == "event").count(),
            1
        );
    }
    assert_eq!(store.load().unwrap(), snapshot);
    let req = request(&store, &a, &b);
    assert!(
        verify(&store, &b, &req).is_err(),
        "same-owner auxiliary status was confused with the root lifecycle field"
    );
}

#[test]
fn lifecycle_field_on_the_exact_root_owner_alias_preserves_unfinished() {
    let (a, b) = lifecycle_named_auxiliary_candidate(get(field("memory", "job"), "status"), false);
    let (_dir, store) = setup(&a);
    let req = request(&store, &a, &b);
    verify(&store, &b, &req).unwrap();
}

#[test]
fn lifecycle_field_on_an_unrelated_root_alias_is_refused() {
    let (a, b) = lifecycle_named_auxiliary_candidate(field("other", "status"), true);
    let (_dir, store) = setup(&a);
    let req = request(&store, &a, &b);
    assert!(verify(&store, &b, &req).is_err());
}
