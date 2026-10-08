//! Offline controller regressions. No live AI, native GUI, or user acceptance proof.
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
#[path = "fixtures/product_runtime/mod.rs"]
mod other_shape;
#[path = "../src/product_studio.rs"]
mod product_studio;
use fixture::*;
use product_contract::*;
use product_store::{scope::*, ProductStore};
use product_studio::{ProductStudio, TestHooks};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

// The shared backend corpus intentionally has no UI bindings. Host examples
// expose the same business behavior through the production generated renderer.
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
        "defaults":{"promised":{"kind":"date","days":20010}}},"actions":[],"keys":[]
    }));
    capture(value)
}

#[cfg(unix)]
fn frame(
    h: &mut egui_harness::EguiHarness,
    s: &mut ProductStudio,
) -> product_runtime_view::WidgetTrace {
    s.poll();
    h.frame(|ctx| {
        egui::CentralPanel::default()
            .show(ctx, |ui| s.show(ui))
            .inner
    })
}

#[cfg(unix)]
fn click(h: &mut egui_harness::EguiHarness, s: &mut ProductStudio, key: &str) {
    let trace = frame(h, s);
    let control = trace
        .controls
        .get(key)
        .unwrap_or_else(|| panic!("missing {key}: {:?}", trace.text));
    assert!(control.enabled, "disabled {key}: {:?}", trace.text);
    let point = control.rect.center();
    h.press_at(point);
    frame(h, s);
    h.release_at(point);
    frame(h, s);
    settle(s);
}

