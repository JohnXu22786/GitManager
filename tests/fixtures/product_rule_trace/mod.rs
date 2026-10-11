//! Offline checked-trace fixture. Real store, compiler, runtime and host Gate.
//! Runner functions have no #[test]; only the isolated integration root runs them.
use super::super::rule_discovery::{
    RuleCopyTrace, RuleDiscoveryDraft, RuleDiscoveryQueue, RuleSelection,
};
use super::*;
use crate::product_discovery::DiscoveryPolicy;
use crate::product_store;
#[path = "../product_scope/mod.rs"]
mod fixture;
use fixture::*;

fn cancel() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}
fn decision_engine(store: &ProductStore) -> DecisionEngine<LocalRuntime> {
    DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()))
}
const NEED: &str = "Change waiting-time calculations while preserving customer commitments";

// Expose the authentic four-slot corpus through real generated controls.
fn program(pause: bool) -> CapturedProgram {
    let mut value = serde_json::to_value(fixture::program(pause).program).unwrap();
    let actions = value["actions"].as_array().unwrap().clone();
    for action in actions.iter().filter(|a| a["id"] != "add") {
        let row = !action["parameters"].as_object().unwrap().is_empty();
        value["views"][0]["actions"].as_array_mut().unwrap().push(serde_json::json!({
            "id":action["id"],"label":action["label"],"action":action["id"],
            "placement":if row {"row"} else {"toolbar"},
            "arguments":if row {serde_json::json!({"row":var("row")})} else {serde_json::json!({})},
            "enabled":boolean(true)
        }));
    }
    value["views"].as_array_mut().unwrap().push(serde_json::json!({
        "id":"new_work","label":"New work","kind":{"kind":"form","action":"add",
        "fields":[{"parameter":"name","label":"Name"},{"parameter":"promised","label":"Customer commitment"}],
        "defaults":{"promised":{"kind":"date","days":20020}}},"actions":[],"keys":[]
    }));
    capture(value)
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
fn managed(store: &ProductStore) -> (Record, Record, CapturedProgram) {
    let selected = add(store, "selected", "Selected commitment");
    let archived = add(store, "archived", "Completed history");
    action(store, "finish-old", "complete", &archived);
    action(store, "archive-old", "archive", &archived);
    action(store, "wait-selected", "wait", &selected);
    tick(store, "day", 20003);
    let current = store.load().unwrap();
    let authored = program(true);
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
    let e = decision_engine(store);
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
    let pointers: &[&str] = &[
        "/actions/3/steps/0/values/production",
        "/actions/4/steps/0/values/production",
        "/actions/6/steps/0/columns/production",
        "/views/0/kind/columns/1/value",
    ];
    for pointer in pointers {
        let old = raw.pointer(pointer).unwrap().clone();
        *raw.pointer_mut(pointer).unwrap() =
            serde_json::json!({"kind":"add","left":old,"right":int(1)});
    }
    (selected, archived, capture(raw))
}

fn change(
    store: &ProductStore,
    candidate: CapturedProgram,
    population: ScopePopulation,
    operation: &str,
) -> ChangeDraft {
    let snapshot = store.load().unwrap();
    let mut draft = ChangeDraft::new(snapshot.clone(), candidate, NEED.into()).unwrap();
    draft.lifecycles = fixture::request(&snapshot, ScopePopulation::All).lifecycles;
    draft.population = population;
    draft.operation = operation.into();
    draft.prepare(store, &Gate::default()).unwrap();
    draft
}
fn selection(store: &ProductStore, change: &ChangeDraft) -> RuleSelection {
    let request = change_adapter::request(
        &change.snapshot,
        &change.candidate,
        change.population.clone(),
        change.lifecycles.clone(),
        change.scope_operations.clone(),
    )
    .unwrap();
    RuleSelection::checked(
        store,
        &change.snapshot,
        change.prepared.clone().unwrap(),
        request,
        &change.operation,
        cancel(),
    )
    .unwrap()
}
fn discovery(store: &ProductStore, change: &ChangeDraft) -> RuleDiscoveryDraft {
    let request = decision_engine(store)
        .development_request(
            &change.snapshot,
            &format!("modify-{}", change.operation),
            DevelopmentOperation::Modify,
            NEED,
            DevelopmentContext {
                view: Some("work".into()),
                selected: vec![],
                recent_inputs: vec![],
                data_digest: Some(change.snapshot.data.identity().unwrap()),
                session_digest: Some(change.snapshot.session.identity().unwrap()),
            },
        )
        .unwrap();
    let result = DevelopmentResult {
        producer: change.candidate.binding.producer.clone(),
        response: DevelopmentResponse {
            version: 1,
            request_digest: request.identity().unwrap(),
            candidates: vec![GeneratedCandidate {
                id: "changed".into(),
                source_json: String::from_utf8(change.candidate.source_bytes.clone()).unwrap(),
            }],
            hypotheses: vec![],
            evolutions: vec![],
            unsupported: vec![],
        },
    };
    RuleDiscoveryDraft::after_modify(
        store,
        &change.snapshot,
        request,
        result,
        "changed",
        selection(store, change),
        &format!("discover-{}", change.operation),
        NEED,
        cancel(),
    )
    .unwrap()
}
fn result(draft: &RuleDiscoveryDraft, scenario: &ScenarioSpec) -> DevelopmentResult {
    let before = &draft.request().sources[0];
    let after = &draft.request().sources[1];
    DevelopmentResult {
        producer: Producer::Fixture {
            name: "Offline discovery of actual multi-slot rules".into(),
        },
        response: DevelopmentResponse {
            version: 1,
            request_digest: draft.request().identity().unwrap(),
            candidates: [("before", before), ("after", after)]
                .into_iter()
                .map(|(id, p)| GeneratedCandidate {
                    id: id.into(),
                    source_json: String::from_utf8(p.source_bytes.clone()).unwrap(),
                })
                .collect(),
            hypotheses: vec![ChoiceHypothesis {
                id: "waiting-choice".into(),
                statement: "Waiting changes production calculations".into(),
                kind: HypothesisKind::UnresolvedChoice,
                action: "export".into(),
                observable: "waiting_jobs".into(),
                sources: vec![SourceLocus {
                    relative_path: after.binding.program_path.clone(),
                    raw_digest: after.artifact.raw_digest.clone(),
                    pointer: "/views/0/kind/columns/1/value".into(),
                }],
                alternatives: vec!["before".into(), "after".into()],
                related_decisions: vec![],
                scenario_json: serde_json::to_string(scenario).unwrap(),
                unknowns: vec![],
            }],
            evolutions: vec![],
            unsupported: vec![],
        },
    }
}
fn queue(
    store: &ProductStore,
    draft: &RuleDiscoveryDraft,
    scenario: &ScenarioSpec,
) -> RuleDiscoveryQueue {
    draft
        .evaluate(
            store,
            draft.selection(),
            result(draft, scenario),
            DiscoveryPolicy::default(),
            vec![],
            cancel(),
        )
        .unwrap()
}
fn copy(
    store: &ProductStore,
    draft: &RuleDiscoveryDraft,
    queue: &RuleDiscoveryQueue,
) -> RuleCopyTrace {
    assert!(!queue.report().questions.is_empty(), "{:?}", queue.report());
    queue
        .copy_trace(
            store,
            draft.selection(),
            &queue.report().questions[0].id,
            0,
            cancel(),
        )
        .unwrap()
}
fn assert_no_authority(draft: &ChangeDraft) {
    assert!(
        draft.scenes.is_none(),
        "Failed or cancelled work retained accepted scenes"
    );
    let view = draft.view().unwrap();
    assert!(
        view.alternative.is_none(),
        "A failed comparison still displays an alternative as usable"
    );
    assert!(!view.can_accept && !view.can_keep_current && !view.can_retain);
}

pub fn imported_trace_keeps_observations_and_created_ids() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let (_, _, candidate) = managed(&store);
    let before = store.load().unwrap();
    let mut change = change(
        &store,
        candidate,
        ScopePopulation::FutureWork,
        "import-rule",
    );
    assert_eq!(
        change.analysis.patches.len(),
        4,
        "Use the actual multi-slot managed corpus"
    );
    let draft = discovery(&store, &change);
    let (original, created) = created_scene(&before);
    let queue = queue(&store, &draft, &original);
    let question = &queue.report().questions[0];
    let example = queue
        .open_example(&store, draft.selection(), &question.id, 0, cancel())
        .unwrap();
    assert_eq!(
        example.scenario(),
        &question.witnesses[0].witness().scenario
    );
    assert!(
        change.scenes.is_none(),
        "An exact example alone cannot authorize a decision"
    );
    let copy = copy(&store, &draft, &queue);
    let observations: Vec<_> = copy
        .original_scenario()
        .inputs
        .iter()
        .filter_map(|i| match i {
            SemanticInput::Observe { point } => Some(point.clone()),
            _ => None,
        })
        .collect();
    assert!(observations.contains(&"result".into()));
    assert!(
        observations.len() > 1,
        "Exercise multiple observation points in the checked copy"
    );
    change
        .import_rule_trace(&store, draft.selection(), &copy, &Gate::default())
        .unwrap();
    assert_eq!(&change.scenario, copy.original_scenario());
    assert_eq!(change.prepared.as_ref(), Some(copy.preparation()));
    let (current, alternative) = change.scenes.as_ref().unwrap();
    for accepted in [current, alternative] {
        assert_eq!(accepted.scenario(), copy.scenario());
        assert_eq!(
            accepted
                .observations()
                .iter()
                .map(|o| o.point.clone())
                .collect::<Vec<_>>(),
            observations
        );
        assert!(accepted.observations().iter().any(|o| o
            .view
            .rows
            .iter()
            .any(|r| r.record.record == created.id)));
    }
    let view = change.checked_view(&store, &Gate::default()).unwrap();
    assert!(
        view.can_accept && view.can_keep_current && view.can_retain,
        "{:?}",
        view.readiness_notes
    );
    assert_eq!(store.load().unwrap(), before);
    change
        .trial(
            &store,
            SemanticInput::AdvanceClock { days: 1 },
            &Gate::default(),
        )
        .unwrap();
    assert_eq!(
        &change.scenario.inputs[..copy.original_scenario().inputs.len()],
        &copy.original_scenario().inputs
    );
    let updated = change.scenes.as_ref().unwrap().0.observations();
    assert_eq!(updated.len(), observations.len() + 1);
    assert_eq!(
        updated
            .iter()
            .take(observations.len())
            .map(|o| o.point.clone())
            .collect::<Vec<_>>(),
        observations
    );
    assert!(updated
        .iter()
        .any(|o| o.view.rows.iter().any(|r| r.record.record == created.id)));
    // Reopening the same checked copy replaces the edited trial rather than
    // appending its inputs or borrowing the edited correspondence inventory.
    change
        .import_rule_trace(&store, draft.selection(), &copy, &Gate::default())
        .unwrap();
    assert_eq!(&change.scenario, copy.original_scenario());
    assert_eq!(store.load().unwrap(), before);
}

