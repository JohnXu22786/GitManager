//! Offline controller regressions. No live AI, native GUI, or user acceptance proof.
#[path = "fixtures/product_runtime/mod.rs"]
mod fixture;
#[path = "../src/harness.rs"]
mod harness;
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
#[path = "../src/product_sources/mod.rs"]
mod product_sources;
#[path = "../src/product_store/mod.rs"]
mod product_store;
#[path = "../src/task_delivery.rs"]
mod task_delivery;
#[path = "../src/task_verification.rs"]
mod task_verification;
#[path = "../src/tasks.rs"]
mod tasks;
#[path = "../src/tool_proposal_input.rs"]
mod tool_proposal_input;
mod ui {
    pub(crate) use super::product_runtime_view;
}
#[path = "support/egui_harness.rs"]
mod egui_harness;
#[path = "../src/product_studio.rs"]
mod product_studio;
use fixture::*;
use product_contract::*;
use product_store::ProductStore;
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
fn frame(
    h: &mut egui_harness::EguiHarness,
    studio: &mut ProductStudio,
) -> product_runtime_view::WidgetTrace {
    studio.poll();
    h.frame(|ctx| {
        egui::CentralPanel::default()
            .show(ctx, |ui| studio.show(ui))
            .inner
    })
}
fn click(h: &mut egui_harness::EguiHarness, studio: &mut ProductStudio, key: &str) {
    let view = frame(h, studio);
    let widget = view
        .controls
        .get(key)
        .unwrap_or_else(|| panic!("Missing {key}: {:?}", view.text));
    assert!(widget.enabled, "Disabled {key}: {:?}", view.text);
    h.press_at(widget.rect.center());
    frame(h, studio);
    h.release_at(widget.rect.center());
    frame(h, studio);
    settle(studio);
}
#[cfg(unix)]
fn transport(root: &Path) -> product_provider::ProviderTransport {
    use std::os::unix::fs::PermissionsExt;
    let executable = root.join("discover-fixture.py");
    let _guard = product_provider::fixture_executable_write_guard();
    // This subprocess is explicitly synthetic. It never contacts a provider.
    let body = "{'version':1,'request_digest':json.loads(wire['prompt'])['request_digest'],'candidates':([{'id':'primary','source_json':json.dumps(cfg['program'])},{'id':'other','source_json':json.dumps(cfg['other'])}] if json.loads(wire['prompt'])['request']['operation']=='modify' else []),'hypotheses':[],'evolutions':[],'unsupported':(['This synthetic fixture offers no hypotheses'] if json.loads(wire['prompt'])['request']['operation']=='discover' else [])}";
    fs::write(
        &executable,
        include_str!("fixtures/provider_transport/fake_cli.py").replace(
            "{'passed': True, 'text': wire['prompt'], 'command': 'untrusted-do-not-execute'}",
            body,
        ),
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let mut candidate = organizer();
    candidate["views"][0]["label"] = serde_json::json!("Edited working list");
    // A genuine different design shape, rather than a fixed rule preparation.
    let second_kind = candidate["views"][0]["kind"].clone();
    candidate["views"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id":"second_list", "label":"Another view", "kind":second_kind, "actions":[], "keys":[]
        }));
    let mut other = candidate.clone();
    other["label"] = serde_json::json!("Other candidate");
    fs::write(
        executable.with_extension("json"),
        serde_json::to_vec(&serde_json::json!({"mode":"good", "program":candidate, "other":other}))
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
fn modified(root: &Path) -> (ProductStudio, ProductStore, DevelopmentRequest) {
    modified_with_hooks(root, TestHooks::default())
}
#[cfg(unix)]
fn modified_with_hooks(
    root: &Path,
    hooks: TestHooks,
) -> (ProductStudio, ProductStore, DevelopmentRequest) {
    let store = ProductStore::create(root.join("tool"), &capture(organizer()), 20000).unwrap();
    let mut studio = ProductStudio::testing(root.into(), Some(transport(root)), hooks);
    settle(&mut studio);
    studio.test_open(root.join("tool"));
    settle(&mut studio);
    studio.test_modify("Please make this easier to use");
    settle(&mut studio);
    let request = studio.test_prepared_request().unwrap().clone();
    studio.test_consent();
    settle(&mut studio);
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    (studio, store, request)
}
fn journal(root: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(root.join("studio/session.json")).unwrap()).unwrap()
}

#[cfg(unix)]
#[test]
fn completed_modify_is_durable_before_discover_is_offered() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let (studio, store, request) = modified(&root);
    let value = journal(&root);
    assert!(value.get("discovery").is_some(), "The completed original Modify response, exact need, receipt and selected candidate were discarded before Discover");
    let saved = value["discovery"].to_string();
    assert!(saved.contains(&request.id));
    assert!(saved.contains("Please make this easier to use"));
    assert!(saved.contains("primary"));
    assert!(
        !studio.test_generation_blocked(),
        "{}",
        studio.test_notice()
    );
    assert_eq!(
        store.load().unwrap().program().unwrap(),
        &capture(organizer())
    );
}

#[cfg(unix)]
#[test]
fn real_discover_control_requires_separate_consent_and_reports_unsupported_only_result() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let (mut studio, store, modify) = modified(&root);
    let before = store.load().unwrap();
    let mut h = egui_harness::EguiHarness::new(egui::vec2(1400.0, 5000.0));
    click(&mut h, &mut studio, "studio.discover");
    assert_eq!(studio.test_page(), "consent", "{}", studio.test_notice());
    let discover = studio.test_prepared_request().unwrap().clone();
    assert_eq!(discover.operation, DevelopmentOperation::Discover);
    assert_ne!(discover.id, modify.id);
    assert_eq!(discover.sources[0], modify.sources.last().unwrap().clone());
    assert_eq!(journal(&root)["provider"]["issued"], false);
    let text = frame(&mut h, &mut studio).text.join("\n");
    assert!(text.contains("Discover"), "{text}");
    click(&mut h, &mut studio, "studio.consent");
    assert_eq!(studio.test_page(), "discovery", "{}", studio.test_notice());
    let report = frame(&mut h, &mut studio).text.join("\n");
    assert!(
        report.contains("This synthetic fixture offers no hypotheses"),
        "{report}"
    );
    assert!(report.contains("0"), "No fabricated question: {report}");
    assert_eq!(store.load().unwrap(), before);
}

#[path = "fixtures/product_discovery/mod.rs"]
mod discovery_fixture;

