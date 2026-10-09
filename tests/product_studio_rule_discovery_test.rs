//! Production interpreter/store/decision integration; no live-provider claims.
#[path = "../src/product_studio/change_adapter.rs"]
mod change_adapter;
#[path = "../src/product_studio/discovery_flow.rs"]
mod discovery_flow;
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
#[path = "../src/product_scenarios/mod.rs"]
mod product_scenarios;
#[path = "../src/product_store/mod.rs"]
mod product_store;
#[path = "../src/product_studio/rule_discovery.rs"]
mod rule_discovery;
use fixture::*;
use product_contract::*;
use product_decisions::*;
use product_discovery::*;
use product_runtime::LocalRuntime;
use product_store::{scope::*, ProductStore};
use rule_discovery::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[test]
fn checked_history_replay_keeps_created_ids() {
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
            retained_history: Some(history.clone()),
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
    // A witness's projected seed is insufficient: replay also needs its
    // authenticated operation frame, or created references name missing rows.
    for witness in report.questions.iter().flat_map(|q| &q.witnesses) {
        let checked = history
            .checked_witness_replay(&prepared, witness, cancel())
            .unwrap();
        let context = checked.admission();
        let mut wrong_frame = witness.witness().scenario.clone();
        wrong_frame.clock_day += 1;
        assert!(context
            .replay_operation_ids(witness.before_program(), &wrong_frame)
            .is_err());
        wrong_frame = witness.witness().scenario.clone();
        wrong_frame.random_seed += 1;
        assert!(context
            .replay_operation_ids(witness.after_program(), &wrong_frame)
            .is_err());
        let wrong = store
            .prepare_scoped_change(
                &program(true),
                &fixture::request(&current, ScopePopulation::All),
                "prospective",
            )
            .unwrap();
        assert!(history
            .checked_witness_replay(&wrong, witness, cancel())
            .is_err());
        let cancelled = cancel();
        cancelled.store(true, Ordering::Release);
        assert!(history
            .checked_witness_replay(&prepared, witness, cancelled)
            .is_err());
        for (source, expected) in [
            (witness.before_program(), &witness.witness().before),
            (witness.after_program(), &witness.witness().after),
        ] {
            let actual = runtime
                .replay_admitted(
                    source,
                    &witness.witness().scenario,
                    &current.decisions,
                    RuntimeLimits::default(),
                    "exact-reopened-witness",
                    Some(context.as_ref()),
                )
                .unwrap();
            assert_eq!(actual.state, EvidenceState::Observed, "{:?}", actual.errors);
            assert_eq!(actual.observations, expected.observations);
        }
    }
}

