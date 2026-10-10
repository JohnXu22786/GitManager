//! Synthetic control integration; fixture output is not live AI or native GUI evidence.
#[path = "fixtures/product_runtime/mod.rs"]
mod fixture;
#[path = "../src/product_backup.rs"]
mod product_backup;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_decisions/mod.rs"]
mod product_decisions;
#[path = "../src/product_discovery/mod.rs"]
mod product_discovery;
#[path = "../src/product_export.rs"]
mod product_export;
#[path = "../src/product_locations.rs"]
mod product_locations;
#[path = "../src/product_protocol.rs"]
mod product_protocol;
#[path = "../src/product_provider/mod.rs"]
mod product_provider;
#[path = "../src/product_runtime/mod.rs"]
mod product_runtime;
#[path = "../src/ui/product_runtime_view.rs"]
pub(crate) mod product_runtime_view;
#[path = "../src/product_scenarios/mod.rs"]
mod product_scenarios;
#[path = "../src/product_store/mod.rs"]
mod product_store;
#[path = "../src/tool_proposal_input.rs"]
mod tool_proposal_input;
mod ui {
    pub(crate) use super::product_runtime_view;
}
#[path = "support/egui_harness.rs"]
mod egui_harness;
#[path = "../src/product_studio.rs"]
mod product_studio;
use egui_harness::EguiHarness;
use product_runtime_view::WidgetTrace;
use product_studio::{ProductStudio, TestHooks};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

fn frame(h: &mut EguiHarness, s: &mut ProductStudio) -> WidgetTrace {
    s.poll();
    h.frame(|ctx| {
        egui::CentralPanel::default()
            .show(ctx, |ui| s.show(ui))
            .inner
    })
}
fn click(h: &mut EguiHarness, s: &mut ProductStudio, key: &str) {
    let trace = frame(h, s);
    let control = trace
        .controls
        .get(key)
        .unwrap_or_else(|| panic!("missing control {key}; {:?}", trace.text));
    assert!(control.enabled, "disabled {key}");
    let p = control.rect.center();
    h.press_at(p);
    frame(h, s);
    h.release_at(p);
    frame(h, s);
}
fn fill(h: &mut EguiHarness, s: &mut ProductStudio, key: &str, text: &str) {
    click(h, s, key);
    h.text(text);
    frame(h, s);
}
fn settle(h: &mut EguiHarness, s: &mut ProductStudio) {
    let start = Instant::now();
    while s.is_busy() {
        frame(h, s);
        assert!(
            start.elapsed() < Duration::from_secs(120),
            "{}",
            s.test_notice()
        );
        // The actual busy host requests a repaint every 30 ms. Match that
        // cadence instead of making the renderer compete with its worker in
        // a 2 ms redraw loop; the same 120-second completion gate still applies.
        std::thread::sleep(Duration::from_millis(30));
    }
    frame(h, s);
}

use product_contract::*;
use product_decisions::*;
use product_runtime::LocalRuntime;
use product_store::ProductStore;
use std::sync::atomic::Ordering;

fn engine(store: &ProductStore) -> DecisionEngine<LocalRuntime> {
    DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()))
}
fn record(store: &ProductStore, id: &str, outcome: DecisionOutcome, scenes: Vec<AcceptedScene>) {
    let snapshot = store.load().unwrap();
    let ready = engine(store)
        .prepare_choice(
            store,
            snapshot.program().unwrap(),
            Choice {
                id: id.into(),
                request: format!("Keep {id}"),
                rationale: None,
                scope: DecisionScope {
                    operations: ["export_people".into()].into(),
                    population: Population::All,
                    conditions: Values::new(),
                    excluded_records: vec![],
                    unknowns: vec![],
                },
                outcome,
                obligations: vec![],
                binding: IntentionBinding::ObservedOutcome,
            },
            scenes,
            &format!("save-{id}"),
        )
        .unwrap();
    engine(store).adopt(store, &ready).unwrap();
}
fn accepted(program: &CapturedProgram, id: &str) -> AcceptedScene {
    let mut scene = fixture::scenario(
        program,
        vec![
            fixture::add("Ada"),
            fixture::invoke("collect", Values::new()),
            fixture::invoke("export_people", Values::new()),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    scene.id = id.into();
    accept_scene(
        &LocalRuntime::default(),
        program,
        &scene,
        Disclosure::Synthetic,
        RuntimeLimits::default(),
    )
    .unwrap()
}
fn seeded(root: &Path) -> ProductStore {
    let program = fixture::capture(fixture::organizer());
    let store = ProductStore::create(root.join("tool"), &program, 20000).unwrap();
    record(
        &store,
        "gathered",
        DecisionOutcome::KeepCurrent,
        vec![accepted(&program, "gathered-example")],
    );
    record(
        &store,
        "both",
        DecisionOutcome::BothNeeded,
        vec![accepted(&program, "both-example")],
    );
    store
}
fn open(
    root: &Path,
    transport: Option<product_provider::ProviderTransport>,
    hooks: TestHooks,
) -> (ProductStudio, EguiHarness) {
    let mut studio = ProductStudio::testing(root.into(), transport, hooks);
    let mut h = EguiHarness::new(egui::vec2(1500.0, 2400.0));
    settle(&mut h, &mut studio);
    studio.test_open(root.join("tool"));
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    (studio, h)
}
#[test]
fn history_is_reachable_and_preserves_actual_status_and_source_bound_examples() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let store = seeded(&root);
    let before = store.load().unwrap();
    let (mut s, mut h) = open(&root, None, TestHooks::default());
    click(&mut h, &mut s, "studio.history");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "history");
    let trace = frame(&mut h, &mut s);
    assert!(
        trace
            .text
            .iter()
            .any(|t| t.contains("Both ways of working are needed")),
        "{:?}",
        trace.text
    );
    click(&mut h, &mut s, "intention-details-gathered");
    let trace = frame(&mut h, &mut s);
    assert!(
        trace.text.iter().any(|t| t.contains("Ada")),
        "{:?}",
        trace.text
    );
    assert!(
        trace.text.iter().any(|t| t.contains("Recorded")),
        "{:?}",
        trace.text
    );
    assert_eq!(store.load().unwrap(), before);
    click(&mut h, &mut s, "intention-return");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "daily");
}
#[test]
fn unsent_daily_form_blocks_history_until_exact_submission_completes() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    seeded(&root);
    let (mut s, mut h) = open(&root, None, TestHooks::default());
    click(&mut h, &mut s, "daily.navigate.new_person");
    settle(&mut h, &mut s);
    fill(&mut h, &mut s, "daily.field.name", "Later work");
    fill(&mut h, &mut s, "daily.field.area", "north");
    assert!(!frame(&mut h, &mut s).controls["studio.history"].enabled);
    click(&mut h, &mut s, "daily.submit");
    settle(&mut h, &mut s);
    click(&mut h, &mut s, "studio.history");
    settle(&mut h, &mut s);
    click(&mut h, &mut s, "intention-return");
    settle(&mut h, &mut s);
    assert!(s
        .test_runtime()
        .unwrap()
        .retained_records
        .iter()
        .any(|r| r.values.get("name")
            == Some(&DataValue::Text {
                value: "Later work".into()
            })));
}