pub fn trial_keeps_an_existing_result_observation() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let row = add(&store, "existing", "Copied work");
    let mut draft = change(
        &store,
        program(true),
        ScopePopulation::All,
        "trial-observations",
    );
    let gate = Gate::default();
    draft
        .trial(
            &store,
            invoke("calculate", &[("row", reference(&row))]),
            &gate,
        )
        .unwrap();
    draft
        .trial(
            &store,
            SemanticInput::Observe {
                point: "result".into(),
            },
            &gate,
        )
        .unwrap();
    draft.trial(&store, invoke("export", &[]), &gate).unwrap();
    let scene = &draft.scenes.as_ref().unwrap().0;
    let points: Vec<_> = scene
        .observations()
        .iter()
        .map(|o| o.point.as_str())
        .collect();
    assert_eq!(points.iter().filter(|p| **p == "result").count(), 1);
    assert_eq!(
        points.len(),
        2,
        "Later work needs a distinct final observation"
    );
}

pub fn failing_or_cancelled_trial_clears_old_authority() {
    for cancelled in [false, true] {
        let dir = tempdir();
        let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
        let row = add(&store, "existing", "Copied work");
        let before = store.load().unwrap();
        let mut draft = change(
            &store,
            program(true),
            ScopePopulation::All,
            "trial-revocation",
        );
        draft
            .trial(
                &store,
                invoke("calculate", &[("row", reference(&row))]),
                &Gate::default(),
            )
            .unwrap();
        assert!(draft.scenes.is_some());
        let gate = Gate::default();
        if cancelled {
            assert!(gate.cancel());
        }
        let input = if cancelled {
            invoke("export", &[])
        } else {
            invoke("missing-action", &[])
        };
        assert!(draft.trial(&store, input, &gate).is_err());
        assert_no_authority(&draft);
        assert_eq!(store.load().unwrap(), before);
    }
}

