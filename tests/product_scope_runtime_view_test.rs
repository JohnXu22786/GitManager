//! Production interpreter/store/decision integration; no live-provider claims.
#[path = "support/egui_harness.rs"]
mod egui_harness;
#[path = "fixtures/product_scope/mod.rs"]
mod fixture;
#[path = "../src/product_backup.rs"]
mod product_backup;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_decisions/mod.rs"]
mod product_decisions;
#[path = "../src/product_discovery/mod.rs"]
mod product_discovery;
#[path = "../src/product_locations.rs"]
mod product_locations;
#[path = "../src/product_protocol.rs"]
mod product_protocol;
#[path = "../src/product_provider/mod.rs"]
mod product_provider;
#[path = "../src/product_runtime/mod.rs"]
mod product_runtime;
#[path = "../src/ui/product_runtime_view.rs"]
mod product_runtime_view;
#[path = "../src/product_scenarios/mod.rs"]
mod product_scenarios;
#[path = "../src/product_store/mod.rs"]
mod product_store;
use fixture::*;
use product_contract::*;
use product_decisions::*;
use product_runtime::LocalRuntime;
use product_store::{scope::*, ProductStore};

fn selected_scene(prepared: &PreparedScopedChange, row: &Record) -> ScenarioSpec {
    ScenarioSpec {
        version: 1,
        id: "accepted-scope".into(),
        label: "Pause production and preserve commitment".into(),
        seed: prepared.seed().clone(),
        session: SessionState::initial(&prepared.target().program).unwrap(),
        clock_day: 20003,
        random_seed: 42,
        inputs: vec![
            invoke("calculate", &[("row", reference(row))]),
            invoke("complete", &[("row", reference(row))]),
            invoke("export", &[]),
            SemanticInput::Observe {
                point: "result".into(),
            },
        ],
        validity: vec![],
    }
}
#[test]
fn scoped_choice_replays_after_restart_backup_and_current_data_withdrawal() {
    let dir = tempdir();
    let path = dir.path().join("tool");
    let store = ProductStore::create(&path, &program(false), 20000).unwrap();
    let selected = add(&store, "selected", "Selected");
    action(&store, "wait", "wait", &selected);
    tick(&store, "days", 20003);
    let before = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(
            &program(true),
            &request(
                &before,
                ScopePopulation::SelectedUnfinished {
                    records: vec![RecordRef {
                        entity: selected.entity.clone(),
                        record: selected.id.clone(),
                    }],
                },
            ),
            "scope",
        )
        .unwrap();
    let layer = prepared.layer_id().unwrap().unwrap();
    let scenario = selected_scene(&prepared, &selected);
    assert!(accept_scene(
        &LocalRuntime::default(),
        prepared.target(),
        &scenario,
        Disclosure::Synthetic,
        RuntimeLimits::default()
    )
    .is_err());
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let accepted = engine
        .accept_scoped_scene(&store, &prepared, &scenario, Disclosure::Synthetic)
        .unwrap();
    let choice = Choice {
        id: "pause-production".into(),
        request: "Pause production while waiting; retain the customer commitment and follow-up"
            .into(),
        rationale: None,
        scope: prepared.scope().clone(),
        outcome: DecisionOutcome::Accept {
            artifact: prepared.target().artifact.program_digest.clone(),
        },
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    };
    let change = engine
        .prepare_scoped_choice(&store, prepared, choice, vec![accepted], "scope")
        .unwrap();
    assert_eq!(change.report().disposition, CheckDisposition::Ready);
    let adopted = engine.adopt(&store, &change).unwrap();
    assert_eq!(
        adopted.scope.adoptions[0].decisions,
        vec!["pause-production"]
    );
    let late = add(&store, "late", "Later independent work");
    action(&store, "complete-live", "complete", &selected);
    apply(&store, "export-live", invoke("export", &[]));
    let current = store.load().unwrap();
    assert_eq!(
        engine.check_current(&current).unwrap().disposition,
        CheckDisposition::Ready
    );
    let backup = product_backup::VerifiedBackup::capture(&store).unwrap();
    let verified = product_backup::VerifiedBackup::from_bytes(&backup.to_bytes().unwrap()).unwrap();
    let recovery = dir.path().join("recovered");
    let recovered = verified.recover_new(&recovery).unwrap();
    let recovered_engine = DecisionEngine::new(
        LocalRuntime::default(),
        IntentArchive::new(recovered.clone()),
    );
    assert_eq!(
        recovered_engine
            .check_current(&recovered.load().unwrap())
            .unwrap()
            .disposition,
        CheckDisposition::Ready
    );
    let withdrawal = recovered_engine
        .prepare_scoped_withdrawal(&recovered, &[layer], "withdraw")
        .unwrap();
    let withdrawn = recovered_engine.adopt(&recovered, &withdrawal).unwrap();
    assert_eq!(withdrawn.data.records, current.data.records);
    assert_eq!(withdrawn.data.events, current.data.events);
    assert_eq!(withdrawn.artifacts, current.artifacts);
    assert!(matches!(
        withdrawn.decisions.decisions[0].status,
        DecisionStatus::Withdrawn { .. }
    ));
    action(&recovered, "continued", "wait", &late);
    assert_eq!(store.load().unwrap(), current);
}
#[test]
fn scoped_replay_rejects_removed_provenance_and_reports_saved_origin() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let row = add(&store, "old", "Completed");
    action(&store, "finish", "complete", &row);
    let before = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(
            &program(true),
            &request(&before, ScopePopulation::FutureWork),
            "scope",
        )
        .unwrap();
    let context = ScopedExecutionContext::prepared(&before, &prepared).unwrap();
    let mut seed = prepared.seed().clone();
    seed.records[0]
        .values
        .retain(|key, _| !key.starts_with("gm_scope_"));
    assert!(context
        .verify_seed(prepared.target(), &seed, 20000)
        .is_err());
    assert!(context
        .project_seed(&program(false), &seed, prepared.target(), &seed, 20000)
        .is_err());
    store.adopt_scoped(before.revision, &prepared).unwrap();
    let view = store.runtime_view().unwrap();
    assert!(!view.history.results.is_empty());
    assert!(view
        .history
        .results
        .iter()
        .any(|fact| fact.origin == product_protocol::HistoryOrigin::CapturedAtAdoption));
    let mut renderer = product_runtime_view::ProductRuntimeView::default();
    let mut harness = egui_harness::EguiHarness::new(egui::vec2(1200.0, 1800.0));
    let output = harness.frame(|ctx| {
        egui::CentralPanel::default()
            .show(ctx, |ui| {
                renderer.show(ui, &view, "scope-history", true, true, &[])
            })
            .inner
    });
    assert!(output
        .trace
        .text
        .iter()
        .any(|text| text.contains("earlier completion value was not reconstructed")));
    assert_eq!(store.load().unwrap().data.events, before.data.events);
}

#[test]
fn scoped_comparison_cannot_drop_protected_seed_or_claim_unguarded_evidence() {
    use std::sync::{atomic::AtomicBool, Arc};
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let row = add(&store, "selected", "Selected");
    action(&store, "wait", "wait", &row);
    tick(&store, "days", 20003);
    let current = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(
            &program(true),
            &request(
                &current,
                ScopePopulation::SelectedUnfinished {
                    records: vec![RecordRef {
                        entity: row.entity.clone(),
                        record: row.id.clone(),
                    }],
                },
            ),
            "scope",
        )
        .unwrap();
    let scene = selected_scene(&prepared, &row);
    let context = ScopedExecutionContext::prepared(&current, &prepared).unwrap();
    let target = product_scenarios::ObservationTarget::ViewColumn {
        point: "result".into(),
        column: "production".into(),
    };
    let comparison = product_scenarios::ComparisonEngine::new(Arc::new(AtomicBool::new(false)));
    let rejected = comparison
        .compare(
            current.program().unwrap(),
            prepared.target(),
            &scene,
            &current.decisions,
            target.clone(),
            RuntimeLimits::default(),
        )
        .unwrap();
    assert_eq!(rejected.state, EvidenceState::Unsupported);
    assert!(rejected.witness.is_none());
    let comparison = comparison.with_admission(Arc::new(context));
    let accepted = comparison
        .compare(
            current.program().unwrap(),
            prepared.target(),
            &scene,
            &current.decisions,
            target,
            RuntimeLimits::default(),
        )
        .unwrap();
    assert_eq!(accepted.state, EvidenceState::Observed);
    assert!(accepted.witness.is_some());
    for (_, reduced) in comparison
        .single_reductions(current.program().unwrap(), prepared.target(), &scene)
        .unwrap()
    {
        assert_eq!(reduced.seed.records, scene.seed.records);
    }
}

#[test]
fn concrete_scoped_intention_survives_a_mapped_new_implementation() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let selected = add(&store, "selected", "Selected");
    action(&store, "wait", "wait", &selected);
    tick(&store, "days", 20003);
    let before = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(
            &program(true),
            &request(
                &before,
                ScopePopulation::SelectedUnfinished {
                    records: vec![RecordRef {
                        entity: selected.entity.clone(),
                        record: selected.id.clone(),
                    }],
                },
            ),
            "scope",
        )
        .unwrap();
    let layer = prepared.layer_id().unwrap().unwrap();
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let accepted = engine
        .accept_scoped_scene(
            &store,
            &prepared,
            &selected_scene(&prepared, &selected),
            Disclosure::Synthetic,
        )
        .unwrap();
    let choice = Choice {
        id: "production-promise".into(),
        request: "Pause production, keep the commitment and reminder".into(),
        rationale: None,
        scope: prepared.scope().clone(),
        outcome: DecisionOutcome::Accept {
            artifact: prepared.target().artifact.program_digest.clone(),
        },
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    };
    let change = engine
        .prepare_scoped_choice(&store, prepared, choice, vec![accepted], "scope")
        .unwrap();
    engine.adopt(&store, &change).unwrap();
    let mut source = serde_json::to_value(program(true).program).unwrap();
    for pointer in [
        "/actions/3/steps/0/values/production",
        "/actions/4/steps/0/values/production",
        "/actions/6/steps/0/columns/production",
        "/views/0/kind/columns/1/value",
    ] {
        let expression = source.pointer(pointer).unwrap().clone();
        *source.pointer_mut(pointer).unwrap() = serde_json::json!({"kind":"if","condition":boolean(true),"then_value":expression,"else_value":int(0)});
    }
    source["entities"][0]["fields"].as_array_mut().unwrap().push(serde_json::json!({"id":"note","label":"Later work note","value_type":{"kind":"optional","item":{"kind":"text"}}}));
    source["actions"].as_array_mut().unwrap().push(serde_json::json!({"id":"annotate","label":"Record a note","parameters":{"row":{"kind":"reference","entity":"job"}},"guards":[],"steps":[{"kind":"update","record":var("row"),"values":{"note":lit(text("Later legitimate fact"),Type::Text)}}],"ensures":[]}));
    let target = capture(source);
    let current = store.load().unwrap();
    let source_id = canonical_digest(IdentityDomain::Source, &target).unwrap();
    let mappings: Vec<_> = current.scope.layers[&layer]
        .patches
        .iter()
        .enumerate()
        .map(|(patch, p)| ScopeSlotMapping {
            layer: layer.clone(),
            patch,
            from_source: current.active_revision.clone(),
            from: p.request.destination.clone(),
            to_source: source_id.clone(),
            to: p.request.destination.clone(),
            subject: p.request.subject.clone(),
        })
        .collect();
    let managed = store
        .prepare_managed_evolution(&target, &mappings, "fresh-implementation")
        .unwrap();
    let evolved_scene = selected_scene(&managed, &selected);
    ScopedExecutionContext::prepared(&current, &managed)
        .unwrap()
        .verify_seed(managed.target(), managed.seed(), evolved_scene.clock_day)
        .unwrap();
    let evolved_accepted = engine
        .accept_scoped_scene(&store, &managed, &evolved_scene, Disclosure::Synthetic)
        .unwrap();
    verify_managed_discovery(&store, &engine, &managed);
    let change = engine
        .prepare_managed_change(&store, managed, &[], "fresh-implementation")
        .unwrap();
    assert_eq!(change.report().disposition, CheckDisposition::Ready);
    engine.adopt(&store, &change).unwrap();
    let adopted = store.load().unwrap();
    let retained = Choice {
        id: "evolved-seed".into(),
        request: "Retain the experienced updated tool".into(),
        rationale: None,
        scope: DecisionScope {
            operations: ["calculate".into(), "complete".into(), "export".into()]
                .into_iter()
                .collect(),
            population: Population::All,
            conditions: Values::new(),
            excluded_records: vec![],
            unknowns: vec![],
        },
        outcome: DecisionOutcome::KeepCurrent,
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    };
    let retained = engine
        .prepare_choice(
            &store,
            adopted.program().unwrap(),
            retained,
            vec![evolved_accepted],
            "save-evolved-scene",
        )
        .unwrap();
    engine.adopt(&store, &retained).unwrap();
    let annotated = action(&store, "new-fact", "annotate", &selected);
    assert_eq!(
        engine.check_current(&annotated).unwrap().disposition,
        CheckDisposition::Ready
    );
    product_backup::VerifiedBackup::capture(&store).unwrap();

    let retired = engine
        .prepare_withdrawal(
            &store,
            annotated.program().unwrap(),
            &["evolved-seed".into()],
            "retire-evolved-scene",
        )
        .unwrap();
    engine.adopt(&store, &retired).unwrap();
    let withdrawal = engine
        .prepare_scoped_withdrawal(&store, &[layer], "withdraw")
        .unwrap();
    let withdrawn = engine.adopt(&store, &withdrawal).unwrap();
    assert_eq!(withdrawn.data.records, annotated.data.records);
    assert_eq!(
        row(&withdrawn, &selected).values["note"],
        text("Later legitimate fact")
    );
    action(&store, "continue", "annotate", &selected);
}