#[cfg(unix)]
fn transport(
    root: &Path,
    response: &DevelopmentResponse,
    mode: &str,
) -> product_provider::ProviderTransport {
    use std::os::unix::fs::PermissionsExt;
    let executable = root.join("fixture.py");
    let _guard = product_provider::fixture_executable_write_guard();
    let script=include_str!("fixtures/provider_transport/fake_cli.py").replace(
        "envelope['payload'] = {'passed': True, 'text': wire['prompt'], 'command': 'untrusted-do-not-execute'}",
        "domain=json.loads(wire['prompt'])\nresponse=cfg['response']\nresponse['request_digest']=domain['request_digest']\nresponse['evolutions'][0]['id']=domain['request']['request'].split('evolution suggestion ID: ')[1].split()[0]\nenvelope['payload']=response");
    fs::write(&executable, script).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(
        executable.with_extension("json"),
        serde_json::to_vec(&serde_json::json!({"response":response,"mode":mode})).unwrap(),
    )
    .unwrap();
    let home = root.join("fixture-home");
    fs::create_dir(&home).unwrap();
    product_provider::ProviderTransport::new_fixture(
        root.join("jobs"),
        product_provider::ProviderKind::Codex,
        executable,
        home,
    )
    .unwrap()
}
fn invocation_count(root: &Path) -> usize {
    root.join("jobs")
        .read_dir()
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().join("fixture-invocation.json").exists())
        .count()
}
fn distinct_needs(root: &Path) -> (ProductStore, DevelopmentResponse) {
    use fixture::*;
    use serde_json::json;
    let p = capture(organizer());
    let store = ProductStore::create(root.join("tool"), &p, 20000).unwrap();
    let a = accepted(&p, "gathered-example");
    record(
        &store,
        "old-default",
        DecisionOutcome::KeepCurrent,
        vec![a.clone()],
    );
    record(
        &store,
        "independent-concrete",
        DecisionOutcome::KeepCurrent,
        vec![a.clone()],
    );
    let invariant = AcceptedProperty {
        id: "count-match".into(),
        description: "Preview count equals actual output rows".into(),
        predicate: PropertyPredicate::Equal {
            left: PropertyTerm::Observed {
                point: "done".into(),
                observable: "selected_count".into(),
                value_type: Type::Integer,
            },
            right: PropertyTerm::OutputCount {
                point: "done".into(),
                output: "roster".into(),
            },
        },
    };
    let ready = engine(&store)
        .prepare_choice(
            &store,
            &p,
            Choice {
                id: "independent".into(),
                request: "The count must match the exported rows".into(),
                rationale: None,
                scope: DecisionScope {
                    operations: ["export_people".into()].into(),
                    population: Population::All,
                    conditions: Values::new(),
                    excluded_records: vec![],
                    unknowns: vec![],
                },
                outcome: DecisionOutcome::KeepCurrent,
                obligations: vec![invariant.clone()],
                binding: IntentionBinding::PropertiesOnly,
            },
            vec![a.clone()],
            "save-invariant",
        )
        .unwrap();
    engine(&store).adopt(&store, &ready).unwrap();
    let mut other = organizer();
    other["actions"][2]["steps"].as_array_mut().unwrap().insert(0,json!({"kind":"set_state","state":"selected","value":{"kind":"literal","value_type":{"kind":"list","item":{"kind":"reference","entity":"person"}},"value":empty("person")}}));
    other["actions"][2]["id"] = json!("one_off_original");
    other["views"][0]["actions"][0]["action"] = json!("one_off_original");
    let other = capture(other);
    let mut example = scenario(
        &other,
        vec![
            add("Ada"),
            invoke("collect", Values::new()),
            invoke("one_off_original", Values::new()),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    example.id = "one-off".into();
    let b = accept_scene(
        &LocalRuntime::default(),
        &other,
        &example,
        Disclosure::Synthetic,
        RuntimeLimits::default(),
    )
    .unwrap();
    let ready = engine(&store)
        .prepare_choice(
            &store,
            &p,
            Choice {
                id: "second-need".into(),
                request: "I also need a separate one-off dispatch".into(),
                rationale: None,
                scope: DecisionScope {
                    operations: ["one_off_original".into()].into(),
                    population: Population::All,
                    conditions: Values::new(),
                    excluded_records: vec![],
                    unknowns: vec![],
                },
                outcome: DecisionOutcome::BothNeeded,
                obligations: vec![],
                binding: IntentionBinding::ObservedOutcome,
            },
            vec![b.clone()],
            "save-second",
        )
        .unwrap();
    engine(&store).adopt(&store, &ready).unwrap();
    assert_ne!(a.observations(), b.observations());
    assert_eq!(a.observations()[0].outputs[0].rows.len(), 1);
    assert_eq!(b.observations()[0].outputs[0].rows.len(), 0);
    let mut design = organizer();
    design["entities"].as_array_mut().unwrap().push(json!({"id":"dispatch","label":"Dispatch log","fields":[{"id":"purpose","label":"Purpose","value_type":{"kind":"text"}}],"unique":[],"constraints":[]}));
    let mut special = design["actions"][2].clone();
    special["id"] = json!("dispatch_one_off");
    special["steps"].as_array_mut().unwrap().insert(0,json!({"kind":"set_state","state":"selected","value":{"kind":"literal","value_type":{"kind":"list","item":{"kind":"reference","entity":"person"}},"value":empty("person")}}));
    special["steps"].as_array_mut().unwrap().insert(0,json!({"kind":"create","entity":"dispatch","values":{"purpose":text("One-off work completed")},"bind":"logged"}));
    design["actions"].as_array_mut().unwrap().push(special);
    design["views"][0]["actions"].as_array_mut().unwrap().push(json!({"id":"one_off_button","label":"One-off dispatch","placement":"toolbar","action":"dispatch_one_off","arguments":{},"enabled":yes()}));
    let mut mapped = b.scenario().clone();
    mapped.inputs[2] = invoke("dispatch_one_off", Values::new());
    let response = DevelopmentResponse {
        version: 1,
        request_digest: canonical_digest(IdentityDomain::Request, &"fixture placeholder").unwrap(),
        candidates: vec![GeneratedCandidate {
            id: "new-design".into(),
            source_json: serde_json::to_string(&design).unwrap(),
        }],
        hypotheses: vec![],
        unsupported: vec![],
        evolutions: vec![EvolutionSuggestion {
            id: "fixture-evolution".into(),
            candidate: "new-design".into(),
            needs: vec!["old-default".into(), "second-need".into()],
            proposed_retirement: vec!["old-default".into()],
            preserved_obligations: vec![invariant.identity().unwrap()],
            mappings: vec![SemanticMapping {
                from: SemanticKey {
                    kind: SemanticKind::Action,
                    entity: None,
                    id: "one_off_original".into(),
                },
                to: SemanticKey {
                    kind: SemanticKind::Action,
                    entity: None,
                    id: "dispatch_one_off".into(),
                },
            }],
            scenarios: vec![SuggestedScenarioMapping {
                source_program: None,
                original: b.scenario().identity().unwrap(),
                replacement_json: serde_json::to_string(&mapped).unwrap(),
                explanation: "Use the new explicit one-off transaction".into(),
            }],
        }],
    };
    (store, response)
}
fn choose_needs(h: &mut EguiHarness, s: &mut ProductStudio) {
    click(h, s, "studio.history");
    settle(h, s);
    click(h, s, "intention-need-old-default");
    click(h, s, "intention-need-second-need");
    fill(
        h,
        s,
        "intention-request",
        "Keep collected exports and separate one-off work",
    );
    click(h, s, "intention-reconcile");
    settle(h, s);
    assert_eq!(s.test_page(), "consent", "{}", s.test_notice());
    assert_eq!(
        s.test_prepared_request().unwrap().operation,
        DevelopmentOperation::Reconcile
    );
}
#[cfg(unix)]
#[test]
fn actual_controls_reconcile_distinct_needs_once_and_adopt_only_after_copied_trial() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let (store, response) = distinct_needs(&root);
    let before = store.load().unwrap();
    let hooks = TestHooks::default();
    let stopped = hooks.stopped.clone();
    hooks.lose_ack.store(true, Ordering::Release);
    let transport = transport(&root, &response, "good");
    let (mut s, mut h) = open(&root, Some(transport), hooks);
    choose_needs(&mut h, &mut s);
    let request = s.test_prepared_request().unwrap().clone();
    assert!(request.accepted_scenes.len() >= 4);
    assert!(s.test_disclosure_payload().unwrap().contains("Ada"));
    assert_eq!(invocation_count(&root), 0);
    click(&mut h, &mut s, "studio.consent");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "design", "{}", s.test_notice());
    assert_eq!(invocation_count(&root), 1);
    let text = frame(&mut h, &mut s).text.join("\n");
    assert!(
        text.contains("old-default")
            && text.contains("independent-concrete")
            && text.contains("independent"),
        "{text}"
    );
    assert!(text.contains("Synthetic"), "{text}");
    click(&mut h, &mut s, "intention-copy.action.one_off_button");
    settle(&mut h, &mut s);
    assert_eq!(store.load().unwrap(), before);
    click(&mut h, &mut s, "intention-accept-design");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "daily", "{}", s.test_notice());
    let saved = store.load().unwrap();
    assert!(saved
        .program()
        .unwrap()
        .program
        .entities
        .iter()
        .any(|e| e.id == "dispatch"));
    for id in ["independent-concrete", "independent"] {
        assert_eq!(
            saved
                .decisions
                .decisions
                .iter()
                .find(|d| d.id == id)
                .unwrap()
                .status,
            DecisionStatus::Active
        );
    }
    assert!(matches!(
        saved
            .decisions
            .decisions
            .iter()
            .find(|d| d.id == "old-default")
            .unwrap()
            .status,
        DecisionStatus::Superseded { .. }
    ));
    assert_eq!(
        saved
            .decisions
            .decisions
            .iter()
            .find(|d| d.id == "second-need")
            .unwrap()
            .status,
        DecisionStatus::Pending
    );
    assert_added_entity_preserves_data(&before.data, &saved.data, "dispatch");
    assert_eq!(invocation_count(&root), 1);
    drop(s);
    while !stopped.load(Ordering::Acquire) {
        std::thread::sleep(Duration::from_millis(2));
    }
    let (mut s, mut h) = open(&root, None, TestHooks::default());
    click(&mut h, &mut s, "daily.action.one_off_button");
    settle(&mut h, &mut s);
    assert!(store
        .load()
        .unwrap()
        .data
        .records
        .iter()
        .any(|r| r.entity == "dispatch"));
    assert_eq!(invocation_count(&root), 1);
}
#[cfg(unix)]
#[test]
fn reconcile_consent_return_restart_and_tampered_association_never_send() {
    for fault in [
        None,
        Some(product_studio::TestProviderAssociationFault::Missing),
        Some(product_studio::TestProviderAssociationFault::DifferentRequest),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let (store, response) = distinct_needs(&root);
        let before = store.load().unwrap();
        let hooks = TestHooks {
            provider_association_fault: fault,
            ..TestHooks::default()
        };
        let stopped = hooks.stopped.clone();
        let transport = transport(&root, &response, "good");
        let (mut s, mut h) = open(&root, Some(transport.clone()), hooks);
        choose_needs(&mut h, &mut s);
        if fault.is_some() {
            click(&mut h, &mut s, "studio.consent");
            settle(&mut h, &mut s);
            assert_eq!(s.test_page(), "consent");
        }
        assert_eq!(invocation_count(&root), 0);
        assert_eq!(store.load().unwrap(), before);
        drop(s);
        while !stopped.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(2));
        }
        let (mut s, mut h) = open(&root, Some(transport), TestHooks::default());
        assert_eq!(invocation_count(&root), 0);
        choose_needs(&mut h, &mut s);
        click(&mut h, &mut s, "studio.return");
        settle(&mut h, &mut s);
        assert_eq!(s.test_page(), "daily");
        assert_eq!(invocation_count(&root), 0);
        assert_eq!(store.load().unwrap(), before);
    }
}

