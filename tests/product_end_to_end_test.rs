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
        // The actual create commits, then its acknowledgement is lost. The
        // host must find the same tool instead of allocating a second path.
        hooks
            .lose_ack
            .store(true, std::sync::atomic::Ordering::Release);
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

#[cfg(unix)]
fn generated(root: &Path, hooks: TestHooks) -> (ProductStudio, EguiHarness) {
    let transport = transport(root, fixture::organizer(), "good");
    let mut s = ProductStudio::testing(root.into(), Some(transport), hooks);
    let mut h = EguiHarness::new(egui::vec2(1000.0, 1200.0));
    settle(&mut h, &mut s);
    fill(
        &mut h,
        &mut s,
        "studio.need",
        "Fictional list for controller regression",
    );
    click(&mut h, &mut s, "studio.prepare");
    settle(&mut h, &mut s);
    click(&mut h, &mut s, "studio.consent");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "draft", "{}", s.test_notice());
    (s, h)
}
#[cfg(unix)]
fn wait_flag(flag: &std::sync::atomic::AtomicBool) {
    let started = Instant::now();
    while !flag.load(std::sync::atomic::Ordering::Acquire) {
        assert!(started.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[cfg(unix)]
#[test]
fn cancelling_save_or_preview_keeps_the_same_returned_draft() {
    use std::sync::{atomic::Ordering, Arc};
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let save = Arc::new(product_studio::TestPause::default());
    let preview = Arc::new(product_studio::TestPause::default());
    let hooks = TestHooks {
        before_commit: Some(save.clone()),
        before_preview: Some(preview.clone()),
        ..TestHooks::default()
    };
    let (mut s, mut h) = generated(&root, hooks);
    let source = s.test_runtime().unwrap().program.clone();
    click(&mut h, &mut s, "draft.navigate.new_person");
    wait_flag(&preview.reached);
    assert!(s.test_cancel());
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "draft");
    assert_eq!(s.test_runtime().unwrap().observation.view, "people");
    assert_eq!(s.test_runtime().unwrap().program, source);
    click(&mut h, &mut s, "studio.keep");
    wait_flag(&save.reached);
    assert!(s.test_cancel());
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "draft");
    assert_eq!(s.test_runtime().unwrap().program, source);
    preview.release.store(true, Ordering::Release);
    save.release.store(true, Ordering::Release);
    click(&mut h, &mut s, "studio.keep");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "daily");
}
#[cfg(unix)]
#[test]
fn a_host_dialog_does_not_acknowledge_an_unsent_queued_renderer_input() {
    use std::sync::{atomic::Ordering, Arc};
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let pause = Arc::new(product_studio::TestPause::default());
    let hooks = TestHooks {
        before_preview: Some(pause.clone()),
        ..TestHooks::default()
    };
    let (mut s, mut h) = generated(&root, hooks);
    fill(&mut h, &mut s, "draft.control.search_input", "a");
    wait_flag(&pause.reached);
    h.text("b");
    let trace = frame(&mut h, &mut s);
    let button = trace.controls["studio.destination"].rect.center();
    pause.release.store(true, Ordering::Release);
    let start = Instant::now();
    while s.is_busy() {
        s.poll();
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(2));
    }
    h.press_at(button);
    h.release_at(button);
    frame(&mut h, &mut s);
    settle(&mut h, &mut s);
    assert_eq!(
        s.test_runtime().unwrap().observation.controls["search_input"],
        fixture::string("ab")
    );
    assert_eq!(s.test_page(), "draft");
}
#[cfg(unix)]
#[test]
fn returned_keyboard_bindings_run_in_both_draft_and_saved_tool() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let (mut s, mut h) = generated(&root, TestHooks::default());
    for prefix in ["draft", "daily"] {
        if prefix == "daily" {
            click(&mut h, &mut s, "studio.keep");
            settle(&mut h, &mut s);
        }
        click(&mut h, &mut s, &format!("{prefix}.navigate.new_person"));
        settle(&mut h, &mut s);
        fill(
            &mut h,
            &mut s,
            &format!("{prefix}.field.name"),
            "One fictional person",
        );
        fill(&mut h, &mut s, &format!("{prefix}.field.area"), "north");
        click(&mut h, &mut s, &format!("{prefix}.submit"));
        settle(&mut h, &mut s);
        click(&mut h, &mut s, &format!("{prefix}.navigate.people"));
        settle(&mut h, &mut s);
        let trace = frame(&mut h, &mut s);
        let selection = trace
            .controls
            .keys()
            .find(|s| s.starts_with(&format!("{prefix}.select.person.")))
            .unwrap()
            .clone();
        click(&mut h, &mut s, &selection);
        settle(&mut h, &mut s);
        h.key(egui::Key::E, true, egui::Modifiers::CTRL);
        frame(&mut h, &mut s);
        h.key(egui::Key::E, false, egui::Modifiers::CTRL);
        frame(&mut h, &mut s);
        settle(&mut h, &mut s);
        assert_eq!(s.test_runtime().unwrap().artifacts.len(), 1);
        assert_eq!(s.test_runtime().unwrap().artifacts[0].rows.len(), 1);
    }
}