fn verify_managed_discovery(
    store: &ProductStore,
    engine: &DecisionEngine<LocalRuntime>,
    prepared: &PreparedScopedChange,
) {
    use product_discovery::{discover, DiscoveryPolicy, VerifiedRetainedHistory};
    use std::sync::{atomic::AtomicBool, Arc};
    let current = store.load().unwrap();
    let request = engine
        .inherit_request(
            &current,
            DevelopmentRequest {
                version: 1,
                id: "scoped-discovery".into(),
                project_id: current.data.project_id.clone(),
                operation: DevelopmentOperation::Discover,
                request: "Keep the production rule in this different implementation".into(),
                sources: vec![
                    current.program().unwrap().clone(),
                    prepared.target().clone(),
                ],
                context: DevelopmentContext {
                    view: Some("work".into()),
                    selected: vec![],
                    recent_inputs: vec![],
                    data_digest: Some(current.data.identity().unwrap()),
                    session_digest: Some(current.session.identity().unwrap()),
                },
                examples: vec![],
                accepted_scenes: vec![],
                decisions: current.decisions.clone(),
                unknowns: vec![],
                required_capabilities: Default::default(),
            },
        )
        .unwrap();
    let response = |request: &DevelopmentRequest| DevelopmentResult {
        producer: Producer::Fixture {
            name: "scope inheritance response".into(),
        },
        response: DevelopmentResponse {
            version: 1,
            request_digest: request.identity().unwrap(),
            candidates: vec![GeneratedCandidate {
                id: "candidate".into(),
                source_json: String::from_utf8(request.sources[1].source_bytes.clone()).unwrap(),
            }],
            hypotheses: vec![],
            evolutions: vec![],
            unsupported: vec![],
        },
    };
    let mut history = VerifiedRetainedHistory::load(store).unwrap();
    let naked = discover(
        &request,
        &response(&request),
        &DiscoveryPolicy {
            retained_history: Some(history.clone()),
            ..Default::default()
        },
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert!(!naked.unverified.is_empty());
    assert!(naked.questions.is_empty());
    history
        .map_prepared_target(prepared.clone(), vec![])
        .unwrap();
    let policy = DiscoveryPolicy {
        retained_history: Some(history.clone()),
        ..Default::default()
    };
    let observed = discover(
        &request,
        &response(&request),
        &policy,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert!(observed.unverified.is_empty(), "{:?}", observed.unverified);
    assert!(observed.questions.is_empty());
    assert!(observed
        .checks
        .iter()
        .any(|c| c.state == CheckState::Satisfied));
    assert!(observed
        .runs
        .iter()
        .any(|r| r.binding.artifact == prepared.target().artifact
            && r.state == EvidenceState::Observed));
    let mut tampered = serde_json::to_value(prepared.target().program.clone()).unwrap();
    tampered["label"] = serde_json::json!("Unauthenticated edited envelope");
    let target = CapturedProgram::capture(
        &serde_json::to_vec(&tampered).unwrap(),
        &current.data.project_id,
        prepared.target().binding.producer.clone(),
        None,
    )
    .unwrap();
    let mut request_tampered = request.clone();
    request_tampered.sources[1] = target;
    let blocked = discover(
        &request_tampered,
        &response(&request_tampered),
        &policy,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert!(!blocked.unverified.is_empty());
    assert!(blocked.questions.is_empty());
}

#[test]
fn prepared_discovery_authority_goes_stale_when_daily_data_changes() {
    use product_discovery::VerifiedRetainedHistory;
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let before = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(
            &program(true),
            &request(&before, ScopePopulation::FutureWork),
            "scope",
        )
        .unwrap();
    let mut history = VerifiedRetainedHistory::load(&store).unwrap();
    history
        .map_prepared_target(prepared.clone(), vec![])
        .unwrap();
    add(&store, "later", "A real later edit");
    assert!(history
        .map_prepared_target(prepared.clone(), vec![])
        .is_err());
    assert!(ScopedExecutionContext::prepared(&store.load().unwrap(), &prepared).is_err());
    assert!(store.adopt_scoped(before.revision, &prepared).is_err());
}

#[test]
fn future_scoped_scene_uses_real_creation_provenance_for_later_operations() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let before = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(
            &program(true),
            &request(&before, ScopePopulation::FutureWork),
            "future-rule",
        )
        .unwrap();
    let mut scene = ScenarioSpec {
        version: 1,
        id: "future-scene".into(),
        label: "Work created after this change".into(),
        seed: prepared.seed().clone(),
        session: SessionState::initial(&prepared.target().program).unwrap(),
        clock_day: 20000,
        random_seed: 42,
        inputs: vec![invoke(
            "add",
            &[
                ("name", text("Future scene")),
                ("promised", DataValue::Date { days: 20020 }),
            ],
        )],
        validity: vec![],
    };
    let runtime = LocalRuntime::default();
    let mut run = runtime
        .start(
            prepared.target(),
            &scene.seed,
            &scene.session,
            scene.clock_day,
            scene.random_seed,
            RuntimeLimits::default(),
        )
        .unwrap();
    runtime
        .apply(
            &mut run,
            &scene.inputs[0],
            &scene.replay_operation_ids().unwrap()[0],
        )
        .unwrap();
    let row = runtime.data(&run).records[0].clone();
    scene.inputs.extend([
        invoke("wait", &[("row", reference(&row))]),
        SemanticInput::AdvanceClock { days: 3 },
        invoke("calculate", &[("row", reference(&row))]),
        invoke("complete", &[("row", reference(&row))]),
        invoke("export", &[]),
        SemanticInput::Observe {
            point: "result".into(),
        },
    ]);
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let accepted = engine
        .accept_scoped_scene(&store, &prepared, &scene, Disclosure::Synthetic)
        .unwrap();
    let choice = Choice {
        id: "future-production".into(),
        request: "Use paused production timing for later work".into(),
        rationale: None,
        scope: prepared.scope().clone(),
        outcome: DecisionOutcome::Accept {
            artifact: prepared.target().artifact.program_digest.clone(),
        },
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    };
    let change = engine
        .prepare_scoped_choice(&store, prepared, choice, vec![accepted], "future-rule")
        .unwrap();
    let adopted = engine.adopt(&store, &change).unwrap();
    assert_eq!(
        engine.check_current(&adopted).unwrap().disposition,
        CheckDisposition::Ready
    );
}

#[test]
fn a_retired_intention_does_not_trap_its_remaining_behavior_layer() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let selected = add(&store, "selected", "Selected");
    action(&store, "wait", "wait", &selected);
    tick(&store, "days", 20003);
    let before = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(
            &program(true),
            &request(
                &before,
                ScopePopulation::SelectedUnfinished {
                    records: vec![RecordRef {
                        entity: selected.entity.clone(),
                        record: selected.id.clone(),
                    }],
                },
            ),
            "scope",
        )
        .unwrap();
    let layer = prepared.layer_id().unwrap().unwrap();
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let accepted = engine
        .accept_scoped_scene(
            &store,
            &prepared,
            &selected_scene(&prepared, &selected),
            Disclosure::Synthetic,
        )
        .unwrap();
    let choice = Choice {
        id: "timing".into(),
        request: "Retain timing".into(),
        rationale: None,
        scope: prepared.scope().clone(),
        outcome: DecisionOutcome::Accept {
            artifact: prepared.target().artifact.program_digest.clone(),
        },
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    };
    let change = engine
        .prepare_scoped_choice(&store, prepared, choice, vec![accepted], "scope")
        .unwrap();
    let current = engine.adopt(&store, &change).unwrap();
    let retired = engine
        .prepare_withdrawal(
            &store,
            current.program().unwrap(),
            &["timing".into()],
            "retire-promise",
        )
        .unwrap();
    let retired = engine.adopt(&store, &retired).unwrap();
    let old_decision = retired.decisions.decisions[0].clone();
    let withdrawal = engine
        .prepare_scoped_withdrawal(&store, &[layer], "withdraw-behavior")
        .unwrap();
    let withdrawn = engine.adopt(&store, &withdrawal).unwrap();
    assert_eq!(withdrawn.decisions.decisions[0], old_decision);
    assert_eq!(withdrawn.data, retired.data);
    assert!(withdrawn.scope.compositions[&withdrawn.active_revision]
        .active
        .is_empty());
}

#[test]
fn fresh_development_receives_honest_editable_source_and_exact_slots() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let before = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(
            &program(true),
            &request(&before, ScopePopulation::FutureWork),
            "scope",
        )
        .unwrap();
    store.adopt_scoped(before.revision, &prepared).unwrap();
    let current = store.load().unwrap();
    let editable = current.editable_scope_context().unwrap().unwrap();
    assert_eq!(editable.compiled_source, current.active_revision);
    assert!(!product_runtime::has_protected_fields(&editable.editable));
    assert_eq!(editable.editable.program, program(true).program);
    assert!(
        matches!(&editable.editable.binding.producer, Producer::ExternalAuthor{description} if description.contains("editable"))
    );
    assert_eq!(editable.slots.len(), 4);
    let layer = prepared.layer_id().unwrap().unwrap();
    for (index, slot) in editable.slots.iter().enumerate() {
        assert_eq!(slot.layer, layer);
        assert_eq!(slot.patch, index);
        assert_eq!(
            slot.destination,
            current.scope.layers[&layer].patches[index]
                .request
                .destination
        );
        assert!(slot.active);
    }
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let context = DevelopmentContext {
        view: Some("work".into()),
        selected: vec![],
        recent_inputs: vec![],
        data_digest: Some(current.data.identity().unwrap()),
        session_digest: Some(current.session.identity().unwrap()),
    };
    let inherited = engine
        .development_request(
            &current,
            "fresh",
            DevelopmentOperation::Modify,
            "Add an optional work note",
            context.clone(),
        )
        .unwrap();
    assert_eq!(inherited.sources.last(), Some(current.program().unwrap()));
    assert!(inherited.sources.iter().any(|p| p == &editable.editable));
    assert!(inherited
        .request
        .contains(editable.compiled_source.as_str()));
    assert!(inherited.request.contains("pre-instrumentation"));
    assert!(!inherited.request.contains("FrozenBasis"));
    let reconciled = engine
        .development_request(
            &current,
            "reconcile",
            DevelopmentOperation::Reconcile,
            "Keep the recorded promise",
            context.clone(),
        )
        .unwrap();
    reconciled.validate().unwrap();
    assert_eq!(reconciled.sources.last(), Some(current.program().unwrap()));
    assert!(reconciled.sources.contains(&editable.editable));
    let mut discovery = inherited.clone();
    discovery.operation = DevelopmentOperation::Discover;
    discovery.request = "Inspect the exact pair".into();
    discovery.sources = vec![
        current.program().unwrap().clone(),
        current.program().unwrap().clone(),
    ];
    let discovery = engine.inherit_request(&current, discovery).unwrap();
    discovery.validate().unwrap();
    assert_eq!(discovery.sources.len(), 2);
    assert!(discovery.request.contains("pre-instrumentation"));
    assert!(engine
        .development_request(
            &current,
            "too-long",
            DevelopmentOperation::Modify,
            &"x".repeat(MAX_TEXT_BYTES),
            context
        )
        .is_err());
    let mut crowded = inherited.clone();
    crowded.request = "Keep all supplied sources".into();
    crowded.sources.clear();
    for index in 0..7 {
        let mut source = serde_json::to_value(program(false).program).unwrap();
        source["label"] = serde_json::json!(format!("Historical source {index}"));
        crowded.sources.push(capture(source));
    }
    crowded.sources.push(current.program().unwrap().clone());
    assert!(engine.inherit_request(&current, crowded).is_err());
}