fn cancel() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}
fn engine(store: &ProductStore) -> DecisionEngine<LocalRuntime> {
    DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()))
}
fn modify(
    store: &ProductStore,
    candidate: &CapturedProgram,
    id: &str,
) -> (DevelopmentRequest, DevelopmentResult) {
    let current = store.load().unwrap();
    let req = engine(store)
        .development_request(
            &current,
            &format!("modify-{id}"),
            DevelopmentOperation::Modify,
            "Change how waiting contributes to production; preserve commitments",
            DevelopmentContext {
                view: Some("work".into()),
                selected: vec![],
                recent_inputs: vec![],
                data_digest: Some(current.data.identity().unwrap()),
                session_digest: Some(current.session.identity().unwrap()),
            },
        )
        .unwrap();
    let result = DevelopmentResult {
        producer: candidate.binding.producer.clone(),
        response: DevelopmentResponse {
            version: 1,
            request_digest: req.identity().unwrap(),
            candidates: vec![GeneratedCandidate {
                id: "changed".into(),
                source_json: String::from_utf8(candidate.source_bytes.clone()).unwrap(),
            }],
            hypotheses: vec![],
            evolutions: vec![],
            unsupported: vec![],
        },
    };
    (req, result)
}
fn draft_from(
    store: &ProductStore,
    req: DevelopmentRequest,
    result: DevelopmentResult,
    population: ScopePopulation,
    id: &str,
) -> RuleDiscoveryDraft {
    let snapshot = store.load().unwrap();
    let source = CapturedProgram::capture(
        result.response.candidates[0].source_json.as_bytes(),
        &req.project_id,
        result.producer.clone(),
        None,
    )
    .unwrap();
    let scope = change_adapter::request(
        &snapshot,
        &source,
        population,
        fixture::request(&snapshot, ScopePopulation::All).lifecycles,
        Default::default(),
    )
    .unwrap();
    let prepared = store.prepare_scoped_change(&source, &scope, id).unwrap();
    let selection =
        RuleSelection::checked(store, &snapshot, prepared, scope, id, cancel()).unwrap();
    RuleDiscoveryDraft::after_modify(
        store,
        &snapshot,
        req,
        result,
        "changed",
        selection,
        &format!("discover-{id}"),
        cancel(),
    )
    .unwrap()
}
fn draft(
    store: &ProductStore,
    candidate: &CapturedProgram,
    population: ScopePopulation,
    id: &str,
) -> RuleDiscoveryDraft {
    let (req, result) = modify(store, candidate, id);
    draft_from(store, req, result, population, id)
}
fn created_scene(current: &product_store::ProjectSnapshot) -> (ScenarioSpec, Record) {
    let mut scene = ScenarioSpec {
        version: 1,
        id: "new-waiting-work".into(),
        label: "New work and its customer commitment".into(),
        seed: current.data.clone(),
        session: current.session.clone(),
        clock_day: current.clock_day,
        random_seed: 42,
        inputs: vec![invoke(
            "add",
            &[
                ("name", text("Example new work")),
                ("promised", DataValue::Date { days: 20020 }),
            ],
        )],
        validity: vec![],
    };
    let context = ScopedExecutionContext::committed(current).unwrap();
    let runtime = LocalRuntime::default().with_admission(Arc::new(context));
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
        .find(|r| r.values["name"] == text("Example new work"))
        .unwrap()
        .clone();
    scene.inputs.extend([
        SemanticInput::Observe {
            point: "unrelated".into(),
        },
        invoke("wait", &[("row", reference(&created))]),
        SemanticInput::AdvanceClock { days: 3 },
        invoke("calculate", &[("row", reference(&created))]),
        invoke("complete", &[("row", reference(&created))]),
        invoke("export", &[]),
        SemanticInput::Observe {
            point: "result".into(),
        },
    ]);
    (scene, created)
}
fn discovery_result(draft: &RuleDiscoveryDraft, scene: &ScenarioSpec) -> DevelopmentResult {
    let before = &draft.request().sources[0];
    let after = &draft.request().sources[1];
    // The compact fixture changes the stored calculation, while the original
    // fixture changes the displayed/exported expression directly.
    let (action, pointer) = if draft.selection().request().operations.contains("export") {
        ("export", "/views/0/kind/columns/1/value")
    } else {
        ("calculate", "/actions/3")
    };
    DevelopmentResult {
        producer: Producer::Fixture {
            name: "Offline discover result".into(),
        },
        response: DevelopmentResponse {
            version: 1,
            request_digest: draft.request().identity().unwrap(),
            candidates: vec![
                GeneratedCandidate {
                    id: "before".into(),
                    source_json: String::from_utf8(before.source_bytes.clone()).unwrap(),
                },
                GeneratedCandidate {
                    id: "after".into(),
                    source_json: String::from_utf8(after.source_bytes.clone()).unwrap(),
                },
            ],
            hypotheses: vec![ChoiceHypothesis {
                id: "waiting-choice".into(),
                statement: "Waiting changes production calculations".into(),
                kind: HypothesisKind::UnresolvedChoice,
                action: action.into(),
                observable: "waiting_jobs".into(),
                sources: vec![SourceLocus {
                    relative_path: after.binding.program_path.clone(),
                    raw_digest: after.artifact.raw_digest.clone(),
                    pointer: pointer.into(),
                }],
                alternatives: vec!["before".into(), "after".into()],
                related_decisions: vec![],
                scenario_json: serde_json::to_string(scene).unwrap(),
                unknowns: vec![],
            }],
            evolutions: vec![],
            unsupported: vec![],
        },
    }
}
fn queue(store: &ProductStore, d: &RuleDiscoveryDraft, scene: &ScenarioSpec) -> RuleDiscoveryQueue {
    d.evaluate(
        store,
        d.selection(),
        discovery_result(d, scene),
        DiscoveryPolicy::default(),
        vec![],
        cancel(),
    )
    .unwrap()
}
fn first_copy(
    store: &ProductStore,
    d: &RuleDiscoveryDraft,
    q: &RuleDiscoveryQueue,
) -> RuleCopyTrace {
    assert!(!q.report().questions.is_empty(), "{:?}", q.report());
    q.copy_trace(
        store,
        d.selection(),
        &q.report().questions[0].id,
        0,
        cancel(),
    )
    .unwrap()
}
#[cfg(unix)]
fn transport(
    root: &std::path::Path,
    request: &DevelopmentRequest,
    result: &DevelopmentResult,
) -> (DevelopmentResult, product_provider::JobReceipt) {
    use product_provider::{unix_ms, ConsentReceipt, ProviderKind, ProviderTransport};
    use std::{fs, os::unix::fs::PermissionsExt};
    fs::create_dir_all(root).unwrap();
    let bin = root.join("fixture.py");
    let script = include_str!("fixtures/provider_transport/fake_cli.py").replace(
        "{'passed': True, 'text': wire['prompt'], 'command': 'untrusted-do-not-execute'}",
        "cfg['response']",
    );
    {
        let _guard = product_provider::fixture_executable_write_guard();
        fs::write(&bin, script).unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(
            bin.with_extension("json"),
            serde_json::to_vec(&serde_json::json!({"response":result.response})).unwrap(),
        )
        .unwrap();
    }
    let home = root.join("home");
    fs::create_dir(&home).unwrap();
    let t =
        ProviderTransport::new_fixture(root.join("jobs"), ProviderKind::Codex, bin, home).unwrap();
    let p = prepare_development(t, request, ProviderOptions::default()).unwrap_or_else(|error| {
        panic!(
            "request {} ({} serialized bytes): {error:?}",
            request.id,
            serde_json::to_vec(request).unwrap().len()
        )
    });
    let c = ConsentReceipt {
        disclosure_digest: p.disclosure().digest(),
        approval_reference: "Fictional offline test only".into(),
        expires_at_unix_ms: unix_ms() + 60_000,
    };
    let provider = p.authorize(c);
    let result = provider.develop(request, &|| false).unwrap();
    (result, provider.receipt().unwrap())
}