#[cfg(unix)]
#[test]
fn interrupted_reconcile_keeps_other_tools_writable_across_restart() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let (original, response) = distinct_needs(&root);
    let before = original.load().unwrap();
    let transport = transport(&root, &response, "good");
    let hooks = TestHooks::default();
    let stopped = hooks.stopped.clone();
    let (mut s, mut h) = open(&root, Some(transport.clone()), hooks);
    choose_needs(&mut h, &mut s);
    let request = s.test_prepared_request().unwrap().clone();
    drop(s);
    wait_until(|| stopped.load(Ordering::Acquire));

    // Inject the same ownerless-process crash used by the existing provider
    // restart regression, retaining the actual prepared Reconcile binding.
    let session = root.join("studio/session.json");
    let mut journal: serde_json::Value =
        serde_json::from_slice(&fs::read(&session).unwrap()).unwrap();
    journal["provider"]["issued"] = serde_json::json!(true);
    fs::write(&session, serde_json::to_vec(&journal).unwrap()).unwrap();
    let binding = journal["provider"].clone();
    let mut receipt = transport.inspect(&request.id).unwrap();
    receipt.state = product_provider::JobState::Running;
    fs::write(
        root.join("jobs").join(&request.id).join("receipt.json"),
        serde_json::to_vec(&receipt).unwrap(),
    )
    .unwrap();

    let other_path = root.join("other-tool");
    let other =
        ProductStore::create(&other_path, &fixture::capture(fixture::organizer()), 20000).unwrap();
    let hooks = TestHooks {
        folder_choice: Some(other_path.clone()),
        ..TestHooks::default()
    };
    let stopped = hooks.stopped.clone();
    let mut s = ProductStudio::testing(root.clone(), Some(transport.clone()), hooks);
    settle(&mut h, &mut s);
    assert!(s.test_generation_blocked());
    assert_eq!(
        transport.inspect(&request.id).unwrap().state,
        product_provider::JobState::Interrupted
    );
    click(&mut h, &mut s, "studio.close");
    settle(&mut h, &mut s);
    click(&mut h, &mut s, "studio.open");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "daily", "{}", s.test_notice());
    let journal: serde_json::Value = serde_json::from_slice(&fs::read(&session).unwrap()).unwrap();
    assert_eq!(journal["last"]["path"], serde_json::json!(other_path));
    assert_eq!(journal["provider"], binding);

    for (index, name) in ["First offline work", "Second offline work"]
        .iter()
        .enumerate()
    {
        click(&mut h, &mut s, "daily.navigate.new_person");
        settle(&mut h, &mut s);
        fill(&mut h, &mut s, "daily.field.name", name);
        fill(&mut h, &mut s, "daily.field.area", "north");
        click(&mut h, &mut s, "daily.submit");
        settle(&mut h, &mut s);
        assert_eq!(other.load().unwrap().data.records.len(), index + 1);
        let journal: serde_json::Value =
            serde_json::from_slice(&fs::read(&session).unwrap()).unwrap();
        assert!(journal["pending"].is_null());
        assert_eq!(journal["provider"], binding);
        click(&mut h, &mut s, "daily.navigate.people");
        settle(&mut h, &mut s);
    }
    drop(s);
    wait_until(|| stopped.load(Ordering::Acquire));
    let mut s = ProductStudio::testing(root.clone(), Some(transport), TestHooks::default());
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "daily", "{}", s.test_notice());
    assert!(s.test_generation_blocked());
    assert_eq!(s.test_runtime().unwrap().retained_records.len(), 2);
    click(&mut h, &mut s, "daily.navigate.new_person");
    settle(&mut h, &mut s);
    fill(&mut h, &mut s, "daily.field.name", "After restart");
    fill(&mut h, &mut s, "daily.field.area", "north");
    click(&mut h, &mut s, "daily.submit");
    settle(&mut h, &mut s);
    let saved = other.load().unwrap();
    assert_eq!(saved.data.records.len(), 3);
    for name in ["First offline work", "Second offline work", "After restart"] {
        assert!(saved
            .data
            .records
            .iter()
            .any(|record| { record.values.get("name") == Some(&fixture::string(name)) }));
    }
    let journal: serde_json::Value = serde_json::from_slice(&fs::read(&session).unwrap()).unwrap();
    assert!(journal["pending"].is_null());
    assert_eq!(journal["provider"], binding);
    assert_eq!(original.load().unwrap(), before);
    assert_eq!(invocation_count(&root), 0);
    click(&mut h, &mut s, "studio.close");
    settle(&mut h, &mut s);
    assert!(!frame(&mut h, &mut s).controls["studio.prepare"].enabled);
}