pub fn import_rejects_changed_selection_and_preparation() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    add(&store, "existing", "Current work");
    let before = store.load().unwrap();
    let mut initial = change(
        &store,
        program(true),
        ScopePopulation::FutureWork,
        "reject-import",
    );
    let discovery = discovery(&store, &initial);
    let queue = queue(&store, &discovery, &created_scene(&before).0);
    let copy = copy(&store, &discovery, &queue);
    initial
        .import_rule_trace(&store, discovery.selection(), &copy, &Gate::default())
        .unwrap();
    for case in [
        "population",
        "lifecycle",
        "operation",
        "operations",
        "candidate",
        "preparation",
        "cancel",
    ] {
        let mut draft = initial.clone();
        let gate = Gate::default();
        match case {
            "population" => draft.population = ScopePopulation::All,
            "lifecycle" => draft.lifecycles[0].completed = boolean(false),
            "operation" => draft.operation = "other-operation".into(),
            "operations" => {
                draft.scope_operations.insert("wait".into());
            }
            "candidate" => draft.candidate = program(false),
            "preparation" => {
                draft.prepared = Some(
                    store
                        .prepare_scoped_change(
                            &draft.candidate,
                            discovery.selection().request(),
                            "other-operation",
                        )
                        .unwrap(),
                );
            }
            "cancel" => {
                assert!(gate.cancel());
            }
            _ => unreachable!(),
        }
        assert!(
            draft
                .import_rule_trace(&store, discovery.selection(), &copy, &gate)
                .is_err(),
            "{case}"
        );
        assert_no_authority(&draft);
        assert_eq!(store.load().unwrap(), before);
    }
    // An actual later save invalidates a previously imported trace and its page.
    add(&store, "later", "Later legitimate work");
    let later = store.load().unwrap();
    assert!(initial
        .import_rule_trace(&store, discovery.selection(), &copy, &Gate::default())
        .is_err());
    assert_no_authority(&initial);
    assert_eq!(store.load().unwrap(), later);
}