#[cfg(unix)]
#[test]
fn first_layers_use_actual_modify_discover_transport_and_distinct_copy_authority() {
    for population in [ScopePopulation::All, ScopePopulation::FutureWork] {
        let dir = tempdir();
        let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
        add(&store, "existing", "Existing saved work");
        let current = store.load().unwrap();
        let (req, result) = modify(&store, &program(true), "primary");
        let (result, modify_receipt) = transport(&dir.path().join("modify"), &req, &result);
        let d = draft_from(
            &store,
            req.clone(),
            result.clone(),
            population.clone(),
            "primary",
        );
        assert_eq!(d.modify_request(), &req);
        assert_eq!(d.modify_result(), &result);
        assert_eq!(d.selection().request().population, population);
        assert_eq!(
            d.selection().preparation().candidate().binding.producer,
            result.producer
        );
        assert_eq!(d.request().operation, DevelopmentOperation::Discover);
        assert_ne!(d.request().identity().unwrap(), req.identity().unwrap());
        let (scene, created) = created_scene(&current);
        let (result, receipt) = transport(
            &dir.path().join("discover"),
            d.request(),
            &discovery_result(&d, &scene),
        );
        assert_ne!(receipt.request_id, modify_receipt.request_id);
        assert_eq!(
            receipt.state,
            product_provider::JobState::TransportValidated
        );
        let q = d
            .evaluate(
                &store,
                d.selection(),
                result,
                DiscoveryPolicy::default(),
                vec![],
                cancel(),
            )
            .unwrap();
        assert!(!q.report().questions.is_empty(), "{:?}", q.report());
        assert!(
            q.report().lowerings.is_empty(),
            "Modify must not be relabeled as Discover authorship"
        );
        let question = &q.report().questions[0];
        let witness = &question.witnesses[0];
        let mut example = q
            .open_example(&store, d.selection(), &question.id, 0, cancel())
            .unwrap();
        assert_eq!(example.scenario(), &witness.witness().scenario);
        assert_eq!(
            example.playback().unwrap().labels,
            ["Current", "Alternative"]
        );
        assert!(example
            .evidence()
            .unwrap()
            .iter()
            .all(|run| run.observations.iter().any(|o| o
                .view
                .rows
                .iter()
                .any(|r| r.record.record == created.id))));
        let copy = q
            .copy_trace(&store, d.selection(), &question.id, 0, cancel())
            .unwrap();
        assert_eq!(copy.original_scenario().seed, current.data);
        assert_eq!(copy.original_scenario().clock_day, current.clock_day);
        assert_eq!(
            copy.preparation().scope(),
            d.selection().preparation().scope()
        );
        copy.check(&store, d.selection(), cancel()).unwrap();
        assert!(copy
            .evidence()
            .iter()
            .all(|run| run.state == EvidenceState::Observed));
        example
            .trial(
                &store,
                d.selection(),
                SemanticInput::AdvanceClock { days: 1 },
                cancel(),
            )
            .unwrap();
        assert!(!example.playback().unwrap().minimal);
        assert_eq!(store.load().unwrap(), current);
    }
}