#[path = "../src/product_studio/change_adapter.rs"]
mod mapping_fixture;
#[path = "fixtures/product_scope/mod.rs"]
mod scope_fixture;
fn work_program(pause: bool) -> CapturedProgram {
    use serde_json::json;
    let mut p = serde_json::to_value(scope_fixture::program(pause).program).unwrap();
    for (name, label) in [
        ("wait", "Wait for materials"),
        ("complete", "Complete work"),
        ("calculate", "Record production"),
    ] {
        p["views"][0]["actions"].as_array_mut().unwrap().push(json!({"id":name,"label":label,"placement":"row","action":name,"arguments":{"row":scope_fixture::var("row")},"enabled":scope_fixture::boolean(true)}));
    }
    p["views"][0]["actions"].as_array_mut().unwrap().push(json!({"id":"export","label":"Generate current work sheet","placement":"toolbar","action":"export","arguments":{},"enabled":scope_fixture::boolean(true)}));
    p["views"].as_array_mut().unwrap().push(json!({"id":"add_work","label":"Add work","kind":{"kind":"form","action":"add","fields":[{"parameter":"name","label":"Name"},{"parameter":"promised","label":"Customer commitment"}],"defaults":{"promised":{"kind":"date","days":20020}}},"actions":[],"keys":[]}));
    scope_fixture::capture(p)
}
fn scoped_work(root: &Path) -> (ProductStore, Digest) {
    use scope_fixture::{action, add, invoke, reference, request, tick};
    let store = ProductStore::create(root.join("tool"), &work_program(false), 20000).unwrap();
    let first = add(&store, "existing", "Existing work");
    action(&store, "wait-existing", "wait", &first);
    tick(&store, "advance", 20003);
    let before = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(
            &work_program(true),
            &request(&before, product_store::scope::ScopePopulation::All),
            "bad-rule",
        )
        .unwrap();
    let layer = prepared.layer_id().unwrap().unwrap();
    let scene = ScenarioSpec {
        version: 1,
        id: "bad-rule-example".into(),
        label: "Try paused production".into(),
        seed: prepared.seed().clone(),
        session: SessionState::initial(&prepared.target().program).unwrap(),
        clock_day: 20003,
        random_seed: 42,
        inputs: vec![
            invoke("calculate", &[("row", reference(&first))]),
            invoke("complete", &[("row", reference(&first))]),
            invoke("export", &[]),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
        validity: vec![],
    };
    let e = engine(&store);
    let accepted = e
        .accept_scoped_scene(&store, &prepared, &scene, Disclosure::Synthetic)
        .unwrap();
    let choice = Choice {
        id: "bad-rule-intent".into(),
        request: "Pause production while waiting".into(),
        rationale: None,
        scope: prepared.scope().clone(),
        outcome: DecisionOutcome::Accept {
            artifact: prepared.target().artifact.program_digest.clone(),
        },
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    };
    let ready = e
        .prepare_scoped_choice(&store, prepared, choice, vec![accepted], "bad-rule")
        .unwrap();
    e.adopt(&store, &ready).unwrap();
    // A real later additive design, admitted through the existing managed route.
    let before = store.load().unwrap();
    let baseline = mapping_fixture::baseline(&before).unwrap();
    let mut extended = serde_json::to_value(&baseline.program).unwrap();
    use serde_json::json;
    extended["entities"][0]["fields"].as_array_mut().unwrap().push(json!({"id":"note","label":"Work note","value_type":{"kind":"optional","item":{"kind":"text"}}}));
    extended["actions"].as_array_mut().unwrap().push(json!({"id":"set_note","label":"Keep later work note","parameters":{"row":{"kind":"reference","entity":"job"}},"guards":[],"steps":[{"kind":"update","record":scope_fixture::var("row"),"values":{"note":{"kind":"literal","value_type":{"kind":"text"},"value":{"kind":"text","value":"Keep later note"}}}}],"ensures":[]}));
    extended["views"][0]["actions"].as_array_mut().unwrap().push(json!({"id":"note","label":"Keep later work note","placement":"row","action":"set_note","arguments":{"row":scope_fixture::var("row")},"enabled":scope_fixture::boolean(true)}));
    let candidate = scope_fixture::capture(extended);
    let maps = mapping_fixture::slot_mappings(&before, &candidate).unwrap();
    let prepared = store
        .prepare_managed_evolution(&candidate, &maps, "add-field")
        .unwrap();
    let ready = e
        .prepare_managed_change(&store, prepared, &[], "add-field")
        .unwrap();
    e.adopt(&store, &ready).unwrap();
    (store, layer)
}
#[test]
fn withdrawal_controls_preserve_later_created_edited_completed_fields_events_outputs_and_restart() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let (store, layer) = scoped_work(&root);
    let hooks = TestHooks::default();
    hooks.today.store(20003, Ordering::Release);
    let clock = hooks.today.clone();
    let stopped = hooks.stopped.clone();
    let lose_ack = hooks.lose_ack.clone();
    let (mut s, mut h) = open(&root, None, hooks);
    click(&mut h, &mut s, "daily.navigate.add_work");
    settle(&mut h, &mut s);
    fill(&mut h, &mut s, "daily.field.name", "Later legitimate work");
    click(&mut h, &mut s, "daily.submit");
    settle(&mut h, &mut s);
    click(&mut h, &mut s, "daily.navigate.work");
    settle(&mut h, &mut s);
    let later = store
        .load()
        .unwrap()
        .data
        .records
        .iter()
        .find(|r| r.values.get("name") == Some(&scope_fixture::text("Later legitimate work")))
        .unwrap()
        .clone();
    for action in ["wait", "note"] {
        click(
            &mut h,
            &mut s,
            &format!("daily.row.job.{}.{action}", later.id),
        );
        settle(&mut h, &mut s);
    }
    clock.store(20006, Ordering::Release);
    frame(&mut h, &mut s);
    settle(&mut h, &mut s);
    click(
        &mut h,
        &mut s,
        &format!("daily.row.job.{}.complete", later.id),
    );
    settle(&mut h, &mut s);
    click(&mut h, &mut s, "daily.action.export");
    settle(&mut h, &mut s);
    let facts = store.load().unwrap();
    assert!(facts.artifacts.len() > 0);
    assert_eq!(
        scope_fixture::row(&facts, &later).values["note"],
        scope_fixture::text("Keep later note")
    );
    click(&mut h, &mut s, "studio.history");
    settle(&mut h, &mut s);
    click(
        &mut h,
        &mut s,
        &format!("intention-layer-{}", layer.as_str()),
    );
    click(&mut h, &mut s, "intention-preview-withdrawal");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "withdrawal", "{}", s.test_notice());
    click(&mut h, &mut s, "intention-copy.navigate.add_work");
    settle(&mut h, &mut s);
    fill(&mut h, &mut s, "intention-copy.field.name", "Copied only");
    assert!(!frame(&mut h, &mut s).controls["intention-withdraw"].enabled);
    click(&mut h, &mut s, "intention-copy.submit");
    settle(&mut h, &mut s);
    assert_eq!(store.load().unwrap(), facts);
    lose_ack.store(true, Ordering::Release);
    click(&mut h, &mut s, "intention-withdraw");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "daily", "{}", s.test_notice());
    let saved = store.load().unwrap();
    assert_eq!(saved.data, facts.data);
    assert_eq!(saved.artifacts, facts.artifacts);
    assert!(matches!(
        saved
            .decisions
            .decisions
            .iter()
            .find(|d| d.id == "bad-rule-intent")
            .unwrap()
            .status,
        DecisionStatus::Withdrawn { .. }
    ));
    drop(s);
    while !stopped.load(Ordering::Acquire) {
        std::thread::sleep(Duration::from_millis(2));
    }
    let hooks = TestHooks::default();
    hooks.today.store(20006, Ordering::Release);
    let (mut s, mut h) = open(&root, None, hooks);
    assert_eq!(store.load().unwrap(), saved);
    click(&mut h, &mut s, "daily.navigate.add_work");
    settle(&mut h, &mut s);
    fill(&mut h, &mut s, "daily.field.name", "Usable after restart");
    click(&mut h, &mut s, "daily.submit");
    settle(&mut h, &mut s);
    let continued = store.load().unwrap();
    assert_eq!(
        scope_fixture::row(&continued, &later),
        scope_fixture::row(&facts, &later)
    );
    assert_eq!(continued.data.records.len(), saved.data.records.len() + 1);
}