#[cfg(unix)]
fn choice_transport(root: &Path) -> (product_provider::ProviderTransport, CapturedProgram) {
    use std::os::unix::fs::PermissionsExt;
    let (request, result) = discovery_fixture::input();
    let executable = root.join("choice-fixture.py");
    let _guard = product_provider::fixture_executable_write_guard();
    let body = "dict(cfg['discover'],request_digest=json.loads(wire['prompt'])['request_digest']) if json.loads(wire['prompt'])['request']['operation']=='discover' else {'version':1,'request_digest':json.loads(wire['prompt'])['request_digest'],'candidates':[{'id':'changed','source_json':cfg['modified']}],'hypotheses':[],'evolutions':[],'unsupported':[]}";
    fs::write(
        &executable,
        include_str!("fixtures/provider_transport/fake_cli.py").replace(
            "{'passed': True, 'text': wire['prompt'], 'command': 'untrusted-do-not-execute'}",
            body,
        ),
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(executable.with_extension("json"), serde_json::to_vec(&serde_json::json!({"mode":"good", "modified":String::from_utf8(request.sources[1].source_bytes.clone()).unwrap(), "discover":result.response})).unwrap()).unwrap();
    let home = root.join("fixture-home");
    fs::create_dir(&home).unwrap();
    (
        product_provider::ProviderTransport::new_fixture(
            root.join("jobs"),
            product_provider::ProviderKind::Codex,
            executable,
            home,
        )
        .unwrap(),
        request.sources[0].clone(),
    )
}
#[cfg(unix)]
fn discovered(root: &Path) -> (ProductStudio, ProductStore, egui_harness::EguiHarness) {
    discovered_with_hooks(root, TestHooks::default())
}
#[cfg(unix)]
fn discovered_with_hooks(
    root: &Path,
    hooks: TestHooks,
) -> (ProductStudio, ProductStore, egui_harness::EguiHarness) {
    let (transport, before) = choice_transport(root);
    let store = ProductStore::create(root.join("tool"), &before, 20000).unwrap();
    let mut studio = ProductStudio::testing(root.into(), Some(transport), hooks);
    settle(&mut studio);
    studio.test_open(root.join("tool"));
    settle(&mut studio);
    studio.test_modify("Make collecting the current search results work smoothly");
    settle(&mut studio);
    studio.test_consent();
    settle(&mut studio);
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    let mut h = egui_harness::EguiHarness::new(egui::vec2(1400.0, 5000.0));
    click(&mut h, &mut studio, "studio.discover");
    click(&mut h, &mut studio, "studio.consent");
    assert_eq!(studio.test_page(), "discovery", "{}", studio.test_notice());
    (studio, store, h)
}
fn first_control(
    h: &mut egui_harness::EguiHarness,
    studio: &mut ProductStudio,
    prefix: &str,
) -> String {
    let trace = frame(h, studio);
    trace
        .controls
        .keys()
        .find(|id| id.starts_with(prefix))
        .unwrap_or_else(|| panic!("Missing {prefix}: {:?}", trace.text))
        .clone()
}
#[cfg(unix)]
#[test]
fn actual_discovered_example_is_separate_from_current_work_and_reset_preserves_the_witness() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let (mut studio, store, mut h) = discovered(&root);
    let before = store.load().unwrap();
    let example = first_control(&mut h, &mut studio, "studio.discovery.example.");
    click(&mut h, &mut studio, &example);
    assert_eq!(
        studio.test_page(),
        "discovery-example",
        "{}",
        studio.test_notice()
    );
    let original = frame(&mut h, &mut studio);
    assert!(!original
        .controls
        .keys()
        .any(|k| k.starts_with("studio.discovery.choose.")));
    click(&mut h, &mut studio, "studio.discovery.next-day");
    click(&mut h, &mut studio, "studio.discovery.reset");
    assert_eq!(frame(&mut h, &mut studio).text, original.text);
    assert_eq!(store.load().unwrap(), before);
    click(&mut h, &mut studio, "studio.discovery.report");
    let copy = first_control(&mut h, &mut studio, "studio.discovery.copy.");
    click(&mut h, &mut studio, &copy);
    assert_eq!(
        studio.test_page(),
        "discovery-copy",
        "{}",
        studio.test_notice()
    );
    assert_eq!(store.load().unwrap(), before);
}
#[cfg(unix)]
#[test]
fn all_six_discovered_choices_use_checked_copies_and_pending_choices_reopen_after_real_work() {
    for choice in 0..6 {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let (mut studio, store, mut h) = discovered(&root);
        let before = store.load().unwrap();
        let copy = first_control(&mut h, &mut studio, "studio.discovery.copy.");
        click(&mut h, &mut studio, &copy);
        click(
            &mut h,
            &mut studio,
            &format!("studio.discovery.choose.{choice}"),
        );
        assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
        let saved = store.load().unwrap();
        assert_eq!(saved.data, before.data);
        let decision = saved.decisions.decisions.last().unwrap();
        if choice >= 2 {
            assert_eq!(decision.status, DecisionStatus::Pending);
            assert_eq!(saved.active_revision, before.active_revision);
            assert_eq!(
                decision.outcome,
                [
                    DecisionOutcome::EitherAcceptable,
                    DecisionOutcome::BothNeeded,
                    DecisionOutcome::NeitherFits,
                    DecisionOutcome::Deferred
                ][choice - 2]
            );
            let decision = decision.id.clone();
            studio.test_daily(add("Later real work"));
            settle(&mut studio);
            let after_work = store.load().unwrap();
            studio.test_close();
            settle(&mut studio);
            studio.test_open(root.join("tool"));
            settle(&mut studio);
            click(&mut h, &mut studio, &format!("studio.resume.{decision}"));
            assert_eq!(
                studio.test_page(),
                "discovery-copy",
                "{}",
                studio.test_notice()
            );
            assert_eq!(store.load().unwrap(), after_work);
        } else {
            assert_eq!(decision.status, DecisionStatus::Active);
        }
    }
}

#[cfg(unix)]
#[test]
fn selected_modify_candidate_and_completed_discover_reopen_offline_without_reauthorizing() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let (mut studio, store, _) = modified(&root);
    let mut h = egui_harness::EguiHarness::new(egui::vec2(1400.0, 5000.0));
    click(&mut h, &mut studio, "studio.modified-candidate.other");
    click(&mut h, &mut studio, "studio.discover");
    assert_eq!(
        studio.test_prepared_request().unwrap().sources[1]
            .program
            .label,
        "Other candidate"
    );
    click(&mut h, &mut studio, "studio.consent");
    let before = store.load().unwrap();
    let record = journal(&root)["discovery"].clone();
    studio.test_close();
    settle(&mut studio);
    drop(studio);
    let start = Instant::now();
    while product_locations::Folder::open(&root.join("studio"))
        .unwrap()
        .lock()
        .is_err()
    {
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(2));
    }
    // Wait for the existing single host lock to be released, not a new invocation.
    let mut restarted = ProductStudio::testing(root.clone(), None, TestHooks::default());
    settle(&mut restarted);
    assert_eq!(
        restarted.test_page(),
        "daily",
        "{}",
        restarted.test_notice()
    );
    click(&mut h, &mut restarted, "studio.reopen-discovery");
    assert_eq!(
        restarted.test_page(),
        "discovery",
        "{}",
        restarted.test_notice()
    );
    assert_eq!(journal(&root)["discovery"], record);
    assert_eq!(store.load().unwrap(), before);
}
#[cfg(unix)]
#[test]
fn later_work_marks_retained_discovery_stale_without_losing_original_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let (mut studio, store, mut h) = discovered(&root);
    let retained = journal(&root)["discovery"].clone();
    click(&mut h, &mut studio, "studio.return");
    studio.test_daily(add("Later current work"));
    settle(&mut studio);
    let later = store.load().unwrap();
    click(&mut h, &mut studio, "studio.reopen-discovery");
    let view = frame(&mut h, &mut studio);
    assert!(
        view.text.iter().any(|t| t.contains("stale")),
        "{:?}",
        view.text
    );
    assert!(!view
        .controls
        .iter()
        .any(|(id, control)| id.starts_with("studio.discovery.choose.") && control.enabled));
    assert_eq!(journal(&root)["discovery"], retained);
    assert_eq!(store.load().unwrap(), later);
}

#[cfg(unix)]
fn accepted_feature_case(
    root: &Path,
    hooks: TestHooks,
) -> (
    ProductStudio,
    ProductStore,
    egui_harness::EguiHarness,
    serde_json::Value,
) {
    accepted_feature_case_with_state(root, hooks, false)
}

#[cfg(unix)]
fn accepted_feature_case_with_state(
    root: &Path,
    hooks: TestHooks,
    extra_state: bool,
) -> (
    ProductStudio,
    ProductStore,
    egui_harness::EguiHarness,
    serde_json::Value,
) {
    let transport = transport(&root);
    let config_path = root.join("discover-fixture.json");
    let mut cfg: serde_json::Value =
        serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    let mut program = filtered();
    let query = program["actions"][1]["steps"][0]["items"].clone();
    program["actions"].as_array_mut().unwrap().push(serde_json::json!({"id":"collect_new","label":"Replace with these matches","parameters":{},"guards":[],"steps":[{"kind":"set_state","state":"selected","value":query}],"ensures":[]}));
    program["views"][0]["actions"].as_array_mut().unwrap().push(serde_json::json!({"id":"collect_new_button","label":"Replace with these matches","action":"collect_new","placement":"toolbar","arguments":{},"enabled":yes()}));
    if extra_state {
        program["state"].as_array_mut().unwrap().push(serde_json::json!({"id":"feature_runs","label":"New feature executions","value_type":{"kind":"integer"},"initial":{"kind":"integer","value":0}}));
        program["actions"][3]["steps"].as_array_mut().unwrap().push(serde_json::json!({"kind":"set_state","state":"feature_runs","value":{"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":1}}}));
    }
    cfg["program"] = program.clone();
    cfg["other"] = program;
    fs::write(config_path, serde_json::to_vec(&cfg).unwrap()).unwrap();
    let store = ProductStore::create(root.join("tool"), &capture(filtered()), 20000).unwrap();
    store
        .apply(0, "real-ada", &add("Ada"), RuntimeLimits::default())
        .unwrap();
    let mut studio = ProductStudio::testing(root.to_path_buf(), Some(transport), hooks);
    settle(&mut studio);
    studio.test_open(root.join("tool"));
    settle(&mut studio);
    studio.test_modify("I need a separate replace-matches button");
    settle(&mut studio);
    studio.test_consent();
    settle(&mut studio);
    assert_eq!(
        studio.test_page(),
        "change",
        "The actual valid new feature must reach copied review first: {}",
        studio.test_notice()
    );
    let mut h = egui_harness::EguiHarness::new(egui::vec2(1400.0, 5000.0));
    click(&mut h, &mut studio, "studio.feature-example");
    assert_eq!(
        studio.test_page(),
        "discovery-requirement",
        "{}",
        studio.test_notice()
    );
    assert!(journal(&root)["discovery"]["accepted_case"].is_null());
    click(
        &mut h,
        &mut studio,
        "feature-candidate.action.collect_new_button",
    );
    assert!(
        journal(&root)["discovery"]["accepted_case"].is_null(),
        "Running or viewing a copied result must not approve a requirement"
    );
    let control = first_control(
        &mut h,
        &mut studio,
        "studio.feature.accept.observed.selected_count",
    );
    click(&mut h, &mut studio, &control);
    let case = journal(&root)["discovery"]["accepted_case"].clone();
    assert_eq!(
        case["property"]["predicate"]["right"]["value"],
        serde_json::json!({"kind":"integer","value":1})
    );
    assert!(!case["approval_operation"].as_str().unwrap().is_empty());
    assert_eq!(case["required_actions"], serde_json::json!(["collect_new"]));
    (studio, store, h, case)
}