#[test]
fn changed_scope_lifecycle_operation_and_cancel_cannot_borrow_a_queue() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    add(&store, "existing", "Existing");
    let current = store.load().unwrap();
    let d = draft(
        &store,
        &program(true),
        ScopePopulation::FutureWork,
        "primary",
    );
    for (population, operation) in [
        (ScopePopulation::All, "primary"),
        (ScopePopulation::FutureWork, "other-operation"),
    ] {
        let scope = request(&current, population);
        let p = store
            .prepare_scoped_change(d.selection().preparation().candidate(), &scope, operation)
            .unwrap();
        let wrong =
            RuleSelection::checked(&store, &current, p, scope, operation, cancel()).unwrap();
        assert!(d
            .evaluate(
                &store,
                &wrong,
                discovery_result(&d, &created_scene(&current).0),
                DiscoveryPolicy::default(),
                vec![],
                cancel()
            )
            .is_err());
    }
    let mut missing = d.selection().request().clone();
    missing.lifecycles.clear();
    let error = RuleSelection::checked(
        &store,
        &current,
        d.selection().preparation().clone(),
        missing,
        "primary",
        cancel(),
    )
    .err()
    .unwrap();
    assert!(error.to_lowercase().contains("finished"), "{error}");
    let mut wrong = d.selection().request().clone();
    wrong.lifecycles[0].completed = boolean(false);
    assert!(RuleSelection::checked(
        &store,
        &current,
        d.selection().preparation().clone(),
        wrong,
        "primary",
        cancel()
    )
    .is_err());
    let cancelled = cancel();
    cancelled.store(true, Ordering::Release);
    assert!(d
        .evaluate(
            &store,
            d.selection(),
            discovery_result(&d, &created_scene(&current).0),
            DiscoveryPolicy::default(),
            vec![],
            cancelled
        )
        .is_err());
    tick(&store, "later", 20001);
    assert!(d
        .evaluate(
            &store,
            d.selection(),
            discovery_result(&d, &created_scene(&current).0),
            DiscoveryPolicy::default(),
            vec![],
            cancel()
        )
        .is_err());
}

#[test]
fn duplicate_observation_invalidates_example_and_copy_checks_current_data() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    add(&store, "existing", "Existing");
    let current = store.load().unwrap();
    let d = draft(
        &store,
        &program(true),
        ScopePopulation::FutureWork,
        "primary",
    );
    let q = queue(&store, &d, &created_scene(&current).0);
    let question = &q.report().questions[0];
    let mut e = q
        .open_example(&store, d.selection(), &question.id, 0, cancel())
        .unwrap();
    let point = e
        .scenario()
        .inputs
        .iter()
        .find_map(|i| {
            if let SemanticInput::Observe { point } = i {
                Some(point.clone())
            } else {
                None
            }
        })
        .unwrap();
    assert!(e
        .trial(
            &store,
            d.selection(),
            SemanticInput::Observe { point },
            cancel()
        )
        .is_err());
    assert!(e.playback().is_err());
    assert!(e.evidence().is_err());
    let copy = first_copy(&store, &d, &q);
    add(&store, "later", "Later saved work");
    assert!(copy.check(&store, d.selection(), cancel()).is_err());
    assert!(q
        .open_example(&store, d.selection(), &question.id, 0, cancel())
        .is_err());
}