fn settle(studio: &mut ProductStudio) {
    let start = Instant::now();
    loop {
        studio.poll();
        if !studio.is_busy() {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(120),
            "{}",
            studio.test_notice()
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn lifecycle(snapshot: &product_store::ProjectSnapshot) -> LifecycleBinding {
    LifecycleBinding {
        entity: "job".into(),
        completed: field("record", "done"),
        source: snapshot.active_revision.clone(),
    }
}

#[test]
fn adapter_extracts_real_effects_and_protects_unchanged_writers() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let current = store.load().unwrap();
    let mut changed = serde_json::to_value(&program(false).program).unwrap();
    changed["actions"][3]["steps"][0]["values"]["production"] =
        serde_json::to_value(elapsed("row", true)).unwrap();
    let candidate = capture(changed);
    let request = product_studio::test_scope_request(
        &current,
        &candidate,
        ScopePopulation::FutureWork,
        vec![lifecycle(&current)],
    )
    .unwrap();
    assert!(request.patches.iter().any(|p| matches!(&p.destination, EffectDestination::Update { action, field, .. } if action == "calculate" && field == "production")));
    assert!(request.patches.iter().any(|p| matches!(&p.destination, EffectDestination::Update { action, field, .. } if action == "complete" && field == "production")));
    assert_eq!(
        request.operations,
        ["calculate".into(), "complete".into()]
            .into_iter()
            .collect()
    );
    store
        .prepare_scoped_change(&candidate, &request, "verified-adapter")
        .unwrap();
}

#[test]
fn adapter_never_guesses_completion_and_refuses_structural_patch_disguise() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let current = store.load().unwrap();
    assert!(product_studio::test_scope_request(
        &current,
        &program(true),
        ScopePopulation::All,
        vec![]
    )
    .unwrap_err()
    .contains("finished"));
    let mut changed = serde_json::to_value(&program(true).program).unwrap();
    changed["actions"][3]["guards"] = serde_json::json!([boolean(true)]);
    assert!(product_studio::test_scope_request(
        &current,
        &capture(changed),
        ScopePopulation::All,
        vec![lifecycle(&current)]
    )
    .unwrap_err()
    .contains("design"));
    assert_eq!(store.load().unwrap(), current);
}

#[test]
fn adapter_uses_bound_nested_variables_and_static_empty_view_types() {
    let dir = tempdir();
    let mut original = serde_json::to_value(&program(false).program).unwrap();
    let items = original["views"][0]["kind"]["rows"].clone();
    original["actions"].as_array_mut().unwrap().push(serde_json::json!({
        "id":"batch", "label":"Record every item", "parameters":{}, "guards":[], "ensures":[],
        "steps":[{"kind":"for_each", "items": items, "binding":"item", "steps":[{"kind":"update", "record":var("item"), "values":{"production":elapsed("item", false)}}]}]
    }));
    let store =
        ProductStore::create(dir.path().join("tool"), &capture(original.clone()), 20000).unwrap();
    let current = store.load().unwrap();
    original["actions"][7]["steps"][0]["steps"][0]["values"]["production"] =
        serde_json::to_value(elapsed("item", true)).unwrap();
    original["views"][0]["kind"]["columns"][1]["value"] =
        serde_json::to_value(elapsed("row", true)).unwrap();
    let candidate = capture(original);
    let req = product_studio::test_scope_request(
        &current,
        &candidate,
        ScopePopulation::All,
        vec![lifecycle(&current)],
    )
    .unwrap();
    assert!(req.patches.iter().any(|p| p.subject == "item"
        && matches!(&p.destination, EffectDestination::Update {path,..} if path == &[0,0])));
    assert!(req.patches.iter().any(|p| p.value_type == Type::Integer
        && matches!(p.destination, EffectDestination::ViewColumn { .. })));
    store
        .prepare_scoped_change(&candidate, &req, "nested-adapter")
        .unwrap();
}

#[cfg(unix)]
fn transport(root: &Path, candidate: &CapturedProgram) -> product_provider::ProviderTransport {
    use std::os::unix::fs::PermissionsExt;
    let executable = root.join("change-fixture.py");
    let _guard = product_provider::fixture_executable_write_guard();
    let body = "{'version':1,'request_digest':json.loads(wire['prompt'])['request_digest'],'candidates':[{'id':'changed','source_json':json.dumps(cfg['program'])}],'hypotheses':[],'evolutions':[],'unsupported':[]}";
    fs::write(
        &executable,
        include_str!("fixtures/provider_transport/fake_cli.py").replace(
            "{'passed': True, 'text': wire['prompt'], 'command': 'untrusted-do-not-execute'}",
            body,
        ),
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(
        executable.with_extension("json"),
        serde_json::to_vec(&serde_json::json!({"program": candidate.program, "mode":"good"}))
            .unwrap(),
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
#[cfg(unix)]
fn change(studio: &mut ProductStudio, path: &Path) {
    studio.test_open(path.into());
    settle(studio);
    studio.test_modify("Pause production while waiting and keep customer commitments");
    settle(studio);
    assert_eq!(studio.test_page(), "consent", "{}", studio.test_notice());
    studio.test_consent();
    settle(studio);
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
}
#[cfg(unix)]
#[test]
fn modify_context_is_exact_and_no_provider_runs_before_disclosure_consent() {
    let dir = tempdir();
    let root = dir.path();
    let path = root.join("tool");
    let store = ProductStore::create(&path, &program(false), 20000).unwrap();
    let row = add(&store, "first", "Private example");
    let mut studio = ProductStudio::testing(
        root.into(),
        Some(transport(root, &program(true))),
        TestHooks::default(),
    );
    settle(&mut studio);
    studio.test_open(path);
    settle(&mut studio);
    studio.test_daily(invoke("wait", &[("row", reference(&row))]));
    settle(&mut studio);
    studio.test_modify("Pause production while waiting");
    settle(&mut studio);
    let request = studio.test_prepared_request().unwrap();
    assert_eq!(request.operation, DevelopmentOperation::Modify);
    assert_eq!(
        request.sources.last(),
        Some(store.load().unwrap().program().unwrap())
    );
    assert_eq!(
        request.context.recent_inputs,
        vec![invoke("wait", &[("row", reference(&row))])]
    );
    assert_eq!(
        request.context.data_digest,
        Some(store.load().unwrap().data.identity().unwrap())
    );
    assert!(!root
        .join("jobs")
        .read_dir()
        .unwrap()
        .filter_map(Result::ok)
        .any(|e| e.path().join("fixture-invocation.json").exists()));
}

#[cfg(unix)]
#[test]
fn scoped_host_choices_preserve_mixed_completed_work_and_rehearse_after_scope_change() {
    for population in [
        ScopePopulation::All,
        ScopePopulation::FutureWork,
        ScopePopulation::SelectedUnfinished { records: vec![] },
    ] {
        let dir = tempdir();
        let root = dir.path();
        let path = root.join("tool");
        let store = ProductStore::create(&path, &program(false), 20000).unwrap();
        let selected = add(&store, "selected", "Selected");
        let other = add(&store, "other", "Other");
        let completed = add(&store, "completed", "Completed");
        let archived = add(&store, "archived", "Archived");
        action(&store, "archive-original", "archive", &archived);
        action(&store, "wait-first", "wait", &selected);
        action(&store, "wait-other", "wait", &other);
        action(&store, "complete-original", "complete", &completed);
        let before = store.load().unwrap();
        let hooks = TestHooks::default();
        let today = hooks.today.clone();
        hooks
            .lose_ack
            .store(true, std::sync::atomic::Ordering::Release);
        let mut studio =
            ProductStudio::testing(root.into(), Some(transport(root, &program(true))), hooks);
        settle(&mut studio);
        change(&mut studio, &path);
        studio.test_lifecycle_outcome(
            RecordRef {
                entity: completed.entity.clone(),
                record: completed.id.clone(),
            },
            "done",
        );
        settle(&mut studio);
        assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
        let population = match population {
            ScopePopulation::SelectedUnfinished { .. } => ScopePopulation::SelectedUnfinished {
                records: vec![RecordRef {
                    entity: selected.entity.clone(),
                    record: selected.id.clone(),
                }],
            },
            x => x,
        };
        studio.test_scope(population.clone());
        settle(&mut studio);
        assert!(!studio.test_can_accept());
        studio.test_trial(SemanticInput::AdvanceClock { days: 3 });
        settle(&mut studio);
        // All affected operations must be represented, including fresh work for FutureWork.
        studio.test_trial(invoke(
            "add",
            &[
                ("name", text("Future")),
                ("promised", DataValue::Date { days: 20010 }),
            ],
        ));
        settle(&mut studio);
        let row = if matches!(population, ScopePopulation::FutureWork) {
            studio
                .test_alternative()
                .unwrap()
                .retained_records
                .iter()
                .find(|r| r.values.get("name") == Some(&text("Future")))
                .unwrap()
                .clone()
        } else {
            selected.clone()
        };
        for action_id in ["calculate", "complete", "export"] {
            studio.test_trial(if action_id == "export" {
                invoke(action_id, &[])
            } else {
                invoke(action_id, &[("row", reference(&row))])
            });
            settle(&mut studio);
        }
        assert!(studio.test_can_accept(), "{}", studio.test_notice());
        studio.test_decide(DecisionOutcome::Accept {
            artifact: program(true).artifact.program_digest,
        });
        settle(&mut studio);
        assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
        let saved = store.load().unwrap();
        assert_eq!(
            saved
                .data
                .records
                .iter()
                .map(|r| (&r.id, &r.values["name"]))
                .collect::<Vec<_>>(),
            before
                .data
                .records
                .iter()
                .map(|r| (&r.id, &r.values["name"]))
                .collect::<Vec<_>>()
        );
        assert_eq!(saved.data.events, before.data.events);
        assert_eq!(saved.scope.adoptions.len(), 1);
        assert!(studio
            .test_runtime()
            .unwrap()
            .history
            .results
            .iter()
            .any(|r| r.record.record == completed.id));
        today.store(20003, std::sync::atomic::Ordering::Release);
        settle(&mut studio);
        for live in [&selected, &other] {
            studio.test_daily(invoke("calculate", &[("row", reference(live))]));
            settle(&mut studio);
        }
        let calculated = store.load().unwrap();
        let selected_days = if matches!(population, ScopePopulation::FutureWork) {
            3
        } else {
            0
        };
        let other_days = if population == ScopePopulation::All {
            0
        } else {
            3
        };
        assert_eq!(
            fixture::row(&calculated, &selected).values["production"],
            DataValue::Integer {
                value: selected_days
            }
        );
        assert_eq!(
            fixture::row(&calculated, &other).values["production"],
            DataValue::Integer { value: other_days }
        );
        studio.test_daily(invoke("complete", &[("row", reference(&selected))]));
        settle(&mut studio);
        studio.test_daily(invoke(
            "add",
            &[
                ("name", text("Later saved work")),
                ("promised", DataValue::Date { days: 20020 }),
            ],
        ));
        settle(&mut studio);
        let later = store
            .load()
            .unwrap()
            .data
            .records
            .into_iter()
            .find(|r| r.values.get("name") == Some(&text("Later saved work")))
            .unwrap();
        studio.test_daily(invoke("wait", &[("row", reference(&later))]));
        settle(&mut studio);
        today.store(20006, std::sync::atomic::Ordering::Release);
        settle(&mut studio);
        studio.test_daily(invoke("calculate", &[("row", reference(&later))]));
        settle(&mut studio);
        studio.test_daily(invoke("export", &[]));
        settle(&mut studio);
        let final_work = store.load().unwrap();
        let later_days = if matches!(population, ScopePopulation::SelectedUnfinished { .. }) {
            3
        } else {
            0
        };
        assert_eq!(
            fixture::row(&final_work, &later).values["production"],
            DataValue::Integer { value: later_days }
        );
        assert_eq!(
            fixture::row(&final_work, &selected).values["production"],
            DataValue::Integer {
                value: selected_days
            }
        );
        assert_eq!(
            fixture::row(&final_work, &completed).values["production"],
            DataValue::Integer { value: 0 }
        );
        assert!(fixture::row(&final_work, &archived).archived);
        let output = final_work.artifacts.last().unwrap();
        for (name, expected) in [
            ("Selected", selected_days),
            (
                "Other",
                if population == ScopePopulation::All {
                    0
                } else {
                    6
                },
            ),
            ("Completed", 0),
            ("Archived", 0),
            ("Later saved work", later_days),
        ] {
            let record = output
                .rows
                .iter()
                .find(|r| r.get("name") == Some(&text(name)))
                .unwrap();
            assert_eq!(record["production"], DataValue::Integer { value: expected });
        }
        assert_eq!(final_work.data.records.len(), 5);
        assert!(final_work.data.events.len() > before.data.events.len());
        studio.test_close();
        settle(&mut studio);
        studio.test_open(path.clone());
        settle(&mut studio);
        assert_eq!(studio.test_page(), "daily");
        assert_eq!(store.load().unwrap(), final_work);
    }
}

#[cfg(unix)]
#[test]
fn every_pending_choice_keeps_live_state_and_resolves_fresh_after_work_and_reopen() {
    for outcome in [
        DecisionOutcome::EitherAcceptable,
        DecisionOutcome::BothNeeded,
        DecisionOutcome::NeitherFits,
        DecisionOutcome::Deferred,
    ] {
        let dir = tempdir();
        let root = dir.path();
        let path = root.join("tool");
        let store = ProductStore::create(&path, &program(false), 20000).unwrap();
        let row = add(&store, "item", "Copied work");
        action(&store, "wait", "wait", &row);
        action(&store, "finish", "complete", &row);
        let before = store.load().unwrap();
        let hooks = TestHooks::default();
        let stopped = hooks.stopped.clone();
        let mut studio =
            ProductStudio::testing(root.into(), Some(transport(root, &program(true))), hooks);
        settle(&mut studio);
        change(&mut studio, &path);
        studio.test_lifecycle_outcome(
            RecordRef {
                entity: row.entity.clone(),
                record: row.id.clone(),
            },
            "done",
        );
        settle(&mut studio);
        studio.test_trial(invoke("export", &[]));
        settle(&mut studio);
        studio.test_decide(outcome.clone());
        settle(&mut studio);
        assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
        let retained = store.load().unwrap();
        assert_eq!(retained.active_revision, before.active_revision);
        assert_eq!(retained.data, before.data);
        assert_eq!(retained.session, before.session);
        assert_eq!(retained.artifacts, before.artifacts);
        assert_eq!(retained.decisions.decisions[0].outcome, outcome);
        assert_eq!(
            retained.decisions.decisions[0].status,
            DecisionStatus::Pending
        );
        let decision = retained.decisions.decisions[0].id.clone();
        studio.test_daily(invoke(
            "add",
            &[
                ("name", text("Later real work")),
                ("promised", DataValue::Date { days: 20010 }),
            ],
        ));
        settle(&mut studio);
        drop(studio);
        let start = Instant::now();
        while !stopped.load(std::sync::atomic::Ordering::Acquire) {
            assert!(start.elapsed() < Duration::from_secs(30));
            std::thread::sleep(Duration::from_millis(2));
        }
        let mut reopened = ProductStudio::testing(root.into(), None, TestHooks::default());
        settle(&mut reopened);
        reopened.test_resume_choice(&decision);
        settle(&mut reopened);
        assert_eq!(reopened.test_page(), "change", "{}", reopened.test_notice());
        assert!(!reopened.test_can_accept());
        assert_eq!(store.load().unwrap().data.records.len(), 2);
        let backup = product_backup::VerifiedBackup::capture(&store).unwrap();
        let recovered = backup.recover_new(&root.join("recovery")).unwrap();
        assert_eq!(
            recovered.load().unwrap().decisions,
            store.load().unwrap().decisions
        );
        let later = store
            .load()
            .unwrap()
            .data
            .records
            .into_iter()
            .find(|r| r.values.get("name") == Some(&text("Later real work")))
            .unwrap();
        for name in ["calculate", "complete", "export"] {
            reopened.test_trial(if name == "export" {
                invoke(name, &[])
            } else {
                invoke(name, &[("row", reference(&later))])
            });
            settle(&mut reopened);
        }
        reopened.test_decide(DecisionOutcome::Accept {
            artifact: program(true).artifact.program_digest,
        });
        settle(&mut reopened);
        assert_eq!(reopened.test_page(), "daily", "{}", reopened.test_notice());
        let resolved = store.load().unwrap();
        assert!(matches!(
            resolved.decisions.decisions[0].status,
            DecisionStatus::Superseded { .. }
        ));
        assert_eq!(resolved.data.records.len(), 2);
        reopened.test_modify("Keep this result and adjust the title");
        settle(&mut reopened);
        // Offline provider absence must not change the newly accepted tool.
        assert_eq!(store.load().unwrap(), resolved);
    }
}

#[test]
fn real_example_origin_roundtrips_and_prepared_disclosure_reports_mixed_origins() {
    let program = program(false);
    let mut request = DevelopmentRequest {
        version: 1,
        id: "origins".into(),
        project_id: "scope-project".into(),
        operation: DevelopmentOperation::Modify,
        request: "Change a calculation".into(),
        sources: vec![program.clone()],
        context: DevelopmentContext {
            view: None,
            selected: vec![],
            recent_inputs: vec![],
            data_digest: None,
            session_digest: None,
        },
        examples: vec![],
        accepted_scenes: vec![],
        decisions: DecisionGraph {
            version: 1,
            revision: 0,
            decisions: vec![],
        },
        unknowns: vec![],
        required_capabilities: Default::default(),
    };
    for (disclosure, id) in [
        (Disclosure::Synthetic, "synthetic"),
        (Disclosure::ExplicitlySelectedSanitized, "sanitized"),
        (Disclosure::ExplicitlySelected, "real"),
    ] {
        request.examples.push(SelectedScenario {
            disclosure,
            scenario: ScenarioSpec {
                version: 1,
                id: id.into(),
                label: "Selected example".into(),
                seed: DataSnapshot::empty("scope-project", &program.program).unwrap(),
                session: SessionState::initial(&program.program).unwrap(),
                clock_day: 20000,
                random_seed: 0,
                inputs: vec![SemanticInput::Observe {
                    point: "result".into(),
                }],
                validity: vec![],
            },
        });
    }
    let encoded = serde_json::to_vec(&request).unwrap();
    let decoded: DevelopmentRequest = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, request);
    let wire =
        product_discovery::encode_request(&request, &product_discovery::ProviderOptions::default())
            .unwrap();
    assert!(wire
        .data_categories
        .iter()
        .any(|s| s.contains("1 synthetic examples")));
    assert!(wire
        .data_categories
        .iter()
        .any(|s| s.contains("1 explicitly selected sanitized")));
    assert!(wire
        .data_categories
        .iter()
        .any(|s| s.contains("1 explicitly selected real business copies, not sanitized")));
}

#[cfg(unix)]
#[test]
fn stale_saved_work_and_date_refuse_trial_scope_and_accept_without_mutation() {
    let dir = tempdir();
    let root = dir.path();
    let path = root.join("tool");
    let store = ProductStore::create(&path, &program(false), 20000).unwrap();
    let row = add(&store, "first", "Current work");
    action(&store, "finished", "complete", &row);
    let hooks = TestHooks::default();
    let today = hooks.today.clone();
    let mut studio =
        ProductStudio::testing(root.into(), Some(transport(root, &program(true))), hooks);
    settle(&mut studio);
    change(&mut studio, &path);
    studio.test_lifecycle_outcome(
        RecordRef {
            entity: row.entity.clone(),
            record: row.id.clone(),
        },
        "done",
    );
    settle(&mut studio);
    add(&store, "intervening", "Work from another window");
    let changed = store.load().unwrap();
    studio.test_scope(ScopePopulation::FutureWork);
    settle(&mut studio);
    assert!(studio.test_notice().contains("changed"));
    assert_eq!(store.load().unwrap(), changed);
    studio.test_trial(invoke("export", &[]));
    settle(&mut studio);
    assert!(studio.test_notice().contains("changed"));
    today.store(20001, std::sync::atomic::Ordering::Release);
    studio.test_decide(DecisionOutcome::Deferred);
    settle(&mut studio);
    assert!(studio.test_notice().contains("stale"));
    assert_eq!(store.load().unwrap(), changed);
}

#[cfg(unix)]
#[test]
fn modification_cancel_keeps_daily_tool_and_ignores_duplicate_late_delivery() {
    let dir = tempdir();
    let root = dir.path();
    let path = root.join("tool");
    let store = ProductStore::create(&path, &program(false), 20000).unwrap();
    let before = store.load().unwrap();
    let pause = std::sync::Arc::new(product_studio::TestPause::default());
    let hooks = TestHooks {
        before_preview: Some(pause.clone()),
        duplicate_completion: true,
        ..TestHooks::default()
    };
    let mut studio =
        ProductStudio::testing(root.into(), Some(transport(root, &program(true))), hooks);
    settle(&mut studio);
    change(&mut studio, &path);
    studio.test_trial(invoke(
        "add",
        &[
            ("name", text("Cancelled copy")),
            ("promised", DataValue::Date { days: 20010 }),
        ],
    ));
    let start = Instant::now();
    while !pause.reached.load(std::sync::atomic::Ordering::Acquire) {
        studio.poll();
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(studio.test_cancel());
    studio.test_close();
    pause
        .release
        .store(true, std::sync::atomic::Ordering::Release);
    settle(&mut studio);
    assert_eq!(studio.test_page(), "home");
    assert_eq!(store.load().unwrap(), before);
    studio.test_open(path);
    settle(&mut studio);
    assert_eq!(studio.test_page(), "daily");
}

#[cfg(unix)]
#[test]
fn old_journal_and_unissued_modify_reopen_without_resending_or_losing_saved_work() {
    let dir = tempdir();
    let root = dir.path();
    let path = root.join("tool");
    let store = ProductStore::create(&path, &program(false), 20000).unwrap();
    let hooks = TestHooks::default();
    let stopped = hooks.stopped.clone();
    let mut studio =
        ProductStudio::testing(root.into(), Some(transport(root, &program(true))), hooks);
    settle(&mut studio);
    studio.test_open(path.clone());
    settle(&mut studio);
    studio.test_modify("Keep this exact unsent request");
    settle(&mut studio);
    assert_eq!(studio.test_page(), "consent");
    drop(studio);
    let start = Instant::now();
    while !stopped.load(std::sync::atomic::Ordering::Acquire) {
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(2));
    }
    let mut reopened = ProductStudio::testing(root.into(), None, TestHooks::default());
    settle(&mut reopened);
    assert_eq!(reopened.test_page(), "daily");
    assert_eq!(reopened.test_need(), "Keep this exact unsent request");
    assert!(!reopened.test_generation_blocked());
    assert!(store.load().unwrap().decisions.decisions.is_empty());
}

#[cfg(unix)]
#[test]
fn distinct_collection_shape_runs_real_current_and_alternative_exports() {
    let dir = tempdir();
    let root = dir.path();
    let path = root.join("tool");
    let original = other_shape::capture(other_shape::organizer());
    let mut changed = other_shape::organizer();
    changed["actions"][2]["steps"][0]["items"] = other_shape::query("person");
    let candidate = other_shape::capture(changed);
    let store = ProductStore::create(&path, &original, 20000).unwrap();
    store
        .apply(
            0,
            "add-first",
            &other_shape::add("Ada"),
            RuntimeLimits::default(),
        )
        .unwrap();
    let hooks = TestHooks::default();
    let mut studio = ProductStudio::testing(root.into(), Some(transport(root, &candidate)), hooks);
    settle(&mut studio);
    change(&mut studio, &path);
    studio.test_trial(other_shape::invoke("export_people", Default::default()));
    settle(&mut studio);
    assert_eq!(
        studio
            .test_current_trial()
            .unwrap()
            .artifacts
            .last()
            .unwrap()
            .rows
            .len(),
        0
    );
    assert_eq!(
        studio
            .test_alternative()
            .unwrap()
            .artifacts
            .last()
            .unwrap()
            .rows
            .len(),
        1
    );
    studio.test_decide(DecisionOutcome::Accept {
        artifact: candidate.artifact.program_digest,
    });
    settle(&mut studio);
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    studio.test_daily(other_shape::invoke("export_people", Default::default()));
    settle(&mut studio);
    assert_eq!(
        store.load().unwrap().artifacts.last().unwrap().rows.len(),
        1
    );
}

#[test]
fn legacy_provider_wire_identity_stays_stable_without_real_example_origins() {
    let request = DevelopmentRequest {
        version: 1,
        id: "legacy-prepared".into(),
        project_id: "legacy-tool".into(),
        operation: DevelopmentOperation::Generate,
        request: "Keep the original need".into(),
        sources: vec![],
        context: DevelopmentContext {
            view: None,
            selected: vec![],
            recent_inputs: vec![],
            data_digest: None,
            session_digest: None,
        },
        examples: vec![],
        accepted_scenes: vec![],
        decisions: DecisionGraph {
            version: 1,
            revision: 0,
            decisions: vec![],
        },
        unknowns: vec![],
        required_capabilities: Default::default(),
    };
    let wire =
        product_discovery::encode_request(&request, &product_discovery::ProviderOptions::default())
            .unwrap();
    assert_eq!(wire.data_categories,vec!["Selected executable sources and their provenance","User request, selected context and active scoped decisions/unknowns","Explicitly selected synthetic or sanitized scenario examples and accepted observations"]);
}

#[cfg(unix)]
#[test]
fn managed_modify_inherits_exact_intent_and_benign_edit_adds_no_question() {
    let dir = tempdir();
    let root = dir.path();
    let path = root.join("tool");
    let original = other_shape::capture(other_shape::organizer());
    let mut changed = other_shape::organizer();
    changed["actions"][2]["steps"][0]["items"] = other_shape::query("person");
    let candidate = other_shape::capture(changed.clone());
    let store = ProductStore::create(&path, &original, 20000).unwrap();
    store
        .apply(
            0,
            "add-first",
            &other_shape::add("Ada"),
            RuntimeLimits::default(),
        )
        .unwrap();
    let mut studio = ProductStudio::testing(
        root.into(),
        Some(transport(root, &candidate)),
        TestHooks::default(),
    );
    settle(&mut studio);
    change(&mut studio, &path);
    studio.test_trial(other_shape::invoke("export_people", Default::default()));
    settle(&mut studio);
    studio.test_decide(DecisionOutcome::Accept {
        artifact: candidate.artifact.program_digest,
    });
    settle(&mut studio);
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    let first = store.load().unwrap();
    assert_eq!(first.decisions.decisions.len(), 1);
    changed["label"] = serde_json::json!("My club organizer");
    {
        let _guard = product_provider::fixture_executable_write_guard();
        fs::write(
            root.join("change-fixture.json"),
            serde_json::to_vec(&serde_json::json!({"program":changed,"mode":"good"})).unwrap(),
        )
        .unwrap();
    }
    studio.test_modify("Use my own title");
    settle(&mut studio);
    let request = studio.test_prepared_request().unwrap();
    assert_eq!(request.operation, DevelopmentOperation::Modify);
    assert_eq!(request.sources.last(), Some(first.program().unwrap()));
    assert!(!request.accepted_scenes.is_empty());
    assert!(request
        .examples
        .iter()
        .all(|e| e.disclosure == Disclosure::ExplicitlySelected));
    let editable = first.editable_scope_context().unwrap().unwrap();
    let line = request
        .request
        .lines()
        .find(|s| s.starts_with("Host-verified editing guide:"))
        .unwrap();
    let guide: serde_json::Value = serde_json::from_str(&line[line.find('{').unwrap()..]).unwrap();
    let supplied = request
        .sources
        .iter()
        .find(|source| {
            serde_json::to_value(canonical_digest(IdentityDomain::Source, *source).unwrap())
                .unwrap()
                == guide["editable_source"]
        })
        .unwrap();
    assert_eq!(supplied.program, editable.editable.program);
    assert_eq!(supplied.source_bytes, editable.editable.source_bytes);
    assert_eq!(supplied, first.program().unwrap());
    assert_eq!(request.decisions, first.decisions);
    studio.test_consent();
    settle(&mut studio);
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    assert!(
        studio.test_can_accept(),
        "benign wording update should not need another business answer: {}",
        studio.test_notice()
    );
    studio.test_decide(DecisionOutcome::Accept {
        artifact: first.program().unwrap().artifact.program_digest.clone(),
    });
    settle(&mut studio);
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    let next = store.load().unwrap();
    assert_eq!(next.decisions, first.decisions);
    assert_eq!(next.data, first.data);
    assert_eq!(next.program().unwrap().program.label, "My club organizer");
}

#[cfg(unix)]
#[test]
fn revisited_equivalent_pending_choice_requires_fresh_rehearsal() {
    for (expose_collect, choice) in [
        (false, "studio.return"),
        (true, "studio.accept"),
        (true, "studio.keep-current"),
    ] {
        let dir = tempdir();
        let root = dir.path();
        let path = root.join("tool");
        let mut initial = other_shape::organizer();
        if expose_collect {
            let mut collect_button = initial["views"][0]["actions"][0].clone();
            collect_button["id"] = serde_json::json!("collect_button");
            collect_button["label"] = serde_json::json!("Collect current results");
            collect_button["action"] = serde_json::json!("collect");
            initial["views"][0]["actions"]
                .as_array_mut()
                .unwrap()
                .push(collect_button);
        }
        let original = other_shape::capture(initial.clone());
        let mut changed = initial;
        changed["actions"][2]["steps"][0]["items"] = other_shape::query("person");
        let candidate = other_shape::capture(changed);
        let store = ProductStore::create(&path, &original, 20000).unwrap();
        store
            .apply(
                0,
                "first",
                &other_shape::add("Ada"),
                RuntimeLimits::default(),
            )
            .unwrap();
        let mut studio = ProductStudio::testing(
            root.into(),
            Some(transport(root, &candidate)),
            TestHooks::default(),
        );
        settle(&mut studio);
        change(&mut studio, &path);
        studio.test_trial(other_shape::invoke("export_people", Default::default()));
        settle(&mut studio);
        studio.test_decide(DecisionOutcome::Deferred);
        settle(&mut studio);
        let pending = store.load().unwrap().decisions.decisions[0].id.clone();

        // A separate, genuinely requested change can settle the same behavior while
        // this older choice remains pending. It does not resolve that choice for us.
        change(&mut studio, &path);
        studio.test_trial(other_shape::invoke("export_people", Default::default()));
        settle(&mut studio);
        studio.test_decide(DecisionOutcome::Accept {
            artifact: candidate.artifact.program_digest.clone(),
        });
        settle(&mut studio);
        assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
        studio.test_daily(other_shape::add("Later real work"));
        settle(&mut studio);
        let before = store.load().unwrap();
        assert_eq!(
            before.decisions.decisions[0].status,
            DecisionStatus::Pending
        );
        studio.test_resume_choice(&pending);
        settle(&mut studio);
        assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
        assert!(
            !studio.test_can_accept(),
            "Resolving an older choice still needs a fresh experience"
        );
        let mut harness = egui_harness::EguiHarness::new(egui::vec2(1400.0, 1800.0));
        let render = |h: &mut egui_harness::EguiHarness, s: &mut ProductStudio| {
            h.frame(|ctx| {
                egui::CentralPanel::default()
                    .show(ctx, |ui| s.show(ui))
                    .inner
            })
        };
        let fresh = render(&mut harness, &mut studio);
        assert!(!fresh.controls["studio.accept"].enabled);
        if !expose_collect {
            assert!(
                fresh.text.iter().any(|text| text
                    .contains("cannot be tried through controls on both sides")
                    && text.contains("Collect current results")),
                "The missing business task must be explained: {:?}",
                fresh.text
            );
        }
        studio.test_decide(DecisionOutcome::Accept {
            artifact: candidate.artifact.program_digest,
        });
        settle(&mut studio);
        assert_eq!(studio.test_page(), "change");
        assert!(studio
            .test_notice()
            .contains("Try the actual copied alternatives"));
        assert_eq!(store.load().unwrap(), before);

        studio.test_trial(other_shape::invoke("export_people", Default::default()));
        settle(&mut studio);
        if expose_collect {
            // This is now a whole-design managed resolution. Its declared scope also
            // includes the reachable intake and collect actions, not only export.
            assert!(!studio.test_can_accept());
            studio.test_trial(other_shape::add("Only in the copied rehearsal"));
            settle(&mut studio);
            studio.test_trial(other_shape::invoke("collect", Default::default()));
            settle(&mut studio);
            studio.test_trial(other_shape::invoke("export_people", Default::default()));
            settle(&mut studio);
        }
        let experienced = render(&mut harness, &mut studio);
        assert_eq!(
            experienced.controls["studio.accept"].enabled,
            expose_collect
        );
        assert_eq!(
            experienced.controls["studio.keep-current"].enabled,
            expose_collect
        );
        for key in [
            "studio.either",
            "studio.both",
            "studio.neither",
            "studio.defer",
        ] {
            assert!(
                !experienced.controls[key].enabled,
                "An identical artifact is not a new pair: {key}"
            );
        }
        assert!(experienced
            .text
            .iter()
            .any(|text| text.contains("Both sides now use the same tool version")));
        for outcome in [
            DecisionOutcome::EitherAcceptable,
            DecisionOutcome::BothNeeded,
            DecisionOutcome::NeitherFits,
            DecisionOutcome::Deferred,
        ] {
            studio.test_decide(outcome);
            settle(&mut studio);
            assert_eq!(studio.test_page(), "change");
            assert_eq!(store.load().unwrap(), before);
        }
        // Re-render after the refused direct submissions, then use a real control.
        let experienced = render(&mut harness, &mut studio);
        let point = experienced.controls[choice].rect.center();
        harness.press_at(point);
        render(&mut harness, &mut studio);
        harness.release_at(point);
        render(&mut harness, &mut studio);
        settle(&mut studio);
        assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
        let resolved = store.load().unwrap();
        assert_eq!(resolved.data, before.data);
        assert_eq!(resolved.data.records.len(), 2);
        if expose_collect {
            assert!(matches!(
                resolved.decisions.decisions[0].status,
                DecisionStatus::Superseded { .. }
            ));
            let recorded = &resolved.decisions.decisions.last().unwrap().outcome;
            if choice == "studio.accept" {
                assert!(matches!(recorded, DecisionOutcome::Accept { .. }));
            } else {
                assert_eq!(recorded, &DecisionOutcome::KeepCurrent);
                assert_eq!(resolved.active_revision, before.active_revision);
            }
        } else {
            assert_eq!(
                resolved, before,
                "Leaving this unsupported comparison must preserve the original pending choice"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn ordinary_structural_design_uses_checked_whole_design_and_preserves_data() {
    let dir = tempdir();
    let root = dir.path();
    let path = root.join("tool");
    let original = other_shape::capture(other_shape::organizer());
    let mut changed = other_shape::organizer();
    changed["views"][0]["kind"]["columns"].as_array_mut().unwrap().push(serde_json::json!({"id":"area","label":"Area","value":other_shape::field(other_shape::var("row"),"area")}));
    let candidate = other_shape::capture(changed);
    let store = ProductStore::create(&path, &original, 20000).unwrap();
    store
        .apply(
            0,
            "add-first",
            &other_shape::add("Ada"),
            RuntimeLimits::default(),
        )
        .unwrap();
    let before = store.load().unwrap();
    let mut studio = ProductStudio::testing(
        root.into(),
        Some(transport(root, &candidate)),
        TestHooks::default(),
    );
    settle(&mut studio);
    change(&mut studio, &path);
    studio.test_trial(other_shape::invoke("export_people", Default::default()));
    settle(&mut studio);
    studio.test_decide(DecisionOutcome::Accept {
        artifact: candidate.artifact.program_digest,
    });
    settle(&mut studio);
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    let saved = store.load().unwrap();
    assert_eq!(saved.data, before.data);
    assert!(saved.scope.layers.is_empty());
    assert_eq!(
        saved.program().unwrap().program.views[0].kind,
        candidate.program.views[0].kind
    );
}

#[cfg(unix)]
#[test]
fn choice_commit_gate_close_and_lost_ack_record_once_and_reopen_safely() {
    for after in [false, true] {
        let dir = tempdir();
        let root = dir.path();
        let path = root.join("tool");
        let original = other_shape::capture(other_shape::organizer());
        let mut changed = other_shape::organizer();
        changed["actions"][2]["steps"][0]["items"] = other_shape::query("person");
        let candidate = other_shape::capture(changed);
        let store = ProductStore::create(&path, &original, 20000).unwrap();
        store
            .apply(
                0,
                "add-first",
                &other_shape::add("Ada"),
                RuntimeLimits::default(),
            )
            .unwrap();
        let before = store.load().unwrap();
        let pause = std::sync::Arc::new(product_studio::TestPause::default());
        let mut hooks = TestHooks {
            duplicate_completion: true,
            ..TestHooks::default()
        };
        if after {
            hooks.after_commit = Some(pause.clone());
            hooks
                .lose_ack
                .store(true, std::sync::atomic::Ordering::Release);
        } else {
            hooks.before_commit = Some(pause.clone());
        }
        let stopped = hooks.stopped.clone();
        let mut studio =
            ProductStudio::testing(root.into(), Some(transport(root, &candidate)), hooks);
        settle(&mut studio);
        change(&mut studio, &path);
        studio.test_trial(other_shape::invoke("export_people", Default::default()));
        settle(&mut studio);
        studio.test_decide(DecisionOutcome::Deferred);
        let start = Instant::now();
        while !pause.reached.load(std::sync::atomic::Ordering::Acquire) {
            studio.poll();
            assert!(
                start.elapsed() < Duration::from_secs(120),
                "{}",
                studio.test_notice()
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(studio.test_cancel(), !after);
        studio.test_close();
        pause
            .release
            .store(true, std::sync::atomic::Ordering::Release);
        settle(&mut studio);
        assert_eq!(studio.test_page(), "home");
        let saved = store.load().unwrap();
        assert_eq!(saved.data, before.data);
        assert_eq!(saved.active_revision, before.active_revision);
        assert_eq!(saved.decisions.decisions.len(), usize::from(after));
        drop(studio);
        let start = Instant::now();
        while !stopped.load(std::sync::atomic::Ordering::Acquire) {
            assert!(start.elapsed() < Duration::from_secs(30));
            std::thread::sleep(Duration::from_millis(2));
        }
        let mut restarted = ProductStudio::testing(root.into(), None, TestHooks::default());
        settle(&mut restarted);
        assert_eq!(restarted.test_page(), "daily");
        assert_eq!(store.load().unwrap(), saved);
    }
}

#[test]
fn wrong_project_and_altered_retained_lifecycle_do_not_create_scoped_authority() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let current = store.load().unwrap();
    let foreign = CapturedProgram::capture(
        &program(true).source_bytes,
        "other-project",
        Producer::Fixture {
            name: "foreign".into(),
        },
        None,
    )
    .unwrap();
    assert!(product_studio::test_scope_request(
        &current,
        &foreign,
        ScopePopulation::All,
        vec![lifecycle(&current)]
    )
    .is_err());
    let prepared = store
        .prepare_scoped_change(
            &program(true),
            &request(&current, ScopePopulation::All),
            "original-layer",
        )
        .unwrap();
    store.adopt_scoped(current.revision, &prepared).unwrap();
    let managed = store.load().unwrap();
    let mut redefined = lifecycle(&managed);
    redefined.completed = boolean(false);
    assert!(product_studio::test_scope_request(
        &managed,
        &program(false),
        ScopePopulation::All,
        vec![redefined]
    )
    .unwrap_err()
    .contains("meaning"));
    assert_eq!(store.load().unwrap(), managed);
}

#[cfg(unix)]
#[test]
fn projection_only_change_learns_scope_from_actual_copied_task() {
    let dir = tempdir();
    let root = dir.path();
    let path = root.join("tool");
    let original = other_shape::capture(other_shape::organizer());
    let mut changed = other_shape::organizer();
    changed["observables"][0]["value"] =
        serde_json::json!({"kind":"count","items":other_shape::query("person")});
    let candidate = other_shape::capture(changed);
    let store = ProductStore::create(&path, &original, 20000).unwrap();
    store
        .apply(
            0,
            "add-first",
            &other_shape::add("Ada"),
            RuntimeLimits::default(),
        )
        .unwrap();
    let mut studio = ProductStudio::testing(
        root.into(),
        Some(transport(root, &candidate)),
        TestHooks::default(),
    );
    settle(&mut studio);
    change(&mut studio, &path);
    studio.test_trial(other_shape::invoke("export_people", Default::default()));
    settle(&mut studio);
    assert!(studio.test_can_accept(), "{}", studio.test_notice());
    assert!(studio.test_alternative().is_some());
    studio.test_decide(DecisionOutcome::Accept {
        artifact: candidate.artifact.program_digest,
    });
    settle(&mut studio);
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    assert_eq!(store.load().unwrap().scope.layers.len(), 1);
}

#[cfg(unix)]
#[test]
fn ordinary_structural_pending_choice_reopens_and_resolves_exactly() {
    let dir = tempdir();
    let root = dir.path();
    let path = root.join("tool");
    let original = other_shape::capture(other_shape::organizer());
    let mut changed = other_shape::organizer();
    changed["views"][0]["kind"]["columns"].as_array_mut().unwrap().push(serde_json::json!({"id":"area","label":"Area","value":other_shape::field(other_shape::var("row"),"area")}));
    let candidate = other_shape::capture(changed);
    let store = ProductStore::create(&path, &original, 20000).unwrap();
    store
        .apply(
            0,
            "add-first",
            &other_shape::add("Ada"),
            RuntimeLimits::default(),
        )
        .unwrap();
    let mut studio = ProductStudio::testing(
        root.into(),
        Some(transport(root, &candidate)),
        TestHooks::default(),
    );
    settle(&mut studio);
    change(&mut studio, &path);
    studio.test_trial(other_shape::invoke("export_people", Default::default()));
    settle(&mut studio);
    studio.test_decide(DecisionOutcome::BothNeeded);
    settle(&mut studio);
    let pending = store.load().unwrap().decisions.decisions[0].id.clone();
    assert!(store.load().unwrap().scope.rehearsals.is_empty());
    studio.test_daily(other_shape::add("Later work"));
    settle(&mut studio);
    studio.test_resume_choice(&pending);
    settle(&mut studio);
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    studio.test_trial(other_shape::invoke("export_people", Default::default()));
    settle(&mut studio);
    studio.test_decide(DecisionOutcome::Accept {
        artifact: candidate.artifact.program_digest,
    });
    settle(&mut studio);
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    let saved = store.load().unwrap();
    assert_eq!(saved.data.records.len(), 2);
    assert!(matches!(
        saved.decisions.decisions[0].status,
        DecisionStatus::Superseded { .. }
    ));
}

#[cfg(unix)]
#[test]
fn keep_managed_current_replays_committed_seed_and_preserves_the_experienced_result() {
    let dir = tempdir();
    let root = dir.path();
    let path = root.join("tool");
    let store = ProductStore::create(&path, &program(false), 20000).unwrap();
    let row = add(&store, "item", "Existing work");
    let before = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(
            &program(true),
            &request(&before, ScopePopulation::All),
            "first-layer",
        )
        .unwrap();
    store.adopt_scoped(before.revision, &prepared).unwrap();
    let current = store.load().unwrap();
    let mut studio = ProductStudio::testing(
        root.into(),
        Some(transport(root, &program(false))),
        TestHooks::default(),
    );
    settle(&mut studio);
    change(&mut studio, &path);
    for task in ["calculate", "complete", "export"] {
        studio.test_trial(if task == "export" {
            invoke(task, &[])
        } else {
            invoke(task, &[("row", reference(&row))])
        });
        settle(&mut studio);
    }
    studio.test_decide(DecisionOutcome::KeepCurrent);
    settle(&mut studio);
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    let saved = store.load().unwrap();
    assert_eq!(saved.active_revision, current.active_revision);
    assert_eq!(saved.data, current.data);
    assert_eq!(saved.scope.layers.len(), current.scope.layers.len());
    assert_eq!(
        saved.decisions.decisions[0].outcome,
        DecisionOutcome::KeepCurrent
    );
}

#[cfg(unix)]
#[test]
fn inherited_private_trial_inputs_are_in_the_exact_prepared_disclosure() {
    let dir = tempdir();
    let root = dir.path();
    let path = root.join("tool");
    let original = other_shape::capture(other_shape::organizer());
    let mut changed = other_shape::organizer();
    changed["actions"][2]["steps"][0]["items"] = other_shape::query("person");
    let candidate = other_shape::capture(changed);
    let store = ProductStore::create(&path, &original, 20000).unwrap();
    let mut studio = ProductStudio::testing(
        root.into(),
        Some(transport(root, &candidate)),
        TestHooks::default(),
    );
    settle(&mut studio);
    change(&mut studio, &path);
    studio.test_trial(other_shape::add("Private copied name from an input"));
    settle(&mut studio);
    studio.test_trial(other_shape::invoke("export_people", Default::default()));
    settle(&mut studio);
    studio.test_decide(DecisionOutcome::Deferred);
    settle(&mut studio);
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    studio.test_modify("Review the retained actual examples");
    settle(&mut studio);
    assert_eq!(studio.test_page(), "consent", "{}", studio.test_notice());
    let request = studio.test_prepared_request().unwrap();
    assert!(request
        .examples
        .iter()
        .all(|e| e.scenario.seed.records.is_empty()));
    let payload = studio.test_disclosure_payload().unwrap();
    assert!(payload.contains("Private copied name from an input"));
    let wire: serde_json::Value = serde_json::from_str(payload).unwrap();
    assert_eq!(wire["request"], serde_json::to_value(request).unwrap());
    assert!(store.load().unwrap().data.records.is_empty());
}

#[cfg(unix)]
#[test]
fn host_refuses_acceptance_for_a_different_artifact() {
    let dir = tempdir();
    let root = dir.path();
    let path = root.join("tool");
    let original = other_shape::capture(other_shape::organizer());
    let mut changed = other_shape::organizer();
    changed["actions"][2]["steps"][0]["items"] = other_shape::query("person");
    let candidate = other_shape::capture(changed);
    let store = ProductStore::create(&path, &original, 20000).unwrap();
    let mut studio = ProductStudio::testing(
        root.into(),
        Some(transport(root, &candidate)),
        TestHooks::default(),
    );
    settle(&mut studio);
    change(&mut studio, &path);
    studio.test_trial(other_shape::invoke("export_people", Default::default()));
    settle(&mut studio);
    let before = store.load().unwrap();
    studio.test_decide_exact(DecisionOutcome::Accept {
        artifact: original.artifact.program_digest,
    });
    settle(&mut studio);
    assert!(studio.test_notice().contains("artifact"));
    assert_eq!(store.load().unwrap(), before);
}

#[test]
fn restart_verifies_exact_change_receipt_and_refuses_a_conflicting_plan() {
    for forged in [false, true] {
        let dir = tempdir();
        let root = dir.path();
        let path = root.join("tool");
        let store = ProductStore::create(&path, &program(false), 20000).unwrap();
        let before = store.load().unwrap();
        let prepared = store
            .prepare_scoped_change(
                &program(true),
                &request(&before, ScopePopulation::All),
                "saved-change",
            )
            .unwrap();
        let mut plan = store.scoped_plan(&prepared).unwrap().plan().clone();
        store.adopt_scoped(before.revision, &prepared).unwrap();
        let saved = store.load().unwrap();
        if forged {
            plan.required_decisions.push("another-promise".into());
        }
        product_studio::test_stage_change(root, &path, &before, plan);
        let mut studio = ProductStudio::testing(root.into(), None, TestHooks::default());
        settle(&mut studio);
        assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
        assert_eq!(store.load().unwrap(), saved);
        studio.test_reconcile();
        settle(&mut studio);
        if forged {
            assert!(studio.test_notice().contains("different saved plan"));
            studio.test_daily(invoke(
                "add",
                &[
                    ("name", text("Blocked duplicate")),
                    ("promised", DataValue::Date { days: 20010 }),
                ],
            ));
            settle(&mut studio);
            assert_eq!(store.load().unwrap(), saved);
        } else {
            assert_eq!(store.load().unwrap().scope.adoptions.len(), 1);
        }
    }
}

#[cfg(unix)]
#[test]
fn new_scope_offers_only_affected_entities_after_an_earlier_managed_layer() {
    fn dual(a: bool, b: bool) -> CapturedProgram {
        let mut app = serde_json::to_value(&program(a).program).unwrap();
        let second = serde_json::to_value(&program(b).program).unwrap();
        fn substitute(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::String(s) => {
                    if s == "job" {
                        *s = "supply".into()
                    } else if s == "sheet" {
                        *s = "supply_sheet".into()
                    } else if s == "work" {
                        *s = "supplies".into()
                    }
                }
                serde_json::Value::Array(a) => {
                    for v in a {
                        substitute(v)
                    }
                }
                serde_json::Value::Object(o) => {
                    for v in o.values_mut() {
                        substitute(v)
                    }
                }
                _ => (),
            }
        }
        let mut entity = second["entities"][0].clone();
        substitute(&mut entity);
        app["entities"].as_array_mut().unwrap().push(entity);
        for action in second["actions"].as_array().unwrap() {
            let mut action = action.clone();
            substitute(&mut action);
            action["id"] = serde_json::json!(format!("supply_{}", action["id"].as_str().unwrap()));
            app["actions"].as_array_mut().unwrap().push(action);
        }
        let mut output = second["outputs"][0].clone();
        substitute(&mut output);
        app["outputs"].as_array_mut().unwrap().push(output);
        let mut view = second["views"][0].clone();
        substitute(&mut view);
        for binding in view["actions"].as_array_mut().unwrap() {
            binding["action"] =
                serde_json::json!(format!("supply_{}", binding["action"].as_str().unwrap()));
        }
        app["views"].as_array_mut().unwrap().push(view);
        capture(app)
    }
    let dir = tempdir();
    let root = dir.path();
    let path = root.join("tool");
    let store = ProductStore::create(&path, &dual(false, false), 20000).unwrap();
    let old = add(&store, "old-work", "Earlier collection");
    apply(
        &store,
        "new-supply",
        invoke(
            "supply_add",
            &[
                ("name", text("New collection")),
                ("promised", DataValue::Date { days: 20020 }),
            ],
        ),
    );
    let supply = store
        .load()
        .unwrap()
        .data
        .records
        .iter()
        .find(|r| r.entity == "supply")
        .unwrap()
        .clone();
    let current = store.load().unwrap();
    let req = product_studio::test_scope_request(
        &current,
        &dual(true, false),
        ScopePopulation::All,
        vec![lifecycle(&current)],
    )
    .unwrap();
    let first = store
        .prepare_scoped_change(&dual(true, false), &req, "first")
        .unwrap();
    store.adopt_scoped(current.revision, &first).unwrap();
    let mut studio = ProductStudio::testing(
        root.into(),
        Some(transport(root, &dual(true, true))),
        TestHooks::default(),
    );
    settle(&mut studio);
    change(&mut studio, &path);
    // A copied completion demonstrates the new entity's actual finished state.
    let mut h = egui_harness::EguiHarness::new(egui::vec2(1600.0, 2400.0));
    click(&mut h, &mut studio, "current.navigate.supplies");
    click(
        &mut h,
        &mut studio,
        &format!("current.row.supply.{}.complete", supply.id),
    );
    click(
        &mut h,
        &mut studio,
        &format!("studio.finished.supply.{}.done", supply.id),
    );
    let eligible = studio.test_scope_records();
    assert!(!eligible.iter().any(|r| r.record == old.id));
    assert_eq!(
        eligible,
        vec![RecordRef {
            entity: supply.entity,
            record: supply.id
        }]
    );
}

#[test]
fn managed_editing_guide_names_the_actual_supplied_capture_and_measures_envelopes() {
    for row_layer in [false, true] {
        let dir = tempdir();
        let path = dir.path().join("tool");
        let (before, candidate) = if row_layer {
            (fixture::program(false), fixture::program(true))
        } else {
            let original = other_shape::capture(other_shape::organizer());
            let mut changed = other_shape::organizer();
            changed["actions"][2]["steps"][0]["items"] = other_shape::query("person");
            (original, other_shape::capture(changed))
        };
        let store = ProductStore::create(&path, &before, 20000).unwrap();
        let basis = store.load().unwrap();
        let req = product_studio::test_scope_request(
            &basis,
            &candidate,
            ScopePopulation::All,
            if row_layer {
                vec![lifecycle(&basis)]
            } else {
                vec![]
            },
        )
        .unwrap();
        let prepared = store
            .prepare_scoped_change(&candidate, &req, "layer")
            .unwrap();
        store.adopt_scoped(basis.revision, &prepared).unwrap();
        let current = store.load().unwrap();
        let editable = current.editable_scope_context().unwrap().unwrap();
        let request = product_decisions::DecisionEngine::new(
            product_runtime::LocalRuntime::default(),
            product_decisions::IntentArchive::new(store.clone()),
        )
        .development_request(
            &current,
            "measure",
            DevelopmentOperation::Modify,
            "Change the title",
            DevelopmentContext {
                view: Some(current.session.view.clone()),
                selected: vec![],
                recent_inputs: vec![],
                data_digest: Some(current.data.identity().unwrap()),
                session_digest: Some(current.session.identity().unwrap()),
            },
        )
        .unwrap();
        let line = request
            .request
            .lines()
            .find(|s| s.starts_with("Host-verified editing guide:"))
            .unwrap();
        let guide: serde_json::Value =
            serde_json::from_str(&line[line.find('{').unwrap()..]).unwrap();
        let actual = request.sources.iter().find(|s| {
            serde_json::to_value(canonical_digest(IdentityDomain::Source, *s).unwrap()).unwrap()
                == guide["editable_source"]
        });
        assert!(
            actual.is_some(),
            "editable guide points outside actual sources"
        );
        let actual = actual.unwrap();
        assert_eq!(actual.program, editable.editable.program);
        assert_eq!(actual.source_bytes, editable.editable.source_bytes);
        assert_eq!(request.sources.last(), Some(current.program().unwrap()));
        if row_layer {
            assert_eq!(actual, &editable.editable);
        } else {
            assert_eq!(actual, current.program().unwrap());
        }
        eprintln!("MEASURE row_layer={row_layer} request_bytes={} current_capture_bytes={} editable_capture_bytes={} current_source_bytes={} source_count={} schema_bytes={}",canonical_bytes(&request).unwrap().len(),canonical_bytes(current.program().unwrap()).unwrap().len(),canonical_bytes(&editable.editable).unwrap().len(),current.program().unwrap().source_bytes.len(),request.sources.len(),APP_SCHEMA.len());
    }
}

#[cfg(unix)]
fn bounded_transport(
    root: &Path,
    kind: product_provider::ProviderKind,
) -> product_provider::ProviderTransport {
    use std::os::unix::fs::PermissionsExt;
    let _guard = product_provider::fixture_executable_write_guard();
    let executable = root.join("bounded-fixture.py");
    fs::write(
        &executable,
        include_str!("fixtures/provider_transport/fake_cli.py").replace(
            "{'passed': True, 'text': wire['prompt'], 'command': 'untrusted-do-not-execute'}",
            "{'received': len(wire['prompt'])}",
        ),
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(
        executable.with_extension("json"),
        serde_json::to_vec(&serde_json::json!({"provider":kind,"mode":"good"})).unwrap(),
    )
    .unwrap();
    let home = root.join("fixture-home");
    fs::create_dir(&home).unwrap();
    product_provider::ProviderTransport::new_fixture(root.join("jobs"), kind, executable, home)
        .unwrap()
}
#[cfg(unix)]
fn bounded_request(
    id: &str,
    kind: product_provider::ProviderKind,
    prompt: Vec<u8>,
) -> product_provider::ProviderRequest {
    product_provider::ProviderRequest {
        request_id: id.into(),
        provider: kind,
        source_digest: product_provider::digest(b"synthetic bounded input"),
        prompt,
        schema: br#"{"type":"object","additionalProperties":true}"#.to_vec(),
        purpose: "Synthetic request-boundary regression".into(),
        data_categories: vec!["Synthetic bytes only".into()],
        profile: product_provider::CapabilityProfile::DataOnly,
        limits: product_provider::JobLimits::default(),
    }
}
#[cfg(unix)]
fn bounded_consent(prepared: &product_provider::PreparedJob) -> product_provider::ConsentReceipt {
    product_provider::ConsentReceipt {
        disclosure_digest: prepared.disclosure.digest(),
        approval_reference: "synthetic-fixture-only".into(),
        expires_at_unix_ms: product_provider::unix_ms() + 120_000,
    }
}

#[cfg(unix)]
#[test]
fn provider_request_bound_roundtrips_near_limit_and_legacy_bytes_without_digest_changes() {
    use product_provider::*;
    for kind in [ProviderKind::Codex, ProviderKind::Claude] {
        let dir = tempdir();
        let root = dir.path();
        let transport = bounded_transport(root, kind);
        for (name, prompt) in [
            ("legacy", b"Exact old UTF-8 request".to_vec()),
            ("near-limit", vec![0; 1024 * 1024]),
            ("serialized-limit", vec![127; 1024 * 1024]),
        ] {
            let mut request = bounded_request(name, kind, prompt);
            if name != "legacy" {
                request.schema.resize(MAX_SCHEMA_BYTES, b' ');
                request.purpose = "\0".repeat(1024);
                request.data_categories = vec!["\0".repeat(256); 32];
            }
            assert!(request.validate().is_ok());
            let expected = request.digest().unwrap();
            let prepared = transport.prepare(request.clone()).unwrap();
            let job_dir = root.join("jobs").join(name);
            let stored = fs::read(job_dir.join("request.json")).unwrap();
            assert!(stored.len() <= MAX_SERIALIZED_REQUEST_BYTES);
            let decoded: ProviderRequest = serde_json::from_slice(&stored).unwrap();
            assert_eq!(decoded, request);
            assert_eq!(decoded.digest().unwrap(), expected);
            assert_eq!(prepared.disclosure.request_digest, expected);
            assert_eq!(prepared.disclosure.prompt_digest, digest(&request.prompt));
            let stdin = fs::read(job_dir.join("stdin.json")).unwrap();
            assert!(stdin.len() <= MAX_STDIN_BYTES);
            let wire: serde_json::Value = serde_json::from_slice(&stdin).unwrap();
            assert_eq!(wire["prompt"].as_str().unwrap().as_bytes(), request.prompt);
            assert_eq!(prepared.disclosure.stdin_digest, digest(&stdin));
            if name == "near-limit" {
                assert!(stdin.len() > 6 * MAX_PROMPT_BYTES);
            }
            if name == "serialized-limit" {
                assert!(stored.len() > MAX_SERIALIZED_REQUEST_BYTES - 64 * 1024);
            }
            if name != "legacy" {
                assert!(stored.len() > MAX_RESULT_BYTES);
            }
            let consent = bounded_consent(&prepared);
            let mut job = transport.submit(prepared, &consent).unwrap();
            let start = Instant::now();
            let receipt = loop {
                if let Some(r) = job.poll().unwrap() {
                    break r;
                }
                assert!(start.elapsed() < Duration::from_secs(30));
                std::thread::sleep(Duration::from_millis(5));
            };
            assert_eq!(
                receipt.state,
                JobState::TransportValidated,
                "{}",
                receipt.detail
            );
            let actual = transport
                .ingest(name, &expected, &request.source_digest)
                .unwrap()
                .unwrap();
            let result: serde_json::Value = serde_json::from_slice(&actual.final_bytes).unwrap();
            assert_eq!(result["received"], request.prompt.len());
        }
    }
}

#[cfg(unix)]
#[test]
fn provider_request_read_bounds_keep_strict_json_and_refuse_modified_or_oversized_inputs() {
    use product_provider::*;
    let dir = tempdir();
    let root = dir.path();
    let transport = bounded_transport(root, ProviderKind::Codex);
    let oversized = bounded_request(
        "over-prompt",
        ProviderKind::Codex,
        vec![b'x'; MAX_PROMPT_BYTES + 1],
    );
    assert!(transport.prepare(oversized).is_err());
    assert!(!root.join("jobs/over-prompt").exists());
    for mode in [
        "oversized-request",
        "duplicate-key",
        "deep-json",
        "trailing-json",
        "oversized-stdin",
        "changed-stdin",
    ] {
        let request = bounded_request(
            mode,
            ProviderKind::Codex,
            b"Exact small legacy input".to_vec(),
        );
        let prepared = transport.prepare(request).unwrap();
        let consent = bounded_consent(&prepared);
        let job_dir = root.join("jobs").join(mode);
        let expected = match mode {
            "oversized-request" => {
                fs::write(
                    job_dir.join("request.json"),
                    vec![b' '; MAX_SERIALIZED_REQUEST_BYTES + 1],
                )
                .unwrap();
                "byte limit"
            }
            "duplicate-key" => {
                let mut bytes = fs::read(job_dir.join("request.json")).unwrap();
                bytes.pop();
                bytes.extend_from_slice(br#", "request_id":"duplicate"}"#);
                fs::write(job_dir.join("request.json"), bytes).unwrap();
                "duplicate"
            }
            "deep-json" => {
                let mut value: serde_json::Value =
                    serde_json::from_slice(&fs::read(job_dir.join("request.json")).unwrap())
                        .unwrap();
                let mut nested = serde_json::json!("value");
                for _ in 0..66 {
                    nested = serde_json::json!([nested]);
                }
                value["data_categories"] = nested;
                fs::write(
                    job_dir.join("request.json"),
                    serde_json::to_vec(&value).unwrap(),
                )
                .unwrap();
                "nesting"
            }
            "trailing-json" => {
                let mut bytes = fs::read(job_dir.join("request.json")).unwrap();
                bytes.extend_from_slice(b" true");
                fs::write(job_dir.join("request.json"), bytes).unwrap();
                "trailing"
            }
            "oversized-stdin" => {
                fs::write(job_dir.join("stdin.json"), vec![b' '; MAX_STDIN_BYTES + 1]).unwrap();
                "byte limit"
            }
            _ => {
                let mut bytes = fs::read(job_dir.join("stdin.json")).unwrap();
                bytes.push(b' ');
                fs::write(job_dir.join("stdin.json"), bytes).unwrap();
                "changed"
            }
        };
        let error = transport
            .submit(prepared, &consent)
            .err()
            .expect("corrupt input must be refused");
        assert!(error.to_lowercase().contains(expected), "{mode}: {error}");
        assert!(!job_dir.join("fixture-invocation.json").exists());
    }
    assert!(
        tool_proposal_input::parse_json_bytes(&vec![b' '; 1024 * 1024 + 1]).is_err(),
        "the external/source parser bound stays at 1MiB"
    );
}

#[cfg(unix)]
#[test]
fn navigation_only_structural_comparison_keeps_pending_choices_unavailable() {
    let dir = tempdir();
    let root = dir.path();
    let path = root.join("tool");
    let original = other_shape::capture(other_shape::organizer());
    let mut changed = other_shape::organizer();
    changed["views"][0]["kind"]["columns"].as_array_mut().unwrap().push(serde_json::json!({"id":"area","label":"Area","value":other_shape::field(other_shape::var("row"),"area")}));
    let candidate = other_shape::capture(changed);
    let store = ProductStore::create(&path, &original, 20000).unwrap();
    store
        .apply(
            0,
            "first",
            &other_shape::add("Ada"),
            RuntimeLimits::default(),
        )
        .unwrap();
    let before = store.load().unwrap();
    let mut studio = ProductStudio::testing(
        root.into(),
        Some(transport(root, &candidate)),
        TestHooks::default(),
    );
    settle(&mut studio);
    change(&mut studio, &path);
    let mut h = egui_harness::EguiHarness::new(egui::vec2(1400.0, 1800.0));
    click(&mut h, &mut studio, "current.navigate.new_person");
    let trace = frame(&mut h, &mut studio);
    for key in [
        "studio.either",
        "studio.both",
        "studio.neither",
        "studio.defer",
    ] {
        assert!(
            !trace.controls[key].enabled,
            "Navigation alone has no business task scope: {key}"
        );
    }
    assert_eq!(store.load().unwrap(), before);
    click(&mut h, &mut studio, "current.field.name");
    h.text("Copied person");
    frame(&mut h, &mut studio);
    click(&mut h, &mut studio, "current.field.area");
    h.text("north");
    frame(&mut h, &mut studio);
    click(&mut h, &mut studio, "current.submit");
    click(&mut h, &mut studio, "studio.defer");
    let saved = store.load().unwrap();
    assert_eq!(saved.data, before.data);
    assert_eq!(saved.active_revision, before.active_revision);
    assert_eq!(
        saved.decisions.decisions.last().unwrap().outcome,
        DecisionOutcome::Deferred
    );
}

#[cfg(unix)]
#[test]
fn change_commit_rechecks_real_day_after_preparation_and_clears_only_its_pending_marker() {
    let dir = tempdir();
    let root = dir.path();
    let path = root.join("tool");
    let original = other_shape::capture(other_shape::organizer());
    let mut changed = other_shape::organizer();
    changed["actions"][2]["steps"][0]["items"] = other_shape::query("person");
    let candidate = other_shape::capture(changed);
    let store = ProductStore::create(&path, &original, 20000).unwrap();
    store
        .apply(
            0,
            "first",
            &other_shape::add("Ada"),
            RuntimeLimits::default(),
        )
        .unwrap();
    let before = store.load().unwrap();
    let pause = std::sync::Arc::new(product_studio::TestPause::default());
    let hooks = TestHooks {
        before_commit: Some(pause.clone()),
        ..TestHooks::default()
    };
    let clock = hooks.today.clone();
    let stopped = hooks.stopped.clone();
    let mut studio = ProductStudio::testing(root.into(), Some(transport(root, &candidate)), hooks);
    settle(&mut studio);
    change(&mut studio, &path);
    studio.test_trial(other_shape::invoke("export_people", Default::default()));
    settle(&mut studio);
    let original_journal: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("studio/session.json")).unwrap()).unwrap();
    let need = original_journal["need"].as_str().unwrap().to_owned();
    studio.test_decide(DecisionOutcome::Deferred);
    let start = Instant::now();
    while !pause.reached.load(std::sync::atomic::Ordering::Acquire) {
        studio.poll();
        assert!(start.elapsed() < Duration::from_secs(120));
        std::thread::sleep(Duration::from_millis(2));
    }
    clock.store(20001, std::sync::atomic::Ordering::Release);
    pause
        .release
        .store(true, std::sync::atomic::Ordering::Release);
    settle(&mut studio);
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    assert!(studio.test_notice().contains("date changed"));
    assert_eq!(store.load().unwrap(), before);
    let journal: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("studio/session.json")).unwrap()).unwrap();
    assert!(journal["pending"].is_null());
    assert_eq!(journal["need"], original_journal["need"]);
    drop(studio);
    let start = Instant::now();
    while !stopped.load(std::sync::atomic::Ordering::Acquire) {
        assert!(start.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(2));
    }
    let hooks = TestHooks::default();
    hooks
        .today
        .store(20001, std::sync::atomic::Ordering::Release);
    let mut reopened = ProductStudio::testing(root.into(), None, hooks);
    settle(&mut reopened);
    assert_eq!(reopened.test_page(), "daily", "{}", reopened.test_notice());
    assert_eq!(reopened.test_need(), need);
    let saved = store.load().unwrap();
    assert_eq!(saved.decisions, before.decisions);
    assert_eq!(saved.data.records, before.data.records);
    assert_eq!(saved.clock_day, 20001);
}

#[cfg(unix)]
#[test]
fn copied_day_control_plays_waiting_boundary_without_mutating_live_work() {
    let dir = tempdir();
    let root = dir.path();
    let path = root.join("tool");
    let store = ProductStore::create(&path, &program(false), 20000).unwrap();
    let row = add(&store, "first", "Waiting work");
    action(&store, "waiting", "wait", &row);
    let before = store.load().unwrap();
    let mut studio = ProductStudio::testing(
        root.into(),
        Some(transport(root, &program(true))),
        TestHooks::default(),
    );
    settle(&mut studio);
    change(&mut studio, &path);
    let mut h = egui_harness::EguiHarness::new(egui::vec2(1600.0, 2400.0));
    click(
        &mut h,
        &mut studio,
        &format!("current.row.job.{}.complete", row.id),
    );
    click(
        &mut h,
        &mut studio,
        &format!("studio.finished.job.{}.done", row.id),
    );
    for _ in 0..3 {
        click(&mut h, &mut studio, "studio.trial.next-day");
    }
    assert!(frame(&mut h, &mut studio)
        .text
        .iter()
        .any(|text| text.contains("Copied date: 2024-10-07")));
    click(
        &mut h,
        &mut studio,
        &format!("current.row.job.{}.calculate", row.id),
    );
    click(&mut h, &mut studio, "current.action.export");
    let current = studio.test_current_trial().unwrap();
    let alternative = studio.test_alternative().unwrap();
    let old = current
        .retained_records
        .iter()
        .find(|r| r.id == row.id)
        .unwrap();
    let new = alternative
        .retained_records
        .iter()
        .find(|r| r.id == row.id)
        .unwrap();
    assert_eq!(old.values["production"], DataValue::Integer { value: 3 });
    assert_eq!(new.values["production"], DataValue::Integer { value: 0 });
    assert_eq!(old.values["promised"], new.values["promised"]);
    assert_ne!(current.artifacts[0].bytes, alternative.artifacts[0].bytes);
    assert_eq!(
        store.load().unwrap(),
        before,
        "Copied clock, data, and session must stay isolated"
    );
    click(&mut h, &mut studio, "studio.return");
    assert_eq!(studio.test_page(), "daily");
    assert_eq!(store.load().unwrap(), before);
}

#[cfg(unix)]
fn create_copied_work(
    h: &mut egui_harness::EguiHarness,
    studio: &mut ProductStudio,
    name: &str,
) -> Record {
    click(h, studio, "current.navigate.new_work");
    click(h, studio, "current.field.name");
    h.text(name);
    frame(h, studio);
    click(h, studio, "current.submit");
    let row = studio
        .test_current_trial()
        .unwrap()
        .retained_records
        .iter()
        .find(|row| row.values.get("name") == Some(&text(name)))
        .unwrap_or_else(|| panic!("Copied creation failed: {}", studio.test_notice()))
        .clone();
    click(h, studio, "current.navigate.work");
    row
}

#[cfg(unix)]
#[test]
fn projection_reprepare_preserves_created_record_identity_and_retained_pair() {
    let dir = tempdir();
    let root = dir.path();
    let path = root.join("tool");
    let original = program(false);
    let mut changed = serde_json::to_value(&original.program).unwrap();
    changed["views"][0]["kind"]["columns"][1]["value"] = serde_json::to_value(int(99)).unwrap();
    let candidate = capture(changed);
    let store = ProductStore::create(&path, &original, 20000).unwrap();
    let done = add(&store, "existing", "Completed history");
    action(&store, "finish-existing", "complete", &done);
    let before = store.load().unwrap();
    let mut studio = ProductStudio::testing(
        root.into(),
        Some(transport(root, &candidate)),
        TestHooks::default(),
    );
    settle(&mut studio);
    change(&mut studio, &path);
    let mut h = egui_harness::EguiHarness::new(egui::vec2(1600.0, 2400.0));
    click(
        &mut h,
        &mut studio,
        &format!("studio.finished.job.{}.done", done.id),
    );
    let copied = create_copied_work(&mut h, &mut studio, "Copied new work");
    click(
        &mut h,
        &mut studio,
        &format!("current.row.job.{}.wait", copied.id),
    );
    for view in [
        studio.test_current_trial().unwrap(),
        studio.test_alternative().unwrap(),
    ] {
        let row = view
            .retained_records
            .iter()
            .find(|row| row.id == copied.id)
            .unwrap_or_else(|| panic!("Copied identity changed: {}", studio.test_notice()));
        assert_eq!(
            row.values["waiting"],
            DataValue::Boolean { value: true },
            "{}",
            studio.test_notice()
        );
    }
    assert_eq!(store.load().unwrap(), before);
    click(&mut h, &mut studio, "studio.defer");
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    let saved = store.load().unwrap();
    assert_eq!(saved.active_revision, before.active_revision);
    assert_eq!(saved.data, before.data);
    assert_eq!(saved.session, before.session);
    assert_eq!(
        saved.decisions.decisions[0].outcome,
        DecisionOutcome::Deferred
    );
    assert_eq!(saved.decisions.decisions[0].status, DecisionStatus::Pending);
}

#[cfg(unix)]
#[test]
fn future_work_keep_current_preserves_replayed_created_record_identity() {
    let dir = tempdir();
    let root = dir.path();
    let path = root.join("tool");
    let store = ProductStore::create(&path, &program(false), 20000).unwrap();
    let done = add(&store, "existing", "Completed history");
    action(&store, "finish-existing", "complete", &done);
    let before = store.load().unwrap();
    let mut studio = ProductStudio::testing(
        root.into(),
        Some(transport(root, &program(true))),
        TestHooks::default(),
    );
    settle(&mut studio);
    change(&mut studio, &path);
    let mut h = egui_harness::EguiHarness::new(egui::vec2(1600.0, 2400.0));
    click(
        &mut h,
        &mut studio,
        &format!("studio.finished.job.{}.done", done.id),
    );
    click(&mut h, &mut studio, "studio.scope.future");
    let copied = create_copied_work(&mut h, &mut studio, "Future copied work");
    click(
        &mut h,
        &mut studio,
        &format!("current.row.job.{}.wait", copied.id),
    );
    click(&mut h, &mut studio, "studio.trial.next-day");
    for task in ["calculate", "complete"] {
        click(
            &mut h,
            &mut studio,
            &format!("current.row.job.{}.{task}", copied.id),
        );
    }
    click(&mut h, &mut studio, "current.action.export");
    for (view, expected) in [
        (studio.test_current_trial().unwrap(), 1),
        (studio.test_alternative().unwrap(), 0),
    ] {
        let row = view
            .retained_records
            .iter()
            .find(|row| row.id == copied.id)
            .unwrap();
        assert_eq!(
            row.values["production"],
            DataValue::Integer { value: expected }
        );
    }
    assert_eq!(store.load().unwrap(), before);
    click(&mut h, &mut studio, "studio.keep-current");
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    let saved = store.load().unwrap();
    assert_eq!(saved.active_revision, before.active_revision);
    assert_eq!(saved.data, before.data);
    assert_eq!(saved.session, before.session);
    assert_eq!(
        saved.decisions.decisions[0].outcome,
        DecisionOutcome::KeepCurrent
    );
    assert_eq!(saved.decisions.decisions[0].status, DecisionStatus::Active);
}

fn participant_fixture() -> (CapturedProgram, DataSnapshot, Vec<Record>) {
    let dir = tempdir();
    let store = ProductStore::create(&dir.path().join("tool"), &program(false), 20000).unwrap();
    add(&store, "one", "Same visible result");
    add(&store, "two", "Same visible result");
    let data = store.load().unwrap().data;
    let rows = data.records.clone();
    let mut app = serde_json::to_value(&program(false).program).unwrap();
    let references = |values: Vec<DataValue>| {
        lit(
            DataValue::List {
                item_type: Type::reference("job"),
                items: values,
            },
            Type::list(Type::reference("job")),
        )
    };
    let first = participant_emit(references(vec![reference(&rows[0])]));
    let second = participant_emit(references(vec![reference(&rows[1])]));
    let missing = DataValue::Reference {
        entity: "job".into(),
        record: "absent-row".into(),
    };
    let cases = [
        ("pair", vec![first.clone(), second], vec![]),
        ("rejected_pair", vec![first], vec![boolean(false)]),
        (
            "untracked",
            vec![participant_emit(lit(
                DataValue::List {
                    item_type: Type::Integer,
                    items: vec![DataValue::Integer { value: 1 }],
                },
                Type::list(Type::Integer),
            ))],
            vec![],
        ),
        (
            "missing_item",
            vec![participant_emit(references(vec![missing.clone()]))],
            vec![],
        ),
        (
            "mixed_items",
            vec![participant_emit(references(vec![
                reference(&rows[0]),
                missing,
            ]))],
            vec![],
        ),
        (
            "empty_items",
            vec![participant_emit(references(vec![]))],
            vec![],
        ),
    ];
    for (id, steps, ensures) in cases {
        app["actions"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "id":id,"label":id,"parameters":{},"guards":[],"steps":steps,"ensures":ensures
            }));
    }
    (capture(app), data, rows)
}
fn participant_emit(items: Expr) -> serde_json::Value {
    serde_json::json!({"kind":"emit","output":"sheet","items":items,"binding":"item","columns":{
        "name":lit(text("Identical"),Type::Text),"production":int(0),
        "promised":lit(DataValue::Date {days:20000},Type::Date),"reminder":boolean(false)
    }})
}

#[test]
fn emission_participants_bind_actual_rows_not_duplicate_bytes_or_other_events() {
    let (program, data, rows) = participant_fixture();
    let runtime = product_runtime::LocalRuntime::default();
    let mut run = runtime
        .start(
            &program,
            &data,
            &SessionState::initial(&program.program).unwrap(),
            20000,
            0,
            RuntimeLimits::default(),
        )
        .unwrap();
    let first_step = runtime
        .apply(&mut run, &invoke("pair", &[]), "first-event")
        .unwrap();
    let outputs = runtime.emitted_artifacts(&run).unwrap();
    assert_eq!(
        outputs[0], outputs[1],
        "The raw outputs really have identical bytes and digests"
    );
    let receipts = runtime.emitted_record_participants(&run).unwrap().unwrap();
    assert_eq!(receipts.len(), 2);
    for (ordinal, row) in rows.iter().enumerate() {
        receipts[ordinal]
            .validate_for(
                &program,
                "first-event",
                ordinal,
                &outputs[ordinal],
                &first_step,
                20000,
            )
            .unwrap();
        let participants = receipts[ordinal].records().unwrap();
        assert_eq!(participants.len(), 1);
        assert_eq!(
            participants[0].record(),
            &RecordRef {
                entity: row.entity.clone(),
                record: row.id.clone()
            }
        );
        assert_eq!(participants[0].created_program(), &row.created_program);
        assert!(receipts[ordinal]
            .validate_for(
                &program,
                "second-event",
                ordinal,
                &outputs[ordinal],
                &first_step,
                20000
            )
            .is_err());
        assert!(receipts[ordinal]
            .validate_for(
                &program,
                "first-event",
                ordinal,
                &outputs[ordinal],
                &first_step,
                20001
            )
            .is_err());
        let mut other_frame = first_step.clone();
        other_frame.before_data = data.identity().unwrap();
        other_frame.before_session =
            canonical_digest(IdentityDomain::Session, &"other copied session").unwrap();
        assert!(receipts[ordinal]
            .validate_for(
                &program,
                "first-event",
                ordinal,
                &outputs[ordinal],
                &other_frame,
                20000
            )
            .is_err());
        assert!(receipts[ordinal]
            .validate_for(
                &program,
                "first-event",
                1 - ordinal,
                &outputs[ordinal],
                &first_step,
                20000
            )
            .is_err());
    }
    let before = receipts.to_vec();
    runtime
        .apply(&mut run, &invoke("pair", &[]), "first-event")
        .unwrap();
    assert_eq!(
        runtime.emitted_record_participants(&run).unwrap().unwrap(),
        before
    );
    assert_eq!(runtime.emitted_artifacts(&run).unwrap().len(), 2);
    let second_step = runtime
        .apply(&mut run, &invoke("pair", &[]), "second-event")
        .unwrap();
    let receipts = runtime.emitted_record_participants(&run).unwrap().unwrap();
    assert_eq!(receipts.len(), 4);
    receipts[2]
        .validate_for(
            &program,
            "second-event",
            2,
            &runtime.emitted_artifacts(&run).unwrap()[2],
            &second_step,
            20000,
        )
        .unwrap();
    assert!(receipts[0]
        .validate_for(
            &program,
            "second-event",
            2,
            &runtime.emitted_artifacts(&run).unwrap()[2],
            &second_step,
            20000
        )
        .is_err());
}

#[test]
fn emission_participants_preserve_rollback_cancellation_and_unknown_collections() {
    let (program, data, _) = participant_fixture();
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let runtime = product_runtime::LocalRuntime::with_cancellation(cancelled.clone());
    let mut run = runtime
        .start(
            &program,
            &data,
            &SessionState::initial(&program.program).unwrap(),
            20000,
            0,
            RuntimeLimits::default(),
        )
        .unwrap();
    runtime
        .apply(&mut run, &invoke("pair", &[]), "first-event")
        .unwrap();
    let outputs = runtime.emitted_artifacts(&run).unwrap().to_vec();
    let receipts = runtime
        .emitted_record_participants(&run)
        .unwrap()
        .unwrap()
        .to_vec();
    let saved_data = runtime.data(&run).clone();
    assert!(runtime
        .apply(&mut run, &invoke("rejected_pair", &[]), "rejected-event")
        .is_err());
    assert_eq!(runtime.emitted_artifacts(&run).unwrap(), outputs);
    assert_eq!(
        runtime.emitted_record_participants(&run).unwrap().unwrap(),
        receipts
    );
    assert_eq!(runtime.data(&run), &saved_data);
    cancelled.store(true, std::sync::atomic::Ordering::Release);
    assert!(runtime
        .apply(&mut run, &invoke("pair", &[]), "cancelled-event")
        .is_err());
    assert_eq!(
        runtime.emitted_record_participants(&run).unwrap().unwrap(),
        receipts
    );
    cancelled.store(false, std::sync::atomic::Ordering::Release);
    for action in ["missing_item", "mixed_items"] {
        assert!(runtime
            .apply(&mut run, &invoke(action, &[]), action)
            .is_err());
        assert_eq!(runtime.emitted_artifacts(&run).unwrap(), outputs);
        assert_eq!(
            runtime.emitted_record_participants(&run).unwrap().unwrap(),
            receipts
        );
        assert_eq!(runtime.data(&run), &saved_data);
    }
    for (action, expected_rows) in [("untracked", 1), ("empty_items", 0)] {
        runtime
            .apply(&mut run, &invoke(action, &[]), action)
            .unwrap();
        assert_eq!(
            runtime
                .emitted_artifacts(&run)
                .unwrap()
                .last()
                .unwrap()
                .rows
                .len(),
            expected_rows
        );
        assert!(
            runtime
                .emitted_record_participants(&run)
                .unwrap()
                .unwrap()
                .last()
                .unwrap()
                .records()
                .is_none(),
            "{action} must not acquire inferred or partial provenance"
        );
    }
}

#[test]
fn emission_participant_budget_refuses_whole_emission_without_changing_output() {
    let (source, mut data, rows) = participant_fixture();
    let birth = data.events[0].clone();
    data.records.clear();
    data.events.clear();
    for index in 0..64 {
        let mut row = rows[0].clone();
        row.id = format!("seed-{index}");
        let mut event = birth.clone();
        event.id = format!("event-{index}");
        event.operation_id = format!("birth-{index}");
        event.sequence = index + 1;
        event.changes[0].record = row.id.clone();
        data.records.push(row);
        data.events.push(event);
    }
    data.generation = 64;
    data.validate().unwrap();
    let mut app = serde_json::to_value(&source.program).unwrap();
    let query = serde_json::from_value(serde_json::json!({"kind":"query","entity":"job","binding":"q","predicate":boolean(true),"sort":[],"limit":1000,"include_archived":true})).unwrap();
    app["actions"].as_array_mut().unwrap().push(serde_json::json!({"id":"bulk","label":"Bulk copied output","parameters":{},"guards":[],"ensures":[],"steps":[
        {"kind":"for_each","items":lit(DataValue::List { item_type:Type::Integer,items:(0..200).map(|value|DataValue::Integer{value}).collect() },Type::list(Type::Integer)),"binding":"iteration","steps":[participant_emit(query)]}
    ]}));
    let program = capture(app);
    let runtime = product_runtime::LocalRuntime::default();
    let mut run = runtime
        .start(
            &program,
            &data,
            &SessionState::initial(&program.program).unwrap(),
            20000,
            0,
            RuntimeLimits {
                fuel: 10_000_000,
                elapsed_millis: 60_000,
                ..RuntimeLimits::default()
            },
        )
        .unwrap();
    runtime
        .apply(&mut run, &invoke("bulk", &[]), "bulk-event")
        .unwrap();
    let outputs = runtime.emitted_artifacts(&run).unwrap();
    assert_eq!(outputs.len(), 200);
    assert!(outputs
        .iter()
        .all(|output| output == &outputs[0] && output.rows.len() == 64));
    let receipts = runtime.emitted_record_participants(&run).unwrap().unwrap();
    assert_eq!(receipts.len(), outputs.len());
    let known = MAX_COLLECTION / 64;
    assert!(receipts[..known]
        .iter()
        .all(|receipt| receipt.records().is_some_and(|records| records.len() == 64)));
    assert!(
        receipts[known..]
            .iter()
            .all(|receipt| receipt.records().is_none()),
        "An over-bound emission is wholly unavailable, never a truncated subset"
    );
}

#[test]
fn sealed_backup_bytes_and_checkpoint_readback_remain_exact() {
    use product_backup::{CheckpointShelf, VerifiedBackup};
    use product_locations::{ToolIdentity, ToolLocations};
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let before = store.load().unwrap();
    let backup = VerifiedBackup::capture(&store).unwrap();
    let bytes = backup.to_bytes().unwrap();
    let identity = ToolIdentity::from_snapshot(&before).unwrap();
    assert_eq!(backup.summary().unwrap().identity, identity);
    assert_eq!(backup.clone().to_bytes().unwrap(), bytes);
    let locations = ToolLocations::create_default_at(&dir.path().join("host")).unwrap();
    let instance = canonical_digest(IdentityDomain::Evidence, &"checkpoint instance").unwrap();
    let shelf = CheckpointShelf::for_tool(&locations, &identity, &instance).unwrap();
    let mut opened =
        product_backup::open_verified(&dir.path().join("tool"), Some(&identity)).unwrap();
    let first = opened.checkpoint(&shelf).unwrap();
    let repeated = shelf.capture(&store).unwrap();
    assert_eq!(first.path, repeated.path);
    assert_eq!(fs::read(&first.path).unwrap(), bytes);
    assert_eq!(
        VerifiedBackup::from_bytes(&bytes).unwrap().snapshot(),
        &before
    );
    let changed = [bytes.clone(), b" ".to_vec()].concat();
    fs::write(&first.path, &changed).unwrap();
    assert!(shelf.capture(&store).is_err());
    assert_eq!(
        fs::read(&first.path).unwrap(),
        changed,
        "A collision must not overwrite a changed checkpoint"
    );
    assert!(VerifiedBackup::from_bytes(&changed).is_err());
    let mut wrong: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    wrong["payload"]["snapshot"]["session"]["view"] = "unknown-view".into();
    wrong["payload"]["intentions"]["snapshot"] = serde_json::to_value(
        canonical_digest(IdentityDomain::Data, &wrong["payload"]["snapshot"]).unwrap(),
    )
    .unwrap();
    wrong["digest"] = serde_json::to_value(
        canonical_digest(IdentityDomain::Evidence, &wrong["payload"]).unwrap(),
    )
    .unwrap();
    assert!(
        VerifiedBackup::from_bytes(&canonical_bytes(&wrong).unwrap()).is_err(),
        "Recomputed checksums do not authorize an invalid replay context"
    );
    assert_eq!(backup.to_bytes().unwrap(), bytes);
    assert_eq!(store.load().unwrap(), before);
}

#[test]
fn subsequent_backup_actions_recheck_intention_objects_and_exact_basis() {
    use product_backup::VerifiedBackup;
    use product_decisions::{
        accept_scene, Choice, DecisionEngine, IntentArchive, IntentionBinding,
    };
    let dir = tempdir();
    let path = dir.path().join("tool");
    let source = other_shape::capture(other_shape::organizer());
    let store = ProductStore::create(&path, &source, 20000).unwrap();
    let runtime = product_runtime::LocalRuntime::default();
    let scenario = other_shape::scenario(
        &source,
        vec![
            other_shape::add("Copied example"),
            other_shape::invoke("collect", Default::default()),
            other_shape::invoke("export_people", Default::default()),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    let scene = accept_scene(
        &runtime,
        &source,
        &scenario,
        Disclosure::Synthetic,
        RuntimeLimits::default(),
    )
    .unwrap();
    let archive = IntentArchive::new(store.clone());
    let engine = DecisionEngine::new(runtime, archive.clone());
    let prepared = engine
        .prepare_choice(
            &store,
            &source,
            Choice {
                id: "retained-example".into(),
                request: "Keep the tested export".into(),
                rationale: None,
                scope: DecisionScope {
                    operations: ["export_people".into()].into(),
                    population: Population::All,
                    conditions: Values::new(),
                    excluded_records: vec![],
                    unknowns: vec![],
                },
                outcome: DecisionOutcome::KeepCurrent,
                obligations: vec![],
                binding: IntentionBinding::ObservedOutcome,
            },
            vec![scene],
            "record-example",
        )
        .unwrap();
    let before = engine.adopt(&store, &prepared).unwrap();
    let sealed = VerifiedBackup::capture(&store).unwrap();
    let bytes = sealed.to_bytes().unwrap();
    let old_bundle = archive.export_for(&before).unwrap();
    let after = store
        .apply(
            before.revision,
            "later-work",
            &other_shape::add("Actual later work"),
            RuntimeLimits::default(),
        )
        .unwrap();
    assert!(product_decisions::validate_bundle(&after, &old_bundle).is_err());
    let fresh_bundle = archive.export_for(&after).unwrap();
    assert!(product_decisions::validate_bundle(&before, &fresh_bundle).is_err());
    assert_eq!(VerifiedBackup::capture(&store).unwrap().snapshot(), &after);
    assert_eq!(sealed.snapshot(), &before);
    assert_eq!(sealed.to_bytes().unwrap(), bytes);
    let witness = &after.decisions.decisions[0].witness;
    let object = path.join(format!("extension-{}.json", witness.as_str()));
    let original = fs::read(&object).unwrap();
    let mut opened = product_backup::open_verified(&path, None).unwrap();
    let locations =
        product_locations::ToolLocations::create_default_at(&dir.path().join("host")).unwrap();
    let instance = canonical_digest(IdentityDomain::Evidence, &"intention handoff").unwrap();
    let shelf =
        product_backup::CheckpointShelf::for_tool(&locations, &opened.summary.identity, &instance)
            .unwrap();
    fs::write(&object, b"{changed intention object}").unwrap();
    assert!(
        opened.checkpoint(&shelf).is_err(),
        "The verified-open handoff must freshly check every reachable intention object"
    );
    assert!(
        VerifiedBackup::capture(&store).is_err(),
        "The same store handle must re-read the referenced object on a subsequent action"
    );
    assert_eq!(
        sealed.to_bytes().unwrap(),
        bytes,
        "A sealed historical backup remains its own immutable value, not current-project authority"
    );
    assert_eq!(
        VerifiedBackup::from_bytes(&bytes).unwrap().snapshot(),
        &before
    );
    fs::write(&object, original).unwrap();
    assert_eq!(VerifiedBackup::capture(&store).unwrap().snapshot(), &after);
}

#[test]
fn opened_checkpoint_rechecks_concurrent_work_and_subsequent_object_corruption() {
    use product_backup::{open_verified, CheckpointShelf, VerifiedBackup};
    use product_locations::ToolLocations;
    let dir = tempdir();
    let path = dir.path().join("tool");
    let store = ProductStore::create(&path, &program(false), 20000).unwrap();
    let mut opened = open_verified(&path, None).unwrap();
    let before = opened.snapshot.clone();
    let locations = ToolLocations::create_default_at(&dir.path().join("host")).unwrap();
    let instance = canonical_digest(IdentityDomain::Evidence, &"open handoff").unwrap();
    let shelf = CheckpointShelf::for_tool(&locations, &opened.summary.identity, &instance).unwrap();
    // Another writer commits after verified receipt readback but before the
    // opened value reaches its installation/checkpoint handoff.
    let after = store
        .apply(
            before.revision,
            "concurrent-add",
            &invoke(
                "add",
                &[
                    ("name", text("Concurrent work")),
                    ("promised", DataValue::Date { days: 20010 }),
                ],
            ),
            RuntimeLimits::default(),
        )
        .unwrap();
    let receipt = opened.checkpoint(&shelf).unwrap();
    assert_eq!(receipt.summary.revision, after.revision);
    assert_eq!(
        VerifiedBackup::read(&receipt.path).unwrap().snapshot(),
        &after
    );
    assert_eq!(
        opened.snapshot, before,
        "The checkpoint must not relabel the old rendered basis"
    );
    let object = path.join(format!(
        "object-{}.json",
        canonical_digest(IdentityDomain::Data, &after)
            .unwrap()
            .as_str()
    ));
    let bytes = fs::read(&object).unwrap();
    fs::write(&object, b"{corrupted snapshot after handoff}").unwrap();
    assert!(
        opened.checkpoint(&shelf).is_err(),
        "A consumed handoff must not become a cross-action cache"
    );
    fs::write(&object, bytes).unwrap();
    assert_eq!(opened.checkpoint(&shelf).unwrap().digest, receipt.digest);
}