#[cfg(unix)]
#[test]
fn incompatible_withdrawal_keeps_work_usable_and_managed_reconciliation_can_repair_forward() {
    use serde_json::json;
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let (store, layer) = scoped_work(&root);
    let snapshot = store.load().unwrap();
    let scenario = ScenarioSpec {
        version: 1,
        id: "independent-export-example".into(),
        label: "Keep recorded production results".into(),
        seed: snapshot.data.clone(),
        session: snapshot.session.clone(),
        clock_day: 20003,
        random_seed: 0,
        inputs: vec![
            scope_fixture::invoke("export", &[]),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
        validity: vec![],
    };
    let e = engine(&store);
    let accepted = e
        .accept_current_scene(&snapshot, &scenario, Disclosure::Synthetic)
        .unwrap();
    let choice = Choice {
        id: "independent-rule".into(),
        request: "Keep the independently accepted production output".into(),
        rationale: None,
        scope: DecisionScope {
            operations: ["export".into()].into(),
            population: Population::All,
            conditions: Values::new(),
            excluded_records: vec![],
            unknowns: vec![],
        },
        outcome: DecisionOutcome::KeepCurrent,
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    };
    let ready = e
        .prepare_choice(
            &store,
            snapshot.program().unwrap(),
            choice,
            vec![accepted],
            "save-independent",
        )
        .unwrap();
    e.adopt(&store, &ready).unwrap();
    let before = store.load().unwrap();
    let mut design =
        serde_json::to_value(mapping_fixture::baseline(&before).unwrap().program).unwrap();
    design["entities"].as_array_mut().unwrap().push(json!({"id":"correction","label":"Correction notes","fields":[{"id":"name","label":"Correction","value_type":{"kind":"text"}}],"unique":[],"constraints":[]}));
    design["actions"].as_array_mut().unwrap().push(json!({"id":"log_correction","label":"Record separate correction","parameters":{"name":{"kind":"text"}},"guards":[],"steps":[{"kind":"create","entity":"correction","bind":"correction","values":{"name":scope_fixture::var("name")}}],"ensures":[]}));
    design["views"].as_array_mut().unwrap().push(json!({"id":"correction","label":"Record separate correction","kind":{"kind":"form","action":"log_correction","fields":[{"parameter":"name","label":"Correction"}],"defaults":{}},"actions":[],"keys":[]}));
    let response = DevelopmentResponse {
        version: 1,
        request_digest: canonical_digest(IdentityDomain::Request, &"placeholder").unwrap(),
        candidates: vec![GeneratedCandidate {
            id: "compatible-repair".into(),
            source_json: serde_json::to_string(&design).unwrap(),
        }],
        hypotheses: vec![],
        unsupported: vec![],
        evolutions: vec![EvolutionSuggestion {
            id: "fixture-evolution".into(),
            candidate: "compatible-repair".into(),
            needs: vec!["bad-rule-intent".into(), "independent-rule".into()],
            proposed_retirement: vec![],
            preserved_obligations: vec![],
            mappings: vec![],
            scenarios: vec![],
        }],
    };
    let transport = transport(&root, &response, "good");
    let hooks = TestHooks::default();
    hooks.today.store(20003, Ordering::Release);
    let (mut s, mut h) = open(&root, Some(transport), hooks);
    click(&mut h, &mut s, "studio.history");
    settle(&mut h, &mut s);
    click(
        &mut h,
        &mut s,
        &format!("intention-layer-{}", layer.as_str()),
    );
    click(&mut h, &mut s, "intention-preview-withdrawal");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "history");
    assert!(
        s.test_notice().contains("compatible new design"),
        "{}",
        s.test_notice()
    );
    assert_eq!(store.load().unwrap(), before);
    click(&mut h, &mut s, "intention-return");
    settle(&mut h, &mut s);
    click(&mut h, &mut s, "daily.navigate.add_work");
    settle(&mut h, &mut s);
    fill(&mut h, &mut s, "daily.field.name", "Continue after refusal");
    click(&mut h, &mut s, "daily.submit");
    settle(&mut h, &mut s);
    let current = store.load().unwrap();
    click(&mut h, &mut s, "studio.history");
    settle(&mut h, &mut s);
    for id in ["bad-rule-intent", "independent-rule"] {
        click(&mut h, &mut s, &format!("intention-need-{id}"));
    }
    fill(
        &mut h,
        &mut s,
        "intention-request",
        "Keep production promises and let me record a separate correction",
    );
    click(&mut h, &mut s, "intention-reconcile");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "consent", "{}", s.test_notice());
    assert!(s
        .test_disclosure_payload()
        .unwrap()
        .contains("Host-verified editing guide"));
    click(&mut h, &mut s, "studio.consent");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "design", "{}", s.test_notice());
    click(&mut h, &mut s, "intention-accept-design");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "daily", "{}", s.test_notice());
    let saved = store.load().unwrap();
    assert_added_entity_preserves_data(&current.data, &saved.data, "correction");
    assert_eq!(saved.artifacts, current.artifacts);
    assert_eq!(
        saved
            .decisions
            .decisions
            .iter()
            .find(|d| d.id == "independent-rule")
            .unwrap()
            .status,
        DecisionStatus::Active
    );
    click(&mut h, &mut s, "daily.navigate.correction");
    settle(&mut h, &mut s);
    fill(
        &mut h,
        &mut s,
        "daily.field.name",
        "Keep this correction without rewriting past production",
    );
    click(&mut h, &mut s, "daily.submit");
    settle(&mut h, &mut s);
    assert!(store
        .load()
        .unwrap()
        .data
        .records
        .iter()
        .any(|r| r.entity == "correction"));
    assert_eq!(invocation_count(&root), 1);
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let start = Instant::now();
    while !condition() {
        assert!(start.elapsed() < Duration::from_secs(120));
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[cfg(unix)]
#[test]
fn reconcile_provider_cancel_and_close_never_adopt_or_resubmit() {
    for close in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let (store, response) = distinct_needs(&root);
        let before = store.load().unwrap();
        let transport = transport(&root, &response, "slow");
        let (mut s, mut h) = open(&root, Some(transport), TestHooks::default());
        choose_needs(&mut h, &mut s);
        click(&mut h, &mut s, "studio.consent");
        wait_until(|| invocation_count(&root) == 1);
        if close {
            s.test_close();
        } else {
            click(&mut h, &mut s, "studio.cancel");
        }
        settle(&mut h, &mut s);
        assert_eq!(store.load().unwrap(), before);
        assert_eq!(invocation_count(&root), 1);
        s.test_open(root.join("tool"));
        settle(&mut h, &mut s);
        assert_eq!(s.test_page(), "daily");
        assert_eq!(invocation_count(&root), 1);
    }
}
#[cfg(unix)]
#[test]
fn design_final_gate_rejects_cancellation_date_change_and_stale_work() {
    for fault in ["cancel", "date", "stale"] {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let (store, response) = distinct_needs(&root);
        let transport = transport(&root, &response, "good");
        let pause = std::sync::Arc::new(product_studio::TestPause::default());
        let hooks = TestHooks {
            before_commit: Some(pause.clone()),
            ..TestHooks::default()
        };
        let clock = hooks.today.clone();
        let (mut s, mut h) = open(&root, Some(transport), hooks);
        choose_needs(&mut h, &mut s);
        click(&mut h, &mut s, "studio.consent");
        settle(&mut h, &mut s);
        assert_eq!(s.test_page(), "design", "{}", s.test_notice());
        let before = store.load().unwrap();
        click(&mut h, &mut s, "intention-accept-design");
        wait_until(|| pause.reached.load(Ordering::Acquire));
        let expected = match fault {
            "cancel" => {
                assert!(s.test_cancel());
                before.clone()
            }
            "date" => {
                clock.store(20001, Ordering::Release);
                before.clone()
            }
            _ => store
                .apply(
                    before.revision,
                    "later-external-work",
                    &fixture::add("Later work"),
                    RuntimeLimits::default(),
                )
                .unwrap(),
        };
        pause.release.store(true, Ordering::Release);
        settle(&mut h, &mut s);
        assert_eq!(store.load().unwrap(), expected);
        assert!(!store
            .load()
            .unwrap()
            .program()
            .unwrap()
            .program
            .entities
            .iter()
            .any(|e| e.id == "dispatch"));
        assert_eq!(invocation_count(&root), 1);
    }
}