pub fn import_rejects_corrupted_prepared_frames() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    add(&store, "existing", "Current work");
    let before = store.load().unwrap();
    let mut initial = change(
        &store,
        program(true),
        ScopePopulation::FutureWork,
        "reject-frame",
    );
    let discovery = discovery(&store, &initial);
    let queue = queue(&store, &discovery, &created_scene(&before).0);
    let copy = copy(&store, &discovery, &queue);
    initial
        .import_rule_trace(&store, discovery.selection(), &copy, &Gate::default())
        .unwrap();
    for case in ["source", "day", "random", "seed", "record", "input"] {
        let mut draft = initial.clone();
        let proof = draft
            .prepared
            .as_mut()
            .unwrap()
            .correspondences
            .values_mut()
            .find(|p| p.scenario.is_some())
            .unwrap();
        match case {
            "source" => proof.source = program(true),
            "day" => proof.day += 1,
            "random" => proof.scenario.as_mut().unwrap().original.random_seed += 1,
            "seed" => proof.original.generation += 1,
            "record" => {
                proof.scenario.as_mut().unwrap().projected.seed.records[0].id =
                    "different-record".into()
            }
            "input" => proof.scenario.as_mut().unwrap().projected.inputs.swap(0, 1),
            _ => unreachable!(),
        }
        assert!(
            draft
                .import_rule_trace(&store, discovery.selection(), &copy, &Gate::default())
                .is_err(),
            "{case}"
        );
        assert_no_authority(&draft);
        assert_eq!(store.load().unwrap(), before);
    }
}

