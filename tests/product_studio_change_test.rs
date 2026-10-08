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
    original["actions"].as_array_mut().unwrap().push(serde_json::json!({
        "id":"batch", "label":"Record every item", "parameters":{}, "guards":[], "ensures":[],
        "steps":[{"kind":"for_each", "items": original["views"][0]["kind"]["rows"].clone(), "binding":"item", "steps":[{"kind":"update", "record":var("item"), "values":{"production":elapsed("item", false)}}]}]
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
    }
}