#[cfg(unix)]
#[test]
fn copied_design_queue_keeps_newer_values_and_blocks_adoption_until_exact_ack() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let (store, response) = distinct_needs(&root);
    let before = store.load().unwrap();
    let transport = transport(&root, &response, "good");
    let pause = std::sync::Arc::new(product_studio::TestPause::default());
    let hooks = TestHooks {
        before_preview: Some(pause.clone()),
        duplicate_completion: true,
        ..TestHooks::default()
    };
    let (mut s, mut h) = open(&root, Some(transport), hooks);
    choose_needs(&mut h, &mut s);
    click(&mut h, &mut s, "studio.consent");
    settle(&mut h, &mut s);
    click(&mut h, &mut s, "intention-copy.control.search_input");
    h.text("a");
    frame(&mut h, &mut s);
    wait_until(|| pause.reached.load(Ordering::Acquire));
    h.text("b");
    let queued = frame(&mut h, &mut s);
    assert!(queued.controls["intention-copy.control.search_input"].enabled);
    assert!(!queued.controls["intention-accept-design"].enabled);
    pause.release.store(true, Ordering::Release);
    wait_until(|| {
        s.poll();
        !s.is_busy()
    });
    assert_eq!(
        s.test_runtime().unwrap().observation.controls["search_input"],
        fixture::string("a")
    );
    let next = frame(&mut h, &mut s);
    assert!(!next.controls["intention-accept-design"].enabled);
    settle(&mut h, &mut s);
    assert_eq!(
        s.test_runtime().unwrap().observation.controls["search_input"],
        fixture::string("ab")
    );
    assert_eq!(store.load().unwrap(), before);
    click(&mut h, &mut s, "intention-copy.navigate.new_person");
    settle(&mut h, &mut s);
    fill(
        &mut h,
        &mut s,
        "intention-copy.field.name",
        "Unsubmitted copy",
    );
    assert!(!frame(&mut h, &mut s).controls["intention-accept-design"].enabled);
    click(&mut h, &mut s, "intention-discard-design");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "daily");
    assert_eq!(store.load().unwrap(), before);
}
#[cfg(unix)]
#[test]
fn restarted_reconcile_rejects_tampered_purpose_basis_needs_evolution_and_operation() {
    for field in [
        "purpose",
        "source",
        "data",
        "decisions",
        "needs",
        "evolution",
        "operation",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let (store, response) = distinct_needs(&root);
        let before = store.load().unwrap();
        let transport = transport(&root, &response, "good");
        let hooks = TestHooks::default();
        let stopped = hooks.stopped.clone();
        let (mut s, mut h) = open(&root, Some(transport.clone()), hooks);
        choose_needs(&mut h, &mut s);
        drop(s);
        wait_until(|| stopped.load(Ordering::Acquire));
        let file = root.join("studio/session.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        match field {
            "purpose" => value["provider"]["request"]["operation"] = serde_json::json!("modify"),
            "source" | "data" | "decisions" => {
                value["provider"]["reconcile"]["basis"][field] = serde_json::to_value(
                    canonical_digest(IdentityDomain::Evidence, &"wrong basis").unwrap(),
                )
                .unwrap()
            }
            "needs" => value["provider"]["reconcile"]["needs"] = serde_json::json!(["independent"]),
            _ => value["provider"]["reconcile"][field] = serde_json::json!("other-valid-id"),
        }
        let corrupt = serde_json::to_vec(&value).unwrap();
        fs::write(&file, &corrupt).unwrap();
        let mut s = ProductStudio::testing(root.clone(), Some(transport), TestHooks::default());
        settle(&mut h, &mut s);
        assert!(s.test_generation_blocked(), "{field}: {}", s.test_notice());
        assert_eq!(invocation_count(&root), 0);
        assert_eq!(fs::read(&file).unwrap(), corrupt);
        assert_eq!(store.load().unwrap(), before);
    }
}