#[test]
fn discovery_admits_the_matching_second_layer_seed_for_both_sources() {
    use product_discovery::{discover, DiscoveryPolicy, VerifiedRetainedHistory};
    use std::sync::{atomic::AtomicBool, Arc};
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let job = add(&store, "job", "Waiting work");
    action(&store, "wait", "wait", &job);
    let before = store.load().unwrap();
    let first = store
        .prepare_scoped_change(
            &program(true),
            &request(&before, ScopePopulation::All),
            "first",
        )
        .unwrap();
    let current = store.adopt_scoped(before.revision, &first).unwrap();
    let mut raw = serde_json::to_value(program(true).program).unwrap();
    raw["views"][0]["kind"]["columns"][3]["value"] = serde_json::to_value(boolean(false)).unwrap();
    raw["actions"][6]["steps"][0]["columns"]["reminder"] =
        serde_json::to_value(boolean(false)).unwrap();
    let candidate = capture(raw);
    let request_scope = ScopeRequest {
        population: ScopePopulation::All,
        operations: ["export".into()].into_iter().collect(),
        excluded_records: vec![],
        lifecycles: vec![LifecycleBinding {
            entity: "job".into(),
            completed: field("record", "done"),
            source: current.active_revision.clone(),
        }],
        patches: vec![
            EffectPatchRequest {
                destination: EffectDestination::ViewColumn {
                    view: "work".into(),
                    column: "reminder".into(),
                },
                entity: "job".into(),
                subject: "row".into(),
                value_type: Type::Boolean,
            },
            EffectPatchRequest {
                destination: EffectDestination::EmitColumn {
                    action: "export".into(),
                    path: vec![0],
                    column: "reminder".into(),
                },
                entity: "job".into(),
                subject: "row".into(),
                value_type: Type::Boolean,
            },
        ],
    };
    let second = store
        .prepare_scoped_change(&candidate, &request_scope, "second")
        .unwrap();
    assert_ne!(
        second.seed().identity().unwrap(),
        current.data.identity().unwrap()
    );
    assert!(ScopedExecutionContext::committed(&current)
        .unwrap()
        .verify_seed(current.program().unwrap(), second.seed(), 20000)
        .is_err());
    ScopedExecutionContext::prepared(&current, &second)
        .unwrap()
        .verify_seed(current.program().unwrap(), second.seed(), 20000)
        .unwrap();
    let scene = ScenarioSpec {
        version: 1,
        id: "reminders".into(),
        label: "Waiting reminders".into(),
        seed: second.seed().clone(),
        session: current.session.clone(),
        clock_day: 20000,
        random_seed: 42,
        inputs: vec![
            invoke("export", &[]),
            SemanticInput::Observe {
                point: "result".into(),
            },
        ],
        validity: vec![],
    };
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let request = engine
        .inherit_request(
            &current,
            DevelopmentRequest {
                version: 1,
                id: "two-layer-discovery".into(),
                project_id: current.data.project_id.clone(),
                operation: DevelopmentOperation::Discover,
                request: "Compare waiting reminders".into(),
                sources: vec![current.program().unwrap().clone(), second.target().clone()],
                context: DevelopmentContext {
                    view: Some("work".into()),
                    selected: vec![],
                    recent_inputs: vec![],
                    data_digest: Some(current.data.identity().unwrap()),
                    session_digest: Some(current.session.identity().unwrap()),
                },
                examples: vec![],
                accepted_scenes: vec![],
                decisions: current.decisions.clone(),
                unknowns: vec![],
                required_capabilities: Default::default(),
            },
        )
        .unwrap();
    let response = DevelopmentResult {
        producer: Producer::Fixture {
            name: "two-layer alternatives".into(),
        },
        response: DevelopmentResponse {
            version: 1,
            request_digest: request.identity().unwrap(),
            candidates: vec![
                GeneratedCandidate {
                    id: "before".into(),
                    source_json: String::from_utf8(current.program().unwrap().source_bytes.clone())
                        .unwrap(),
                },
                GeneratedCandidate {
                    id: "after".into(),
                    source_json: String::from_utf8(second.target().source_bytes.clone()).unwrap(),
                },
            ],
            hypotheses: vec![ChoiceHypothesis {
                id: "reminder-choice".into(),
                statement: "Waiting reminders may remain visible or be hidden".into(),
                kind: HypothesisKind::UnresolvedChoice,
                action: "export".into(),
                observable: "waiting_jobs".into(),
                sources: vec![SourceLocus {
                    relative_path: second.target().binding.program_path.clone(),
                    raw_digest: second.target().artifact.raw_digest.clone(),
                    pointer: "/views/0/kind/columns/3/value".into(),
                }],
                alternatives: vec!["before".into(), "after".into()],
                related_decisions: vec![],
                scenario_json: serde_json::to_string(&scene).unwrap(),
                unknowns: vec![],
            }],
            evolutions: vec![],
            unsupported: vec![],
        },
    };
    let mut history = VerifiedRetainedHistory::load(&store).unwrap();
    history.map_prepared_target(second.clone(), vec![]).unwrap();
    let report = discover(
        &request,
        &response,
        &DiscoveryPolicy {
            retained_history: Some(history),
            ..Default::default()
        },
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    for source in [current.program().unwrap(), second.target()] {
        assert!(
            report
                .runs
                .iter()
                .any(|run| run.binding.artifact == source.artifact
                    && run.binding.data_digest == scene.seed.identity().unwrap()
                    && run.state == EvidenceState::Observed),
            "{:?}",
            report
        );
    }
    assert!(!report.questions.is_empty(), "{:?}", report);
    let adopted = store.adopt_scoped(current.revision, &second).unwrap();
    let completed = action(&store, "complete-two-layers", "complete", &job);
    assert!(completed.data.generation > adopted.data.generation);
    let context = ScopedExecutionContext::committed(&completed).unwrap();
    context
        .verify_seed(
            current.program().unwrap(),
            &completed.data,
            completed.clock_day,
        )
        .unwrap();
    let historical = ScenarioSpec {
        seed: completed.data.clone(),
        session: completed.session.clone(),
        clock_day: completed.clock_day,
        ..scene
    };
    let run = LocalRuntime::default()
        .replay_admitted(
            current.program().unwrap(),
            &historical,
            &completed.decisions,
            RuntimeLimits::default(),
            "earlier-envelope",
            Some(&context),
        )
        .unwrap();
    assert_eq!(run.state, EvidenceState::Observed);
}

#[test]
fn independent_promises_cross_only_authenticated_layer_initialization() {
    fn keep_commitment(store: &ProductStore, engine: &DecisionEngine<LocalRuntime>, id: &str) {
        let current = store.load().unwrap();
        let scene = ScenarioSpec {
            version: 1,
            id: id.into(),
            label: "Customer commitment remains fixed".into(),
            seed: current.data.clone(),
            session: current.session.clone(),
            clock_day: current.clock_day,
            random_seed: 42,
            inputs: vec![
                invoke("export", &[]),
                SemanticInput::Observe {
                    point: "result".into(),
                },
            ],
            validity: vec![],
        };
        let accepted = engine
            .accept_current_scene(&current, &scene, Disclosure::Synthetic)
            .unwrap();
        // This is explicitly an independent property promise, not an inferred
        // downgrade of any concrete chosen timing or reminder outcome.
        let choice = Choice {
            id: id.into(),
            request: "Keep this customer's original promised date".into(),
            rationale: None,
            scope: DecisionScope {
                operations: ["export".into()].into_iter().collect(),
                population: Population::All,
                conditions: Values::new(),
                excluded_records: vec![],
                unknowns: vec![],
            },
            outcome: DecisionOutcome::KeepCurrent,
            obligations: vec![AcceptedProperty {
                id: format!("date-{id}"),
                description: "The promised date remains unchanged".into(),
                predicate: PropertyPredicate::Equal {
                    left: PropertyTerm::OutputColumn {
                        point: "result".into(),
                        output: "sheet".into(),
                        column: "promised".into(),
                        value_type: Type::Date,
                    },
                    right: PropertyTerm::Literal {
                        value_type: Type::list(Type::Date),
                        value: DataValue::List {
                            item_type: Type::Date,
                            items: vec![DataValue::Date { days: 20020 }],
                        },
                    },
                },
            }],
            binding: IntentionBinding::PropertiesOnly,
        };
        let change = engine
            .prepare_choice(
                store,
                current.program().unwrap(),
                choice,
                vec![accepted],
                &format!("accept-{id}"),
            )
            .unwrap();
        engine.adopt(store, &change).unwrap();
    }
    let dir = tempdir();
    let path = dir.path().join("tool");
    let store = ProductStore::create(&path, &program(false), 20000).unwrap();
    let job = add(&store, "job", "Customer work");
    action(&store, "wait", "wait", &job);
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    keep_commitment(&store, &engine, "ordinary-promise");
    let current = store.load().unwrap();
    let first = store
        .prepare_scoped_change(
            &program(true),
            &request(&current, ScopePopulation::All),
            "first",
        )
        .unwrap();
    let context = ScopedExecutionContext::prepared(&current, &first).unwrap();
    let mut mapped = current.data.clone();
    mapped.schema = first.seed().schema.clone();
    let (_, initialized) = context
        .project_seed(
            current.program().unwrap(),
            &current.data,
            first.target(),
            &mapped,
            current.clock_day,
        )
        .unwrap();
    assert_eq!(&initialized, first.seed());
    let mut tampered = mapped.clone();
    tampered.records[0]
        .values
        .insert("promised".into(), DataValue::Date { days: 20099 });
    assert!(context
        .project_seed(
            current.program().unwrap(),
            &current.data,
            first.target(),
            &tampered,
            current.clock_day
        )
        .is_err());
    let mut unknown = current.data.clone();
    unknown.records[0]
        .values
        .insert("name".into(), text("Unrelated synthetic work"));
    let mut unknown_mapped = unknown.clone();
    unknown_mapped.schema = mapped.schema;
    assert!(context
        .project_seed(
            current.program().unwrap(),
            &unknown,
            first.target(),
            &unknown_mapped,
            current.clock_day
        )
        .is_err());
    // An ordinary scene may be synthetic, but a successful ordinary mapping
    // cannot turn it into host-authenticated input for a subsequent layer.
    let (ordinary_context, synthetic) = context
        .project_seed(
            current.program().unwrap(),
            &unknown,
            current.program().unwrap(),
            &unknown,
            current.clock_day,
        )
        .unwrap();
    assert_eq!(synthetic, unknown);
    assert!(ordinary_context
        .project_seed(
            current.program().unwrap(),
            &synthetic,
            first.target(),
            &unknown_mapped,
            current.clock_day,
        )
        .is_err());
    assert_eq!(store.load().unwrap(), current);
    let change = engine
        .prepare_managed_change(&store, first, &[], "first")
        .unwrap();
    engine.adopt(&store, &change).unwrap();
    keep_commitment(&store, &engine, "managed-promise");
    let current = store.load().unwrap();
    let mut raw = serde_json::to_value(program(true).program).unwrap();
    raw["views"][0]["kind"]["columns"][3]["value"] = serde_json::to_value(boolean(false)).unwrap();
    raw["actions"][6]["steps"][0]["columns"]["reminder"] =
        serde_json::to_value(boolean(false)).unwrap();
    let req = ScopeRequest {
        population: ScopePopulation::All,
        operations: ["export".into()].into_iter().collect(),
        excluded_records: vec![],
        lifecycles: vec![LifecycleBinding {
            entity: "job".into(),
            completed: field("record", "done"),
            source: current.active_revision.clone(),
        }],
        patches: vec![
            EffectPatchRequest {
                destination: EffectDestination::ViewColumn {
                    view: "work".into(),
                    column: "reminder".into(),
                },
                entity: "job".into(),
                subject: "row".into(),
                value_type: Type::Boolean,
            },
            EffectPatchRequest {
                destination: EffectDestination::EmitColumn {
                    action: "export".into(),
                    path: vec![0],
                    column: "reminder".into(),
                },
                entity: "job".into(),
                subject: "row".into(),
                value_type: Type::Boolean,
            },
        ],
    };
    let second = store
        .prepare_scoped_change(&capture(raw), &req, "second")
        .unwrap();
    let change = engine
        .prepare_managed_change(&store, second, &[], "second")
        .unwrap();
    let adopted = engine.adopt(&store, &change).unwrap();
    assert_eq!(adopted.decisions, current.decisions);
    let reopened = ProductStore::open(&path).unwrap();
    let engine = DecisionEngine::new(
        LocalRuntime::default(),
        IntentArchive::new(reopened.clone()),
    );
    let report = engine.check_current(&reopened.load().unwrap()).unwrap();
    assert_eq!(report.disposition, CheckDisposition::Ready);
    assert_eq!(
        report
            .checks
            .iter()
            .filter(|c| c.state == CheckState::Satisfied)
            .count(),
        2
    );
    let exported = apply(&reopened, "actual-export", invoke("export", &[]));
    assert_eq!(
        exported.artifacts.last().unwrap().rows[0]["promised"],
        DataValue::Date { days: 20020 }
    );
    product_backup::VerifiedBackup::capture(&reopened).unwrap();
}

#[test]
fn managed_reconciliation_preserves_real_scenes_and_exact_supersessions() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let job = add(&store, "job", "Real waiting work");
    action(&store, "wait", "wait", &job);
    tick(&store, "days", 20003);
    let before = store.load().unwrap();
    let first = store
        .prepare_scoped_change(
            &program(true),
            &request(&before, ScopePopulation::All),
            "scope",
        )
        .unwrap();
    store.adopt_scoped(before.revision, &first).unwrap();
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    for (id, calculate) in [("timing", true), ("reporting", false)] {
        let current = store.load().unwrap();
        let mut inputs = vec![];
        if calculate {
            inputs.push(invoke("calculate", &[("row", reference(&job))]));
        }
        inputs.extend([
            invoke("export", &[]),
            SemanticInput::Observe {
                point: "result".into(),
            },
        ]);
        let scene = ScenarioSpec {
            version: 1,
            id: id.into(),
            label: id.into(),
            seed: current.data.clone(),
            session: current.session.clone(),
            clock_day: current.clock_day,
            random_seed: 42,
            inputs,
            validity: vec![],
        };
        let accepted = engine
            .accept_current_scene(&current, &scene, Disclosure::Synthetic)
            .unwrap();
        let choice = Choice {
            id: id.into(),
            request: "Preserve the experienced timing and report".into(),
            rationale: None,
            scope: DecisionScope {
                operations: if calculate {
                    ["calculate".into(), "export".into()].into_iter().collect()
                } else {
                    ["export".into()].into_iter().collect()
                },
                population: Population::All,
                conditions: Values::new(),
                excluded_records: vec![],
                unknowns: vec![],
            },
            outcome: DecisionOutcome::KeepCurrent,
            obligations: vec![],
            binding: IntentionBinding::ObservedOutcome,
        };
        let change = engine
            .prepare_choice(
                &store,
                current.program().unwrap(),
                choice,
                vec![accepted],
                &format!("accept-{id}"),
            )
            .unwrap();
        engine.adopt(&store, &change).unwrap();
    }
    let later_work = add(
        &store,
        "before-new-design",
        "Later work before structural evolution",
    );
    let current = store.load().unwrap();
    let request = engine
        .reconciliation_request(
            &current,
            "reconcile",
            "Add a real materials log while preserving both experienced needs",
            &["timing".into(), "reporting".into()],
        )
        .unwrap();
    let mut raw = serde_json::to_value(program(true).program).unwrap();
    raw["entities"].as_array_mut().unwrap().push(serde_json::json!({"id":"material","label":"Materials","fields":[{"id":"name","label":"Material","value_type":{"kind":"text"}}],"unique":[],"constraints":[]}));
    raw["actions"].as_array_mut().unwrap().push(serde_json::json!({"id":"log_material","label":"Log material","parameters":{"name":{"kind":"text"}},"guards":[],"steps":[{"kind":"create","entity":"material","bind":"created","values":{"name":var("name")}}],"ensures":[]}));
    let producer = Producer::Fixture {
        name: "Explicit managed reconciliation response".into(),
    };
    let source_json = serde_json::to_string(&raw).unwrap();
    let authored = CapturedProgram::capture(
        source_json.as_bytes(),
        "scope-project",
        producer.clone(),
        None,
    )
    .unwrap();
    let response = DevelopmentResult {
        producer: producer.clone(),
        response: DevelopmentResponse {
            version: 1,
            request_digest: request.identity().unwrap(),
            candidates: vec![GeneratedCandidate {
                id: "new-design".into(),
                source_json,
            }],
            hypotheses: vec![],
            evolutions: vec![EvolutionSuggestion {
                id: "reconciled".into(),
                candidate: "new-design".into(),
                needs: vec!["timing".into(), "reporting".into()],
                proposed_retirement: vec!["timing".into()],
                preserved_obligations: vec![],
                mappings: vec![],
                scenarios: vec![],
            }],
            unsupported: vec![],
        },
    };
    let make_prepared = |current: &product_store::ProjectSnapshot, source: &CapturedProgram| {
        let editable = current.editable_scope_context().unwrap().unwrap();
        let to_source = canonical_digest(IdentityDomain::Source, source).unwrap();
        let mappings: Vec<_> = editable
            .slots
            .iter()
            .map(|slot| ScopeSlotMapping {
                layer: slot.layer.clone(),
                patch: slot.patch,
                from_source: current.active_revision.clone(),
                from: slot.destination.clone(),
                to_source: to_source.clone(),
                to: slot.destination.clone(),
                subject: slot.subject.clone(),
            })
            .collect();
        store
            .prepare_managed_evolution(source, &mappings, "adopt-design")
            .unwrap()
    };
    let prepared = make_prepared(&current, &authored);
    let mut other = raw.clone();
    other["label"] = serde_json::json!("Different raw candidate");
    let other = CapturedProgram::capture(
        &serde_json::to_vec(&other).unwrap(),
        "scope-project",
        producer,
        None,
    )
    .unwrap();
    assert!(engine
        .develop_prepared_evolution(
            response.clone(),
            &request,
            "reconciled",
            make_prepared(&current, &other),
            &|| false
        )
        .is_err());
    let draft = engine
        .develop_prepared_evolution(
            response.clone(),
            &request,
            "reconciled",
            prepared.clone(),
            &|| false,
        )
        .unwrap();
    assert_eq!(draft.authored_candidate(), &authored);
    assert_eq!(draft.candidate(), prepared.target());
    assert_ne!(
        draft.candidate().binding.producer,
        authored.binding.producer
    );
    tick(&store, "later-clock", 20004);
    assert!(engine
        .prepare_evolution(&store, &draft, "adopt-design")
        .is_err());
    assert!(engine
        .develop_prepared_evolution(response.clone(), &request, "reconciled", prepared, &|| {
            false
        })
        .is_err());
    let current = store.load().unwrap();
    let request = engine
        .reconciliation_request(
            &current,
            "fresh-reconcile",
            "Preserve both needs and add materials",
            &["timing".into(), "reporting".into()],
        )
        .unwrap();
    let mut response = response;
    response.response.request_digest = request.identity().unwrap();
    let draft = engine
        .develop_prepared_evolution(
            response,
            &request,
            "reconciled",
            make_prepared(&current, &authored),
            &|| false,
        )
        .unwrap();
    let change = engine
        .prepare_evolution(&store, &draft, "adopt-design")
        .unwrap();
    let adopted = engine.adopt(&store, &change).unwrap();
    assert!(adopted
        .data
        .records
        .iter()
        .any(|record| record.id == later_work.id));
    assert_eq!(
        adopted
            .decisions
            .decisions
            .iter()
            .find(|d| d.id == "timing")
            .unwrap()
            .status,
        DecisionStatus::Superseded {
            by: "reconciled".into()
        }
    );
    assert_eq!(
        adopted
            .decisions
            .decisions
            .iter()
            .find(|d| d.id == "reporting")
            .unwrap()
            .status,
        DecisionStatus::Active
    );
    assert_eq!(
        engine.check_current(&adopted).unwrap().disposition,
        CheckDisposition::Ready
    );
    let continued = apply(
        &store,
        "material",
        invoke("log_material", &[("name", text("Oak"))]),
    );
    assert!(continued
        .data
        .records
        .iter()
        .any(|r| r.entity == "material"));
    product_backup::VerifiedBackup::capture(&store).unwrap();
}