fn compact_program(pause: bool) -> CapturedProgram {
    // A stored production result is the sole changed semantic slot. Completion
    // preserves that result; both the view and export expose it unchanged.
    let mut raw = serde_json::to_value(program(pause).program).unwrap();
    raw["actions"][4]["steps"][0]["values"]
        .as_object_mut()
        .unwrap()
        .remove("production");
    raw["actions"][6]["steps"][0]["columns"]["production"] =
        serde_json::to_value(field("row", "production")).unwrap();
    raw["views"][0]["kind"]["columns"][1]["value"] =
        serde_json::to_value(field("row", "production")).unwrap();
    capture(raw)
}
fn managed(store: &ProductStore, compact: bool) -> (Record, Record, CapturedProgram) {
    let selected = add(store, "selected", "Selected commitment");
    let archived = add(store, "archived", "Completed history");
    action(store, "finish-old", "complete", &archived);
    action(store, "archive-old", "archive", &archived);
    action(store, "wait-selected", "wait", &selected);
    tick(store, "day", 20003);
    let current = store.load().unwrap();
    let authored = if compact {
        compact_program(true)
    } else {
        program(true)
    };
    let population = ScopePopulation::SelectedUnfinished {
        records: vec![RecordRef {
            entity: selected.entity.clone(),
            record: selected.id.clone(),
        }],
    };
    let scope = change_adapter::request(
        &current,
        &authored,
        population,
        fixture::request(&current, ScopePopulation::All).lifecycles,
        Default::default(),
    )
    .unwrap();
    let first = store
        .prepare_scoped_change(&authored, &scope, "selected-rule")
        .unwrap();
    let scene = ScenarioSpec {
        version: 1,
        id: "selected-promise".into(),
        label: "Keep selected timing and commitment".into(),
        seed: first.seed().clone(),
        session: current.session.clone(),
        clock_day: current.clock_day,
        random_seed: 42,
        inputs: vec![
            invoke("calculate", &[("row", reference(&selected))]),
            invoke("complete", &[("row", reference(&selected))]),
            invoke("export", &[]),
            SemanticInput::Observe {
                point: "result".into(),
            },
        ],
        validity: vec![],
    };
    let e = engine(store);
    let accepted = e
        .accept_scoped_scene(store, &first, &scene, Disclosure::Synthetic)
        .unwrap();
    let choice = Choice {
        id: "selected-promise".into(),
        request: "Keep selected timing, customer commitment and completed history".into(),
        rationale: None,
        scope: first.scope().clone(),
        outcome: DecisionOutcome::Accept {
            artifact: first.target().artifact.program_digest.clone(),
        },
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    };
    let change = e
        .prepare_scoped_choice(store, first, choice, vec![accepted], "selected-rule")
        .unwrap();
    e.adopt(store, &change).unwrap();
    let mut raw = serde_json::to_value(authored.program).unwrap();
    let pointers: &[&str] = if compact {
        &["/actions/3/steps/0/values/production"]
    } else {
        &[
            "/actions/3/steps/0/values/production",
            "/actions/4/steps/0/values/production",
            "/actions/6/steps/0/columns/production",
            "/views/0/kind/columns/1/value",
        ]
    };
    for pointer in pointers {
        let old = raw.pointer(pointer).unwrap().clone();
        *raw.pointer_mut(pointer).unwrap() =
            serde_json::json!({"kind":"add","left":old,"right":int(1)});
    }
    (selected, archived, capture(raw))
}
fn choice(copy: &RuleCopyTrace, outcome: DecisionOutcome, id: &str) -> Choice {
    Choice {
        id: id.into(),
        request: "Retain these experienced waiting-rule outcomes".into(),
        rationale: None,
        scope: copy.preparation().scope().clone(),
        outcome,
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    }
}
fn scenes(store: &ProductStore, copy: &RuleCopyTrace) -> Vec<AcceptedScene> {
    let e = engine(store);
    vec![
        e.accept_prepared_current_scene(
            store,
            copy.preparation(),
            copy.scenario(),
            Disclosure::Synthetic,
        )
        .unwrap(),
        e.accept_scoped_scene(
            store,
            copy.preparation(),
            copy.scenario(),
            Disclosure::Synthetic,
        )
        .unwrap(),
    ]
}
#[test]
fn managed_future_four_pending_outcomes_inherit_and_resolve_after_later_work() {
    for outcome in [
        DecisionOutcome::EitherAcceptable,
        DecisionOutcome::BothNeeded,
        DecisionOutcome::NeitherFits,
        DecisionOutcome::Deferred,
    ] {
        let dir = tempdir();
        let path = dir.path().join("tool");
        let store = ProductStore::create(&path, &compact_program(false), 20000).unwrap();
        let (selected, archived, candidate) = managed(&store, true);
        let current = store.load().unwrap();
        let (req, result) = modify(&store, &candidate, "future-rule");
        #[cfg(unix)]
        let (result, _) = transport(&dir.path().join("modify"), &req, &result);
        let d = draft_from(
            &store,
            req,
            result,
            ScopePopulation::FutureWork,
            "future-rule",
        );
        let original = created_scene(&current).0;
        let result = discovery_result(&d, &original);
        #[cfg(unix)]
        let (result, _) = transport(&dir.path().join("discover"), d.request(), &result);
        let q = d
            .evaluate(
                &store,
                d.selection(),
                result,
                DiscoveryPolicy::default(),
                vec![],
                cancel(),
            )
            .unwrap();
        let copy = first_copy(&store, &d, &q);
        let accepted = scenes(&store, &copy);
        for scene in &accepted {
            let rows = &scene.observations().last().unwrap().outputs[0].rows;
            let selected_output = rows
                .iter()
                .find(|r| r["name"] == text("Selected commitment"))
                .unwrap();
            assert_eq!(
                selected_output["production"],
                DataValue::Integer { value: 0 }
            );
            assert_eq!(selected_output["promised"], DataValue::Date { days: 20020 });
        }
        let e = engine(&store);
        let change = e
            .prepare_rehearsed_choice(
                &store,
                copy.preparation().clone(),
                choice(&copy, outcome.clone(), "pending-rule"),
                accepted,
                &[],
                "record-pending",
            )
            .unwrap();
        let saved = e.adopt(&store, &change).unwrap();
        assert_eq!(saved.data, current.data);
        assert_eq!(saved.session, current.session);
        assert_eq!(saved.clock_day, current.clock_day);
        assert_eq!(saved.active_revision, current.active_revision);
        assert_eq!(saved.artifacts, current.artifacts);
        assert_eq!(saved.scope.layers, current.scope.layers);
        assert_eq!(saved.scope.initializations, current.scope.initializations);
        assert_eq!(saved.scope.compositions, current.scope.compositions);
        assert_eq!(saved.scope.adoptions, current.scope.adoptions);
        assert_eq!(saved.decisions.decisions[0], current.decisions.decisions[0]);
        let pending = saved
            .decisions
            .decisions
            .iter()
            .find(|d| d.id == "pending-rule")
            .unwrap();
        assert_eq!(pending.status, DecisionStatus::Pending);
        assert_eq!(pending.outcome, outcome);
        let reopened = ProductStore::open(&path).unwrap();
        assert_eq!(reopened.load().unwrap(), saved);
        assert_eq!(
            saved
                .scope
                .rehearsals
                .values()
                .filter(|r| r.witnesses.contains_key("pending-rule"))
                .count(),
            1
        );
        // A new actual request inherits the retained pair rather than asking
        // again about the same unchanged business consequences.
        let (next_req, next_result) =
            modify(&reopened, copy.preparation().candidate(), "next-rule");
        assert!(next_req
            .decisions
            .decisions
            .iter()
            .any(|d| d.id == "pending-rule"));
        assert!(!next_req.accepted_scenes.is_empty());
        #[cfg(unix)]
        let (next_result, _) = transport(&dir.path().join("next-modify"), &next_req, &next_result);
        let next = draft_from(
            &reopened,
            next_req,
            next_result,
            ScopePopulation::FutureWork,
            "next-rule",
        );
        let next_result = discovery_result(&next, &original);
        #[cfg(unix)]
        let (next_result, _) = transport(
            &dir.path().join("next-discover"),
            next.request(),
            &next_result,
        );
        let repeated = next
            .evaluate(
                &reopened,
                next.selection(),
                next_result,
                DiscoveryPolicy::default(),
                vec![],
                cancel(),
            )
            .unwrap();
        assert!(
            repeated.report().questions.is_empty(),
            "{:?}",
            repeated.report()
        );
        assert!(
            repeated
                .report()
                .log
                .iter()
                .any(|l| l.disposition == Disposition::Settled),
            "{:?}",
            repeated.report()
        );
        let late = add(&reopened, "late", "Later real work");
        action(&reopened, "late-wait", "wait", &late);
        action(&reopened, "complete-selected", "complete", &selected);
        let facts = reopened.load().unwrap();
        assert_eq!(row(&facts, &archived), row(&current, &archived));
        let reopened = ProductStore::open(&path).unwrap();
        assert_eq!(reopened.load().unwrap(), facts);
        // Reopen through the retained one-layer proof, exactly as the host
        // does, and freshly prepare against the later saved work.
        let proof = facts
            .scope
            .rehearsals
            .values()
            .find(|p| p.witnesses.contains_key("pending-rule"))
            .unwrap();
        let layer = proof.layer.as_ref().unwrap();
        let source = facts
            .programs
            .iter()
            .find(|p| canonical_digest(IdentityDomain::Source, *p).unwrap() == layer.candidate)
            .unwrap();
        let mut scope = layer.request.clone();
        for lifecycle in &mut scope.lifecycles {
            lifecycle.source = facts.active_revision.clone();
        }
        assert_eq!(scope.population, ScopePopulation::FutureWork);
        let mut fresh = reopened
            .prepare_scoped_change(source, &scope, "resolve-rule")
            .unwrap();
        let original = created_scene(&facts).0;
        let mut mapped = original.clone();
        mapped.seed = product_runtime::merged_data(fresh.target(), &original.seed).unwrap();
        let (context, actual) = ScopedExecutionContext::prepared(&facts, &fresh)
            .unwrap()
            .project_scenario(facts.program().unwrap(), &original, fresh.target(), &mapped)
            .unwrap();
        fresh.correspondences = context.correspondence_proofs();
        let alternative = engine(&reopened)
            .accept_scoped_scene(&reopened, &fresh, &actual, Disclosure::Synthetic)
            .unwrap();
        let accepted = Choice {
            id: "resolved-rule".into(),
            request: "Adopt this freshly experienced future rule".into(),
            rationale: None,
            scope: fresh.scope().clone(),
            outcome: DecisionOutcome::Accept {
                artifact: fresh.target().artifact.program_digest.clone(),
            },
            obligations: vec![],
            binding: IntentionBinding::ObservedOutcome,
        };
        let change = engine(&reopened)
            .prepare_scoped_resolution(
                &reopened,
                fresh,
                accepted,
                vec![alternative],
                &["pending-rule".into()],
                "resolve-rule",
            )
            .unwrap();
        let adopted = engine(&reopened).adopt(&reopened, &change).unwrap();
        assert_eq!(adopted.data.events, facts.data.events);
        assert_eq!(row(&adopted, &late).values["name"], text("Later real work"));
        assert_eq!(row(&adopted, &archived), row(&facts, &archived));
        assert_eq!(adopted.decisions.decisions[0], facts.decisions.decisions[0]);
        assert_eq!(
            adopted
                .decisions
                .decisions
                .iter()
                .find(|d| d.id == "pending-rule")
                .unwrap()
                .status,
            DecisionStatus::Superseded {
                by: "resolved-rule".into()
            }
        );
        assert_eq!(
            engine(&reopened)
                .check_current(&adopted)
                .unwrap()
                .disposition,
            CheckDisposition::Ready
        );
        assert_eq!(ProductStore::open(&path).unwrap().load().unwrap(), adopted);
    }
}