fn assert_added_entity_preserves_data(before: &DataSnapshot, after: &DataSnapshot, added: &str) {
    assert_eq!(after.version, before.version);
    assert_eq!(after.project_id, before.project_id);
    assert_eq!(after.generation, before.generation);
    assert_eq!(after.records, before.records);
    assert_eq!(after.events, before.events);
    assert_eq!(after.schema.len(), before.schema.len() + 1);
    for entity in &before.schema {
        assert!(after.schema.contains(entity));
    }
    assert!(after.schema.iter().any(|entity| entity.id == added));
}
#[cfg(unix)]
#[test]
fn history_draft_and_exact_selection_survive_back_and_provider_cancel() {
    for mode in ["good", "slow"] {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let (store, response) = distinct_needs(&root);
        let before = store.load().unwrap();
        let transport = transport(&root, &response, mode);
        let (mut s, mut h) = open(&root, Some(transport), TestHooks::default());
        choose_needs(&mut h, &mut s);
        let first = s.test_prepared_request().unwrap().clone();
        if mode == "slow" {
            click(&mut h, &mut s, "studio.consent");
            wait_until(|| invocation_count(&root) == 1);
            click(&mut h, &mut s, "studio.cancel");
            settle(&mut h, &mut s);
            s.test_open(root.join("tool"));
            settle(&mut h, &mut s);
        } else {
            click(&mut h, &mut s, "studio.return");
            settle(&mut h, &mut s);
        }
        assert_eq!(
            s.test_need(),
            "Keep collected exports and separate one-off work"
        );
        click(&mut h, &mut s, "studio.history");
        settle(&mut h, &mut s);
        assert!(frame(&mut h, &mut s).controls["intention-reconcile"].enabled);
        click(&mut h, &mut s, "intention-reconcile");
        settle(&mut h, &mut s);
        let request = s.test_prepared_request().unwrap();
        assert!(request
            .request
            .starts_with("Keep collected exports and separate one-off work\n"));
        assert!(request
            .request
            .contains("Preserve both accepted needs: old-default, second-need."));
        assert_ne!(request.id, first.id);
        assert_eq!(invocation_count(&root), usize::from(mode == "slow"));
        assert_eq!(store.load().unwrap(), before);
    }
}
#[test]
fn daily_checkpoint_handoff_rechecks_bytes_and_reports_saved_work_warning() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let store = seeded(&root);
    let pause = std::sync::Arc::new(product_studio::TestPause::default());
    let hooks = TestHooks {
        after_commit: Some(pause.clone()),
        ..TestHooks::default()
    };
    let (mut s, mut h) = open(&root, None, hooks);
    s.test_daily(fixture::add("Saved before backup failure"));
    wait_until(|| pause.reached.load(Ordering::Acquire));
    let saved = store.load().unwrap();
    let object = root.join(format!(
        "tool/extension-{}.json",
        saved.decisions.decisions[0].witness.as_str()
    ));
    let original = fs::read(&object).unwrap();
    fs::write(&object, b"broken after verified commit").unwrap();
    pause.release.store(true, Ordering::Release);
    settle(&mut h, &mut s);
    assert!(
        s.test_notice().contains("saved and usable") && s.test_notice().contains("Do not repeat"),
        "{}",
        s.test_notice()
    );
    fs::write(&object, original).unwrap();
    assert_eq!(store.load().unwrap(), saved);
    assert_eq!(saved.data.records.len(), 1);
}