#[test]
fn pending_history_uses_authenticated_initialization_for_shared_discovery_input() {
    check_pending_scoped_discovery(true);
}

#[test]
fn repeated_scoped_history_ignores_only_verified_compiler_columns() {
    check_pending_scoped_discovery(false);
}

fn check_pending_scoped_discovery(novel: bool) {
    use product_discovery::{discover, DiscoveryPolicy, VerifiedRetainedHistory};
    use std::sync::{atomic::AtomicBool, Arc};
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let job = add(&store, "job", "Waiting work");
    action(&store, "wait", "wait", &job);
    tick(&store, "days", 20003);
    let current = store.load().unwrap();
    let scene = ScenarioSpec {
        version: 1,
        id: "retained-waiting".into(),
        label: "Waiting interval".into(),
        seed: current.data.clone(),
        session: current.session.clone(),
        clock_day: current.clock_day,
        random_seed: 42,
        inputs: vec![
            invoke("export", &[]),
            SemanticInput::Observe {
                point: "result".into(),
            },
        ],
        validity: vec![],
    };
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let scenes = [program(false), program(true)]
        .iter()
        .map(|source| {
            accept_scene(
                &LocalRuntime::default(),
                source,
                &scene,
                Disclosure::Synthetic,
                RuntimeLimits::default(),
            )
            .unwrap()
        })
        .collect();
    let choice = Choice {
        id: "pending-timing".into(),
        request: "Keep this unresolved waiting-time distinction".into(),
        rationale: None,
        scope: DecisionScope {
            operations: ["export".into()].into_iter().collect(),
            population: Population::All,
            conditions: Values::new(),
            excluded_records: vec![],
            unknowns: vec![],
        },
        outcome: DecisionOutcome::Deferred,
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    };
    let change = engine
        .prepare_choice(
            &store,
            current.program().unwrap(),
            choice,
            scenes,
            "save-pending",
        )
        .unwrap();
    engine.adopt(&store, &change).unwrap();
    let later = add(
        &store,
        "intervening-history-work",
        "Work after the pending scenes",
    );
    let current = store.load().unwrap();
    let mut raw = serde_json::to_value(program(true).program).unwrap();
    if novel {
        for pointer in [
            "/actions/3/steps/0/values/production",
            "/actions/4/steps/0/values/production",
            "/actions/6/steps/0/columns/production",
            "/views/0/kind/columns/1/value",
        ] {
            let value = raw.pointer(pointer).unwrap().clone();
            *raw.pointer_mut(pointer).unwrap() =
                serde_json::json!({"kind":"add","left":value,"right":int(1)});
        }
    }
    let prepared = store
        .prepare_scoped_change(
            &capture(raw),
            &request(&current, ScopePopulation::All),
            "new-timing",
        )
        .unwrap();
    let mut mapped = scene.clone();
    mapped.seed = product_runtime::merged_data(prepared.target(), &scene.seed).unwrap();
    let (_, projected) = ScopedExecutionContext::prepared(&current, &prepared)
        .unwrap()
        .project_scenario(
            current.program().unwrap(),
            &scene,
            prepared.target(),
            &mapped,
        )
        .unwrap();
    assert_eq!(projected.seed.records.len(), scene.seed.records.len());
    assert!(!projected.seed.records.iter().any(|r| r.id == later.id));
    assert!(current.data.records.iter().any(|r| r.id == later.id));
    let request = engine
        .inherit_request(
            &current,
            DevelopmentRequest {
                version: 1,
                id: "discover-pending-scope".into(),
                project_id: current.data.project_id.clone(),
                operation: DevelopmentOperation::Discover,
                request: "Compare a new waiting-time interpretation".into(),
                sources: vec![
                    current.program().unwrap().clone(),
                    prepared.target().clone(),
                ],
                context: DevelopmentContext {
                    view: Some("work".into()),
                    selected: vec![],
                    recent_inputs: vec![],
                    data_digest: Some(current.data.identity().unwrap()),
                    session_digest: Some(current.session.identity().unwrap()),
                },
                examples: vec![],
                accepted_scenes: vec![],
                decisions: current.decisions.clone(),
                unknowns: vec![],
                required_capabilities: Default::default(),
            },
        )
        .unwrap();
    let response = DevelopmentResult {
        producer: Producer::Fixture {
            name: "Explicit pending scope response".into(),
        },
        response: DevelopmentResponse {
            version: 1,
            request_digest: request.identity().unwrap(),
            candidates: vec![
                GeneratedCandidate {
                    id: "before".into(),
                    source_json: String::from_utf8(current.program().unwrap().source_bytes.clone())
                        .unwrap(),
                },
                GeneratedCandidate {
                    id: "after".into(),
                    source_json: String::from_utf8(prepared.target().source_bytes.clone()).unwrap(),
                },
            ],
            hypotheses: vec![ChoiceHypothesis {
                id: "new-waiting-choice".into(),
                statement: "This rule produces another real waiting-time result".into(),
                kind: HypothesisKind::UnresolvedChoice,
                action: "export".into(),
                observable: "waiting_jobs".into(),
                sources: vec![SourceLocus {
                    relative_path: prepared.target().binding.program_path.clone(),
                    raw_digest: prepared.target().artifact.raw_digest.clone(),
                    pointer: "/views/0/kind/columns/1/value".into(),
                }],
                alternatives: vec!["before".into(), "after".into()],
                related_decisions: vec!["pending-timing".into()],
                scenario_json: serde_json::to_string(&projected).unwrap(),
                unknowns: vec![],
            }],
            evolutions: vec![],
            unsupported: vec![],
        },
    };
    let mut history = VerifiedRetainedHistory::load(&store).unwrap();
    let missing = discover(
        &request,
        &response,
        &DiscoveryPolicy {
            retained_history: Some(history.clone()),
            ..Default::default()
        },
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert!(missing.questions.is_empty());
    assert!(!missing.unverified.is_empty());
    history
        .map_prepared_target(prepared.clone(), vec![])
        .unwrap();
    let mut altered = serde_json::to_value(prepared.target().program.clone()).unwrap();
    altered["label"] = serde_json::json!("Different scoped target");
    let altered = CapturedProgram::capture(
        &serde_json::to_vec(&altered).unwrap(),
        "scope-project",
        prepared.target().binding.producer.clone(),
        None,
    )
    .unwrap();
    let mut mismatched_request = request.clone();
    mismatched_request.sources[1] = altered.clone();
    let mut mismatched_response = response.clone();
    mismatched_response.response.request_digest = mismatched_request.identity().unwrap();
    mismatched_response.response.candidates[1].source_json =
        String::from_utf8(altered.source_bytes.clone()).unwrap();
    mismatched_response.response.hypotheses[0].sources[0].raw_digest =
        altered.artifact.raw_digest.clone();
    let mismatched = discover(
        &mismatched_request,
        &mismatched_response,
        &DiscoveryPolicy {
            retained_history: Some(history.clone()),
            ..Default::default()
        },
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert!(mismatched.questions.is_empty());
    assert!(!mismatched.unverified.is_empty());
    let mut forged = projected.clone();
    forged.seed.records[0]
        .values
        .insert("name".into(), text("Unverified projected business value"));
    let mut forged_response = response.clone();
    forged_response.response.hypotheses[0].scenario_json = serde_json::to_string(&forged).unwrap();
    let refused = discover(
        &request,
        &forged_response,
        &DiscoveryPolicy {
            retained_history: Some(history.clone()),
            ..Default::default()
        },
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert!(refused.questions.is_empty());
    assert!(!refused.unverified.is_empty());
    let report = discover(
        &request,
        &response,
        &DiscoveryPolicy {
            retained_history: Some(history.clone()),
            ..Default::default()
        },
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    if novel {
        assert!(!report.questions.is_empty(), "{:?}", report);
    } else {
        assert!(report.questions.is_empty(), "{:?}", report);
        assert!(
            report
                .log
                .iter()
                .any(|entry| entry.disposition == product_discovery::Disposition::Settled),
            "{:?}",
            report
        );
    }
    assert!(report.unverified.is_empty(), "{:?}", report);
    // Comparison projection never rewrites raw receipts or hides their visible
    // provenance; every returned artifact still validates against its bytes.
    for run in &report.runs {
        run.validate().unwrap();
        for observation in &run.observations {
            for output in &observation.outputs {
                output.validate().unwrap();
            }
        }
    }
    assert!(report
        .runs
        .iter()
        .filter(|run| run.binding.artifact == prepared.target().artifact)
        .flat_map(|run| &run.observations)
        .flat_map(|observation| &observation.outputs)
        .any(|output| output
            .columns
            .iter()
            .any(|column| column.id.starts_with("gm_scope_"))));
    assert_eq!(store.load().unwrap(), current);
    assert!(report
        .runs
        .iter()
        .any(|run| run.binding.artifact == prepared.target().artifact
            && run.binding.data_digest == projected.seed.identity().unwrap()
            && run.state == EvidenceState::Observed));
    assert!(report
        .runs
        .iter()
        .any(|run| run.id.starts_with("retained-projected-")
            && run.binding.data_digest == projected.seed.identity().unwrap()
            && run.state == EvidenceState::Observed));
    for source in [program(false), program(true)] {
        assert!(report
            .runs
            .iter()
            .any(|run| run.binding.artifact == source.artifact
                && run.binding.data_digest == scene.seed.identity().unwrap()
                && run.state == EvidenceState::Observed));
    }
    let accepted = engine
        .accept_scoped_scene(
            &store,
            &prepared,
            &selected_scene(&prepared, &job),
            Disclosure::Synthetic,
        )
        .unwrap();
    let choice = Choice {
        id: "resolved-timing".into(),
        request: "Adopt the experienced new waiting rule".into(),
        rationale: None,
        scope: prepared.scope().clone(),
        outcome: DecisionOutcome::Accept {
            artifact: prepared.target().artifact.program_digest.clone(),
        },
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    };
    let resolution = engine
        .prepare_scoped_resolution(
            &store,
            prepared.clone(),
            choice,
            vec![accepted],
            &["pending-timing".into()],
            "new-timing",
        )
        .unwrap();
    let adopted = engine.adopt(&store, &resolution).unwrap();
    assert_eq!(
        adopted
            .decisions
            .decisions
            .iter()
            .find(|d| d.id == "pending-timing")
            .unwrap()
            .status,
        DecisionStatus::Superseded {
            by: "resolved-timing".into()
        }
    );
    assert_eq!(
        engine.check_current(&adopted).unwrap().disposition,
        CheckDisposition::Ready
    );
    action(&store, "later-work", "resume", &job);
    assert!(history.map_prepared_target(prepared, vec![]).is_err());
}

#[test]
fn all_nonbinary_managed_rehearsals_survive_restart_recovery_without_activation() {
    fn timing(outcome: &DecisionOutcome, phase: &str, started: &std::time::Instant) {
        use std::io::Write;
        let _ = writeln!(
            std::io::stderr(),
            "scope_nonbinary_timing outcome={outcome:?} phase={phase} elapsed_ms={}",
            started.elapsed().as_millis()
        );
    }
    fn revised(offset: i64) -> CapturedProgram {
        let mut raw = serde_json::to_value(program(true).program).unwrap();
        for pointer in [
            "/actions/3/steps/0/values/production",
            "/actions/4/steps/0/values/production",
            "/actions/6/steps/0/columns/production",
            "/views/0/kind/columns/1/value",
        ] {
            let prior = raw.pointer(pointer).unwrap().clone();
            *raw.pointer_mut(pointer).unwrap() =
                serde_json::json!({"kind":"add","left":prior,"right":int(offset)});
        }
        capture(raw)
    }
    for outcome in [
        DecisionOutcome::EitherAcceptable,
        DecisionOutcome::BothNeeded,
        DecisionOutcome::NeitherFits,
        DecisionOutcome::Deferred,
    ] {
        let started = std::time::Instant::now();
        let dir = tempdir();
        let path = dir.path().join("tool");
        let store = ProductStore::create(&path, &program(false), 20000).unwrap();
        let before = store.load().unwrap();
        let scope = store
            .prepare_scoped_change(
                &program(true),
                &request(&before, ScopePopulation::All),
                "initial-scope",
            )
            .unwrap();
        store.adopt_scoped(before.revision, &scope).unwrap();
        let job = add(&store, "job", "Waiting job");
        action(&store, "wait", "wait", &job);
        tick(&store, "days", 20003);
        let current = store.load().unwrap();
        let candidate = revised(1);
        let editable = current.editable_scope_context().unwrap().unwrap();
        let to_source = canonical_digest(IdentityDomain::Source, &candidate).unwrap();
        let mappings: Vec<_> = editable
            .slots
            .iter()
            .map(|slot| ScopeSlotMapping {
                layer: slot.layer.clone(),
                patch: slot.patch,
                from_source: current.active_revision.clone(),
                from: slot.destination.clone(),
                to_source: to_source.clone(),
                to: slot.destination.clone(),
                subject: slot.subject.clone(),
            })
            .collect();
        let proposal = store
            .prepare_managed_evolution(&candidate, &mappings, "prospective-design")
            .unwrap();
        let engine =
            DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
        let prospective = selected_scene(&proposal, &job);
        let baseline = ScenarioSpec {
            seed: current.data.clone(),
            session: current.session.clone(),
            ..prospective.clone()
        };
        let a = engine
            .accept_current_scene(&current, &baseline, Disclosure::Synthetic)
            .unwrap();
        let b = engine
            .accept_scoped_scene(&store, &proposal, &prospective, Disclosure::Synthetic)
            .unwrap();
        assert_eq!(
            a.observations()[0].outputs[0].rows[0]["production"],
            DataValue::Integer { value: 0 }
        );
        assert_eq!(
            b.observations()[0].outputs[0].rows[0]["production"],
            DataValue::Integer { value: 1 }
        );
        let choice = Choice {
            id: "pending".into(),
            request: "Retain both actually experienced alternatives for this unresolved need"
                .into(),
            rationale: None,
            scope: DecisionScope {
                operations: ["calculate".into(), "complete".into(), "export".into()]
                    .into_iter()
                    .collect(),
                population: Population::All,
                conditions: Values::new(),
                excluded_records: vec![],
                unknowns: vec![],
            },
            outcome: outcome.clone(),
            obligations: vec![],
            binding: IntentionBinding::ObservedOutcome,
        };
        let change = engine
            .prepare_rehearsed_choice(
                &store,
                proposal.clone(),
                choice,
                vec![a, b],
                &[],
                "record-nonbinary",
            )
            .unwrap();
        let retained = engine.adopt(&store, &change).unwrap();
        timing(&outcome, "retained", &started);
        assert_eq!(retained.active_revision, current.active_revision);
        assert_eq!(retained.data, current.data);
        assert_eq!(retained.session, current.session);
        assert_eq!(retained.clock_day, current.clock_day);
        assert_eq!(retained.artifacts, current.artifacts);
        assert_eq!(retained.scope.layers, current.scope.layers);
        assert_eq!(retained.scope.compositions, current.scope.compositions);
        assert_eq!(
            retained.scope.initializations,
            current.scope.initializations
        );
        assert_eq!(retained.scope.adoptions, current.scope.adoptions);
        assert_eq!(retained.scope.rehearsals.len(), 1);
        assert_eq!(retained.decisions.decisions[0].outcome, outcome);
        assert_eq!(
            retained.decisions.decisions[0].status,
            DecisionStatus::Pending
        );
        assert_eq!(engine.adopt(&store, &change).unwrap(), retained);
        let reopened = ProductStore::open(&path).unwrap();
        let engine = DecisionEngine::new(
            LocalRuntime::default(),
            IntentArchive::new(reopened.clone()),
        );
        let portable = engine
            .development_request(
                &retained,
                "inherit-pending",
                DevelopmentOperation::Modify,
                "Continue from both experienced alternatives",
                DevelopmentContext {
                    view: Some("work".into()),
                    selected: vec![],
                    recent_inputs: vec![],
                    data_digest: Some(retained.data.identity().unwrap()),
                    session_digest: Some(retained.session.identity().unwrap()),
                },
            )
            .unwrap();
        assert_eq!(portable.accepted_scenes.len(), 2);
        assert_eq!(engine.discovery_scenes(&retained).unwrap().len(), 2);
        assert!(engine
            .prepare_change(&reopened, proposal.target(), &[], "no-latent-activation")
            .is_err());
        let backup = product_backup::VerifiedBackup::capture(&reopened).unwrap();
        let backup =
            product_backup::VerifiedBackup::from_bytes(&backup.to_bytes().unwrap()).unwrap();
        let recovered = backup.recover_new(&dir.path().join("recovered")).unwrap();
        let recovered_snapshot = recovered.load().unwrap();
        timing(&outcome, "recovered", &started);
        assert_eq!(recovered_snapshot, retained);
        let engine = DecisionEngine::new(
            LocalRuntime::default(),
            IntentArchive::new(recovered.clone()),
        );
        assert_eq!(
            engine.discovery_scenes(&recovered_snapshot).unwrap().len(),
            2
        );
        let mut corrupt = recovered_snapshot.clone();
        let proof = corrupt.scope.rehearsals.values_mut().next().unwrap();
        proof.seed = canonical_digest(IdentityDomain::Data, &"invented-seed").unwrap();
        assert!(corrupt.validate().is_err());
        assert!(
            ProductStore::create_recovered_with(dir.path().join("corrupt"), &corrupt, |_| Ok(()))
                .is_err()
        );
        let later = add(&recovered, "later", "Later legitimate work");
        let facts = recovered.load().unwrap();
        assert!(engine
            .prepare_managed_change(&recovered, proposal, &[], "prospective-design")
            .is_err());
        assert_eq!(recovered.load().unwrap(), facts);
        let fresh = if outcome == DecisionOutcome::EitherAcceptable {
            let source = revised(1);
            let target = canonical_digest(IdentityDomain::Source, &source).unwrap();
            let editable = facts.editable_scope_context().unwrap().unwrap();
            let mappings: Vec<_> = editable
                .slots
                .iter()
                .map(|slot| ScopeSlotMapping {
                    layer: slot.layer.clone(),
                    patch: slot.patch,
                    from_source: facts.active_revision.clone(),
                    from: slot.destination.clone(),
                    to_source: target.clone(),
                    to: slot.destination.clone(),
                    subject: slot.subject.clone(),
                })
                .collect();
            let prepared = recovered
                .prepare_managed_evolution(&source, &mappings, "resolve-with-scope")
                .unwrap();
            assert_eq!(
                prepared.target().artifact,
                retained
                    .scope
                    .rehearsals
                    .values()
                    .next()
                    .map(|proof| retained
                        .programs
                        .iter()
                        .find(|p| canonical_digest(IdentityDomain::Source, *p).unwrap()
                            == proof.manifest.output)
                        .unwrap()
                        .artifact
                        .clone())
                    .unwrap()
            );
            prepared
        } else {
            recovered
                .prepare_scoped_change(
                    &revised(2),
                    &request(&facts, ScopePopulation::All),
                    "resolve-with-scope",
                )
                .unwrap()
        };
        let mut resolution_scene = selected_scene(&fresh, &job);
        // A full-work evolution promises every action. Experience the remaining
        // operations explicitly instead of inferring their coverage.
        resolution_scene.inputs.splice(
            0..0,
            [
                invoke("resume", &[("row", reference(&job))]),
                invoke("wait", &[("row", reference(&job))]),
            ],
        );
        let observation = resolution_scene.inputs.len() - 1;
        resolution_scene.inputs.splice(
            observation..observation,
            [
                invoke("archive", &[("row", reference(&job))]),
                invoke(
                    "add",
                    &[
                        ("name", text("Experienced new work")),
                        ("promised", DataValue::Date { days: 20020 }),
                    ],
                ),
            ],
        );
        let accepted = engine
            .accept_scoped_scene(&recovered, &fresh, &resolution_scene, Disclosure::Synthetic)
            .unwrap();
        let choice=Choice{id:"resolved".into(),request:"Explicitly resolve the pending alternatives with this newly experienced revised rule".into(),rationale:None,scope:fresh.scope().clone(),outcome:DecisionOutcome::Accept{artifact:fresh.target().artifact.program_digest.clone()},obligations:vec![],binding:IntentionBinding::ObservedOutcome};
        let resolution = engine
            .prepare_scoped_resolution(
                &recovered,
                fresh,
                choice,
                vec![accepted],
                &["pending".into()],
                "resolve-with-scope",
            )
            .unwrap();
        let adopted = engine.adopt(&recovered, &resolution).unwrap();
        timing(&outcome, "resolved", &started);
        assert_eq!(adopted.data.events, facts.data.events);
        assert_eq!(adopted.artifacts, facts.artifacts);
        assert!(adopted.data.records.iter().any(|row| row.id == later.id));
        assert_eq!(
            adopted.decisions.decisions[0].status,
            DecisionStatus::Superseded {
                by: "resolved".into()
            }
        );
        assert_eq!(
            engine.check_current(&adopted).unwrap().disposition,
            CheckDisposition::Ready
        );
        let continued = action(&recovered, "continued", "calculate", &later);
        assert_eq!(
            row(&continued, &later).values["production"],
            DataValue::Integer {
                value: if outcome == DecisionOutcome::EitherAcceptable {
                    1
                } else {
                    2
                }
            }
        );
        product_backup::VerifiedBackup::capture(&recovered).unwrap();
        assert_eq!(store.load().unwrap(), retained);
        timing(&outcome, "continued", &started);
    }
}

#[test]
fn future_rehearsals_preserve_selected_promises_without_activating_cohorts() {
    fn timing(outcome: &DecisionOutcome, phase: &str, started: &std::time::Instant) {
        use std::io::Write;
        let _ = writeln!(
            std::io::stderr(),
            "scope_future_timing outcome={outcome:?} phase={phase} elapsed_ms={}",
            started.elapsed().as_millis()
        );
    }
    fn future_scene(source: &CapturedProgram, seed: &DataSnapshot) -> ScenarioSpec {
        let mut scene = ScenarioSpec {
            version: 1,
            id: "future-scene".into(),
            label: "New work after the future rule".into(),
            seed: seed.clone(),
            session: SessionState::initial(&source.program).unwrap(),
            clock_day: 20003,
            random_seed: 42,
            inputs: vec![invoke(
                "add",
                &[
                    ("name", text("New work")),
                    ("promised", DataValue::Date { days: 20020 }),
                ],
            )],
            validity: vec![],
        };
        let runtime = LocalRuntime::default();
        let mut run = runtime
            .start(
                source,
                &scene.seed,
                &scene.session,
                scene.clock_day,
                scene.random_seed,
                RuntimeLimits::default(),
            )
            .unwrap();
        runtime
            .apply(
                &mut run,
                &scene.inputs[0],
                &scene.replay_operation_ids().unwrap()[0],
            )
            .unwrap();
        let created = runtime
            .data(&run)
            .records
            .iter()
            .find(|r| r.values["name"] == text("New work"))
            .unwrap();
        scene.inputs.extend([
            invoke("wait", &[("row", reference(created))]),
            SemanticInput::AdvanceClock { days: 3 },
            invoke("calculate", &[("row", reference(created))]),
            invoke("complete", &[("row", reference(created))]),
            invoke("export", &[]),
            SemanticInput::Observe {
                point: "result".into(),
            },
        ]);
        scene
    }

    for outcome in [
        DecisionOutcome::EitherAcceptable,
        DecisionOutcome::BothNeeded,
        DecisionOutcome::NeitherFits,
        DecisionOutcome::Deferred,
    ] {
        let started = std::time::Instant::now();
        let dir = tempdir();
        let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
        let selected = add(&store, "selected", "Selected waiting work");
        let archived = add(&store, "archived", "Completed archived history");
        action(&store, "completed-history", "complete", &archived);
        action(&store, "archive-history", "archive", &archived);
        action(&store, "wait", "wait", &selected);
        tick(&store, "days", 20003);
        let current = store.load().unwrap();
        let first = store
            .prepare_scoped_change(
                &program(true),
                &request(
                    &current,
                    ScopePopulation::SelectedUnfinished {
                        records: vec![RecordRef {
                            entity: selected.entity.clone(),
                            record: selected.id.clone(),
                        }],
                    },
                ),
                "selected-rule",
            )
            .unwrap();
        let engine =
            DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
        let original_scene = selected_scene(&first, &selected);
        let original_source = first.target().clone();
        let selected_scope = first.scope().clone();
        let accepted = engine
            .accept_scoped_scene(&store, &first, &original_scene, Disclosure::Synthetic)
            .unwrap();
        let choice = Choice {
            id: "selected-promise".into(),
            request: "Keep the experienced selected-row timing and customer commitment".into(),
            rationale: None,
            scope: first.scope().clone(),
            outcome: DecisionOutcome::Accept {
                artifact: first.target().artifact.program_digest.clone(),
            },
            obligations: vec![],
            binding: IntentionBinding::ObservedOutcome,
        };
        let change = engine
            .prepare_scoped_choice(&store, first, choice, vec![accepted], "selected-rule")
            .unwrap();
        let current = engine.adopt(&store, &change).unwrap();
        let mut raw = serde_json::to_value(program(true).program).unwrap();
        for pointer in [
            "/actions/3/steps/0/values/production",
            "/actions/4/steps/0/values/production",
            "/actions/6/steps/0/columns/production",
            "/views/0/kind/columns/1/value",
        ] {
            let prior = raw.pointer(pointer).unwrap().clone();
            *raw.pointer_mut(pointer).unwrap() =
                serde_json::json!({"kind":"add","left":prior,"right":int(1)});
        }
        let candidate = capture(raw);
        let proposal = store
            .prepare_scoped_change(
                &candidate,
                &request(&current, ScopePopulation::FutureWork),
                "future-proposal",
            )
            .unwrap();
        let context = ScopedExecutionContext::prepared(&current, &proposal).unwrap();
        let columns = context.provenance_columns(proposal.target()).unwrap();
        assert!(!columns.output("sheet", "production"));
        assert!(!columns.output("sheet", "promised"));
        assert!(!columns.outputs["sheet"].is_empty());
        let mut forged = serde_json::to_value(proposal.target().program.clone()).unwrap();
        forged["label"] = serde_json::json!("Unregistered source copying protected columns");
        assert!(context.provenance_columns(&capture(forged)).is_err());
        // A real business result change on the selected cohort still violates
        // the concrete promise; host provenance normalization cannot hide it.
        let editable = current.editable_scope_context().unwrap().unwrap();
        let to_source = canonical_digest(IdentityDomain::Source, &candidate).unwrap();
        let mappings: Vec<_> = editable
            .slots
            .iter()
            .map(|slot| ScopeSlotMapping {
                layer: slot.layer.clone(),
                patch: slot.patch,
                from_source: current.active_revision.clone(),
                from: slot.destination.clone(),
                to_source: to_source.clone(),
                to: slot.destination.clone(),
                subject: slot.subject.clone(),
            })
            .collect();
        let wrong_cohort = store
            .prepare_managed_evolution(&candidate, &mappings, "wrong-cohort")
            .unwrap();
        assert!(engine
            .prepare_managed_change(&store, wrong_cohort, &[], "wrong-cohort")
            .is_err());
        assert_eq!(store.load().unwrap(), current);
        // The future row is created by the actual production interpreter in
        // both scenes. The selected row keeps its old rule in both sources.
        let prospective = future_scene(proposal.target(), proposal.seed());
        // Both scenes use the same business input; metadata initialization and
        // each full raw source/input binding remain explicit.
        let a = engine
            .accept_prepared_current_scene(&store, &proposal, &prospective, Disclosure::Synthetic)
            .unwrap();
        let b = engine
            .accept_scoped_scene(&store, &proposal, &prospective, Disclosure::Synthetic)
            .unwrap();
        let values = |scene: &AcceptedScene, name: &str| {
            scene.observations()[0].outputs[0]
                .rows
                .iter()
                .find(|row| row["name"] == text(name))
                .unwrap()["production"]
                .clone()
        };
        assert_eq!(
            values(&a, "Selected waiting work"),
            DataValue::Integer { value: 0 }
        );
        assert_eq!(
            values(&b, "Selected waiting work"),
            DataValue::Integer { value: 0 }
        );
        assert_eq!(values(&a, "New work"), DataValue::Integer { value: 3 });
        assert_eq!(values(&b, "New work"), DataValue::Integer { value: 1 });
        let raw_evidence = b.evidence().clone();
        let pending_scope = proposal.scope().clone();
        let choice = Choice {
            id: "pending-future".into(),
            request: "Keep these experienced future alternatives pending".into(),
            rationale: None,
            scope: pending_scope.clone(),
            outcome: outcome.clone(),
            obligations: vec![],
            binding: IntentionBinding::ObservedOutcome,
        };
        let change = engine
            .prepare_rehearsed_choice(
                &store,
                proposal.clone(),
                choice,
                vec![a, b],
                &[],
                "record-future",
            )
            .unwrap();
        let saved = engine.adopt(&store, &change).unwrap();
        assert_eq!(saved.data, current.data);
        assert_eq!(saved.session, current.session);
        assert_eq!(saved.clock_day, current.clock_day);
        assert_eq!(saved.artifacts, current.artifacts);
        assert_eq!(saved.active_revision, current.active_revision);
        assert_eq!(saved.scope.layers, current.scope.layers);
        assert_eq!(saved.scope.initializations, current.scope.initializations);
        assert_eq!(saved.scope.compositions, current.scope.compositions);
        assert_eq!(saved.scope.adoptions, current.scope.adoptions);
        assert_eq!(saved.decisions.decisions[1].scope, pending_scope);
        assert_eq!(saved.decisions.decisions[1].status, DecisionStatus::Pending);
        assert_eq!(engine.adopt(&store, &change).unwrap(), saved);
        timing(&outcome, "retained", &started);
        let reopened = ProductStore::open(dir.path().join("tool")).unwrap();
        let backup = product_backup::VerifiedBackup::capture(&reopened).unwrap();
        let recovered = product_backup::VerifiedBackup::from_bytes(&backup.to_bytes().unwrap())
            .unwrap()
            .recover_new(&dir.path().join("recovered"))
            .unwrap();
        assert_eq!(recovered.load().unwrap(), saved);
        timing(&outcome, "recovered", &started);
        let engine = DecisionEngine::new(
            LocalRuntime::default(),
            IntentArchive::new(recovered.clone()),
        );
        let scenes = engine.discovery_scenes(&saved).unwrap();
        assert_eq!(scenes.len(), 2);
        assert!(scenes
            .iter()
            .any(|scene| scene.original().observations == raw_evidence.observations));
        assert!(engine
            .prepare_managed_change(&recovered, proposal, &[], "future-proposal")
            .is_err());
        let intervening = add(
            &recovered,
            "intervening-work",
            "Actual work before resolution",
        );
        action(&recovered, "intervening-wait", "wait", &intervening);
        let facts = recovered.load().unwrap();
        let fresh = recovered
            .prepare_scoped_change(
                &candidate,
                &request(&facts, ScopePopulation::FutureWork),
                "accept-future",
            )
            .unwrap();
        let scene = future_scene(fresh.target(), fresh.seed());
        let accepted = engine
            .accept_scoped_scene(&recovered, &fresh, &scene, Disclosure::Synthetic)
            .unwrap();
        let choice = Choice {
            id: "future-resolved".into(),
            request: "Adopt this freshly experienced future rule".into(),
            rationale: None,
            scope: fresh.scope().clone(),
            outcome: DecisionOutcome::Accept {
                artifact: fresh.target().artifact.program_digest.clone(),
            },
            obligations: vec![],
            binding: IntentionBinding::ObservedOutcome,
        };
        let change = engine
            .prepare_scoped_resolution(
                &recovered,
                fresh,
                choice,
                vec![accepted],
                &["pending-future".into()],
                "accept-future",
            )
            .unwrap();
        let adopted = engine.adopt(&recovered, &change).unwrap();
        assert_eq!(adopted.data.events, facts.data.events);
        assert!(adopted.data.records.iter().any(|r| r.id == intervening.id));
        assert!(!adopted.scope.correspondences.is_empty());
        let mut forged = adopted.clone();
        forged
            .scope
            .correspondences
            .values_mut()
            .next()
            .unwrap()
            .proof
            .original
            .records[0]
            .values
            .insert("name".into(), text("Forged historical input"));
        assert!(forged.validate().is_err());
        assert!(ProductStore::create_recovered_with(
            dir.path().join("forged"),
            &forged,
            |_| Ok(())
        )
        .is_err());
        let mut ambiguous = adopted.clone();
        let mut receipt = ambiguous
            .scope
            .correspondences
            .values()
            .find(|r| r.proof.scenario.is_some())
            .unwrap()
            .clone();
        receipt.proof.operation_seed =
            canonical_digest(IdentityDomain::Data, &"forged-operation-origin").unwrap();
        ambiguous
            .scope
            .correspondences
            .insert(receipt.proof.identity().unwrap(), receipt);
        assert!(ambiguous.validate().is_err());
        assert_eq!(adopted.artifacts, saved.artifacts);
        assert_eq!(adopted.decisions.decisions[0], saved.decisions.decisions[0]);
        assert_eq!(
            engine.check_current(&adopted).unwrap().disposition,
            CheckDisposition::Ready
        );
        timing(&outcome, "resolved", &started);
        let future = add(&recovered, "real-future", "Real future work");
        let actual = action(&recovered, "calculate-future", "calculate", &future);
        assert_eq!(
            row(&actual, &future).values["production"],
            DataValue::Integer { value: 1 }
        );
        let actual = action(&recovered, "calculate-selected", "calculate", &selected);
        assert_eq!(
            row(&actual, &selected).values["production"],
            DataValue::Integer { value: 0 }
        );
        // Replay the accepted future scene (which actually creates a row)
        // across another later layer. Its original deterministic record ID must
        // survive metadata projection, while a real selected ID is not guessed.
        let current = recovered.load().unwrap();
        let mut equivalent = serde_json::to_value(candidate.program.clone()).unwrap();
        for pointer in [
            "/actions/3/steps/0/values/production",
            "/actions/4/steps/0/values/production",
            "/actions/6/steps/0/columns/production",
            "/views/0/kind/columns/1/value",
        ] {
            let value = equivalent.pointer(pointer).unwrap().clone();
            *equivalent.pointer_mut(pointer).unwrap() = serde_json::json!({"kind":"if","condition":boolean(true),"then_value":value,"else_value":int(0)});
        }
        let third = recovered
            .prepare_scoped_change(
                &capture(equivalent),
                &request(
                    &current,
                    ScopePopulation::SelectedUnfinished {
                        records: vec![RecordRef {
                            entity: future.entity.clone(),
                            record: future.id.clone(),
                        }],
                    },
                ),
                "later-selected-implementation",
            )
            .unwrap();
        let change = engine
            .prepare_managed_change(&recovered, third, &[], "later-selected-implementation")
            .unwrap();
        let later_adoption = engine.adopt(&recovered, &change).unwrap();
        assert_eq!(later_adoption.data.events, current.data.events);
        assert_eq!(
            engine.check_current(&later_adoption).unwrap().disposition,
            CheckDisposition::Ready
        );
        timing(&outcome, "third-layer", &started);
        // A durable successor scene can use this exact derived input after
        // later real work; it keeps a separately bound original operation frame.
        let current = recovered.load().unwrap();
        let context = ScopedExecutionContext::committed(&current).unwrap();
        let mut mapped = original_scene.clone();
        mapped.seed =
            product_runtime::merged_data(current.program().unwrap(), &original_scene.seed).unwrap();
        let (_, projected) = context
            .project_scenario(
                &original_source,
                &original_scene,
                current.program().unwrap(),
                &mapped,
            )
            .unwrap();
        assert_ne!(
            projected.identity().unwrap(),
            original_scene.identity().unwrap()
        );
        let accepted = engine
            .accept_current_scene(&current, &projected, Disclosure::Synthetic)
            .unwrap();
        let choice = Choice {
            id: "continued-selected-promise".into(),
            request: "Keep this newly experienced source-qualified selected result".into(),
            rationale: None,
            scope: selected_scope,
            outcome: DecisionOutcome::KeepCurrent,
            obligations: vec![],
            binding: IntentionBinding::ObservedOutcome,
        };
        let change = engine
            .prepare_choice(
                &recovered,
                current.program().unwrap(),
                choice,
                vec![accepted],
                "save-derived-scene",
            )
            .unwrap();
        let continued = engine.adopt(&recovered, &change).unwrap();
        assert_eq!(continued.data, current.data);
        let final_backup = product_backup::VerifiedBackup::capture(&recovered).unwrap();
        let final_store = final_backup
            .recover_new(&dir.path().join("final-recovery"))
            .unwrap();
        let final_engine = DecisionEngine::new(
            LocalRuntime::default(),
            IntentArchive::new(final_store.clone()),
        );
        assert_eq!(
            final_engine
                .check_current(&final_store.load().unwrap())
                .unwrap()
                .disposition,
            CheckDisposition::Ready
        );
        timing(&outcome, "final-recovery", &started);
    }
}

#[test]
fn first_scope_can_be_rehearsed_and_retained_before_any_live_layer_exists() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let job = add(&store, "job", "Unscoped work");
    action(&store, "wait", "wait", &job);
    tick(&store, "days", 20003);
    let before = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(
            &program(true),
            &request(&before, ScopePopulation::All),
            "first-proposal",
        )
        .unwrap();
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let prospective = selected_scene(&prepared, &job);
    let a = engine
        .accept_prepared_current_scene(&store, &prepared, &prospective, Disclosure::Synthetic)
        .unwrap();
    let b = engine
        .accept_scoped_scene(&store, &prepared, &prospective, Disclosure::Synthetic)
        .unwrap();
    let choice = Choice {
        id: "first-pending".into(),
        request: "Remember the experienced options before choosing a scope".into(),
        rationale: None,
        scope: prepared.scope().clone(),
        outcome: DecisionOutcome::Deferred,
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    };
    let change = engine
        .prepare_rehearsed_choice(
            &store,
            prepared.clone(),
            choice,
            vec![a, b],
            &[],
            "record-first",
        )
        .unwrap();
    let retained = engine.adopt(&store, &change).unwrap();
    assert!(retained.scope.layers.is_empty());
    assert!(retained.scope.compositions.is_empty());
    assert_eq!(retained.data, before.data);
    assert_eq!(retained.session, before.session);
    assert_eq!(retained.active_revision, before.active_revision);
    let reopened = ProductStore::open(dir.path().join("tool")).unwrap();
    let recovered = product_backup::VerifiedBackup::capture(&reopened)
        .unwrap()
        .recover_new(&dir.path().join("recovered"))
        .unwrap();
    let engine = DecisionEngine::new(
        LocalRuntime::default(),
        IntentArchive::new(recovered.clone()),
    );
    assert_eq!(
        engine
            .discovery_scenes(&recovered.load().unwrap())
            .unwrap()
            .len(),
        2
    );
    assert!(engine
        .prepare_change(
            &recovered,
            prepared.target(),
            &[],
            "cannot-activate-rehearsal"
        )
        .is_err());
    let mut corrupt = retained.clone();
    let proof = corrupt.scope.rehearsals.values_mut().next().unwrap();
    proof.initialization.as_mut().unwrap().additions[0]
        .values
        .clear();
    assert!(corrupt.validate().is_err());
    let mut leaked = retained.clone();
    leaked.data = prepared.seed().clone();
    assert!(leaked.validate().is_err());
    let later = add(&recovered, "later", "Unscoped continued work");
    tick(&recovered, "later-days", 20005);
    let actual = action(&recovered, "actual-calculate", "calculate", &later);
    assert_eq!(
        row(&actual, &later).values["production"],
        DataValue::Integer { value: 2 }
    );
    assert!(actual.scope.layers.is_empty());
    product_backup::VerifiedBackup::capture(&recovered).unwrap();
}

#[test]
fn fresh_scoped_discovery_retains_creation_identity_without_pending_history() {
    use product_discovery::{discover, DiscoveryPolicy, VerifiedRetainedHistory};
    use std::sync::{atomic::AtomicBool, Arc};
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    add(
        &store,
        "existing",
        "Existing work changes the metadata-bearing seed",
    );
    let current = store.load().unwrap();
    assert!(current.decisions.decisions.is_empty());
    let prepared = store
        .prepare_scoped_change(
            &program(true),
            &request(&current, ScopePopulation::FutureWork),
            "prospective",
        )
        .unwrap();
    let mut scene = ScenarioSpec {
        version: 1,
        id: "fresh-created-reference".into(),
        label: "New waiting work with an existing seed row".into(),
        seed: current.data.clone(),
        session: current.session.clone(),
        clock_day: current.clock_day,
        random_seed: 42,
        inputs: vec![invoke(
            "add",
            &[
                ("name", text("Scene work")),
                ("promised", DataValue::Date { days: 20020 }),
            ],
        )],
        validity: vec![],
    };
    let runtime = LocalRuntime::default();
    let mut run = runtime
        .start(
            current.program().unwrap(),
            &scene.seed,
            &scene.session,
            scene.clock_day,
            scene.random_seed,
            RuntimeLimits::default(),
        )
        .unwrap();
    runtime
        .apply(
            &mut run,
            &scene.inputs[0],
            &scene.replay_operation_ids().unwrap()[0],
        )
        .unwrap();
    let created = runtime
        .data(&run)
        .records
        .iter()
        .find(|r| r.values["name"] == text("Scene work"))
        .unwrap()
        .clone();
    scene.inputs.extend([
        invoke("wait", &[("row", reference(&created))]),
        SemanticInput::AdvanceClock { days: 3 },
        invoke("calculate", &[("row", reference(&created))]),
        invoke("complete", &[("row", reference(&created))]),
        invoke("export", &[]),
        SemanticInput::Observe {
            point: "result".into(),
        },
    ]);
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let request = engine
        .inherit_request(
            &current,
            DevelopmentRequest {
                version: 1,
                id: "fresh-scope-discovery".into(),
                project_id: current.data.project_id.clone(),
                operation: DevelopmentOperation::Discover,
                request: "Compare waiting-time rules on newly created work".into(),
                sources: vec![
                    current.program().unwrap().clone(),
                    prepared.target().clone(),
                ],
                context: DevelopmentContext {
                    view: Some("work".into()),
                    selected: vec![],
                    recent_inputs: vec![],
                    data_digest: Some(current.data.identity().unwrap()),
                    session_digest: Some(current.session.identity().unwrap()),
                },
                examples: vec![],
                accepted_scenes: vec![],
                decisions: current.decisions.clone(),
                unknowns: vec![],
                required_capabilities: Default::default(),
            },
        )
        .unwrap();
    let response = DevelopmentResult {
        producer: Producer::Fixture {
            name: "Fresh source-qualified scope comparison".into(),
        },
        response: DevelopmentResponse {
            version: 1,
            request_digest: request.identity().unwrap(),
            candidates: vec![
                GeneratedCandidate {
                    id: "before".into(),
                    source_json: String::from_utf8(current.program().unwrap().source_bytes.clone())
                        .unwrap(),
                },
                GeneratedCandidate {
                    id: "after".into(),
                    source_json: String::from_utf8(prepared.target().source_bytes.clone()).unwrap(),
                },
            ],
            hypotheses: vec![ChoiceHypothesis {
                id: "waiting-rule".into(),
                statement: "Waiting can pause production for future work".into(),
                kind: HypothesisKind::UnresolvedChoice,
                action: "export".into(),
                observable: "waiting_jobs".into(),
                sources: vec![SourceLocus {
                    relative_path: prepared.target().binding.program_path.clone(),
                    raw_digest: prepared.target().artifact.raw_digest.clone(),
                    pointer: "/views/0/kind/columns/1/value".into(),
                }],
                alternatives: vec!["before".into(), "after".into()],
                related_decisions: vec![],
                scenario_json: serde_json::to_string(&scene).unwrap(),
                unknowns: vec![],
            }],
            evolutions: vec![],
            unsupported: vec![],
        },
    };
    let mut history = VerifiedRetainedHistory::load(&store).unwrap();
    history
        .map_prepared_target(prepared.clone(), vec![])
        .unwrap();
    let report = discover(
        &request,
        &response,
        &DiscoveryPolicy {
            retained_history: Some(history),
            ..Default::default()
        },
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert!(!report.questions.is_empty(), "{:?}", report);
    for (id, value) in [("captured-baseline", 3), ("captured-candidate", 0)] {
        let run = report.runs.iter().find(|r| r.id == id).unwrap();
        assert_eq!(run.state, EvidenceState::Observed, "{:?}", run.errors);
        assert!(run.observations[0]
            .view
            .rows
            .iter()
            .any(|r| r.record.record == created.id));
        let row = run.observations[0].outputs[0]
            .rows
            .iter()
            .find(|r| r["name"] == text("Scene work"))
            .unwrap();
        assert_eq!(row["production"], DataValue::Integer { value });
        assert_ne!(run.binding.scenario_digest, scene.identity().unwrap());
    }
    // Structural reduction may remove an unrelated observation without
    // changing the authenticated operation frame or future reference IDs.
    let mut reducing = scene.clone();
    reducing.inputs.insert(
        1,
        SemanticInput::Observe {
            point: "unrelated".into(),
        },
    );
    let mut mapped = reducing.clone();
    mapped.seed = product_runtime::merged_data(prepared.target(), &reducing.seed).unwrap();
    let (context, projected) = ScopedExecutionContext::prepared(&current, &prepared)
        .unwrap()
        .project_scenario(
            current.program().unwrap(),
            &reducing,
            prepared.target(),
            &mapped,
        )
        .unwrap();
    let comparison = product_scenarios::ComparisonEngine::new(Arc::new(AtomicBool::new(false)))
        .with_admission(Arc::new(context));
    let target = product_scenarios::ObservationTarget::OutputColumn {
        point: "result".into(),
        output: "sheet".into(),
        column: "production".into(),
    };
    let mut reduced = projected.clone();
    reduced.inputs.remove(1);
    let checked = comparison
        .compare(
            current.program().unwrap(),
            prepared.target(),
            &reduced,
            &current.decisions,
            target.clone(),
            RuntimeLimits::default(),
        )
        .unwrap();
    assert_eq!(
        checked.state,
        EvidenceState::Observed,
        "{:?}",
        checked.diagnostics
    );
    let minimized = comparison
        .minimize(
            current.program().unwrap(),
            prepared.target(),
            &projected,
            &current.decisions,
            target.clone(),
            product_scenarios::SearchBudget::default(),
        )
        .unwrap();
    let witness = minimized.witness.unwrap();
    assert!(!witness
        .witness()
        .scenario
        .inputs
        .iter()
        .any(|input| matches!(input,SemanticInput::Observe{point} if point=="unrelated")));
    assert!(witness
        .reduction_audit()
        .iter()
        .any(|audit| audit.trial.outcome == ReductionOutcome::DifferencePreserved));
    let mut tampered = reduced;
    tampered.clock_day += 1;
    let refused = comparison
        .compare(
            current.program().unwrap(),
            prepared.target(),
            &tampered,
            &current.decisions,
            target,
            RuntimeLimits::default(),
        )
        .unwrap();
    assert!(matches!(
        refused.state,
        EvidenceState::Unsupported | EvidenceState::Inconclusive
    ));
    assert_eq!(store.load().unwrap(), current);
}

#[test]
fn prepared_discovery_restores_correspondence_state_on_the_same_engine() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let job = add(&store, "job", "Accepted current work");
    action(&store, "wait", "wait", &job);
    tick(&store, "days", 20003);
    let current = store.load().unwrap();
    let scene = ScenarioSpec {
        version: 1,
        id: "original-current".into(),
        label: "Keep this experienced timing".into(),
        seed: current.data.clone(),
        session: current.session.clone(),
        clock_day: current.clock_day,
        random_seed: 42,
        inputs: vec![
            invoke("calculate", &[("row", reference(&job))]),
            invoke("complete", &[("row", reference(&job))]),
            invoke("export", &[]),
            SemanticInput::Observe {
                point: "result".into(),
            },
        ],
        validity: vec![],
    };
    let mut engine =
        DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let accepted = engine
        .accept_current_scene(&current, &scene, Disclosure::Synthetic)
        .unwrap();
    let choice = Choice {
        id: "current-promise".into(),
        request: "Keep this actual production timing".into(),
        rationale: None,
        scope: DecisionScope {
            operations: ["calculate".into(), "complete".into(), "export".into()]
                .into_iter()
                .collect(),
            population: Population::All,
            conditions: Values::new(),
            excluded_records: vec![],
            unknowns: vec![],
        },
        outcome: DecisionOutcome::KeepCurrent,
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    };
    let choice = engine
        .prepare_choice(
            &store,
            current.program().unwrap(),
            choice,
            vec![accepted],
            "keep-current",
        )
        .unwrap();
    engine.adopt(&store, &choice).unwrap();
    add(&store, "later", "Real work after the accepted scene");
    let current = store.load().unwrap();
    let future = store
        .prepare_scoped_change(
            &program(true),
            &request(&current, ScopePopulation::FutureWork),
            "future",
        )
        .unwrap();
    let checked = engine
        .check_prepared_discovery_candidate(
            &current,
            future.target(),
            &future,
            &[],
            RuntimeLimits::default(),
        )
        .unwrap();
    assert_eq!(checked.disposition, CheckDisposition::Ready);
    assert_eq!(
        engine.check_current(&current).unwrap().disposition,
        CheckDisposition::Ready
    );
    let incompatible = store
        .prepare_scoped_change(
            &program(true),
            &request(&current, ScopePopulation::All),
            "different-rule",
        )
        .unwrap();
    let checked = engine
        .check_prepared_discovery_candidate(
            &current,
            incompatible.target(),
            &incompatible,
            &[],
            RuntimeLimits::default(),
        )
        .unwrap();
    assert_eq!(checked.disposition, CheckDisposition::RepairRequired);
    assert_eq!(
        engine.check_current(&current).unwrap().disposition,
        CheckDisposition::Ready
    );
    let invalid = RuntimeLimits {
        fuel: 0,
        ..Default::default()
    };
    assert!(engine
        .check_prepared_discovery_candidate(&current, future.target(), &future, &[], invalid)
        .is_err());
    assert_eq!(
        engine.check_current(&current).unwrap().disposition,
        CheckDisposition::Ready
    );
    assert_eq!(store.load().unwrap(), current);
}

#[test]
fn scoped_output_provenance_does_not_evaluate_intermediate_observables() {
    fn deferred_observation(pause: bool) -> CapturedProgram {
        let mut raw = serde_json::to_value(program(pause).program).unwrap();
        raw["state"].as_array_mut().unwrap().push(serde_json::json!({"id":"scratch","label":"Intermediate export state","value_type":{"kind":"integer"},"initial":{"kind":"integer","value":0}}));
        raw["observables"].as_array_mut().unwrap().push(serde_json::json!({"id":"safe_final","label":"Final state","value":{"kind":"add","left":{"kind":"state","state":"scratch"},"right":int(1)}}));
        raw["actions"][6]["steps"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"kind":"set_state","state":"scratch","value":int(i64::MAX)}));
        raw["actions"].as_array_mut().unwrap().push(serde_json::json!({"id":"reset_observation","label":"Finish export state","parameters":{},"guards":[],"steps":[{"kind":"set_state","state":"scratch","value":int(0)}],"ensures":[]}));
        capture(raw)
    }
    let dir = tempdir();
    let store =
        ProductStore::create(dir.path().join("tool"), &deferred_observation(false), 20000).unwrap();
    let job = add(&store, "job", "Waiting export");
    action(&store, "wait", "wait", &job);
    tick(&store, "days", 20003);
    let current = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(
            &deferred_observation(true),
            &request(&current, ScopePopulation::All),
            "scope",
        )
        .unwrap();
    let mut scene = selected_scene(&prepared, &job);
    scene
        .inputs
        .insert(scene.inputs.len() - 1, invoke("reset_observation", &[]));
    let context = ScopedExecutionContext::prepared(&current, &prepared).unwrap();
    let direct = LocalRuntime::default()
        .replay_admitted(
            prepared.target(),
            &scene,
            &current.decisions,
            RuntimeLimits::default(),
            "direct",
            Some(&context),
        )
        .unwrap();
    assert_eq!(direct.state, EvidenceState::Observed);
    assert_eq!(direct.observations.len(), 1);
    assert_eq!(
        direct.observations[0].values["safe_final"],
        DataValue::Integer { value: 1 }
    );
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let accepted = engine
        .accept_scoped_scene(&store, &prepared, &scene, Disclosure::Synthetic)
        .unwrap();
    assert_eq!(accepted.observations(), direct.observations);
    assert_eq!(accepted.evidence().trace, direct.trace);
    let choice = Choice {
        id: "timing".into(),
        request: "Keep the actually experienced scoped export and final observation".into(),
        rationale: None,
        scope: prepared.scope().clone(),
        outcome: DecisionOutcome::Accept {
            artifact: prepared.target().artifact.program_digest.clone(),
        },
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    };
    let change = engine
        .prepare_scoped_choice(&store, prepared, choice, vec![accepted], "scope")
        .unwrap();
    let adopted = engine.adopt(&store, &change).unwrap();
    assert_eq!(
        engine.check_current(&adopted).unwrap().disposition,
        CheckDisposition::Ready
    );
    apply(&store, "live-export", invoke("export", &[]));
    let continued = apply(&store, "live-reset", invoke("reset_observation", &[]));
    assert_eq!(
        continued.session.values["scratch"],
        DataValue::Integer { value: 0 }
    );
    assert_eq!(
        continued.artifacts.last().unwrap().rows[0]["production"],
        DataValue::Integer { value: 0 }
    );
    ProductStore::open(dir.path().join("tool"))
        .unwrap()
        .runtime_view()
        .unwrap();
}

#[test]
fn clock_only_scope_adoption_preserves_original_day_completed_scene() {
    let dir = tempdir();
    let path = dir.path().join("tool");
    let store = ProductStore::create(&path, &program(false), 20000).unwrap();
    let job = add(&store, "job", "Completed before the clock changed");
    let original = action(&store, "complete", "complete", &job);
    let scene = ScenarioSpec {
        version: 1,
        id: "original-day".into(),
        label: "Actually experienced completed result on its original day".into(),
        seed: original.data.clone(),
        session: original.session.clone(),
        clock_day: original.clock_day,
        random_seed: 42,
        inputs: vec![
            invoke("export", &[]),
            SemanticInput::Observe {
                point: "result".into(),
            },
        ],
        validity: vec![],
    };
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let accepted = engine
        .accept_current_scene(&original, &scene, Disclosure::Synthetic)
        .unwrap();
    assert_eq!(
        accepted.observations()[0].outputs[0].rows[0]["production"],
        DataValue::Integer { value: 0 }
    );
    let choice = Choice {
        id: "original-completed-promise".into(),
        request: "Keep the whole completed result experienced on the original day".into(),
        rationale: None,
        scope: DecisionScope {
            operations: ["export".into()].into_iter().collect(),
            population: Population::All,
            conditions: Values::new(),
            excluded_records: vec![],
            unknowns: vec![],
        },
        outcome: DecisionOutcome::KeepCurrent,
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    };
    let change = engine
        .prepare_choice(
            &store,
            original.program().unwrap(),
            choice,
            vec![accepted],
            "keep-original-day",
        )
        .unwrap();
    engine.adopt(&store, &change).unwrap();
    let current = tick(&store, "later-clock", 20003);
    assert_eq!(current.data, original.data);
    let prepared = store
        .prepare_scoped_change(
            &program(true),
            &request(&current, ScopePopulation::FutureWork),
            "future-rule",
        )
        .unwrap();
    let context = ScopedExecutionContext::prepared(&current, &prepared).unwrap();
    // The live initialization captured a different day, so it cannot stand in
    // for the earlier scene even though every business byte is unchanged.
    assert!(context
        .verify_seed(prepared.target(), prepared.seed(), scene.clock_day)
        .is_err());
    let mut mapped = scene.clone();
    mapped.seed = product_runtime::merged_data(prepared.target(), &scene.seed).unwrap();
    let (historical, projected) = context
        .project_scenario(
            original.program().unwrap(),
            &scene,
            prepared.target(),
            &mapped,
        )
        .unwrap();
    assert_eq!(projected.clock_day, scene.clock_day);
    assert_eq!(projected.seed.events, scene.seed.events);
    assert_eq!(projected.seed.generation, scene.seed.generation);
    for (key, value) in &scene.seed.records[0].values {
        assert_eq!(projected.seed.records[0].values.get(key), Some(value));
    }
    let saved = &projected.seed.records[0].values;
    assert!(saved.iter().any(|(key, value)| key.starts_with("gm_scope_")
        && *value == text("CapturedForHistoricalReplay")));
    assert!(saved
        .iter()
        .any(|(key, value)| key.starts_with("gm_scope_")
            && *value == DataValue::Date { days: 20000 }));
    let replay = LocalRuntime::default()
        .replay_admitted(
            prepared.target(),
            &projected,
            &current.decisions,
            RuntimeLimits::default(),
            "original-day",
            Some(&historical),
        )
        .unwrap();
    assert_eq!(replay.state, EvidenceState::Observed);
    assert_eq!(
        replay.observations[0].outputs[0].rows[0]["production"],
        DataValue::Integer { value: 0 }
    );
    assert_eq!(
        replay.observations[0].view.rows[0].cells["production"],
        DataValue::Integer { value: 0 }
    );
    assert_eq!(store.load().unwrap(), current);

    let change = engine
        .prepare_managed_change(&store, prepared, &[], "future-rule")
        .unwrap();
    let adopted = engine.adopt(&store, &change).unwrap();
    assert!(!adopted.scope.correspondences.is_empty());
    assert_eq!(
        row(&adopted, &job).values["production"],
        DataValue::Integer { value: 0 }
    );
    let mut predating = adopted.clone();
    predating.clock_day = 20000;
    assert!(predating.validate().is_err());
    let reopened = ProductStore::open(&path).unwrap();
    let backup = product_backup::VerifiedBackup::capture(&reopened).unwrap();
    let recovered = backup.recover_new(&dir.path().join("recovered")).unwrap();
    let engine = DecisionEngine::new(
        LocalRuntime::default(),
        IntentArchive::new(recovered.clone()),
    );
    assert_eq!(
        engine
            .check_current(&recovered.load().unwrap())
            .unwrap()
            .disposition,
        CheckDisposition::Ready
    );
    // Daily work keeps the separate actual adoption-day capture, while the
    // durable old scene continues to reproduce its earlier-day result.
    tick(&recovered, "continue-clock", 20006);
    let continued = apply(&recovered, "continued-export", invoke("export", &[]));
    assert_eq!(
        continued.artifacts.last().unwrap().rows[0]["production"],
        DataValue::Integer { value: 3 }
    );
    assert_eq!(
        engine.check_current(&continued).unwrap().disposition,
        CheckDisposition::Ready
    );
    ProductStore::open(dir.path().join("recovered")).unwrap();
}

#[test]
fn admission_cache_reuses_only_fresh_exact_contexts() {
    use std::{io::Write, time::Instant};
    fn with_session(pause: bool) -> CapturedProgram {
        let mut raw = serde_json::to_value(program(pause).program).unwrap();
        raw["state"].as_array_mut().unwrap().push(serde_json::json!({"id":"scratch","label":"Session counter","value_type":{"kind":"integer"},"initial":{"kind":"integer","value":0}}));
        raw["actions"].as_array_mut().unwrap().push(serde_json::json!({"id":"change_session","label":"Change session","parameters":{},"guards":[],"steps":[{"kind":"set_state","state":"scratch","value":int(1)}],"ensures":[]}));
        capture(raw)
    }
    fn ready(engine: &DecisionEngine<LocalRuntime>, store: &ProductStore) {
        assert_eq!(
            engine
                .check_current(&store.load().unwrap())
                .unwrap()
                .disposition,
            CheckDisposition::Ready
        );
    }
    let dir = tempdir();
    let path = dir.path().join("tool");
    let store = ProductStore::create(&path, &with_session(false), 20000).unwrap();
    let before = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(
            &with_session(true),
            &request(&before, ScopePopulation::All),
            "initial-scope",
        )
        .unwrap();
    store.adopt_scoped(before.revision, &prepared).unwrap();
    // A fresh handle has no trusted proof result. Clones share only the one
    // verified immutable entry; a separately opened handle verifies afresh.
    let store = ProductStore::open(&path).unwrap();
    assert_eq!(store.validation_cache_stats(), (0, 0));
    let load_start = Instant::now();
    let loaded = store.load().unwrap();
    let cold_load = load_start.elapsed();
    let validated = store.validation_cache_stats();
    let load_start = Instant::now();
    assert_eq!(store.clone().load().unwrap(), loaded);
    let warm_load = load_start.elapsed();
    let reused = store.validation_cache_stats();
    assert!(reused.0 > validated.0);
    assert_eq!(reused.1, validated.1);
    let reopened = ProductStore::open(&path).unwrap();
    assert_eq!(reopened.validation_cache_stats(), (0, 0));
    assert_eq!(reopened.load().unwrap(), loaded);
    assert_eq!(reopened.validation_cache_stats().1, 1);
    let _ = writeln!(
        std::io::stderr(),
        "scope_snapshot_cache cold_load_ms={} warm_load_ms={} hits={} validations={}",
        cold_load.as_millis(),
        warm_load.as_millis(),
        reused.0,
        reused.1
    );
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let start = Instant::now();
    ready(&engine, &store);
    let cold = start.elapsed();
    let first = engine.scope_cache_stats();
    let start = Instant::now();
    ready(&engine, &store);
    let warm = start.elapsed();
    let second = engine.scope_cache_stats();
    assert!(second.0 > first.0);
    assert_eq!(second.1, first.1);
    let _ = writeln!(
        std::io::stderr(),
        "scope_admission_cache cold_ms={} warm_ms={} hits={} builds={}",
        cold.as_millis(),
        warm.as_millis(),
        second.0,
        second.1
    );

    let job = add(&store, "job", "Actual work after cached admission");
    ready(&engine, &store);
    let after_data = engine.scope_cache_stats();
    assert!(after_data.1 > second.1);
    tick(&store, "new-day", 20002);
    ready(&engine, &store);
    let after_day = engine.scope_cache_stats();
    assert!(after_day.1 > after_data.1);
    let session = apply(&store, "session", invoke("change_session", &[]));
    assert_eq!(
        session.session.values["scratch"],
        DataValue::Integer { value: 1 }
    );
    ready(&engine, &store);
    let after_session = engine.scope_cache_stats();
    assert!(after_session.1 > after_day.1);
    let current = store.load().unwrap();
    let scene = ScenarioSpec {
        version: 1,
        id: "cache-actual-scene".into(),
        label: "Actual output after data, clock and session changes".into(),
        seed: current.data.clone(),
        session: current.session.clone(),
        clock_day: current.clock_day,
        random_seed: 42,
        inputs: vec![
            invoke("export", &[]),
            SemanticInput::Observe {
                point: "result".into(),
            },
        ],
        validity: vec![],
    };
    let accepted = engine
        .accept_current_scene(&current, &scene, Disclosure::Synthetic)
        .unwrap();
    assert_eq!(
        accepted.observations()[0].outputs[0].rows[0]["production"],
        DataValue::Integer { value: 2 }
    );
    let choice = Choice {
        id: "cached-promise".into(),
        request: "Keep this actual export".into(),
        rationale: None,
        scope: DecisionScope {
            operations: ["export".into()].into_iter().collect(),
            population: Population::All,
            conditions: Values::new(),
            excluded_records: vec![],
            unknowns: vec![],
        },
        outcome: DecisionOutcome::KeepCurrent,
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    };
    let change = engine
        .prepare_choice(
            &store,
            current.program().unwrap(),
            choice,
            vec![accepted],
            "save-promise",
        )
        .unwrap();
    engine.adopt(&store, &change).unwrap();
    ready(&engine, &store);
    assert!(engine.scope_cache_stats().1 > after_session.1);

    let current = store.load().unwrap();
    let mut raw = serde_json::to_value(with_session(true).program).unwrap();
    raw["label"] = serde_json::json!("Renamed workflow with preserved behavior");
    let source = capture(raw);
    let source_id = canonical_digest(IdentityDomain::Source, &source).unwrap();
    let mappings: Vec<_> = current
        .editable_scope_context()
        .unwrap()
        .unwrap()
        .slots
        .iter()
        .map(|slot| ScopeSlotMapping {
            layer: slot.layer.clone(),
            patch: slot.patch,
            from_source: current.active_revision.clone(),
            from: slot.destination.clone(),
            to_source: source_id.clone(),
            to: slot.destination.clone(),
            subject: slot.subject.clone(),
        })
        .collect();
    let prepared = store
        .prepare_managed_evolution(&source, &mappings, "rename")
        .unwrap();
    let change = engine
        .prepare_managed_change(&store, prepared, &[], "rename")
        .unwrap();
    let changed = engine.adopt(&store, &change).unwrap();
    assert_ne!(changed.active_revision, current.active_revision);
    ready(&engine, &store);
    let cached = engine.scope_cache_stats();
    let pointer = std::fs::read(path.join("CURRENT")).unwrap();
    std::fs::write(path.join("CURRENT"), b"{}").unwrap();
    assert!(engine.check_current(&changed).is_err());
    std::fs::write(path.join("CURRENT"), &pointer).unwrap();
    ready(&engine, &store);
    assert!(engine.scope_cache_stats().1 > cached.1);
    assert_eq!(
        row(&store.load().unwrap(), &job).values["name"],
        text("Actual work after cached admission")
    );
    let healthy = store.load().unwrap();
    let pointer_json: serde_json::Value = serde_json::from_slice(&pointer).unwrap();
    let object = path.join(format!(
        "object-{}.json",
        pointer_json["object"].as_str().unwrap()
    ));
    let bytes = std::fs::read(&object).unwrap();
    let mut tampered: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    tampered["clock_day"] = serde_json::json!(healthy.clock_day + 1);
    let prior_validation = store.validation_cache_stats().1;
    std::fs::write(&object, serde_json::to_vec(&tampered).unwrap()).unwrap();
    assert!(
        store.load().is_err(),
        "unchanged CURRENT cannot hide changed object bytes"
    );
    std::fs::write(&object, &bytes).unwrap();
    assert_eq!(store.load().unwrap(), healthy);
    assert!(store.validation_cache_stats().1 > prior_validation);

    let alias = path.join("unexpected-hardlink");
    std::fs::hard_link(&object, &alias).unwrap();
    assert!(store.clone().load().is_err());
    std::fs::remove_file(&alias).unwrap();
    assert_eq!(store.load().unwrap(), healthy);
    #[cfg(unix)]
    {
        let original = path.join("original-object");
        std::fs::rename(&object, &original).unwrap();
        std::os::unix::fs::symlink(&original, &object).unwrap();
        assert!(store.load().is_err());
        std::fs::remove_file(&object).unwrap();
        std::fs::rename(&original, &object).unwrap();
        assert_eq!(store.load().unwrap(), healthy);
        let moved = dir.path().join("moved-tool");
        std::fs::rename(&path, &moved).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(store.load().is_err());
        std::fs::remove_dir(&path).unwrap();
        std::fs::rename(&moved, &path).unwrap();
        assert_eq!(store.load().unwrap(), healthy);
    }
    let witness = &healthy.decisions.decisions[0].witness;
    let extension = path.join(format!("extension-{}.json", witness.as_str()));
    let archived = std::fs::read(&extension).unwrap();
    std::fs::write(&extension, b"{}").unwrap();
    assert_eq!(store.load().unwrap(), healthy);
    assert!(match engine.check_current(&healthy) {
        Err(_) => true,
        Ok(report) => report.disposition != CheckDisposition::Ready,
    });
    assert!(product_backup::VerifiedBackup::capture(&store).is_err());
    std::fs::write(&extension, &archived).unwrap();
    ready(&engine, &store);
    let backup = product_backup::VerifiedBackup::capture(&store).unwrap();
    let recovered_path = dir.path().join("cache-recovery");
    backup.recover_new(&recovered_path).unwrap();
    let recovered = ProductStore::open(&recovered_path).unwrap();
    assert_eq!(recovered.validation_cache_stats(), (0, 0));
    assert_eq!(recovered.load().unwrap(), healthy);
    let checked = recovered.validation_cache_stats();
    assert_eq!(recovered.clone().load().unwrap(), healthy);
    assert_eq!(recovered.validation_cache_stats().1, checked.1);
    assert!(recovered.validation_cache_stats().0 > checked.0);

    store.poison_validation_cache_for_test();
    let checked = store.validation_cache_stats().1;
    assert_eq!(store.load().unwrap(), healthy);
    assert!(store.validation_cache_stats().1 > checked);
    let checked = store.validation_cache_stats().1;
    assert_eq!(store.clone().load().unwrap(), healthy);
    assert!(store.validation_cache_stats().1 > checked);
    assert_eq!(ProductStore::open(&path).unwrap().load().unwrap(), healthy);
}

#[test]
fn scoped_receipts_cannot_claim_later_independent_intentions() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let selected = add(&store, "selected", "Selected");
    action(&store, "wait", "wait", &selected);
    tick(&store, "days", 20003);
    let before = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(
            &program(true),
            &request(
                &before,
                ScopePopulation::SelectedUnfinished {
                    records: vec![RecordRef {
                        entity: selected.entity.clone(),
                        record: selected.id.clone(),
                    }],
                },
            ),
            "scope",
        )
        .unwrap();
    let layer = prepared.layer_id().unwrap().unwrap();
    let scene = selected_scene(&prepared, &selected);
    let scope = prepared.scope().clone();
    let artifact = prepared.target().artifact.program_digest.clone();
    let choice = |id: &str| Choice {
        id: id.into(),
        request: "Keep the experienced production result".into(),
        rationale: None,
        scope: scope.clone(),
        outcome: DecisionOutcome::Accept {
            artifact: artifact.clone(),
        },
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    };
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let accepted = engine
        .accept_scoped_scene(&store, &prepared, &scene, Disclosure::Synthetic)
        .unwrap();
    let first = engine
        .prepare_scoped_choice(
            &store,
            prepared,
            choice("original"),
            vec![accepted],
            "scope",
        )
        .unwrap();
    let current = engine.adopt(&store, &first).unwrap();
    let mut later_scene = scene;
    later_scene.id = "later-independent-scene".into();
    let accepted = engine
        .accept_current_scene(&current, &later_scene, Disclosure::Synthetic)
        .unwrap();
    let later = engine
        .prepare_choice(
            &store,
            current.program().unwrap(),
            choice("independent"),
            vec![accepted],
            "record-independent",
        )
        .unwrap();
    let healthy = engine.adopt(&store, &later).unwrap();
    assert_eq!(healthy.scope.adoptions[0].decisions, vec!["original"]);
    assert!(!healthy.adoptions[0]
        .plan
        .required_decisions
        .contains(&"independent".into()));
    assert!(healthy
        .adoptions
        .last()
        .unwrap()
        .plan
        .required_decisions
        .contains(&"independent".into()));

    let mut claims_later = healthy.clone();
    claims_later.scope.adoptions[0]
        .decisions
        .push("independent".into());
    let mut loses_original = healthy.clone();
    loses_original.scope.adoptions[0].decisions.clear();
    let mut rewrites_activation = claims_later.clone();
    let later_plan = rewrites_activation.adoptions.last().unwrap().plan.clone();
    let earlier = &mut rewrites_activation.adoptions[0].plan;
    earlier.required_decisions.push("independent".into());
    earlier.checks.extend(
        later_plan
            .checks
            .iter()
            .filter(|check| check.decision == "independent")
            .cloned(),
    );
    earlier.evidence.extend(later_plan.evidence);
    rewrites_activation.scope.adoptions[0].plan = earlier.identity().unwrap();
    // The later plan's exact prior graph still records only the original
    // promise, even when the older required-ID list and receipt are resealed.
    assert_eq!(
        rewrites_activation
            .adoptions
            .last()
            .unwrap()
            .plan
            .expected_decisions,
        current.decisions.identity().unwrap()
    );
    for (index, corrupted) in [claims_later, loses_original, rewrites_activation]
        .iter()
        .enumerate()
    {
        assert!(corrupted.validate().is_err());
        let destination = dir.path().join(format!("forged-{index}"));
        let activated = std::cell::Cell::new(false);
        assert!(
            ProductStore::create_recovered_with(&destination, corrupted, |_| {
                activated.set(true);
                Ok(())
            })
            .is_err()
        );
        assert!(!activated.get());
        assert!(!destination.exists());
        assert_eq!(store.load().unwrap(), healthy);
    }
    let backup = product_backup::VerifiedBackup::capture(&store).unwrap();
    let restored = backup
        .recover_new(&dir.path().join("healthy-recovery"))
        .unwrap();
    let restored_engine = DecisionEngine::new(
        LocalRuntime::default(),
        IntentArchive::new(restored.clone()),
    );
    assert_eq!(restored.load().unwrap(), healthy);
    assert_eq!(
        restored_engine.check_current(&healthy).unwrap().disposition,
        CheckDisposition::Ready
    );
    // Removing this behavior would break the separately recorded promise. It
    // must be rechecked, rather than silently retired through the old receipt.
    assert!(restored_engine
        .prepare_scoped_withdrawal(&restored, &[layer], "withdraw")
        .is_err());
    assert_eq!(restored.load().unwrap(), healthy);
    assert_eq!(
        healthy
            .decisions
            .decisions
            .iter()
            .find(|d| d.id == "independent")
            .unwrap()
            .status,
        DecisionStatus::Active
    );
    add(&restored, "continued", "Continued independent work");
    assert_eq!(store.load().unwrap(), healthy);
}