#[test]
fn observation_only_minimum_uses_its_checked_action_bearing_original() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let row = add(&store, "row", "Waiting");
    action(&store, "wait", "wait", &row);
    tick(&store, "days", 20003);
    let current = store.load().unwrap();
    let d = draft(&store, &program(true), ScopePopulation::All, "primary");
    let mut scene = ScenarioSpec {
        version: 1,
        id: "observation-minimum".into(),
        label: "Observe an exported timing result".into(),
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
    let q = queue(&store, &d, &scene);
    let (question, index, witness) = q
        .report()
        .questions
        .iter()
        .find_map(|q| {
            q.witnesses
                .iter()
                .enumerate()
                .find(|(_, w)| {
                    w.witness()
                        .scenario
                        .inputs
                        .iter()
                        .all(|i| matches!(i, SemanticInput::Observe { .. }))
                })
                .map(|(index, w)| (q, index, w))
        })
        .expect("The real exported example must reduce to an observation-only view witness");
    let example = q
        .open_example(&store, d.selection(), &question.id, index, cancel())
        .unwrap();
    assert_eq!(example.scenario(), &witness.witness().scenario);
    let copy = q
        .copy_trace(&store, d.selection(), &question.id, index, cancel())
        .unwrap();
    assert!(copy
        .original_scenario()
        .inputs
        .iter()
        .any(|i| matches!(i,SemanticInput::Invoke { action, .. } if action=="export")));
    assert_ne!(
        copy.original_scenario().inputs,
        witness.witness().scenario.inputs
    );
    assert!(copy
        .evidence()
        .iter()
        .all(|r| r.state == EvidenceState::Observed));
    // A provider's actionless original is correctly irrelevant. It cannot
    // manufacture a question or a scoped adoption merely from a visible value.
    scene.inputs.remove(0);
    let actionless = queue(&store, &d, &scene);
    assert!(actionless.report().questions.is_empty());
    assert!(actionless
        .report()
        .log
        .iter()
        .any(|l| l.disposition == Disposition::Irrelevant));
    assert_eq!(store.load().unwrap(), current);
}