#[cfg(unix)]
#[test]
fn actual_ui_typed_result_requires_separate_explicit_case_acceptance_before_disclosure() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let (mut studio, store, mut h, case) = accepted_feature_case(&root, TestHooks::default());
    let before = store.load().unwrap();
    click(&mut h, &mut studio, "studio.feature.back");
    click(&mut h, &mut studio, "studio.discover");
    let request = studio.test_prepared_request().unwrap();
    let example = request
        .examples
        .iter()
        .find(|e| e.scenario.id == case["scenario"]["id"].as_str().unwrap())
        .unwrap();
    assert_eq!(example.disclosure, Disclosure::ExplicitlySelected);
    assert_eq!(example.scenario.validity.len(), 1);
    assert!(
        matches!(&example.scenario.inputs[0], SemanticInput::Activate { binding, .. } if binding == "collect_new_button")
    );
    assert_eq!(store.load().unwrap(), before);
    assert!(!journal(&root)["provider"]["issued"].as_bool().unwrap());
}

#[cfg(unix)]
fn response_transport(root: &Path) -> product_provider::ProviderTransport {
    use std::os::unix::fs::PermissionsExt;
    let executable = root.join("response-fixture.py");
    let _guard = product_provider::fixture_executable_write_guard();
    // Exact configured synthetic responses travel through the actual adapter;
    // only its correlation digest is filled from the real prepared wire.
    let body = "dict(cfg['response'],request_digest=json.loads(wire['prompt'])['request_digest'])";
    fs::write(
        &executable,
        include_str!("fixtures/provider_transport/fake_cli.py").replace(
            "{'passed': True, 'text': wire['prompt'], 'command': 'untrusted-do-not-execute'}",
            body,
        ),
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(executable.with_extension("json"), br#"{"mode":"good"}"#).unwrap();
    let home = root.join("response-home");
    fs::create_dir(&home).unwrap();
    product_provider::ProviderTransport::new_fixture(
        root.join("response-jobs"),
        product_provider::ProviderKind::Codex,
        executable,
        home,
    )
    .unwrap()
}
fn configure_response(root: &Path, response: &DevelopmentResponse) {
    fs::write(
        root.join("response-fixture.json"),
        serde_json::to_vec(&serde_json::json!({"mode":"good","response":response})).unwrap(),
    )
    .unwrap();
}
fn new_feature_program(replace: bool) -> serde_json::Value {
    let mut program = filtered();
    let step = if replace {
        serde_json::json!({"kind":"set_state","state":"selected","value":program["actions"][1]["steps"][0]["items"].clone()})
    } else {
        program["actions"][1]["steps"][0].clone()
    };
    program["actions"].as_array_mut().unwrap().push(serde_json::json!({"id":"collect_new","label":"Collect with the new feature","parameters":{},"guards":[],"steps":[step],"ensures":[]}));
    program["views"][0]["actions"].as_array_mut().unwrap().push(serde_json::json!({"id":"collect_new_button","label":"Collect with the new feature","action":"collect_new","placement":"toolbar","arguments":{},"enabled":yes()}));
    program
}
#[cfg(unix)]
fn run_actual_ui_feature_fairness(noop: bool) {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let transport = response_transport(&root);
    let store = ProductStore::create(root.join("tool"), &capture(filtered()), 20000).unwrap();
    store
        .apply(0, "real-ada", &add("Ada"), RuntimeLimits::default())
        .unwrap();
    let before = store.load().unwrap();
    let mut studio = ProductStudio::testing(root.clone(), Some(transport), TestHooks::default());
    settle(&mut studio);
    studio.test_open(root.join("tool"));
    settle(&mut studio);
    studio.test_modify("Add a separate collection action while preserving my work");
    settle(&mut studio);
    let modify = studio
        .test_prepared_request()
        .unwrap_or_else(|| {
            panic!(
                "Actual Modify disclosure must exist before fixture response selection: {}",
                studio.test_notice()
            )
        })
        .clone();
    configure_response(
        &root,
        &DevelopmentResponse {
            version: 1,
            request_digest: modify.identity().unwrap(),
            candidates: vec![GeneratedCandidate {
                id: "changed".into(),
                source_json: serde_json::to_string(&new_feature_program(true)).unwrap(),
            }],
            hypotheses: vec![],
            evolutions: vec![],
            unsupported: vec![],
        },
    );
    studio.test_consent();
    settle(&mut studio);
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    let mut h = egui_harness::EguiHarness::new(egui::vec2(1500.0, 5000.0));
    click(&mut h, &mut studio, "studio.feature-example");
    click(
        &mut h,
        &mut studio,
        "feature-candidate.action.collect_new_button",
    );
    assert!(journal(&root)["discovery"]["accepted_case"].is_null());
    click(
        &mut h,
        &mut studio,
        "studio.feature.accept.observed.selected_count",
    );
    assert_eq!(
        journal(&root)["discovery"]["accepted_case"]["required_actions"],
        serde_json::json!(["collect_new"])
    );
    click(&mut h, &mut studio, "studio.feature.back");
    click(&mut h, &mut studio, "studio.discover");
    let request = studio.test_prepared_request().unwrap().clone();
    let target = &request.sources[1];
    let scene = scenario(
        target,
        vec![
            add("Ada"),
            add("Zoe"),
            SemanticInput::Activate {
                view: "people".into(),
                binding: "collect_new_button".into(),
                row: None,
            },
            SemanticInput::Control {
                view: "people".into(),
                control: "search_input".into(),
                value: string("Ada"),
            },
            SemanticInput::Activate {
                view: "people".into(),
                binding: "collect_new_button".into(),
                row: None,
            },
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    scene.validate(&target.program).unwrap();
    assert!(
        scene.validate(&before.program().unwrap().program).is_err(),
        "The saved Current really lacks the new action"
    );
    let mut other = new_feature_program(false);
    if noop {
        other["actions"][3]["steps"] = serde_json::json!([{"kind":"set_state","state":"selected","value":{"kind":"state","state":"selected"}}]);
    }
    configure_response(
        &root,
        &DevelopmentResponse {
            version: 1,
            request_digest: request.identity().unwrap(),
            candidates: vec![
                GeneratedCandidate {
                    id: "replace".into(),
                    source_json: String::from_utf8(target.source_bytes.clone()).unwrap(),
                },
                GeneratedCandidate {
                    id: "retain".into(),
                    source_json: serde_json::to_string(&other).unwrap(),
                },
            ],
            hypotheses: vec![ChoiceHypothesis {
                id: "new-feature-choice".into(),
                statement: "Repeated new-feature collection may replace or retain previous matches"
                    .into(),
                kind: HypothesisKind::UnresolvedChoice,
                action: "collect_new".into(),
                observable: "selected_count".into(),
                sources: vec![SourceLocus {
                    relative_path: target.binding.program_path.clone(),
                    raw_digest: target.artifact.raw_digest.clone(),
                    pointer: "/actions/3/steps/0".into(),
                }],
                alternatives: vec!["replace".into(), "retain".into()],
                related_decisions: vec![],
                scenario_json: serde_json::to_string(&scene).unwrap(),
                unknowns: vec![],
            }],
            evolutions: vec![],
            unsupported: vec![],
        },
    );
    click(&mut h, &mut studio, "studio.consent");
    assert_eq!(studio.test_page(), "discovery", "{}", studio.test_notice());
    let trace = frame(&mut h, &mut studio);
    let text = trace.text.join("\n");
    if noop {
        assert!(text.contains("0 coherent questions"), "An action that produces no accepted result is a failed implementation, not a business preference: {text}");
        assert!(!trace
            .controls
            .keys()
            .any(|key| key.starts_with("studio.discovery.copy.")));
        assert_eq!(store.load().unwrap(), before);
        return;
    }
    assert!(text.contains("1 coherent questions"), "{text}");
    assert!(!text.contains("Captured-source replay unavailable"), "An explicitly accepted actual UI feature must classify Current's precise missing action instead of inventing or demanding Current execution: {text}");
    assert!(
        text.contains("No current-side experience is claimed"),
        "{text}"
    );
    let example = first_control(&mut h, &mut studio, "studio.discovery.example.");
    click(&mut h, &mut studio, &example);
    assert_eq!(studio.test_page(), "discovery-example");
    assert!(!frame(&mut h, &mut studio)
        .text
        .iter()
        .any(|s| s == "Current"));
    assert_eq!(store.load().unwrap(), before);
}

#[cfg(unix)]
#[test]
fn actual_ui_accepted_feature_compares_two_working_implementations_without_current_capability_fiction(
) {
    run_actual_ui_feature_fairness(false);
}
#[cfg(unix)]
#[test]
fn actual_ui_accepted_typed_result_rejects_a_noop_feature_without_a_preference_question() {
    run_actual_ui_feature_fairness(true);
}

// Cancellation is injected only after the real transport has returned its actual
// validated receipt, at the existing host-supplied cancellation callback.
#[cfg(unix)]
fn cancelled_validated_completion(discover: bool) {
    use std::sync::atomic::Ordering;
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let store = ProductStore::create(root.join("tool"), &capture(organizer()), 20000).unwrap();
    let transport = transport(&root);
    let hooks = TestHooks::default();
    let cancel = hooks.cancel_after_validated_provider.clone();
    let stopped = hooks.stopped.clone();
    let mut studio = ProductStudio::testing(root.clone(), Some(transport.clone()), hooks);
    settle(&mut studio);
    studio.test_open(root.join("tool"));
    settle(&mut studio);
    studio.test_modify("Preserve exact completion when local publication is cancelled");
    settle(&mut studio);
    let mut h = egui_harness::EguiHarness::new(egui::vec2(1400.0, 5000.0));
    if discover {
        studio.test_consent();
        settle(&mut studio);
        assert_eq!(studio.test_page(), "change");
        click(&mut h, &mut studio, "studio.discover");
    }
    let request = studio.test_prepared_request().unwrap().clone();
    cancel.store(true, Ordering::Release);
    studio.test_consent();
    settle(&mut studio);
    let receipt = transport.reconcile(&request.id).unwrap();
    assert_eq!(
        receipt.state,
        product_provider::JobState::TransportValidated,
        "The fixture must reach actual terminal validation before cancelling ingestion"
    );
    let actual = transport
        .ingest(&request.id, &receipt.request_digest, &receipt.source_digest)
        .unwrap()
        .unwrap();
    let value = journal(&root);
    let which = if discover { "discover" } else { "original" };
    assert_eq!(value["discovery"][which]["issued"], true);
    assert!(
        value["provider"].is_object() || value["discovery"][which]["completed"].is_object(),
        "A real validated completion was discarded after cancellation before ingestion"
    );
    assert_ne!(
        studio.test_page(),
        "discovery",
        "Cancellation cannot publish new choice authority"
    );
    let saved = store.load().unwrap();
    drop(studio);
    let start = Instant::now();
    while !stopped.load(Ordering::Acquire) {
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(2));
    }
    let mut restarted =
        ProductStudio::testing(root.clone(), Some(transport.clone()), TestHooks::default());
    settle(&mut restarted);
    let value = journal(&root);
    assert!(value["provider"].is_null());
    assert!(value["discovery"][which]["completed"].is_object());
    let retained = product_studio::journal_testing::invocations(
        &fs::read(root.join("studio/session.json")).unwrap(),
    )
    .unwrap();
    let (_, bytes) = retained.iter().find(|(r, _)| r.id == request.id).unwrap();
    let (raw, kept) = bytes.as_ref().unwrap();
    assert_eq!(raw, &actual.final_bytes);
    assert_eq!(
        canonical_bytes(kept).unwrap(),
        canonical_bytes(&actual.receipt).unwrap()
    );
    assert_eq!(
        transport.reconcile(&request.id).unwrap().session_id,
        receipt.session_id
    );
    assert_eq!(store.load().unwrap(), saved);
}
#[cfg(unix)]
#[test]
fn cancelled_after_validated_modify_preserves_exact_completion_for_fresh_reopen() {
    cancelled_validated_completion(false);
}
#[cfg(unix)]
#[test]
fn cancelled_after_validated_discover_preserves_exact_completion_for_fresh_reopen() {
    cancelled_validated_completion(true);
}

#[cfg(unix)]
fn assert_no_old_change_choices(h: &mut egui_harness::EguiHarness, studio: &mut ProductStudio) {
    let view = frame(h, studio);
    for id in [
        "studio.accept",
        "studio.keep-current",
        "studio.either",
        "studio.both",
        "studio.neither",
        "studio.defer",
    ] {
        assert!(
            !view.controls.get(id).is_some_and(|c| c.enabled),
            "Failed/cancelled copied input left {id} enabled: {:?}",
            view.text
        );
    }
}
#[cfg(unix)]
#[test]
fn failed_actual_alternative_navigation_revokes_published_change_receiver_readiness() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let (mut studio, store, _) = modified(&root);
    let saved = store.load().unwrap();
    let mut h = egui_harness::EguiHarness::new(egui::vec2(1400.0, 5000.0));
    studio.test_trial(add("Only copied business work"));
    settle(&mut studio);
    assert!(
        frame(&mut h, &mut studio).controls["studio.either"].enabled,
        "The previous checked copy must actually be ready"
    );
    click(&mut h, &mut studio, "alternative.navigate.second_list");
    assert!(
        studio.test_notice().contains("unknown") || studio.test_notice().contains("Unknown"),
        "The real target-only navigation must fail Current replay: {}",
        studio.test_notice()
    );
    assert_no_old_change_choices(&mut h, &mut studio);
    assert_eq!(store.load().unwrap(), saved);
}
#[cfg(unix)]
#[test]
fn cancelled_actual_alternative_navigation_revokes_published_change_receiver_readiness() {
    use std::sync::{atomic::Ordering, Arc};
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let pause = Arc::new(product_studio::TestPause::default());
    pause.release.store(true, Ordering::Release);
    let (mut studio, store, _) = modified_with_hooks(
        &root,
        TestHooks {
            before_preview: Some(pause.clone()),
            ..Default::default()
        },
    );
    let saved = store.load().unwrap();
    let mut h = egui_harness::EguiHarness::new(egui::vec2(1400.0, 5000.0));
    studio.test_trial(add("Only copied business work"));
    settle(&mut studio);
    assert!(frame(&mut h, &mut studio).controls["studio.either"].enabled);
    pause.reached.store(false, Ordering::Release);
    pause.release.store(false, Ordering::Release);
    let view = frame(&mut h, &mut studio);
    let p = view.controls["alternative.navigate.second_list"]
        .rect
        .center();
    h.press_at(p);
    frame(&mut h, &mut studio);
    h.release_at(p);
    frame(&mut h, &mut studio);
    let start = Instant::now();
    while !pause.reached.load(Ordering::Acquire) {
        studio.poll();
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(studio.test_cancel());
    pause.release.store(true, Ordering::Release);
    settle(&mut studio);
    assert_no_old_change_choices(&mut h, &mut studio);
    assert_eq!(store.load().unwrap(), saved);
}

#[cfg(unix)]
#[test]
fn a_fitting_exact_response_field_cannot_overflow_the_whole_journal_or_lose_issued_context() {
    use std::sync::{atomic::Ordering, Arc, Mutex};
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let store = ProductStore::create(root.join("tool"), &capture(organizer()), 20000).unwrap();
    let saved = store.load().unwrap();
    let transport = response_transport(&root);
    let hooks = TestHooks::default();
    let stopped = hooks.stopped.clone();
    let mut studio = ProductStudio::testing(root.clone(), Some(transport.clone()), hooks);
    settle(&mut studio);
    studio.test_open(root.join("tool"));
    settle(&mut studio);
    studio.test_modify("Retain exact completion or honestly refuse the complete record");
    settle(&mut studio);
    let request = studio.test_prepared_request().unwrap().clone();
    let mut response = DevelopmentResponse {
        version: 1,
        request_digest: request.identity().unwrap(),
        candidates: vec![GeneratedCandidate {
            id: "actual-candidate".into(),
            source_json: serde_json::to_string(&organizer()).unwrap(),
        }],
        hypotheses: vec![],
        evolutions: vec![],
        unsupported: vec![],
    };
    // Deterministic high-entropy fixture text, not user data or provider prose
    // acceptance. The raw response and each individual field remain valid.
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";
    let mut state = 0x7261775f66697874u64;
    let random: String = (0..600_000)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            alphabet[((state >> 32) as usize) % alphabet.len()] as char
        })
        .collect();
    let mut low = 1usize;
    let mut high = random.len();
    let target = MAX_WIRE_BYTES - 4000;
    while low < high {
        let mid = (low + high) / 2;
        response.unsupported = random.as_bytes()[..mid]
            .chunks(MAX_TEXT_BYTES)
            .map(|c| String::from_utf8(c.to_vec()).unwrap())
            .collect();
        let raw = serde_json::to_vec(&response).unwrap();
        if product_studio::journal_testing::response_field_bytes(&raw).unwrap_or(usize::MAX)
            < target
        {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    response.unsupported = random.as_bytes()[..low]
        .chunks(MAX_TEXT_BYTES)
        .map(|c| String::from_utf8(c.to_vec()).unwrap())
        .collect();
    response.validate_for(&request).unwrap();
    let raw = serde_json::to_vec(&response).unwrap();
    assert!(raw.len() < MAX_WIRE_BYTES);
    assert!(product_studio::journal_testing::response_field_bytes(&raw).unwrap() < MAX_WIRE_BYTES);
    fs::write(
        root.join("response-fixture.json"),
        serde_json::to_vec(&serde_json::json!({"mode":"good","response":response})).unwrap(),
    )
    .unwrap();
    let publications = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let observer = product_studio::journal_testing::observe_synced(&root, {
        let publications = publications.clone();
        move |bytes| publications.lock().unwrap().push(bytes)
    });
    studio.test_consent();
    settle(&mut studio);
    let receipt = transport.reconcile(&request.id).unwrap();
    assert_eq!(
        receipt.state,
        product_provider::JobState::TransportValidated
    );
    let actual = transport
        .ingest(&request.id, &receipt.request_digest, &receipt.source_digest)
        .unwrap()
        .unwrap();
    assert!(
        product_studio::journal_testing::response_field_bytes(&actual.final_bytes).unwrap()
            <= MAX_WIRE_BYTES,
        "The actual receipt-bound raw response must fit its own field"
    );
    let prior = publications.lock().unwrap().last().unwrap().clone();
    let path = root.join("studio/session.json");
    assert_eq!(fs::read(&path).unwrap(), prior);
    let value: serde_json::Value = serde_json::from_slice(&prior).unwrap();
    assert_eq!(value["provider"]["issued"], true);
    assert!(value["discovery"]["original"]["completed"].is_null());
    assert!(
        studio.test_notice().contains("bounded restart record"),
        "{}",
        studio.test_notice()
    );
    assert!(
        studio.test_generation_blocked(),
        "Issued completion refusal must visibly block a second provider request"
    );
    assert!(
        !studio.test_notice().contains("nothing was submitted"),
        "A real completed invocation must not be described as never submitted"
    );
    studio.test_modify("Must not overwrite issued evidence");
    settle(&mut studio);
    assert_eq!(fs::read(&path).unwrap(), prior);
    assert_eq!(store.load().unwrap(), saved);
    drop(observer);
    drop(studio);
    let start = Instant::now();
    while !stopped.load(Ordering::Acquire) {
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(2));
    }
    let mut restarted =
        ProductStudio::testing(root.clone(), Some(transport.clone()), TestHooks::default());
    settle(&mut restarted);
    assert!(restarted.test_generation_blocked());
    assert_eq!(fs::read(&path).unwrap(), prior);
    assert_eq!(store.load().unwrap(), saved);
    assert_eq!(
        fs::read_dir(root.join("response-jobs"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_type().unwrap().is_dir())
            .count(),
        1
    );
}

#[cfg(unix)]
#[test]
fn actual_ui_accepted_case_reopens_exactly_and_changed_typed_value_cannot_authorize_discover() {
    use std::sync::atomic::Ordering;
    for tamper in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let hooks = TestHooks::default();
        let stopped = hooks.stopped.clone();
        let (studio, store, mut h, case) = accepted_feature_case(&root, hooks);
        let saved = store.load().unwrap();
        drop(studio);
        let start = Instant::now();
        while !stopped.load(Ordering::Acquire) {
            assert!(start.elapsed() < Duration::from_secs(20));
            std::thread::sleep(Duration::from_millis(2));
        }
        let path = root.join("studio/session.json");
        if tamper {
            let mut value = journal(&root);
            value["discovery"]["accepted_case"]["property"]["predicate"]["right"]["value"]
                ["value"] = serde_json::json!(777);
            fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        }
        let retained = fs::read(&path).unwrap();
        // Reuse the existing verified fixture configuration/jobs for readback
        // and preparation only. No consent is given after this restart.
        let transport = product_provider::ProviderTransport::new_fixture(
            root.join("jobs"),
            product_provider::ProviderKind::Codex,
            root.join("discover-fixture.py"),
            root.join("fixture-home"),
        )
        .unwrap();
        let mut studio =
            ProductStudio::testing(root.clone(), Some(transport.clone()), TestHooks::default());
        settle(&mut studio);
        click(&mut h, &mut studio, "studio.reopen-discovery");
        assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
        click(&mut h, &mut studio, "studio.discover");
        if tamper {
            assert_eq!(studio.test_page(), "change");
            assert!(
                studio.test_notice().contains("accepted typed result"),
                "{}",
                studio.test_notice()
            );
            assert!(journal(&root)["provider"].is_null());
            assert_eq!(fs::read(&path).unwrap(), retained);
        } else {
            assert_eq!(studio.test_page(), "consent", "{}", studio.test_notice());
            let request = studio.test_prepared_request().unwrap();
            assert!(request.examples.iter().any(|e| e.disclosure
                == Disclosure::ExplicitlySelected
                && e.scenario
                    .validity
                    .iter()
                    .any(|p| serde_json::to_value(p).unwrap() == case["property"])));
            assert_eq!(journal(&root)["provider"]["issued"], false);
        }
        assert_eq!(store.load().unwrap(), saved);
        let receipts: Vec<_> = fs::read_dir(root.join("jobs"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_type().unwrap().is_dir())
            .map(|e| {
                transport
                    .reconcile(e.file_name().to_str().unwrap())
                    .unwrap()
            })
            .collect();
        assert_eq!(
            receipts
                .iter()
                .filter(|r| r.state == product_provider::JobState::TransportValidated)
                .count(),
            1
        );
        assert!(receipts
            .iter()
            .filter(|r| r.state != product_provider::JobState::TransportValidated)
            .all(|r| r.state == product_provider::JobState::Prepared && r.consent.is_none()));
    }
}

#[cfg(unix)]
#[test]
fn failed_changed_feature_replay_revokes_the_actual_old_ticket_not_only_the_button() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let (mut studio, store, mut h, _) = accepted_feature_case(&root, TestHooks::default());
    let saved = store.load().unwrap();
    studio.test_feature_trial(SemanticInput::Invoke {
        action: "no-longer-valid-binding".into(),
        arguments: Values::new(),
    });
    settle(&mut studio);
    assert!(
        studio.test_notice().contains("unknown"),
        "The changed replay must actually fail: {}",
        studio.test_notice()
    );
    assert!(journal(&root)["discovery"]["accepted_case"].is_null());
    assert!(
        !frame(&mut h, &mut studio).controls["studio.feature.accept.observed.selected_count"]
            .enabled
    );
    studio.test_reissue_displayed_feature_acceptance("observed.selected_count");
    settle(&mut studio);
    assert!(
        journal(&root)["discovery"]["accepted_case"].is_null(),
        "A late command resurrected the actual old ticket after failed changed replay"
    );
    assert_eq!(store.load().unwrap(), saved);
}

#[cfg(unix)]
#[test]
fn ordinary_new_state_typed_case_reopens_without_requiring_current_to_run_the_new_state() {
    use std::sync::atomic::Ordering;
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let hooks = TestHooks::default();
    let stopped = hooks.stopped.clone();
    let (studio, store, mut h, case) = accepted_feature_case_with_state(&root, hooks, true);
    let saved = store.load().unwrap();
    assert!(saved.editable_scope_context().unwrap().is_none());
    assert!(!saved.session.values.contains_key("feature_runs"));
    assert_eq!(
        case["scenario"]["session"]["values"]["feature_runs"],
        serde_json::json!({"kind":"integer","value":0})
    );
    drop(studio);
    let start = Instant::now();
    while !stopped.load(Ordering::Acquire) {
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(2));
    }
    let transport = product_provider::ProviderTransport::new_fixture(
        root.join("jobs"),
        product_provider::ProviderKind::Codex,
        root.join("discover-fixture.py"),
        root.join("fixture-home"),
    )
    .unwrap();
    let mut studio = ProductStudio::testing(root.clone(), Some(transport), TestHooks::default());
    settle(&mut studio);
    click(&mut h, &mut studio, "studio.reopen-discovery");
    assert_eq!(
        studio.test_page(),
        "change",
        "The ordinary independently checked candidate must remain reviewable after restart: {}",
        studio.test_notice()
    );
    assert_no_old_change_choices(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.discover");
    assert_eq!(studio.test_page(), "consent", "{}", studio.test_notice());
    assert_eq!(journal(&root)["discovery"]["accepted_case"], case);
    assert_eq!(journal(&root)["provider"]["issued"], false);
    assert_eq!(store.load().unwrap(), saved);
}

#[cfg(unix)]
#[test]
fn back_to_discovery_report_keeps_unsubmitted_copied_form_edits() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let (mut studio, store, mut h) = discovered(&root);
    let saved = store.load().unwrap();
    let copy = first_control(&mut h, &mut studio, "studio.discovery.copy.");
    click(&mut h, &mut studio, &copy);
    click(&mut h, &mut studio, "discovery-left.navigate.new_person");
    click(&mut h, &mut studio, "discovery-left.field.name");
    h.text("Unsubmitted copied person");
    let dirty = frame(&mut h, &mut studio);
    assert!(
        dirty
            .text
            .iter()
            .any(|s| s.contains("form has changes that have not been submitted")),
        "The actual renderer must own an unsubmitted edit before navigation"
    );
    let point = dirty.controls["studio.discovery.report"].rect.center();
    h.press_at(point);
    frame(&mut h, &mut studio);
    h.release_at(point);
    frame(&mut h, &mut studio);
    settle(&mut studio);
    assert_eq!(
        studio.test_page(),
        "discovery-copy",
        "Returning to the report must not silently discard copied form edits"
    );
    assert!(!frame(&mut h, &mut studio).controls["studio.discovery.report"].enabled);
    click(&mut h, &mut studio, "discovery-left.field.area");
    h.text("west");
    frame(&mut h, &mut studio);
    click(&mut h, &mut studio, "discovery-left.submit");
    let mut copied_ids = vec![];
    for run in studio.test_discovery_runs().unwrap() {
        let row = run
            .retained_records
            .iter()
            .find(|record| record.values.get("name") == Some(&string("Unsubmitted copied person")));
        assert!(row.is_some(), "The submitted name buffer was lost: {run:?}");
        let row = row.unwrap();
        assert_eq!(row.values.get("area"), Some(&string("west")));
        assert!(run
            .history
            .events
            .iter()
            .any(|event| event.action == "add_person"
                && event.changes.iter().any(|change| change.record == row.id
                    && change.after.get("name") == Some(&string("Unsubmitted copied person"))
                    && change.after.get("area") == Some(&string("west")))));
        copied_ids.push(row.id.clone());
    }
    assert_eq!(copied_ids[0], copied_ids[1]);
    click(&mut h, &mut studio, "discovery-left.navigate.people");
    for run in studio.test_discovery_runs().unwrap() {
        assert_eq!(
            run.observation.controls.get("search_input"),
            Some(&string("Ada"))
        );
    }
    // The checked witness left a real Ada filter in both copies. Reveal the
    // newly submitted name through the ordinary control, without changing it.
    click(&mut h, &mut studio, "discovery-left.control.search_input");
    h.key(
        egui::Key::A,
        true,
        egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        },
    );
    h.key(egui::Key::A, false, egui::Modifiers::NONE);
    h.text("Unsubmitted copied person");
    frame(&mut h, &mut studio);
    settle(&mut studio);
    for run in studio.test_discovery_runs().unwrap() {
        assert_eq!(
            run.observation.controls.get("search_input"),
            Some(&string("Unsubmitted copied person"))
        );
        assert!(run
            .observation
            .rows
            .iter()
            .any(|row| row.cells.get("name") == Some(&string("Unsubmitted copied person"))));
    }
    assert!(
        frame(&mut h, &mut studio)
            .text
            .iter()
            .filter(|s| s.contains("Unsubmitted copied person"))
            .count()
            >= 2,
        "The preserved input must be visible on both copied lists"
    );
    click(&mut h, &mut studio, "studio.discovery.report");
    assert_eq!(studio.test_page(), "discovery");
    assert_eq!(store.load().unwrap(), saved);
}

