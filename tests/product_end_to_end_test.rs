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
            start.elapsed() < Duration::from_secs(30),
            "{}",
            s.test_notice()
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    frame(h, s);
}
#[cfg(unix)]
fn transport(
    root: &Path,
    program: serde_json::Value,
    mode: &str,
) -> product_provider::ProviderTransport {
    use std::os::unix::fs::PermissionsExt;
    let executable = root.join("fixture.py");
    let _guard = product_provider::fixture_executable_write_guard();
    let body="{'version':1,'request_digest':json.loads(wire['prompt'])['request_digest'],'candidates':[{'id':'returned-draft','source_json':json.dumps(cfg['program'])}],'hypotheses':[],'evolutions':[],'unsupported':[]}";
    let script = include_str!("fixtures/provider_transport/fake_cli.py").replace(
        "{'passed': True, 'text': wire['prompt'], 'command': 'untrusted-do-not-execute'}",
        body,
    );
    fs::write(&executable, script).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(
        executable.with_extension("json"),
        serde_json::to_vec(&serde_json::json!({"program":program,"mode":mode})).unwrap(),
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
fn equipment_with_intake() -> serde_json::Value {
    use serde_json::json;
    let mut p = fixture::equipment();
    p["actions"].as_array_mut().unwrap().push(json!({"id":"add_asset","label":"Add equipment","parameters":{"name":{"kind":"text"}},"guards":[],"steps":[{"kind":"create","entity":"asset","values":{"name":fixture::var("name")},"bind":"created"}],"ensures":[]}));
    p["views"].as_array_mut().unwrap().push(json!({"id":"intake","label":"Add equipment","kind":{"kind":"form","action":"add_asset","fields":[{"parameter":"name","label":"Name"}],"defaults":{}},"actions":[],"keys":[]}));
    p
}

#[cfg(unix)]
#[test]
fn controls_generate_preview_save_and_reopen_two_distinct_fixture_shapes() {
    for (program, need, form, name) in [
        (
            fixture::organizer(),
            "Help me collect a fictional club list",
            "new_person",
            "Ada",
        ),
        (
            equipment_with_intake(),
            "Help me track fictional equipment loans",
            "intake",
            "Drill",
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let transport = transport(&root, program, "good");
        let hooks = TestHooks::default();
        let stopped = hooks.stopped.clone();
        let mut studio = ProductStudio::testing(root.clone(), Some(transport), hooks);
        let mut h = EguiHarness::new(egui::vec2(1000.0, 1200.0));
        settle(&mut h, &mut studio);
        fill(&mut h, &mut studio, "studio.need", need);
        click(&mut h, &mut studio, "studio.prepare");
        settle(&mut h, &mut studio);
        assert_eq!(studio.test_page(), "consent");
        assert!(frame(&mut h, &mut studio)
            .text
            .iter()
            .any(|s| s.contains("fixture")));
        assert!(!root
            .join("jobs")
            .read_dir()
            .unwrap()
            .filter_map(Result::ok)
            .any(|e| e.path().join("fixture-invocation.json").exists()));
        click(&mut h, &mut studio, "studio.consent");
        settle(&mut h, &mut studio);
        assert_eq!(studio.test_page(), "draft", "{}", studio.test_notice());
        click(&mut h, &mut studio, &format!("draft.navigate.{form}"));
        settle(&mut h, &mut studio);
        fill(&mut h, &mut studio, "draft.field.name", "Rehearsal only");
        if form == "new_person" {
            fill(&mut h, &mut studio, "draft.field.area", "north");
        }
        click(&mut h, &mut studio, "draft.submit");
        settle(&mut h, &mut studio);
        assert_eq!(studio.test_runtime().unwrap().retained_records.len(), 1);
        click(&mut h, &mut studio, "studio.keep");
        settle(&mut h, &mut studio);
        assert_eq!(studio.test_page(), "daily");
        assert!(
            studio.test_runtime().unwrap().retained_records.is_empty(),
            "rehearsal data must not become real work"
        );
        click(&mut h, &mut studio, &format!("daily.navigate.{form}"));
        settle(&mut h, &mut studio);
        fill(&mut h, &mut studio, "daily.field.name", name);
        if form == "new_person" {
            fill(&mut h, &mut studio, "daily.field.area", "north");
        }
        click(&mut h, &mut studio, "daily.submit");
        settle(&mut h, &mut studio);
        let path = studio.test_location().unwrap().to_path_buf();
        let saved = product_store::ProductStore::open(&path)
            .unwrap()
            .load()
            .unwrap();
        assert_eq!(saved.data.records.len(), 1);
        assert!(matches!(
            saved.program().unwrap().binding.producer,
            product_contract::Producer::Fixture { .. }
        ));
        click(&mut h, &mut studio, "studio.close");
        settle(&mut h, &mut studio);
        assert_eq!(studio.test_page(), "home");
        let trace = frame(&mut h, &mut studio);
        let recent = trace
            .controls
            .keys()
            .find(|s| s.starts_with("studio.recent."))
            .unwrap()
            .clone();
        click(&mut h, &mut studio, &recent);
        settle(&mut h, &mut studio);
        assert_eq!(studio.test_runtime().unwrap().retained_records.len(), 1);
        drop(studio);
        let start = Instant::now();
        while !stopped.load(std::sync::atomic::Ordering::Acquire) {
            assert!(start.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(2));
        }
        let mut restarted = ProductStudio::testing(root, None, TestHooks::default());
        settle(&mut h, &mut restarted);
        assert_eq!(restarted.test_page(), "daily");
        assert_eq!(restarted.test_runtime().unwrap().retained_records.len(), 1);
    }
}

#[cfg(unix)]
#[test]
fn provider_failure_keeps_the_need_and_back_discards_prepared_consent() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let transport = transport(&root, fixture::organizer(), "quota");
    let mut s = ProductStudio::testing(root.clone(), Some(transport), TestHooks::default());
    let mut h = EguiHarness::new(egui::vec2(1000.0, 1200.0));
    settle(&mut h, &mut s);
    fill(
        &mut h,
        &mut s,
        "studio.need",
        "Keep my original fictional need",
    );
    click(&mut h, &mut s, "studio.prepare");
    settle(&mut h, &mut s);
    click(&mut h, &mut s, "studio.back");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "home");
    assert_eq!(s.test_need(), "Keep my original fictional need");
    click(&mut h, &mut s, "studio.prepare");
    settle(&mut h, &mut s);
    click(&mut h, &mut s, "studio.consent");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "home");
    assert_eq!(s.test_need(), "Keep my original fictional need");
    assert!(s.test_notice().contains("Quota"), "{}", s.test_notice());
    assert!(product_locations::RecentTools::open(root)
        .unwrap()
        .list()
        .unwrap()
        .is_empty());
}