#[test]
fn oversized_managed_context_is_refused_before_transport_without_changing_work() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let (_, _, candidate) = managed(&store, false);
    let current = store.load().unwrap();
    let (req, result) = modify(&store, &candidate, "wide-managed");
    let modify_wire = encode_request(&req, &ProviderOptions::default()).unwrap();
    let d = draft_from(
        &store,
        req,
        result,
        ScopePopulation::FutureWork,
        "wide-managed",
    );
    let req = d.request();
    let bytes = serde_json::to_vec(req).unwrap().len();
    let error = encode_request(req, &ProviderOptions::default())
        .err()
        .expect("The full multi-slot retained context exceeds the fixed transport bound");
    eprintln!(
        "Bounded managed {:?} request {}: {} serialized request bytes; preceding Modify prompt {} bytes; fixed prompt cap {} bytes; {error:?}",
        req.operation, req.id, bytes, modify_wire.prompt.len(), product_provider::MAX_PROMPT_BYTES
    );
    assert!(format!("{error:?}").contains("prompt must be bounded"));
    assert_eq!(store.load().unwrap(), current);
}

#[test]
fn actual_lowerings_preserve_two_new_rule_refusal_and_reject_forged_result() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    add(&store, "existing", "Existing");
    let current = store.load().unwrap();
    let d = draft(
        &store,
        &program(true),
        ScopePopulation::FutureWork,
        "primary",
    );
    let mut result = discovery_result(&d, &created_scene(&current).0);
    result.response.candidates.push(GeneratedCandidate {
        id: "other".into(),
        source_json: String::from_utf8(program(true).source_bytes).unwrap(),
    });
    result.response.hypotheses[0].alternatives = vec!["after".into(), "other".into()];
    let authored = CapturedProgram::capture(
        result.response.candidates[2].source_json.as_bytes(),
        &current.data.project_id,
        result.producer.clone(),
        None,
    )
    .unwrap();
    let other = store
        .prepare_scoped_change(&authored, &request(&current, ScopePopulation::All), "other")
        .unwrap();
    assert!(
        ScopedExecutionContext::rehearsed_pair(&current, d.selection().preparation(), &other)
            .is_err()
    );
    let lowering = PreparedDiscoveryCandidate::from_result(
        &current,
        d.request(),
        &result,
        "other",
        other,
        vec![],
    )
    .unwrap();
    assert!(PreparedDiscoveryCandidate::from_result(
        &current,
        d.request(),
        &result,
        "other",
        d.selection().preparation().clone(),
        vec![]
    )
    .is_err());
    let q = d
        .evaluate(
            &store,
            d.selection(),
            result.clone(),
            DiscoveryPolicy::default(),
            vec![lowering.clone()],
            cancel(),
        )
        .unwrap();
    assert_eq!(q.report().lowerings.len(), 1);
    assert!(q
        .view()
        .unverified
        .iter()
        .any(|s| s.contains("prepared rule")));
    for question in &q.report().questions {
        for (index, w) in question.witnesses.iter().enumerate() {
            if !w.matches_sources(
                current.program().unwrap(),
                d.selection().preparation().target(),
            ) {
                assert!(q
                    .open_example(&store, d.selection(), &question.id, index, cancel())
                    .is_err());
                assert!(q
                    .copy_trace(&store, d.selection(), &question.id, index, cancel())
                    .is_err());
            }
        }
    }
    result.producer = Producer::Fixture {
        name: "Different returned producer".into(),
    };
    assert!(d
        .evaluate(
            &store,
            d.selection(),
            result,
            DiscoveryPolicy::default(),
            vec![lowering],
            cancel()
        )
        .is_err());
    assert_eq!(store.load().unwrap(), current);
}