#[cfg(unix)]
#[test]
fn actual_row_checkbox_requirement_checks_its_selection_callback_before_discovering_a_choice() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let transport = response_transport(&root);
    let mut current = filtered();
    current["views"][0]["kind"]["selection"] = serde_json::Value::Null;
    let store = ProductStore::create(root.join("tool"), &capture(current), 20000).unwrap();
    store
        .apply(
            0,
            "actual-saved-person",
            &add("Ada"),
            RuntimeLimits::default(),
        )
        .unwrap();
    let saved = store.load().unwrap();
    let mut candidate = filtered();
    candidate["state"].as_array_mut().unwrap().push(serde_json::json!({"id":"selection_events","label":"Selection events","value_type":{"kind":"integer"},"initial":{"kind":"integer","value":0}}));
    let one = serde_json::json!({"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":1}});
    candidate["actions"].as_array_mut().unwrap().push(serde_json::json!({"id":"selection_changed","label":"Record this selection","parameters":{"value":{"kind":"list","item":{"kind":"reference","entity":"person"}}},"guards":[],"steps":[{"kind":"set_state","state":"selection_events","value":{"kind":"add","left":{"kind":"state","state":"selection_events"},"right":one}}],"ensures":[]}));
    candidate["views"][0]["kind"]["selection"]["on_change"] =
        serde_json::json!("selection_changed");
    candidate["observables"].as_array_mut().unwrap().push(serde_json::json!({"id":"selection_events","label":"Selection events","value":{"kind":"state","state":"selection_events"}}));
    capture(candidate.clone()).validate().unwrap();
    let mut studio = ProductStudio::testing(root.clone(), Some(transport), TestHooks::default());
    settle(&mut studio);
    studio.test_open(root.join("tool"));
    settle(&mut studio);
    studio.test_modify("Record a checked selection result through the row checkbox");
    settle(&mut studio);
    let modify = studio
        .test_prepared_request()
        .unwrap_or_else(|| panic!("{}", studio.test_notice()))
        .clone();
    configure_response(
        &root,
        &DevelopmentResponse {
            version: 1,
            request_digest: modify.identity().unwrap(),
            candidates: vec![GeneratedCandidate {
                id: "changed".into(),
                source_json: serde_json::to_string(&candidate).unwrap(),
            }],
            hypotheses: vec![],
            evolutions: vec![],
            unsupported: vec![],
        },
    );
    studio.test_consent();
    settle(&mut studio);
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    let mut h = egui_harness::EguiHarness::new(egui::vec2(1500.0, 5000.0));
    click(&mut h, &mut studio, "studio.feature-example");
    let checkbox = first_control(&mut h, &mut studio, "feature-candidate.select.person.");
    click(&mut h, &mut studio, &checkbox);
    assert!(journal(&root)["discovery"]["accepted_case"].is_null());
    click(
        &mut h,
        &mut studio,
        "studio.feature.accept.observed.selection_events",
    );
    let case = journal(&root)["discovery"]["accepted_case"].clone();
    assert_eq!(
        case["required_actions"],
        serde_json::json!(["selection_changed"])
    );
    assert_eq!(
        case["property"]["predicate"]["right"]["value"],
        serde_json::json!({"kind":"integer","value":1})
    );
    let mut scene: ScenarioSpec = serde_json::from_value(case["scenario"].clone()).unwrap();
    assert!(
        matches!(&scene.inputs[0], SemanticInput::Control { control, value:DataValue::List { item_type:Type::Reference { entity }, items }, .. } if control=="pick" && entity=="person" && items.len()==1)
    );
    scene.inputs.push(SemanticInput::Control {
        view: "people".into(),
        control: "pick".into(),
        value: DataValue::List {
            item_type: Type::reference("person"),
            items: vec![],
        },
    });
    scene.inputs.push(SemanticInput::Observe {
        point: "done".into(),
    });
    click(&mut h, &mut studio, "studio.feature.back");
    click(&mut h, &mut studio, "studio.discover");
    let request = studio.test_prepared_request().unwrap().clone();
    let target = &request.sources[1];
    scene.validate(&target.program).unwrap();
    assert!(scene.validate(&saved.program().unwrap().program).is_err());
    let mut once = candidate;
    once["actions"][3]["steps"][0]["value"] = one;
    capture(once.clone()).validate().unwrap();
    configure_response(
        &root,
        &DevelopmentResponse {
            version: 1,
            request_digest: request.identity().unwrap(),
            candidates: vec![
                GeneratedCandidate {
                    id: "each-event".into(),
                    source_json: String::from_utf8(target.source_bytes.clone()).unwrap(),
                },
                GeneratedCandidate {
                    id: "once".into(),
                    source_json: serde_json::to_string(&once).unwrap(),
                },
            ],
            hypotheses: vec![ChoiceHypothesis {
                id: "selection-callback-choice".into(),
                statement:
                    "Repeated actual selections may count each event or record a one-time result"
                        .into(),
                kind: HypothesisKind::UnresolvedChoice,
                action: "selection_changed".into(),
                observable: "selection_events".into(),
                sources: vec![SourceLocus {
                    relative_path: target.binding.program_path.clone(),
                    raw_digest: target.artifact.raw_digest.clone(),
                    pointer: "/actions/3/steps/0".into(),
                }],
                alternatives: vec!["each-event".into(), "once".into()],
                related_decisions: vec![],
                scenario_json: serde_json::to_string(&scene).unwrap(),
                unknowns: vec![],
            }],
            evolutions: vec![],
            unsupported: vec![],
        },
    );
    click(&mut h, &mut studio, "studio.consent");
    assert_eq!(studio.test_page(), "discovery", "{}", studio.test_notice());
    let text = frame(&mut h, &mut studio).text.join("\n");
    assert!(text.contains("1 coherent questions"), "The independently accepted actual checkbox callback must have positive coverage and produce its genuine checked choice: {text}");
    assert_eq!(store.load().unwrap(), saved);
}