#[cfg(unix)]
#[test]
fn ownerless_provider_restart_never_ingests_or_resends_and_offline_work_stays_usable() {
    use product_contract::*;
    use product_provider::*;
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let transport = transport(&root, fixture::organizer(), "good");
    let request = DevelopmentRequest {
        version: CONTRACT_VERSION,
        id: "interrupted-provider".into(),
        project_id: "fixture-project".into(),
        operation: DevelopmentOperation::Generate,
        request: "Keep this original fictional request".into(),
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
        decisions: fixture::decisions(),
        unknowns: vec![],
        required_capabilities: Default::default(),
    };
    let options = product_discovery::ProviderOptions::default();
    let wire = product_discovery::encode_request(&request, &options).unwrap();
    transport.prepare(wire).unwrap();
    let mut receipt = transport.inspect(&request.id).unwrap();
    receipt.state = JobState::Running;
    fs::write(
        root.join("jobs/interrupted-provider/receipt.json"),
        serde_json::to_vec(&receipt).unwrap(),
    )
    .unwrap();
    product_studio::test_stage_provider(
        &root,
        request.clone(),
        ProviderKind::Codex,
        CapabilityProfile::DataOnly,
    );
    let path = root.join("offline");
    product_store::ProductStore::create(&path, &fixture::capture(fixture::organizer()), 20000)
        .unwrap();
    let mut studio =
        ProductStudio::testing(root.clone(), Some(transport.clone()), TestHooks::default());
    let mut h = EguiHarness::new(egui::vec2(1000.0, 1200.0));
    settle(&mut h, &mut studio);
    assert!(studio.test_notice().contains("cleanup is unconfirmed"));
    assert_eq!(studio.test_need(), request.request);
    assert_eq!(
        transport.inspect(&request.id).unwrap().state,
        JobState::Interrupted
    );
    assert!(!root
        .join("jobs/interrupted-provider/fixture-invocation.json")
        .exists());
    studio.test_open(path.clone());
    settle(&mut h, &mut studio);
    studio.test_daily(fixture::add("Offline after interrupted generation"));
    settle(&mut h, &mut studio);
    assert_eq!(
        product_store::ProductStore::open(path)
            .unwrap()
            .load()
            .unwrap()
            .data
            .records
            .len(),
        1
    );
    click(&mut h, &mut studio, "studio.close");
    settle(&mut h, &mut studio);
    assert!(!frame(&mut h, &mut studio).controls["studio.prepare"].enabled);
}