#[cfg(unix)]
#[test]
fn host_cancel_keeps_draft_and_destination_then_deliberate_folder_save_works() {
    use std::sync::{atomic::Ordering, Arc};
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let chosen = root.join("chosen-data-folder");
    fs::create_dir(&chosen).unwrap();
    let pause = Arc::new(product_studio::TestPause::default());
    let hooks = TestHooks {
        before_destination: Some(pause.clone()),
        folder_choice: Some(chosen.clone()),
        ..TestHooks::default()
    };
    let (mut s, mut h) = generated(&root, hooks);
    let source = s.test_runtime().unwrap().program.clone();
    let original = s.test_destination().unwrap().to_path_buf();
    click(&mut h, &mut s, "studio.destination");
    wait_flag(&pause.reached);
    click(&mut h, &mut s, "studio.cancel");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "draft");
    assert_eq!(s.test_runtime().unwrap().program, source);
    assert_eq!(s.test_destination(), Some(original.as_path()));
    pause.release.store(true, Ordering::Release);
    click(&mut h, &mut s, "studio.destination");
    settle(&mut h, &mut s);
    assert_eq!(s.test_destination(), Some(chosen.as_path()));
    click(&mut h, &mut s, "studio.keep");
    settle(&mut h, &mut s);
    assert_eq!(s.test_location().unwrap().parent(), Some(chosen.as_path()));
    assert_eq!(s.test_page(), "daily");
}

#[cfg(unix)]
#[test]
fn cancelling_unactivated_save_recovery_preserves_draft_destination_and_pending_identity() {
    use std::sync::{atomic::Ordering, Arc};
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let pause = Arc::new(product_studio::TestPause::default());
    let hooks = TestHooks {
        before_abandon: Some(pause.clone()),
        fail_creation: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        ..TestHooks::default()
    };
    let (mut s, mut h) = generated(&root, hooks);
    let source = s.test_runtime().unwrap().program.clone();
    let destination = s.test_destination().unwrap().to_path_buf();
    click(&mut h, &mut s, "studio.keep");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "draft");
    assert!(s.test_generation_blocked());
    let before: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("studio/session.json")).unwrap()).unwrap();
    click(&mut h, &mut s, "studio.abandon");
    wait_flag(&pause.reached);
    click(&mut h, &mut s, "studio.cancel");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "draft");
    assert_eq!(s.test_runtime().unwrap().program, source);
    assert_eq!(s.test_destination(), Some(destination.as_path()));
    let after: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("studio/session.json")).unwrap()).unwrap();
    assert_eq!(after["pending"], before["pending"]);
    pause.release.store(true, Ordering::Release);
    click(&mut h, &mut s, "studio.abandon");
    settle(&mut h, &mut s);
    click(&mut h, &mut s, "studio.keep");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "daily");
}
#[cfg(unix)]
#[test]
fn failed_recent_registration_does_not_recreate_an_already_saved_tool() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let hooks = TestHooks::default();
    let stopped = hooks.stopped.clone();
    let (mut s, mut h) = generated(&root, hooks);
    fs::write(root.join("recent-tools.json"), b"damaged recent list kept").unwrap();
    click(&mut h, &mut s, "studio.keep");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "daily");
    assert!(s.test_notice().contains("registration"));
    let path = s.test_location().unwrap().to_path_buf();
    assert_eq!(
        fs::read(root.join("recent-tools.json")).unwrap(),
        b"damaged recent list kept"
    );
    assert_eq!(
        fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with("tool-"))
            .count(),
        1
    );
    drop(s);
    wait_flag(&stopped);
    let mut restarted = ProductStudio::testing(root, None, TestHooks::default());
    settle(&mut h, &mut restarted);
    assert_eq!(restarted.test_location(), Some(path.as_path()));
    restarted.test_daily(fixture::add("Saved despite registration failure"));
    settle(&mut h, &mut restarted);
    assert_eq!(restarted.test_runtime().unwrap().retained_records.len(), 1);
}

#[test]
fn format_upgrade_controls_require_explicit_confirmation_and_reopen_fresh_daily_view() {
    use product_contract::*;
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let path = root.join("old-generated");
    let store =
        product_store::ProductStore::create(&path, &fixture::capture(fixture::organizer()), 20000)
            .unwrap();
    let snapshot = store
        .apply(
            0,
            "earlier",
            &fixture::add("Earlier fictional work"),
            RuntimeLimits::default(),
        )
        .unwrap();
    let mut wire = serde_json::to_value(&snapshot).unwrap();
    wire.as_object_mut().unwrap().remove("scope");
    wire["version"] = serde_json::json!(1);
    let digest = canonical_digest(IdentityDomain::Data, &wire).unwrap();
    fs::write(
        path.join(format!("object-{}.json", digest.as_str())),
        canonical_bytes(&wire).unwrap(),
    )
    .unwrap();
    let pointer = canonical_bytes(&serde_json::json!({"magic":"gitmanager.generated-project","version":1,"project_id":snapshot.data.project_id,"revision":snapshot.revision,"object":digest})).unwrap();
    fs::write(path.join("CURRENT"), &pointer).unwrap();
    let hooks = TestHooks {
        folder_choice: Some(path.clone()),
        ..TestHooks::default()
    };
    let mut studio = ProductStudio::testing(root, None, hooks);
    let mut h = EguiHarness::new(egui::vec2(1000.0, 1200.0));
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.open");
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "upgrade");
    let trace = frame(&mut h, &mut studio);
    assert!(trace
        .text
        .iter()
        .any(|s| s.contains("Original snapshots and backups are kept")));
    click(&mut h, &mut studio, "studio.back");
    settle(&mut h, &mut studio);
    assert_eq!(fs::read(path.join("CURRENT")).unwrap(), pointer);
    click(&mut h, &mut studio, "studio.open");
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.upgrade");
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    assert_eq!(
        studio.test_runtime().unwrap().retained_records,
        snapshot.data.records
    );
    assert_eq!(
        product_store::ProductStore::open(path)
            .unwrap()
            .load()
            .unwrap()
            .version,
        2
    );
}