#[cfg(unix)]
fn history_keeps_same_run_report(saved_choice: bool) {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let (mut studio, store, mut h) = discovered(&root);
    let complete_report = format!(
        "{:?}",
        studio
            .test_discovery_report()
            .expect("Actual complete current-run report")
    );
    let lineage = journal(&root)["discovery"].clone();
    if saved_choice {
        let copy = first_control(&mut h, &mut studio, "studio.discovery.copy.");
        click(&mut h, &mut studio, &copy);
        click(&mut h, &mut studio, "studio.discovery.choose.5");
        assert!(matches!(
            store
                .load()
                .unwrap()
                .decisions
                .decisions
                .last()
                .unwrap()
                .outcome,
            DecisionOutcome::Deferred
        ));
    } else {
        click(&mut h, &mut studio, "studio.return");
        click(&mut h, &mut studio, "daily.navigate.new_person");
        click(&mut h, &mut studio, "daily.field.name");
        h.text("Later work before inspecting history");
        frame(&mut h, &mut studio);
        click(&mut h, &mut studio, "daily.field.area");
        h.text("north");
        frame(&mut h, &mut studio);
        click(&mut h, &mut studio, "daily.submit");
        assert!(store
            .load()
            .unwrap()
            .data
            .records
            .iter()
            .any(|row| row.values.get("name")
                == Some(&string("Later work before inspecting history"))));
    }
    assert_eq!(studio.test_page(), "daily");
    let saved = store.load().unwrap();
    // Establish that the same report is still present after the genuine Basis
    // change, before isolating the non-destructive History navigation.
    click(&mut h, &mut studio, "studio.reopen-discovery");
    assert_eq!(
        format!(
            "{:?}",
            studio
                .test_discovery_report()
                .expect("Later work retains the complete same-run report")
        ),
        complete_report
    );
    assert!(frame(&mut h, &mut studio)
        .text
        .iter()
        .any(|line| line.contains("report is stale")));
    click(&mut h, &mut studio, "studio.return");
    assert_eq!(journal(&root)["discovery"], lineage);
    click(&mut h, &mut studio, "studio.history");
    assert_eq!(studio.test_page(), "history");
    assert_eq!(
        journal(&root)["discovery"],
        lineage,
        "History must not replace the discovery lineage"
    );
    assert_eq!(store.load().unwrap(), saved);
    click(&mut h, &mut studio, "intention-return");
    click(&mut h, &mut studio, "studio.reopen-discovery");
    let report = studio.test_discovery_report().expect("History navigation must retain the complete current-run report, rather than claim it ran in an earlier host session");
    assert_eq!(format!("{report:?}"), complete_report);
    let trace = frame(&mut h, &mut studio);
    assert!(trace
        .text
        .iter()
        .any(|line| line.contains("report is stale")));
    assert!(!trace
        .text
        .iter()
        .any(|line| line.contains("earlier host session")));
    assert!(
        !trace.controls.iter().any(|(key, control)| control.enabled
            && (key.starts_with("studio.discovery.example.")
                || key.starts_with("studio.discovery.copy.")
                || key.starts_with("studio.discovery.choose."))),
        "A retained stale report must not restore copy or choice authority"
    );
    assert_eq!(journal(&root)["discovery"], lineage);
    assert_eq!(store.load().unwrap(), saved);
}
#[cfg(unix)]
#[test]
fn history_navigation_keeps_complete_stale_report_after_later_business_work() {
    history_keeps_same_run_report(false);
}
#[cfg(unix)]
#[test]
fn history_navigation_keeps_complete_stale_report_after_pending_choice() {
    history_keeps_same_run_report(true);
}

