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
        action(&store, "wait-first", "wait", &selected);
        action(&store, "wait-other", "wait", &other);
        action(&store, "complete-original", "complete", &completed);
        let before = store.load().unwrap();
        let hooks = TestHooks::default();
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
        studio.test_daily(invoke("calculate", &[("row", reference(&other))]));
        settle(&mut studio);
        assert_eq!(studio.test_page(), "daily");
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
        let recovered = backup.recover_new(root.join("recovery")).unwrap();
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
