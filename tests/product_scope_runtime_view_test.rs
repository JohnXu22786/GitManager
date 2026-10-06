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
    verify_managed_discovery(&store, &engine, &managed);
    let change = engine
        .prepare_managed_change(&store, managed, &[], "fresh-implementation")
        .unwrap();
    assert_eq!(change.report().disposition, CheckDisposition::Ready);
    engine.adopt(&store, &change).unwrap();
    let annotated = action(&store, "new-fact", "annotate", &selected);
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