#[cfg(unix)]
fn save_report_pending(
    h: &mut egui_harness::EguiHarness,
    studio: &mut ProductStudio,
    store: &ProductStore,
    choice: usize,
) -> String {
    let copy = first_control(h, studio, "studio.discovery.copy.");
    click(h, studio, &copy);
    click(h, studio, &format!("studio.discovery.choose.{choice}"));
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    let saved = store.load().unwrap();
    let decision = saved.decisions.decisions.last().unwrap();
    assert_eq!(decision.status, DecisionStatus::Pending);
    decision.id.clone()
}
#[cfg(unix)]
fn save_report_later_work(h: &mut egui_harness::EguiHarness, studio: &mut ProductStudio) {
    click(h, studio, "daily.navigate.new_person");
    click(h, studio, "daily.field.name");
    h.text("Work after the complete report");
    frame(h, studio);
    click(h, studio, "daily.field.area");
    h.text("east");
    frame(h, studio);
    click(h, studio, "daily.submit");
}
#[cfg(unix)]
fn assert_stale_report_has_no_authority(
    h: &mut egui_harness::EguiHarness,
    studio: &mut ProductStudio,
) {
    assert_eq!(studio.test_page(), "discovery", "{}", studio.test_notice());
    let trace = frame(h, studio);
    assert!(
        trace
            .text
            .iter()
            .any(|line| line.contains("report is stale")),
        "{:?}",
        trace.text
    );
    assert!(!trace.controls.iter().any(|(key, control)| control.enabled
        && (key.starts_with("studio.discovery.example.")
            || key.starts_with("studio.discovery.copy.")
            || key.starts_with("studio.discovery.choose."))));
}
#[cfg(unix)]
fn report_job_artifacts(root: &Path) -> std::collections::BTreeMap<String, serde_json::Value> {
    fs::read_dir(root.join("jobs"))
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            assert!(entry.file_type().unwrap().is_dir());
            let mut files = serde_json::Map::new();
            for name in [
                "request.json",
                "stdin.json",
                "schema.json",
                "receipt.json",
                "fixture-invocation.json",
                "result.json",
                "final.json",
            ] {
                let path = entry.path().join(name);
                files.insert(
                    name.into(),
                    if path.exists() {
                        serde_json::Value::String(fs::read_to_string(path).unwrap())
                    } else {
                        serde_json::Value::Null
                    },
                );
            }
            (
                entry.file_name().into_string().unwrap(),
                serde_json::Value::Object(files),
            )
        })
        .collect()
}
#[cfg(unix)]
fn report_retention_navigation(path: usize) {
    use std::sync::{atomic::Ordering, Arc};
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let pause = Arc::new(product_studio::TestPause::default());
    pause.release.store(true, Ordering::Release);
    let hooks = TestHooks {
        before_discovery_evaluation: Some(pause.clone()),
        ..Default::default()
    };
    let (mut studio, store, mut h) = discovered_with_hooks(&root, hooks);
    let complete_report = format!(
        "{:?}",
        studio
            .test_discovery_report()
            .expect("An actual complete report was published")
    );
    let lineage = journal(&root)["discovery"].clone();
    let jobs = report_job_artifacts(&root);
    assert_eq!(jobs.len(), 2);
    assert!(
        jobs.values()
            .all(|job| job["fixture-invocation.json"].is_string()),
        "Both initial Modify and Discover must actually invoke the synthetic transport"
    );
    eprintln!(
        "Report navigation {path} job artifacts before: {}",
        serde_json::to_string(&jobs).unwrap()
    );
    let mut expected_jobs = jobs.clone();
    if path < 3 {
        let decision =
            save_report_pending(&mut h, &mut studio, &store, if path == 2 { 3 } else { 5 });
        let saved = store.load().unwrap();
        match path {
            0 => {
                click(&mut h, &mut studio, &format!("studio.resume.{decision}"));
                assert_eq!(
                    studio.test_page(),
                    "discovery-copy",
                    "{}",
                    studio.test_notice()
                );
                assert_eq!(store.load().unwrap(), saved);
                click(&mut h, &mut studio, "studio.return");
            }
            1 => {
                click(&mut h, &mut studio, "studio.close");
                assert_eq!(studio.test_page(), "home");
                let recent = first_control(&mut h, &mut studio, "studio.recent.");
                click(&mut h, &mut studio, &recent);
            }
            2 => {
                click(&mut h, &mut studio, "studio.history");
                click(&mut h, &mut studio, &format!("intention-need-{decision}"));
                click(&mut h, &mut studio, "intention-reconcile");
                assert_eq!(studio.test_page(), "consent", "{}", studio.test_notice());
                assert_eq!(
                    studio.test_prepared_request().unwrap().operation,
                    DevelopmentOperation::Reconcile
                );
                assert_eq!(journal(&root)["provider"]["issued"], false);
                assert_eq!(journal(&root)["discovery"], lineage);
                let request_id = studio.test_prepared_request().unwrap().id.clone();
                expected_jobs = report_job_artifacts(&root);
                assert_eq!(expected_jobs.len(), jobs.len() + 1);
                for (id, before) in &jobs {
                    assert_eq!(&expected_jobs[id], before);
                }
                let prepared = &expected_jobs[&request_id];
                let receipt: product_provider::JobReceipt =
                    serde_json::from_str(prepared["receipt.json"].as_str().unwrap()).unwrap();
                assert_eq!(receipt.request_id, request_id);
                assert_eq!(receipt.state, product_provider::JobState::Prepared);
                assert!(
                    receipt.consent.is_none()
                        && receipt.started_at_unix_ms.is_none()
                        && receipt.finished_at_unix_ms.is_none()
                        && receipt.exit_code.is_none()
                        && receipt.session_id.is_none()
                        && receipt.result_digest.is_none()
                );
                assert!(
                    prepared["fixture-invocation.json"].is_null()
                        && prepared["result.json"].is_null()
                        && prepared["final.json"].is_null()
                );
                eprintln!(
                    "Report navigation Reconcile prepared job artifacts: {}",
                    serde_json::to_string(&expected_jobs).unwrap()
                );
                eprintln!(
                    "Report navigation Reconcile prepared journal provider: {}",
                    journal(&root)["provider"]
                );
                click(&mut h, &mut studio, "studio.return");
                assert!(journal(&root)["provider"].is_null());
                assert_eq!(
                    report_job_artifacts(&root),
                    expected_jobs,
                    "Declining cannot submit or alter any prepared or earlier invocation"
                );
            }
            _ => unreachable!(),
        }
        assert_eq!(store.load().unwrap(), saved);
    } else {
        click(&mut h, &mut studio, "studio.return");
        let saved = store.load().unwrap();
        pause.reached.store(false, Ordering::Release);
        pause.release.store(false, Ordering::Release);
        let trace = frame(&mut h, &mut studio);
        let button = &trace.controls["studio.reopen-discovery"];
        assert!(button.enabled);
        h.press_at(button.rect.center());
        frame(&mut h, &mut studio);
        h.release_at(button.rect.center());
        frame(&mut h, &mut studio);
        let start = Instant::now();
        while !pause.reached.load(Ordering::Acquire) {
            studio.poll();
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "Local evaluation did not reach its actual post-State-replacement boundary: {}",
                studio.test_notice()
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        click(&mut h, &mut studio, "studio.cancel");
        pause.release.store(true, Ordering::Release);
        assert!(
            studio.test_notice().to_ascii_lowercase().contains("cancel"),
            "{}",
            studio.test_notice()
        );
        assert_eq!(store.load().unwrap(), saved);
        assert_eq!(journal(&root)["discovery"], lineage);
        save_report_later_work(&mut h, &mut studio);
        assert!(store
            .load()
            .unwrap()
            .data
            .records
            .iter()
            .any(|row| row.values.get("name") == Some(&string("Work after the complete report"))));
    }
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    let saved = store.load().unwrap();
    assert_eq!(journal(&root)["discovery"], lineage);
    click(&mut h, &mut studio, "studio.reopen-discovery");
    assert_stale_report_has_no_authority(&mut h, &mut studio);
    assert_eq!(format!("{:?}", studio.test_discovery_report().expect("Same-record navigation must retain the exact current-run report independently of transient authority")), complete_report);
    assert_eq!(journal(&root)["discovery"], lineage);
    assert_eq!(store.load().unwrap(), saved);
    let after_jobs = report_job_artifacts(&root);
    eprintln!(
        "Report navigation {path} job artifacts after: {}",
        serde_json::to_string(&after_jobs).unwrap()
    );
    assert_eq!(after_jobs, expected_jobs, "Local navigation must not start or resend a provider request; Reconcile may retain only its exact unissued preparation");
}
#[cfg(unix)]
#[test]
fn pending_pair_resume_keeps_the_complete_same_run_report_without_choice_authority() {
    report_retention_navigation(0);
}
#[cfg(unix)]
#[test]
fn same_tool_close_and_reopen_keeps_the_complete_same_run_report() {
    report_retention_navigation(1);
}
#[cfg(unix)]
#[test]
fn declined_reconcile_disclosure_keeps_the_original_complete_report() {
    report_retention_navigation(2);
}
#[cfg(unix)]
#[test]
fn cancelled_local_discovery_reevaluation_keeps_the_previous_complete_report() {
    report_retention_navigation(3);
}
#[cfg(unix)]
#[test]
fn a_fresh_host_process_has_no_cached_full_report_or_stale_choice_authority() {
    use std::sync::atomic::Ordering;
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let hooks = TestHooks::default();
    let stopped = hooks.stopped.clone();
    let (mut studio, store, mut h) = discovered_with_hooks(&root, hooks);
    assert!(studio.test_discovery_report().is_some());
    save_report_pending(&mut h, &mut studio, &store, 5);
    let saved = store.load().unwrap();
    let lineage = journal(&root)["discovery"].clone();
    let navigation_jobs = report_job_artifacts(&root);
    drop(studio);
    let start = Instant::now();
    while !stopped.load(Ordering::Acquire) {
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(2));
    }
    let mut restarted = ProductStudio::testing(root.clone(), None, TestHooks::default());
    settle(&mut restarted);
    click(&mut h, &mut restarted, "studio.reopen-discovery");
    assert_stale_report_has_no_authority(&mut h, &mut restarted);
    assert_missing_report_details_do_not_invent_a_previous_session(&mut h, &mut restarted);
    assert!(restarted.test_discovery_report().is_none());
    assert_eq!(journal(&root)["discovery"], lineage);
    assert_eq!(store.load().unwrap(), saved);
    assert_eq!(
        report_job_artifacts(&root),
        navigation_jobs,
        "Navigation cannot add, start or resend any provider job"
    );
}
#[cfg(unix)]
#[test]
fn a_different_saved_tool_cannot_inherit_or_return_the_cached_report() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let other_path = root.join("other-tool");
    let other = ProductStore::create(other_path.clone(), &capture(organizer()), 20000).unwrap();
    let other_saved = other.load().unwrap();
    let hooks = TestHooks {
        folder_choice: Some(other_path),
        ..Default::default()
    };
    let (mut studio, store, mut h) = discovered_with_hooks(&root, hooks);
    save_report_pending(&mut h, &mut studio, &store, 5);
    let saved = store.load().unwrap();
    let lineage = journal(&root)["discovery"].clone();
    let navigation_jobs = report_job_artifacts(&root);
    click(&mut h, &mut studio, "studio.close");
    let original_recent = first_control(&mut h, &mut studio, "studio.recent.");
    click(&mut h, &mut studio, "studio.open");
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    click(&mut h, &mut studio, "studio.reopen-discovery");
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    assert!(
        studio.test_notice().contains("another saved tool"),
        "{}",
        studio.test_notice()
    );
    assert!(studio.test_discovery_report().is_none());
    click(&mut h, &mut studio, "studio.close");
    click(&mut h, &mut studio, &original_recent);
    click(&mut h, &mut studio, "studio.reopen-discovery");
    assert_stale_report_has_no_authority(&mut h, &mut studio);
    assert_missing_report_details_do_not_invent_a_previous_session(&mut h, &mut studio);
    assert!(
        studio.test_discovery_report().is_none(),
        "A committed different-tool replacement must invalidate the single report cache"
    );
    assert_eq!(journal(&root)["discovery"], lineage);
    assert_eq!(store.load().unwrap(), saved);
    assert_eq!(other.load().unwrap(), other_saved);
    assert_eq!(
        report_job_artifacts(&root),
        navigation_jobs,
        "Navigation cannot add, start or resend any provider job"
    );
}
#[cfg(unix)]
#[test]
fn a_new_original_modify_record_cannot_borrow_the_previous_full_report() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let (mut studio, store, mut h) = discovered(&root);
    let original = journal(&root)["discovery"].clone();
    assert!(studio.test_discovery_report().is_some());
    click(&mut h, &mut studio, "studio.return");
    click(&mut h, &mut studio, "studio.change-need");
    h.key(
        egui::Key::A,
        true,
        egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        },
    );
    frame(&mut h, &mut studio);
    h.key(egui::Key::A, false, egui::Modifiers::NONE);
    h.text("A different original change must have its own report");
    frame(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.modify");
    assert_eq!(studio.test_page(), "consent", "{}", studio.test_notice());
    assert_eq!(
        studio.test_prepared_request().unwrap().operation,
        DevelopmentOperation::Modify
    );
    assert_ne!(journal(&root)["discovery"], original);
    click(&mut h, &mut studio, "studio.consent");
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    let navigation_jobs = report_job_artifacts(&root);
    let replacement = journal(&root)["discovery"].clone();
    assert!(replacement["discover"].is_null());
    click(&mut h, &mut studio, "studio.return");
    save_report_later_work(&mut h, &mut studio);
    let saved = store.load().unwrap();
    click(&mut h, &mut studio, "studio.reopen-discovery");
    assert_stale_report_has_no_authority(&mut h, &mut studio);
    assert_missing_report_details_do_not_invent_a_previous_session(&mut h, &mut studio);
    assert!(studio.test_discovery_report().is_none());
    assert_eq!(journal(&root)["discovery"], replacement);
    assert_eq!(store.load().unwrap(), saved);
    assert_eq!(
        report_job_artifacts(&root),
        navigation_jobs,
        "Navigation cannot add, start or resend any provider job"
    );
}