#[test]
fn no_question_uncertainty_and_failed_requirements_remain_in_report() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    add(&store, "existing", "Existing");
    let current = store.load().unwrap();
    let d = draft(
        &store,
        &program(true),
        ScopePopulation::FutureWork,
        "primary",
    );
    let scene = created_scene(&current).0;
    let mut empty = discovery_result(&d, &scene);
    empty.response.hypotheses.clear();
    let q = d
        .evaluate(
            &store,
            d.selection(),
            empty.clone(),
            DiscoveryPolicy::default(),
            vec![],
            cancel(),
        )
        .unwrap();
    assert!(q.report().questions.is_empty());
    assert!(!q.report().coverage.is_empty());
    empty
        .response
        .unsupported
        .push("An uncovered workflow needs a business example".into());
    let q = d
        .evaluate(
            &store,
            d.selection(),
            empty,
            DiscoveryPolicy::default(),
            vec![],
            cancel(),
        )
        .unwrap();
    assert!(!q.report().unverified.is_empty());
    let mut policy = DiscoveryPolicy::default();
    policy.requirements.push(RequirementCase {
        id: "known-requirement".into(),
        scenario: scene.clone(),
        properties: vec![AcceptedProperty {
            id: "too-many-waiting".into(),
            description: "Independent fixture requirement cannot be met".into(),
            predicate: PropertyPredicate::Less {
                left: PropertyTerm::Literal {
                    value_type: Type::Integer,
                    value: DataValue::Integer { value: 100 },
                },
                right: PropertyTerm::Observed {
                    point: "result".into(),
                    observable: "waiting_jobs".into(),
                    value_type: Type::Integer,
                },
            },
        }],
    });
    let q = d
        .evaluate(
            &store,
            d.selection(),
            discovery_result(&d, &scene),
            policy,
            vec![],
            cancel(),
        )
        .unwrap();
    assert!(q.report().questions.is_empty());
    assert!(!q.report().defects.is_empty() || !q.report().unverified.is_empty());
}
