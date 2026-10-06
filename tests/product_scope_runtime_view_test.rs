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
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
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
            candidates: vec![],
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
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
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
                observable: "reminder".into(),
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
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
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
    use product_discovery::{discover, DiscoveryPolicy, VerifiedRetainedHistory};
    use std::sync::{atomic::AtomicBool, Arc};
    let dir = tempfile::tempdir().unwrap();
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
    let current = engine.adopt(&store, &change).unwrap();
    let mut raw = serde_json::to_value(program(true).program).unwrap();
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
    let prepared = store
        .prepare_scoped_change(
            &capture(raw),
            &request(&current, ScopePopulation::All),
            "new-timing",
        )
        .unwrap();
    let projected = ScenarioSpec {
        seed: prepared.seed().clone(),
        ..scene.clone()
    };
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
                observable: "production".into(),
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
    assert!(!report.questions.is_empty(), "{:?}", report);
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