#[cfg(unix)]
fn assert_missing_report_details_do_not_invent_a_previous_session(
    h: &mut egui_harness::EguiHarness,
    studio: &mut ProductStudio,
) {
    assert!(studio.test_discovery_report().is_none());
    click(h, studio, "studio.discovery.details");
    let expanded = frame(h, studio);
    assert!(
        expanded
            .controls
            .contains_key("studio.discovery.detail-text"),
        "The actual read-only details must be expanded"
    );
    assert!(expanded
        .text
        .iter()
        .any(|text| text.contains("No complete report")));
    assert!(
        !expanded
            .text
            .iter()
            .any(|text| text.contains("earlier host session")),
        "Absent evidence must not claim a previous host session: {:?}",
        expanded
            .text
            .iter()
            .filter(|text| text.contains("No complete report"))
            .collect::<Vec<_>>()
    );
}
#[cfg(unix)]
#[test]
fn copied_experience_return_discloses_dirty_discard_and_keeps_saved_work_unchanged() {
    for mode in 0..3 {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let (mut studio, store, mut h) = discovered(&root);
        match mode {
            0 => {
                let example = first_control(&mut h, &mut studio, "studio.discovery.example.");
                click(&mut h, &mut studio, &example);
            }
            1 => {
                let copy = first_control(&mut h, &mut studio, "studio.discovery.copy.");
                click(&mut h, &mut studio, &copy);
            }
            2 => {
                let decision = save_report_pending(&mut h, &mut studio, &store, 5);
                click(&mut h, &mut studio, &format!("studio.resume.{decision}"));
            }
            _ => unreachable!(),
        }
        let saved = store.load().unwrap();
        let lineage = journal(&root)["discovery"].clone();
        let jobs = report_job_artifacts(&root);
        let clean = frame(&mut h, &mut studio);
        assert!(clean.text.iter().any(|text| text == "Return to saved work"));
        let pane = if mode == 1 {
            "discovery-right"
        } else {
            "discovery-left"
        };
        click(&mut h, &mut studio, &format!("{pane}.navigate.new_person"));
        click(&mut h, &mut studio, &format!("{pane}.field.name"));
        h.text("A deliberately unsubmitted copied edit");
        frame(&mut h, &mut studio);
        let dirty = frame(&mut h, &mut studio);
        assert!(
            dirty
                .text
                .iter()
                .any(|text| text.contains("form has changes that have not been submitted")),
            "The actual copied renderer must own pending edits: {:?}",
            dirty.text
        );
        assert!(!dirty.controls["studio.discovery.report"].enabled);
        assert!(dirty.controls["studio.return"].enabled);
        assert!(
            dirty
                .text
                .iter()
                .any(|text| text == "Discard copied edits and return"),
            "Returning with unsubmitted copied input must explicitly disclose the discard: {:?}",
            dirty.text
        );
        click(&mut h, &mut studio, "studio.return");
        assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
        assert_eq!(store.load().unwrap(), saved);
        assert_eq!(journal(&root)["discovery"], lineage);
        assert_eq!(report_job_artifacts(&root), jobs);
        assert!(!frame(&mut h, &mut studio)
            .text
            .iter()
            .any(|text| text.contains("form has changes that have not been submitted")));
    }
}