fn settle(studio: &mut ProductStudio, stage: &str) {
    let start = std::time::Instant::now();
    eprintln!("{stage}: waiting on {}", studio.test_page());
    loop {
        studio.poll();
        if !studio.is_busy() {
            break;
        }
        // This four-slot workflow performs fresh proof, adoption and verified
        // reopen in one command. Each runtime evaluation keeps its own limits.
        assert!(
            start.elapsed() < Duration::from_secs(300),
            "{stage}: still busy on {} after {:?}: {}",
            studio.test_page(),
            start.elapsed(),
            studio.test_notice()
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    eprintln!(
        "{stage}: settled on {} after {:?}",
        studio.test_page(),
        start.elapsed()
    );
}
fn stop(studio: ProductStudio, stopped: Arc<AtomicBool>) {
    drop(studio);
    let start = std::time::Instant::now();
    while !stopped.load(Ordering::Acquire) {
        assert!(start.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn choose(outcome: DecisionOutcome) {
    let dir = tempdir();
    let path = dir.path().join("tool");
    let store = ProductStore::create(&path, &program(false), 20000).unwrap();
    let (selected, archived, candidate) = managed(&store);
    let before = store.load().unwrap();
    let mut change = change(
        &store,
        candidate,
        ScopePopulation::FutureWork,
        "choose-import",
    );
    let discovery = discovery(&store, &change);
    let queue = queue(&store, &discovery, &created_scene(&before).0);
    let copy = copy(&store, &discovery, &queue);
    change
        .import_rule_trace(&store, discovery.selection(), &copy, &Gate::default())
        .unwrap();
    let outcome = if matches!(outcome, DecisionOutcome::Accept { .. }) {
        DecisionOutcome::Accept {
            artifact: change
                .prepared
                .as_ref()
                .unwrap()
                .target()
                .artifact
                .program_digest
                .clone(),
        }
    } else {
        outcome
    };
    let verified = change
        .decision(&store, outcome.clone(), "record-choice", &Gate::default())
        .unwrap();
    let saved = decision_engine(&store).adopt(&store, &verified).unwrap();
    assert_eq!(saved.decisions.decisions[0], before.decisions.decisions[0]);
    assert_eq!(saved.data.events, before.data.events);
    assert_eq!(saved.session, before.session);
    assert_eq!(saved.clock_day, before.clock_day);
    for old in &before.data.records {
        let current = saved
            .data
            .records
            .iter()
            .find(|r| r.entity == old.entity && r.id == old.id)
            .unwrap();
        assert_eq!(current.archived, old.archived);
        assert_eq!(current.revision, old.revision);
        for (field, value) in &old.values {
            assert_eq!(current.values.get(field), Some(value));
        }
    }
    let decision = saved
        .decisions
        .decisions
        .iter()
        .find(|d| !before.decisions.decisions.iter().any(|old| old.id == d.id))
        .unwrap();
    assert_eq!(decision.outcome, outcome);
    assert_eq!(ProductStore::open(&path).unwrap().load().unwrap(), saved);
    if matches!(outcome, DecisionOutcome::Accept { .. }) {
        assert_ne!(saved.active_revision, before.active_revision);
        assert_eq!(saved.scope.layers.len(), before.scope.layers.len() + 1);
        assert_eq!(decision.status, DecisionStatus::Active);
        return;
    }
    assert_eq!(saved.data, before.data);
    assert_eq!(saved.active_revision, before.active_revision);
    assert_eq!(saved.artifacts, before.artifacts);
    assert_eq!(saved.scope.layers, before.scope.layers);
    assert_eq!(saved.scope.initializations, before.scope.initializations);
    assert_eq!(saved.scope.compositions, before.scope.compositions);
    assert_eq!(saved.scope.adoptions, before.scope.adoptions);
    if outcome == DecisionOutcome::KeepCurrent {
        assert_eq!(decision.status, DecisionStatus::Active);
        let inherited = decision_engine(&store)
            .development_request(
                &saved,
                "inherit-kept-current",
                DevelopmentOperation::Modify,
                NEED,
                DevelopmentContext {
                    view: Some("work".into()),
                    selected: vec![],
                    recent_inputs: vec![],
                    data_digest: Some(saved.data.identity().unwrap()),
                    session_digest: Some(saved.session.identity().unwrap()),
                },
            )
            .unwrap();
        let retained: Vec<_> = inherited
            .accepted_scenes
            .iter()
            .filter(|s| s.decision == decision.id)
            .collect();
        assert!(!retained.is_empty());
        assert!(retained
            .iter()
            .all(|s| s.source == before.program().unwrap().artifact));
        assert_eq!(
            decision_engine(&store)
                .check_current(&saved)
                .unwrap()
                .disposition,
            crate::product_decisions::CheckDisposition::Ready
        );
        return;
    }
    assert_eq!(decision.status, DecisionStatus::Pending);
    let pending = decision.id.clone();
    if outcome == DecisionOutcome::EitherAcceptable {
        let repeated_change = self::change(
            &store,
            copy.preparation().candidate().clone(),
            ScopePopulation::FutureWork,
            "repeat-saved-rule",
        );
        let repeated_draft = self::discovery(&store, &repeated_change);
        let repeated = self::queue(&store, &repeated_draft, copy.original_scenario());
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
                .any(|entry| entry.disposition == crate::product_discovery::Disposition::Settled),
            "{:?}",
            repeated.report()
        );
    }
    assert_eq!(
        saved
            .scope
            .rehearsals
            .values()
            .filter(|p| p.witnesses.contains_key(&pending))
            .count(),
        1
    );
    let late = add(&store, "later", "Later real work");
    action(&store, "later-wait", "wait", &late);
    action(&store, "finish-selected", "complete", &selected);
    let facts = store.load().unwrap();
    assert_eq!(row(&facts, &archived), row(&saved, &archived));
    let hooks = TestHooks::default();
    hooks.today.store(facts.clock_day, Ordering::Release);
    let stopped = hooks.stopped.clone();
    let mut studio = ProductStudio::testing(dir.path().into(), None, hooks);
    settle(&mut studio, "boot before pending reopen");
    studio.test_open(path.clone());
    settle(&mut studio, "open saved tool after later work");
    studio.test_resume_choice(&pending);
    settle(&mut studio, "reopen retained pending choice");
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    assert!(!studio.test_can_accept());
    studio.test_trial(invoke(
        "add",
        &[
            ("name", text("Fresh copied work")),
            ("promised", DataValue::Date { days: 20020 }),
        ],
    ));
    settle(&mut studio, "trial: add fresh copied work");
    let copied = studio
        .test_current_trial()
        .unwrap()
        .retained_records
        .iter()
        .find(|r| r.values.get("name") == Some(&text("Fresh copied work")))
        .unwrap()
        .clone();
    for input in [
        SemanticInput::Observe {
            point: "unrelated".into(),
        },
        invoke("wait", &[("row", reference(&copied))]),
        SemanticInput::AdvanceClock { days: 3 },
        invoke("calculate", &[("row", reference(&copied))]),
        invoke("complete", &[("row", reference(&copied))]),
        invoke("export", &[]),
        SemanticInput::Observe {
            point: "result".into(),
        },
    ] {
        let stage = format!("trial: {input:?}");
        studio.test_trial(input);
        settle(&mut studio, &stage);
        assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    }
    assert!(studio.test_can_accept(), "{}", studio.test_notice());
    studio.test_decide(DecisionOutcome::Accept {
        artifact: copy.preparation().target().artifact.program_digest.clone(),
    });
    settle(&mut studio, "accept freshly reexperienced pending choice");
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    let resolved = store.load().unwrap();
    assert_eq!(
        resolved.decisions.decisions[0],
        facts.decisions.decisions[0]
    );
    assert_eq!(resolved.data.events, facts.data.events);
    assert_eq!(
        row(&resolved, &late).values["name"],
        text("Later real work")
    );
    assert!(matches!(
        resolved
            .decisions
            .decisions
            .iter()
            .find(|d| d.id == pending)
            .unwrap()
            .status,
        DecisionStatus::Superseded { .. }
    ));
    assert_eq!(
        decision_engine(&store)
            .check_current(&resolved)
            .unwrap()
            .disposition,
        crate::product_decisions::CheckDisposition::Ready
    );
    stop(studio, stopped);
    assert_eq!(ProductStore::open(&path).unwrap().load().unwrap(), resolved);
}

pub fn imported_either_acceptable_reopens_after_later_work() {
    choose(DecisionOutcome::EitherAcceptable);
}
pub fn imported_both_needed_reopens_after_later_work() {
    choose(DecisionOutcome::BothNeeded);
}
pub fn imported_neither_fits_reopens_after_later_work() {
    choose(DecisionOutcome::NeitherFits);
}
pub fn imported_deferred_reopens_after_later_work() {
    choose(DecisionOutcome::Deferred);
}
pub fn imported_keep_current_retains_committed_current() {
    choose(DecisionOutcome::KeepCurrent);
}
pub fn imported_accept_adopts_freshly_prepared_rule() {
    choose(DecisionOutcome::Accept {
        artifact: program(true).artifact.program_digest,
    });
}

pub fn actionless_minimum_imports_only_checked_original_without_broadening_scope() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let row = add(&store, "waiting", "Waiting work");
    action(&store, "wait", "wait", &row);
    tick(&store, "days", 20003);
    let before = store.load().unwrap();
    let mut change = change(&store, program(true), ScopePopulation::All, "partial-copy");
    let discovery = discovery(&store, &change);
    let original = ScenarioSpec {
        version: 1,
        id: "exported-example".into(),
        label: "Observe exported waiting time".into(),
        seed: before.data.clone(),
        session: before.session.clone(),
        clock_day: before.clock_day,
        random_seed: 42,
        inputs: vec![
            invoke("export", &[]),
            SemanticInput::Observe {
                point: "result".into(),
            },
        ],
        validity: vec![],
    };
    let queue = queue(&store, &discovery, &original);
    let (question, index, witness) = queue
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
        .expect("An actual view witness reduces to observations only");
    let example = queue
        .open_example(&store, discovery.selection(), &question.id, index, cancel())
        .unwrap();
    assert_eq!(example.scenario(), &witness.witness().scenario);
    let copy = queue
        .copy_trace(&store, discovery.selection(), &question.id, index, cancel())
        .unwrap();
    assert_eq!(copy.original_scenario().inputs, original.inputs);
    change
        .import_rule_trace(&store, discovery.selection(), &copy, &Gate::default())
        .unwrap();
    assert_eq!(change.population, ScopePopulation::All);
    let view = change.checked_view(&store, &Gate::default()).unwrap();
    assert!(
        !view.can_accept && !view.can_keep_current,
        "An export cannot promise unexperienced calculation/completion tasks"
    );
    assert!(change
        .decision(
            &store,
            DecisionOutcome::Accept {
                artifact: view.target_artifact
            },
            "incomplete",
            &Gate::default()
        )
        .is_err());
    assert!(change
        .decision(
            &store,
            DecisionOutcome::KeepCurrent,
            "incomplete",
            &Gate::default()
        )
        .is_err());
    assert_eq!(store.load().unwrap(), before);
}

pub fn unverified_synthetic_frame_cannot_create_copy_authority() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let (synthetic, created) = created_scene(&store.load().unwrap());
    add(&store, "later", "Real work changes the allocation frame");
    let current = store.load().unwrap();
    let change = change(
        &store,
        program(true),
        ScopePopulation::FutureWork,
        "unavailable-copy",
    );
    let discovery = discovery(&store, &change);
    let queue = queue(&store, &discovery, &synthetic);
    // The actual checked-history boundary refuses this raw synthetic frame
    // before it can become a witness or a copy. Do not relax that admission
    // merely to manufacture a later copy-error path.
    assert!(queue.report().questions.is_empty());
    assert!(queue.report().runs.is_empty());
    assert!(
        queue
            .report()
            .unverified
            .iter()
            .any(|issue| issue.contains("unverified raw seed")),
        "{:?}",
        queue.report()
    );
    assert!(queue
        .report()
        .log
        .iter()
        .any(|entry| entry.disposition == crate::product_discovery::Disposition::Unverified));
    let view = queue
        .checked_view(&store, discovery.selection(), cancel())
        .unwrap();
    assert_eq!(
        view.state,
        super::super::discovery_flow::QueueState::Unverified
    );
    assert!(view.questions.is_empty());
    assert!(queue
        .copy_trace(&store, discovery.selection(), "waiting-choice", 0, cancel())
        .is_err());
    assert!(change.scenes.is_none());
    assert!(!current.data.records.iter().any(|row| row.id == created.id));
    assert_eq!(store.load().unwrap(), current);
}
