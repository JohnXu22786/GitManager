//! Synthetic egui and dialog fixtures; not native OS dialogs, packaged GUI or live AI evidence.
#[path = "../src/product_studio/change_adapter.rs"]
mod change_adapter;
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
#[path = "fixtures/product_scope/mod.rs"]
mod scope_fixture;
#[path = "../src/tool_proposal_input.rs"]
mod tool_proposal_input;
#[path = "support/discovery_wide.rs"]
mod wide_fixture;
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
    while s.is_busy() || s.test_source_polling() {
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

#[path = "../src/task_delivery.rs"]
mod task_delivery;
#[path = "../src/task_verification.rs"]
mod task_verification;
#[path = "../src/tasks.rs"]
mod tasks;
use product_store::ProductStore;

#[test]
fn saved_daily_work_exposes_actual_task_selection_without_json_copying() {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    let store = ProductStore::create(
        root.join("tool"),
        &fixture::capture(fixture::organizer()),
        20000,
    )
    .unwrap();
    let before = store.load().unwrap();
    let mut studio = ProductStudio::testing(root.clone(), None, TestHooks::default());
    let mut h = EguiHarness::new(egui::vec2(1500.0, 2400.0));
    settle(&mut h, &mut studio);
    studio.test_open(root.join("tool"));
    settle(&mut h, &mut studio);
    let trace = frame(&mut h, &mut studio);
    assert!(
        trace
            .controls
            .get("studio.tasks")
            .is_some_and(|c| c.enabled),
        "saved daily work has no real task-source entry: {:?}",
        trace.text
    );
    click(&mut h, &mut studio, "studio.tasks");
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "tasks");
    assert_eq!(store.load().unwrap(), before);
}

// A real local registry and Git worktree, containing only synthetic examples.
// Registry location is process-wide, so these tests use one explicit lease.
static REGISTRY: std::sync::Mutex<()> = std::sync::Mutex::new(());
struct TaskFixture {
    _lease: std::sync::MutexGuard<'static, ()>,
    _temp: tempfile::TempDir,
    root: std::path::PathBuf,
    data_home: std::path::PathBuf,
    source: std::path::PathBuf,
    record: tasks::TaskRecord,
    store: ProductStore,
}
impl TaskFixture {
    fn new() -> Self {
        Self::with_program(fixture::capture(fixture::organizer()))
    }
    fn with_program(program: product_contract::CapturedProgram) -> Self {
        let lease = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
        static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
        HOME.get_or_init(|| {
            let home = tempfile::tempdir().unwrap();
            for key in ["HOME", "USERPROFILE", "XDG_CONFIG_HOME", "APPDATA"] {
                std::env::set_var(key, fs::canonicalize(home.path()).unwrap());
            }
            home
        });
        let temp = tempfile::tempdir().unwrap();
        let data_home = fs::canonicalize(temp.path()).unwrap();
        let root = data_home.join("GitManager/generated-tools");
        fs::create_dir_all(&root).unwrap();
        let source = data_home.join("source");
        let repo = git2::Repository::init(&source).unwrap();
        let tree = repo
            .find_tree(repo.index().unwrap().write_tree().unwrap())
            .unwrap();
        let who = git2::Signature::now("Synthetic fixture", "fixture@invalid.example").unwrap();
        repo.commit(Some("HEAD"), &who, &who, "Synthetic fixture", &tree, &[])
            .unwrap();
        fs::create_dir(source.join(".gitmanager")).unwrap();
        fs::write(source.join(".gitmanager/product.json"), serde_json::to_vec(&serde_json::json!({
            "version":1,"project_id":program.binding.project_id,"artifact_kind":"generated_app","program_path":"app.json"
        })).unwrap()).unwrap();
        fs::write(source.join("app.json"), program.source_bytes.clone()).unwrap();
        let record = tasks::TaskRecord::from_worktree("Synthetic existing task", &source).unwrap();
        tasks::TaskRegistry::load().add(record.clone()).unwrap();
        let store = ProductStore::create(root.join("tool"), &program, 20000).unwrap();
        Self {
            _lease: lease,
            _temp: temp,
            root,
            data_home,
            source,
            record,
            store,
        }
    }
    fn open(&self) -> (ProductStudio, EguiHarness) {
        self.open_with(TestHooks::default())
    }
    fn open_with(&self, hooks: TestHooks) -> (ProductStudio, EguiHarness) {
        self.open_with_transport(hooks, None)
    }
    fn open_with_transport(
        &self,
        hooks: TestHooks,
        transport: Option<product_provider::ProviderTransport>,
    ) -> (ProductStudio, EguiHarness) {
        let mut studio = ProductStudio::testing(self.root.clone(), transport, hooks);
        let mut h = EguiHarness::new(egui::vec2(1500.0, 2800.0));
        settle(&mut h, &mut studio);
        studio.test_open(self.root.join("tool"));
        settle(&mut h, &mut studio);
        click(&mut h, &mut studio, "studio.tasks");
        settle(&mut h, &mut studio);
        click(
            &mut h,
            &mut studio,
            &format!("studio.task.{}", self.record.id),
        );
        settle(&mut h, &mut studio);
        await_text(&mut h, &mut studio, "Complete source captured");
        (studio, h)
    }
    fn edit_label(&self, label: &str) {
        let mut value = fixture::organizer();
        value["label"] = serde_json::json!(label);
        fs::write(
            self.source.join("app.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
    }
}
impl Drop for TaskFixture {
    fn drop(&mut self) {
        let _ = tasks::TaskRegistry::load().unlink(&self.record.id);
    }
}
fn await_text(h: &mut EguiHarness, studio: &mut ProductStudio, text: &str) {
    let start = Instant::now();
    loop {
        let trace = frame(h, studio);
        if trace.text.iter().any(|line| line.contains(text)) {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "missing {text}: {:?}; {}",
            trace.text,
            studio.test_notice()
        );
        std::thread::sleep(Duration::from_millis(30));
    }
    settle(h, studio);
}

fn await_review_control(h: &mut EguiHarness, studio: &mut ProductStudio) {
    let start = Instant::now();
    loop {
        let trace = frame(h, studio);
        if trace
            .controls
            .get("studio.task-review")
            .is_some_and(|c| c.enabled)
        {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "{:?}; {}",
            trace.text,
            studio.test_notice()
        );
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[test]
fn registry_link_keeps_last_complete_stale_capture_and_daily_work() {
    let task = TaskFixture::new();
    let before = task.store.load().unwrap();
    let (mut studio, mut h) = task.open();
    task.edit_label("A complete external revision");
    await_text(&mut h, &mut studio, "A complete external revision");
    fs::write(task.source.join("app.json"), b"{partial").unwrap();
    await_text(&mut h, &mut studio, "stale");
    let trace = frame(&mut h, &mut studio);
    assert!(trace
        .text
        .iter()
        .any(|s| s.contains("A complete external revision")));
    assert!(!trace
        .controls
        .get("studio.task-review")
        .is_some_and(|c| c.enabled));
    click(&mut h, &mut studio, "studio.tasks-back");
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "daily");
    assert_eq!(task.store.load().unwrap(), before);
}

#[test]
fn disappearing_task_disables_source_review_without_blocking_saved_tool() {
    let task = TaskFixture::new();
    let before = task.store.load().unwrap();
    let (mut studio, mut h) = task.open();
    fill(
        &mut h,
        &mut studio,
        "studio.task-need",
        "A request cannot use a vanished task",
    );
    tasks::TaskRegistry::load().unlink(&task.record.id).unwrap();
    await_text(&mut h, &mut studio, "unavailable");
    let trace = frame(&mut h, &mut studio);
    assert!(!trace
        .controls
        .get("studio.task-review")
        .is_some_and(|c| c.enabled));
    assert!(
        !trace
            .text
            .iter()
            .any(|t| t.contains("Complete source captured")),
        "{:?}",
        trace.text
    );
    assert!(
        trace.text.iter().any(|t| t.contains("stale")),
        "{:?}",
        trace.text
    );
    assert!(!trace.controls["studio.external-request"].enabled);
    click(&mut h, &mut studio, "studio.tasks-back");
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "daily");
    assert_eq!(task.store.load().unwrap(), before);
}

#[test]
fn external_request_contains_exact_current_context_and_declining_never_prepares_job() {
    let task = TaskFixture::new();
    task.store
        .apply(
            0,
            "add",
            &fixture::add("Synthetic Ada"),
            product_contract::RuntimeLimits::default(),
        )
        .unwrap();
    let (mut studio, mut h) = task.open();
    studio.test_task_request("Keep the cumulative selection while filtering");
    settle(&mut h, &mut studio);
    let request = studio.test_prepared_request().expect(studio.test_notice());
    assert_eq!(
        request.context.data_digest,
        Some(task.store.load().unwrap().data.identity().unwrap())
    );
    assert_eq!(
        request.sources[0].binding.task.as_ref().unwrap().task_id,
        task.record.id
    );
    assert_eq!(
        request.sources.last().unwrap(),
        task.store.load().unwrap().program().unwrap()
    );
    assert!(!task.root.join("external-task-jobs").exists());
    let trace = frame(&mut h, &mut studio);
    assert!(trace
        .text
        .iter()
        .any(|s| s.contains("not independently attested")));
    assert!(trace.text.iter().any(|s| s.contains("charges")));
    click(&mut h, &mut studio, "studio.external-decline");
    settle(&mut h, &mut studio);
    assert!(studio.test_prepared_request().is_none());
    let journal: serde_json::Value =
        serde_json::from_slice(&fs::read(task.root.join("studio/session.json")).unwrap()).unwrap();
    assert!(journal["task"]["external"].is_null());
}

#[test]
fn source_changes_after_local_review_cannot_be_adopted() {
    let task = TaskFixture::new();
    let before = task.store.load().unwrap();
    let (mut studio, mut h) = task.open();
    task.edit_label("Revised name to rehearse");
    await_text(&mut h, &mut studio, "Revised name to rehearse");
    studio.test_task_review();
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    // Full-source freshness includes unrelated files, not only executable bytes.
    fs::write(task.source.join("unrelated.txt"), "new independent edit").unwrap();
    studio.test_decide(product_contract::DecisionOutcome::Accept {
        artifact: before.program().unwrap().artifact.program_digest.clone(),
    });
    settle(&mut h, &mut studio);
    assert_eq!(task.store.load().unwrap(), before);
    assert!(
        studio.test_notice().to_lowercase().contains("source"),
        "{}",
        studio.test_notice()
    );
}

#[test]
fn machine_grammar_rejects_arbitrary_paths_and_unregistered_ids() {
    for values in [
        vec!["--product-task", "submit", "--path", "/tmp/job"],
        vec![
            "--product-task",
            "submit",
            "--task",
            "../task",
            "--request",
            "request",
        ],
        vec![
            "--product-task",
            "submit",
            "--task",
            "task",
            "--request",
            "request",
            "--root",
            "/tmp",
        ],
        vec![
            "--product-task",
            "launch",
            "--task",
            "task",
            "--request",
            "request",
        ],
    ] {
        let args = values
            .iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>();
        assert!(product_studio::machine_command(&args).unwrap().is_err());
    }
    let task = TaskFixture::new();
    let (_studio, _h) = task.open();
    assert!(product_studio::test_task_machine(
        &task.root,
        "submit",
        &task.record.id,
        "invented-request"
    )
    .is_err());
    assert!(!task.root.join("external-task-jobs").exists());
}

#[cfg(target_os = "linux")]
struct SyntheticHarness {
    _dir: tempfile::TempDir,
    path: std::ffi::OsString,
    log: std::path::PathBuf,
}
#[cfg(target_os = "linux")]
impl SyntheticHarness {
    fn new(task: &TaskFixture) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("invocation.json");
        let codex = dir.path().join("codex");
        fs::write(&codex, format!(r#"#!/usr/bin/python3
import json, pathlib, sys
journal = json.loads(pathlib.Path({journal:?}).read_text())
pathlib.Path({log:?}).write_text(json.dumps({{"argv": sys.argv, "journal": journal, "synthetic": True}}))
"#, journal=task.root.join("studio/session.json").to_str().unwrap(), log=log.to_str().unwrap())).unwrap();
        fs::set_permissions(&codex, fs::Permissions::from_mode(0o700)).unwrap();
        let terminal = dir.path().join("x-terminal-emulator");
        fs::write(
            &terminal,
            r#"#!/bin/sh
[ "$1" = -e ] && shift
exec "$@"
"#,
        )
        .unwrap();
        fs::set_permissions(&terminal, fs::Permissions::from_mode(0o700)).unwrap();
        let path = std::env::var_os("PATH").unwrap();
        let mut paths = vec![dir.path().to_path_buf()];
        paths.extend(std::env::split_paths(&path));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
        Self {
            _dir: dir,
            path,
            log,
        }
    }
    fn read(&self) -> serde_json::Value {
        let start = Instant::now();
        while !self.log.exists() {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "synthetic Harness was not invoked"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        serde_json::from_slice(&fs::read(&self.log).unwrap()).unwrap()
    }
}
#[cfg(target_os = "linux")]
impl Drop for SyntheticHarness {
    fn drop(&mut self) {
        std::env::set_var("PATH", &self.path);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn synthetic_harness_receives_registered_bundle_then_machine_submission_is_automatically_reviewed()
{
    let task = TaskFixture::new();
    let harness = SyntheticHarness::new(&task);
    let (mut studio, mut h) = task.open_with(TestHooks {
        task_product_executable: Some(env!("CARGO_BIN_EXE_git_manager").into()),
        ..Default::default()
    });
    studio.test_task_request("A harmless display change; do not send actual customer data");
    settle(&mut h, &mut studio);
    let request = studio
        .test_prepared_request()
        .expect(studio.test_notice())
        .clone();
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    let launched = harness.read();
    assert_eq!(launched["synthetic"], true);
    let ext = &launched["journal"]["task"]["external"];
    assert_eq!(ext["launch_attempted"], true);
    assert!(
        !ext["pending"]["ticket"].is_null(),
        "ticket must be durable before invocation"
    );
    assert_eq!(
        ext["pending"]["request"],
        serde_json::to_value(&request).unwrap()
    );
    let prompt = launched["argv"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .as_str()
        .unwrap();
    assert!(prompt.contains(request.identity().unwrap().as_str()));
    assert!(prompt.contains("--product-task"));
    assert!(!prompt.eq(&task.record.title));
    let prepared = std::process::Command::new(env!("CARGO_BIN_EXE_git_manager"))
        .env("XDG_DATA_HOME", &task.data_home)
        .args([
            "--product-task",
            "prepare",
            "--task",
            &task.record.id,
            "--request",
            &request.id,
        ])
        .output()
        .unwrap();
    assert!(
        prepared.status.success(),
        "{}",
        String::from_utf8_lossy(&prepared.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<product_contract::DevelopmentRequest>(&prepared.stdout).unwrap(),
        request
    );
    task.edit_label("Actual synthetic external edit");
    let submitted = std::process::Command::new(env!("CARGO_BIN_EXE_git_manager"))
        .env("XDG_DATA_HOME", &task.data_home)
        .args([
            "--product-task",
            "submit",
            "--task",
            &task.record.id,
            "--request",
            &request.id,
        ])
        .output()
        .unwrap();
    assert!(
        submitted.status.success(),
        "{}",
        String::from_utf8_lossy(&submitted.stderr)
    );
    let start = Instant::now();
    while !studio.test_task_can_review() {
        frame(&mut h, &mut studio);
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "{}",
            studio.test_notice()
        );
        std::thread::sleep(Duration::from_millis(30));
    }
    settle(&mut h, &mut studio);
    studio.test_task_review();
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    let journal: serde_json::Value =
        serde_json::from_slice(&fs::read(task.root.join("studio/session.json")).unwrap()).unwrap();
    assert!(journal["task"]["external"].is_null());
    assert!(matches!(
        studio.test_task_source_producer().unwrap(),
        product_contract::Producer::ExternalAuthor { .. }
    ));
}

#[path = "../src/product_studio/task_source_flow.rs"]
mod flow_fixture;
fn task_request(
    capture: product_contract::CapturedProgram,
    id: &str,
) -> product_contract::DevelopmentRequest {
    use product_contract::*;
    DevelopmentRequest {
        version: CONTRACT_VERSION,
        id: id.into(),
        project_id: "runtime-project".into(),
        operation: DevelopmentOperation::Modify,
        request: "Synthetic registered handoff".into(),
        sources: vec![capture],
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
            version: CONTRACT_VERSION,
            revision: 0,
            decisions: vec![],
        },
        unknowns: vec![],
        required_capabilities: Default::default(),
    }
}
#[test]
fn first_editable_baseline_preserves_distinct_same_task_history_and_refuses_ambiguous_or_stale_first(
) {
    let task = TaskFixture::new();
    let adapter =
        product_sources::TaskSourceAdapter::link("runtime-project", &task.record.id).unwrap();
    let historical = adapter.capture_current().unwrap();
    task.edit_label("New editable task baseline");
    let current = adapter.capture_current().unwrap();
    let mut request = task_request(current.clone(), "same-task-history");
    request.sources.push(historical.clone());
    let root_dir = tempfile::tempdir().unwrap();
    // Normalize our owned temp root before the strict no-follow boundary.
    let root = fs::canonicalize(root_dir.path()).unwrap();
    let mut flow = flow_fixture::TaskSourceFlow::link("runtime-project", &task.record.id).unwrap();
    let invocation = flow
        .prepare_external(&root, &request, &std::sync::atomic::AtomicBool::new(false))
        .unwrap();
    assert_eq!(invocation.baseline(), &current);
    assert_eq!(
        invocation.request().sources,
        vec![current.clone(), historical.clone()]
    );
    let pending = flow.pending_external().unwrap().clone();
    let mut reopened = flow_fixture::TaskSourceFlow::interrupted(
        "runtime-project",
        &task.record.id,
        pending.clone(),
    )
    .unwrap();
    reopened
        .reopen_external(&root, &false.into(), &false.into())
        .unwrap();
    for (name, sources) in [
        ("duplicate-first", vec![current.clone(), current.clone()]),
        ("stale-first", vec![historical.clone(), current.clone()]),
    ] {
        let mut bad = task_request(current.clone(), name);
        bad.sources = sources;
        let mut next =
            flow_fixture::TaskSourceFlow::link("runtime-project", &task.record.id).unwrap();
        assert!(next.prepare_external(&root, &bad, &false.into()).is_err());
    }
    let mut value = serde_json::to_value(&pending).unwrap();
    value["request"]["sources"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    let tampered = serde_json::from_value(value).unwrap();
    let mut next =
        flow_fixture::TaskSourceFlow::interrupted("runtime-project", &task.record.id, tampered)
            .unwrap();
    assert!(next
        .reopen_external(&root, &false.into(), &false.into())
        .is_err());
    let mut wrong = task_request(current, "wrong-first");
    wrong.sources[0].binding.task.as_mut().unwrap().task_id = "another-task".into();
    let mut next = flow_fixture::TaskSourceFlow::link("runtime-project", &task.record.id).unwrap();
    assert!(next.prepare_external(&root, &wrong, &false.into()).is_err());
}

#[test]
fn final_pre_adoption_gate_rehashes_unrelated_source_edits_and_preserves_saved_work() {
    use std::sync::{atomic::Ordering, Arc};
    let task = TaskFixture::new();
    let pause = Arc::new(product_studio::TestPause::default());
    let (mut studio, mut h) = task.open_with(TestHooks {
        before_commit: Some(pause.clone()),
        ..Default::default()
    });
    task.edit_label("Rehearsed display change");
    await_text(&mut h, &mut studio, "Rehearsed display change");
    studio.test_task_review();
    settle(&mut h, &mut studio);
    let before = task.store.load().unwrap();
    studio.test_decide(product_contract::DecisionOutcome::Accept {
        artifact: before.program().unwrap().artifact.program_digest.clone(),
    });
    let start = Instant::now();
    while !pause.reached.load(Ordering::Acquire) {
        frame(&mut h, &mut studio);
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "commit boundary not reached: {}",
            studio.test_notice()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    fs::write(task.source.join("late.txt"), "late external source edit").unwrap();
    pause.release.store(true, Ordering::Release);
    settle(&mut h, &mut studio);
    assert_eq!(task.store.load().unwrap(), before);
    assert!(
        studio.test_notice().contains("Task source changed"),
        "{}",
        studio.test_notice()
    );
    let journal: serde_json::Value =
        serde_json::from_slice(&fs::read(task.root.join("studio/session.json")).unwrap()).unwrap();
    assert!(journal["pending"].is_null());
}

#[cfg(target_os = "linux")]
#[test]
fn restart_reopens_same_ticket_without_relaunch_and_explicit_abandonment_revokes_machine_intake() {
    use std::sync::atomic::Ordering;
    let task = TaskFixture::new();
    let harness = SyntheticHarness::new(&task);
    let hooks = TestHooks::default();
    let stopped = hooks.stopped.clone();
    let (mut studio, mut h) = task.open_with(hooks);
    studio.test_task_request("Synthetic request kept through restart");
    settle(&mut h, &mut studio);
    let request = studio
        .test_prepared_request()
        .expect(studio.test_notice())
        .clone();
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    let first = fs::read(&harness.log).unwrap_or_else(|_| {
        harness.read();
        fs::read(&harness.log).unwrap()
    });
    let journal_before = fs::read(task.root.join("studio/session.json")).unwrap();
    drop(studio);
    let start = Instant::now();
    while !stopped.load(Ordering::Acquire) {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut studio = ProductStudio::testing(task.root.clone(), None, TestHooks::default());
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "daily");
    click(&mut h, &mut studio, "studio.tasks");
    settle(&mut h, &mut studio);
    assert!(
        product_studio::test_task_machine(&task.root, "prepare", &task.record.id, &request.id)
            .is_ok()
    );
    assert_eq!(
        fs::read(&harness.log).unwrap(),
        first,
        "restart must not launch a second author"
    );
    assert_eq!(
        fs::read(task.root.join("studio/session.json")).unwrap(),
        journal_before,
        "read-only reopen must not renew authorization"
    );
    studio.test_task_abandon(&request.id);
    settle(&mut h, &mut studio);
    assert!(
        product_studio::test_task_machine(&task.root, "submit", &task.record.id, &request.id)
            .is_err()
    );
    let journal: serde_json::Value =
        serde_json::from_slice(&fs::read(task.root.join("studio/session.json")).unwrap()).unwrap();
    assert!(journal["task"]["external"].is_null());
    assert!(frame(&mut h, &mut studio)
        .text
        .iter()
        .any(|t| t.contains("did not stop") || t.contains("not stopped")));
}

#[test]
fn bundle_prompt_keeps_metacharacters_in_one_argument_and_rejects_wrong_task() {
    let task = TaskFixture::new();
    let bundle = harness::ProductTaskBundle {
        task_id: task.record.id.clone(),
        request_id: "synthetic-request".into(),
        request_digest: "sha256:synthetic-only".into(),
        request_path: task
            .root
            .join("中文 ' $(touch should-not-exist); bundle/request.json"),
        product_executable: task.root.join("app with ' quotes; $(nope)"),
        worktree: task.source.clone(),
    };
    let prompt = bundle.prompt(&task.record).unwrap();
    assert!(prompt.contains("without shell interpolation"));
    assert!(prompt.contains("中文"));
    assert!(prompt.contains("--product-task"));
    let mut wrong = task.record.clone();
    wrong.id = "wrong-task".into();
    assert!(bundle.prompt(&wrong).is_err());
    #[cfg(target_os = "linux")]
    {
        use harness::TaskHarness;
        let (_studio, _h) = task.open();
        let synthetic = SyntheticHarness::new(&task);
        harness::CodexHarness
            .start_product_task(&task.record, &bundle)
            .unwrap();
        let launched = synthetic.read();
        let args = launched["argv"].as_array().unwrap();
        assert_eq!(args.len(), 5);
        assert_eq!(args[3], "--");
        assert_eq!(
            args[4], prompt,
            "The complete Unicode/metacharacter prompt must remain one argument"
        );
        assert!(!task.source.join("should-not-exist").exists());
    }
}

#[test]
fn edits_during_worker_analysis_discard_the_entire_source_bound_result() {
    use std::sync::{atomic::Ordering, Arc};
    let task = TaskFixture::new();
    let pause = Arc::new(product_studio::TestPause::default());
    pause.release.store(true, Ordering::Release);
    let (mut studio, mut h) = task.open_with(TestHooks {
        before_task_analysis: Some(pause.clone()),
        ..Default::default()
    });
    pause.reached.store(false, Ordering::Release);
    pause.release.store(false, Ordering::Release);
    task.edit_label("Analysis in progress");
    let start = Instant::now();
    while !pause.reached.load(Ordering::Acquire) {
        frame(&mut h, &mut studio);
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "{}",
            studio.test_notice()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    fs::write(
        task.source.join("during-analysis.txt"),
        "different full source",
    )
    .unwrap();
    pause.release.store(true, Ordering::Release);
    settle(&mut h, &mut studio);
    assert!(!studio.test_task_can_review());
    assert!(frame(&mut h, &mut studio)
        .text
        .iter()
        .any(|t| t.contains("Task source changed")));
}

#[cfg(target_os = "linux")]
#[test]
fn cancelling_reopen_operation_preserves_the_same_authorized_ticket() {
    use std::sync::{atomic::Ordering, Arc};
    let task = TaskFixture::new();
    let _harness = SyntheticHarness::new(&task);
    let pause = Arc::new(product_studio::TestPause::default());
    let (mut studio, mut h) = task.open_with(TestHooks {
        before_context_transition: Some(pause.clone()),
        ..Default::default()
    });
    studio.test_task_request("Keep this synthetic job pending through a cancelled read");
    settle(&mut h, &mut studio);
    let request = studio.test_prepared_request().unwrap().id.clone();
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    let before = fs::read(task.root.join("studio/session.json")).unwrap();
    studio.test_task_reopen(&request);
    let start = Instant::now();
    while !pause.reached.load(Ordering::Acquire) {
        frame(&mut h, &mut studio);
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(studio.test_cancel());
    pause.release.store(true, Ordering::Release);
    settle(&mut h, &mut studio);
    assert_eq!(
        fs::read(task.root.join("studio/session.json")).unwrap(),
        before
    );
    assert!(
        product_studio::test_task_machine(&task.root, "prepare", &task.record.id, &request).is_ok()
    );
}

#[test]
fn old_shape_journal_reopens_without_migration_and_preserves_recovery_metadata() {
    use std::sync::atomic::Ordering;
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    let store = ProductStore::create(
        root.join("tool"),
        &fixture::capture(fixture::organizer()),
        20000,
    )
    .unwrap();
    let hooks = TestHooks::default();
    let stopped = hooks.stopped.clone();
    let mut studio = ProductStudio::testing(root.clone(), None, hooks);
    let mut h = EguiHarness::new(egui::vec2(1500.0, 2400.0));
    settle(&mut h, &mut studio);
    studio.test_open(root.join("tool"));
    settle(&mut h, &mut studio);
    let before = store.load().unwrap();
    drop(studio);
    let start = Instant::now();
    while !stopped.load(Ordering::Acquire) {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(10));
    }
    let path = root.join("studio/session.json");
    let mut journal: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    journal.as_object_mut().unwrap().remove("task");
    let old = serde_json::to_vec(&journal).unwrap();
    fs::write(&path, &old).unwrap();
    let mut studio = ProductStudio::testing(root, None, TestHooks::default());
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    assert_eq!(store.load().unwrap(), before);
    let after: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(after, journal);
}

fn retain_synthetic_intention(store: &ProductStore, id: &str) {
    use product_contract::*;
    use product_decisions::*;
    use product_runtime::LocalRuntime;
    let snapshot = store.load().unwrap();
    let capture = snapshot.program().unwrap();
    let mut scene = fixture::scenario(
        capture,
        vec![
            fixture::add("Synthetic Ada"),
            fixture::invoke("collect", Values::new()),
            fixture::invoke("export_people", Values::new()),
            SemanticInput::Observe {
                point: "complete".into(),
            },
        ],
    );
    scene.id = format!("scene-{id}");
    let accepted = accept_scene(
        &LocalRuntime::default(),
        capture,
        &scene,
        Disclosure::Synthetic,
        RuntimeLimits::default(),
    )
    .unwrap();
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let prepared = engine
        .prepare_choice(
            store,
            capture,
            Choice {
                id: id.into(),
                request: "Keep actual cumulative selection and export".into(),
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
            vec![accepted],
            &format!("retain-{id}"),
        )
        .unwrap();
    engine.adopt(store, &prepared).unwrap();
}

#[test]
fn subsequent_handoff_keeps_identical_current_once_and_distinct_accepted_task_history_verbatim() {
    let task = TaskFixture::new();
    let (mut studio, mut h) = task.open();
    studio.test_task_review();
    settle(&mut h, &mut studio);
    studio.test_decide(product_contract::DecisionOutcome::Accept {
        artifact: task
            .store
            .load()
            .unwrap()
            .program()
            .unwrap()
            .artifact
            .program_digest
            .clone(),
    });
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    let historical = task.store.load().unwrap().program().unwrap().clone();
    assert!(matches!(
        historical.binding.producer,
        product_contract::Producer::ExternalAuthor { .. }
    ));
    retain_synthetic_intention(&task.store, "first-task-intention");
    studio.test_open(task.root.join("tool"));
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.tasks");
    settle(&mut h, &mut studio);
    task.edit_label("A later actual task implementation");
    await_text(&mut h, &mut studio, "A later actual task implementation");
    studio.test_task_review();
    settle(&mut h, &mut studio);
    studio.test_decide(product_contract::DecisionOutcome::Accept {
        artifact: historical.artifact.program_digest.clone(),
    });
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    retain_synthetic_intention(&task.store, "second-task-intention");
    studio.test_open(task.root.join("tool"));
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.tasks");
    settle(&mut h, &mut studio);
    await_text(&mut h, &mut studio, "Complete source captured");
    studio
        .test_task_request("Continue with another actual task author, preserving both intentions");
    settle(&mut h, &mut studio);
    let request = studio
        .test_prepared_request()
        .expect(studio.test_notice())
        .clone();
    let current = task.store.load().unwrap().program().unwrap().clone();
    assert_eq!(request.sources.first(), Some(&current));
    assert_eq!(
        request
            .sources
            .iter()
            .filter(|s| s.binding == current.binding)
            .count(),
        1
    );
    assert!(request.sources.contains(&historical));
    assert_eq!(request.decisions.decisions.len(), 2);
    assert_eq!(request.accepted_scenes.len(), 2);
    request.validate().unwrap();
    let jobs = tempfile::tempdir().unwrap();
    let jobs_root = fs::canonicalize(jobs.path()).unwrap();
    let mut next = flow_fixture::TaskSourceFlow::link("runtime-project", &task.record.id).unwrap();
    let invocation = next
        .prepare_external(&jobs_root, &request, &false.into())
        .unwrap();
    assert_eq!(invocation.request(), &request);
    assert_eq!(invocation.baseline(), &current);
}

#[cfg(target_os = "linux")]
#[test]
fn completion_followed_by_source_edit_is_rejected_and_original_daily_work_still_opens() {
    let task = TaskFixture::new();
    let _harness = SyntheticHarness::new(&task);
    let before = task.store.load().unwrap();
    let (mut studio, mut h) = task.open();
    studio.test_task_request("Synthetic candidate for stale completion test");
    settle(&mut h, &mut studio);
    let request = studio.test_prepared_request().unwrap().id.clone();
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    task.edit_label("Completed source");
    product_studio::test_task_machine(&task.root, "submit", &task.record.id, &request).unwrap();
    fs::write(
        task.source.join("after-completion.txt"),
        "source no longer matches published completion",
    )
    .unwrap();
    studio.test_task_refresh();
    settle(&mut h, &mut studio);
    assert!(!studio.test_task_can_review());
    assert!(frame(&mut h, &mut studio)
        .text
        .iter()
        .any(|t| t.contains("rejected") || t.contains("changed")));
    click(&mut h, &mut studio, "studio.tasks-back");
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "daily");
    assert_eq!(task.store.load().unwrap(), before);
}

#[test]
fn unchanged_task_review_reopens_after_return_and_refreshes_for_new_daily_data() {
    let task = TaskFixture::new();
    let (mut studio, mut h) = task.open();
    studio.test_task_review();
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    click(&mut h, &mut studio, "studio.return");
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.tasks");
    settle(&mut h, &mut studio);
    assert!(
        studio.test_task_can_review(),
        "Unchanged complete source lost its review after returning"
    );
    studio.test_task_review();
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "change");
    click(&mut h, &mut studio, "studio.return");
    settle(&mut h, &mut studio);
    studio.test_daily(fixture::add("New saved daily row"));
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.tasks");
    settle(&mut h, &mut studio);
    assert!(
        studio.test_task_can_review(),
        "Unchanged source must be rechecked on current daily data"
    );
    studio.test_task_review();
    settle(&mut h, &mut studio);
    assert!(studio
        .test_current_trial()
        .unwrap()
        .retained_records
        .iter()
        .any(|r| r.values.get("name")
            == Some(&product_contract::DataValue::Text {
                value: "New saved daily row".into()
            })));
}

#[test]
fn background_source_analysis_keeps_controls_available_and_retains_first_foreground_request() {
    use std::sync::{atomic::Ordering, Arc};
    let task = TaskFixture::new();
    let pause = Arc::new(product_studio::TestPause::default());
    pause.release.store(true, Ordering::Release);
    let (mut studio, mut h) = task.open_with(TestHooks {
        before_task_analysis: Some(pause.clone()),
        ..Default::default()
    });
    pause.reached.store(false, Ordering::Release);
    pause.release.store(false, Ordering::Release);
    task.edit_label("Source read still running");
    let start = Instant::now();
    while !pause.reached.load(Ordering::Acquire) {
        frame(&mut h, &mut studio);
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(10));
    }
    // This is a real automatic source poll, not a foreground refresh. A slow
    // read must not steal or disable navigation and request controls.
    let trace = frame(&mut h, &mut studio);
    assert!(
        trace.controls["studio.tasks-back"].enabled,
        "A background read disabled foreground controls"
    );
    assert!(!studio.is_busy());
    studio.test_task_request("The first clicked exact request");
    assert!(studio.is_busy());
    studio.test_task_request("A later duplicate must not replace it");
    assert!(studio.test_notice().contains("already pending"));
    pause.release.store(true, Ordering::Release);
    settle(&mut h, &mut studio);
    assert!(
        studio.test_prepared_request().is_none(),
        "A queued request must not silently switch to newly captured source"
    );
    assert!(
        studio.test_notice().to_lowercase().contains("source"),
        "{}",
        studio.test_notice()
    );
    studio.test_task_request("Fresh explicit request after the stale queued one");
    settle(&mut h, &mut studio);
    assert_eq!(
        studio.test_prepared_request().unwrap().request,
        "Fresh explicit request after the stale queued one"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn completed_handoff_retains_original_request_across_later_settled_polls() {
    let task = TaskFixture::new();
    let _harness = SyntheticHarness::new(&task);
    let (mut studio, mut h) = task.open();
    let need = "Keep this exact authorized human request through completed source analysis";
    studio.test_task_request(need);
    settle(&mut h, &mut studio);
    let request = studio.test_prepared_request().unwrap().clone();
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    task.edit_label("Exact completed source");
    product_studio::test_task_machine(&task.root, "submit", &task.record.id, &request.id).unwrap();
    studio.test_task_refresh();
    settle(&mut h, &mut studio);
    // The settled source event can arrive after intake. Both must carry the
    // original request/Basis, never the ordinary source review fallback.
    std::thread::sleep(Duration::from_millis(650));
    studio.test_task_refresh();
    settle(&mut h, &mut studio);
    studio.test_task_review();
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    assert!(frame(&mut h, &mut studio)
        .text
        .iter()
        .any(|t| t == &format!("Requested change: {need}")));
    assert_eq!(
        _harness.read()["journal"]["task"]["external"]["pending"]["request"],
        serde_json::to_value(&request).unwrap()
    );
}

#[cfg(unix)]
#[test]
fn abandoned_source_review_does_not_label_or_block_an_unrelated_provider_change() {
    use std::os::unix::fs::PermissionsExt;
    let task = TaskFixture::new();
    task.store
        .apply(
            0,
            "seed-independent-person",
            &fixture::add("Synthetic independent choice"),
            product_contract::RuntimeLimits::default(),
        )
        .unwrap();
    let executable = task.root.join("unrelated-provider-fixture.py");
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
    let mut program = fixture::organizer();
    program["label"] = serde_json::json!("Unrelated provider title");
    program["actions"][2]["steps"][0]["items"] = fixture::query("person");
    fs::write(
        executable.with_extension("json"),
        serde_json::to_vec(&serde_json::json!({"program":program,"mode":"good"})).unwrap(),
    )
    .unwrap();
    let home = task.root.join("unrelated-provider-home");
    fs::create_dir(&home).unwrap();
    let transport = product_provider::ProviderTransport::new_fixture(
        task.root.join("provider-jobs"),
        product_provider::ProviderKind::Codex,
        executable,
        home,
    )
    .unwrap();
    drop(_guard);
    let (mut studio, mut h) = task.open_with_transport(TestHooks::default(), Some(transport));
    studio.test_task_review();
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "change");
    click(&mut h, &mut studio, "studio.return");
    settle(&mut h, &mut studio);
    tasks::TaskRegistry::load().unlink(&task.record.id).unwrap();
    studio.test_modify("A separate provider request after abandoning source review");
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "consent", "{}", studio.test_notice());
    studio.test_consent();
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    assert!(!frame(&mut h, &mut studio)
        .text
        .iter()
        .any(|t| t.starts_with("Actual captured external task edits")));
    studio.test_trial(fixture::invoke(
        "export_people",
        product_contract::Values::new(),
    ));
    settle(&mut h, &mut studio);
    studio.test_decide(product_contract::DecisionOutcome::Deferred);
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    let pending = task
        .store
        .load()
        .unwrap()
        .decisions
        .decisions
        .last()
        .unwrap()
        .id
        .clone();
    click(&mut h, &mut studio, &format!("studio.resume.{pending}"));
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    assert!(!frame(&mut h, &mut studio)
        .text
        .iter()
        .any(|t| t.starts_with("Actual captured external task edits")));
    studio.test_trial(fixture::invoke(
        "export_people",
        product_contract::Values::new(),
    ));
    settle(&mut h, &mut studio);
    studio.test_decide(product_contract::DecisionOutcome::Accept {
        artifact: task
            .store
            .load()
            .unwrap()
            .program()
            .unwrap()
            .artifact
            .program_digest
            .clone(),
    });
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    assert_eq!(
        task.store.load().unwrap().program().unwrap().program.label,
        "Unrelated provider title"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn completed_handoff_with_stale_original_data_never_rebases_its_evidence() {
    let task = TaskFixture::new();
    let _harness = SyntheticHarness::new(&task);
    let (mut studio, mut h) = task.open();
    studio.test_task_request("Keep the original authorized data basis");
    settle(&mut h, &mut studio);
    let request = studio.test_prepared_request().unwrap().clone();
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.tasks-back");
    settle(&mut h, &mut studio);
    studio.test_daily(fixture::add("Daily work continued during authoring"));
    settle(&mut h, &mut studio);
    let saved = task.store.load().unwrap();
    assert_ne!(
        request.context.data_digest,
        Some(saved.data.identity().unwrap())
    );
    click(&mut h, &mut studio, "studio.tasks");
    settle(&mut h, &mut studio);
    task.edit_label("Completed against old saved data");
    product_studio::test_task_machine(&task.root, "submit", &task.record.id, &request.id).unwrap();
    studio.test_task_refresh();
    settle(&mut h, &mut studio);
    std::thread::sleep(Duration::from_millis(650));
    studio.test_task_refresh();
    settle(&mut h, &mut studio);
    assert!(
        !studio.test_task_can_review(),
        "Later settled capture must not silently rebase completed evidence"
    );
    studio.test_task_review();
    settle(&mut h, &mut studio);
    assert_ne!(studio.test_page(), "change");
    assert_eq!(task.store.load().unwrap(), saved);
    assert_eq!(
        _harness.read()["journal"]["task"]["external"]["pending"]["request"],
        serde_json::to_value(&request).unwrap()
    );
}

#[test]
fn queued_navigation_remains_available_and_cancellation_does_not_run_it() {
    use std::sync::{atomic::Ordering, Arc};
    let task = TaskFixture::new();
    let before = task.store.load().unwrap();
    let pause = Arc::new(product_studio::TestPause::default());
    pause.release.store(true, Ordering::Release);
    let (mut studio, mut h) = task.open_with(TestHooks {
        before_task_analysis: Some(pause.clone()),
        duplicate_completion: true,
        ..Default::default()
    });
    for cancelled in [false, true] {
        if studio.test_page() == "daily" {
            click(&mut h, &mut studio, "studio.tasks");
            settle(&mut h, &mut studio);
        }
        pause.reached.store(false, Ordering::Release);
        pause.release.store(false, Ordering::Release);
        task.edit_label(if cancelled {
            "Second long source analysis"
        } else {
            "First long source analysis"
        });
        let start = Instant::now();
        while !pause.reached.load(Ordering::Acquire) {
            frame(&mut h, &mut studio);
            assert!(start.elapsed() < Duration::from_secs(20));
            std::thread::sleep(Duration::from_millis(10));
        }
        click(&mut h, &mut studio, "studio.tasks-back");
        assert!(
            studio.is_busy(),
            "The clicked navigation must be retained while the source read is in progress"
        );
        assert!(frame(&mut h, &mut studio)
            .text
            .iter()
            .any(|t| t.contains("exact command is pending")));
        if cancelled {
            assert!(studio.test_cancel());
        }
        pause.release.store(true, Ordering::Release);
        settle(&mut h, &mut studio);
        assert_eq!(
            studio.test_page(),
            if cancelled { "tasks" } else { "daily" }
        );
        assert_eq!(task.store.load().unwrap(), before);
    }
}

#[test]
fn late_background_capture_preserves_dirty_daily_form_and_its_exact_submission() {
    use std::sync::{atomic::Ordering, Arc};
    let task = TaskFixture::new();
    let pause = Arc::new(product_studio::TestPause::default());
    pause.release.store(true, Ordering::Release);
    let (mut studio, mut h) = task.open_with(TestHooks {
        before_task_analysis: Some(pause.clone()),
        ..Default::default()
    });
    click(&mut h, &mut studio, "studio.tasks-back");
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, "daily.navigate.new_person");
    settle(&mut h, &mut studio);
    pause.reached.store(false, Ordering::Release);
    pause.release.store(false, Ordering::Release);
    task.edit_label("Late background view must stay separate");
    let start = Instant::now();
    while !pause.reached.load(Ordering::Acquire) {
        frame(&mut h, &mut studio);
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(10));
    }
    fill(
        &mut h,
        &mut studio,
        "daily.field.name",
        "Exact dirty foreground name",
    );
    fill(&mut h, &mut studio, "daily.field.area", "north");
    assert!(!frame(&mut h, &mut studio).controls["studio.history"].enabled);
    pause.release.store(true, Ordering::Release);
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "daily");
    assert!(
        !frame(&mut h, &mut studio).controls["studio.history"].enabled,
        "Background completion must not acknowledge or discard unsent form input"
    );
    click(&mut h, &mut studio, "daily.submit");
    settle(&mut h, &mut studio);
    let saved = task.store.load().unwrap();
    assert_eq!(saved.data.records.len(), 1);
    assert_eq!(
        saved.data.records[0].values["name"],
        product_contract::DataValue::Text {
            value: "Exact dirty foreground name".into()
        }
    );
    assert_eq!(saved.program().unwrap().program.label, "Club organizer");
}

fn source_choice_freshness(outcome: product_contract::DecisionOutcome) {
    use std::sync::{atomic::Ordering, Arc};
    for changed_after_preparation in [false, true] {
        let task = TaskFixture::new();
        task.store
            .apply(
                0,
                "seed-person",
                &fixture::add("Synthetic Ada"),
                product_contract::RuntimeLimits::default(),
            )
            .unwrap();
        let mut candidate = fixture::organizer();
        candidate["actions"][2]["steps"][0]["items"] = fixture::query("person");
        fs::write(
            task.source.join("app.json"),
            serde_json::to_vec(&candidate).unwrap(),
        )
        .unwrap();
        let pause = Arc::new(product_studio::TestPause::default());
        pause
            .release
            .store(!changed_after_preparation, Ordering::Release);
        let (mut studio, mut h) = task.open_with(TestHooks {
            before_commit: Some(pause.clone()),
            ..Default::default()
        });
        studio.test_task_review();
        settle(&mut h, &mut studio);
        assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
        studio.test_trial(fixture::invoke(
            "export_people",
            product_contract::Values::new(),
        ));
        settle(&mut h, &mut studio);
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
        let before = task.store.load().unwrap();
        studio.test_decide(outcome.clone());
        if changed_after_preparation {
            let start = Instant::now();
            while !pause.reached.load(Ordering::Acquire) {
                frame(&mut h, &mut studio);
                assert!(
                    start.elapsed() < Duration::from_secs(30),
                    "{}",
                    studio.test_notice()
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            fs::write(
                task.source.join("late-recording-source.txt"),
                "Freshness applies even when the current program is kept",
            )
            .unwrap();
            pause.release.store(true, Ordering::Release);
        }
        settle(&mut h, &mut studio);
        let after = task.store.load().unwrap();
        if changed_after_preparation {
            assert_eq!(
                after, before,
                "A stale source comparison must not record any choice or change data"
            );
            assert!(
                studio.test_notice().contains("Task source changed"),
                "{}",
                studio.test_notice()
            );
            let journal: serde_json::Value =
                serde_json::from_slice(&fs::read(task.root.join("studio/session.json")).unwrap())
                    .unwrap();
            assert!(journal["pending"].is_null());
        } else {
            assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
            assert_eq!(after.data, before.data);
            assert_eq!(after.decisions.decisions.last().unwrap().outcome, outcome);
        }
    }
}
#[test]
fn keep_current_checks_originating_source_at_final_recording_gate() {
    source_choice_freshness(product_contract::DecisionOutcome::KeepCurrent);
}
#[test]
fn either_acceptable_checks_originating_source_at_final_recording_gate() {
    source_choice_freshness(product_contract::DecisionOutcome::EitherAcceptable);
}
#[test]
fn both_needed_checks_originating_source_at_final_recording_gate() {
    source_choice_freshness(product_contract::DecisionOutcome::BothNeeded);
}
#[test]
fn neither_fits_checks_originating_source_at_final_recording_gate() {
    source_choice_freshness(product_contract::DecisionOutcome::NeitherFits);
}
#[test]
fn deferred_checks_originating_source_at_final_recording_gate() {
    source_choice_freshness(product_contract::DecisionOutcome::Deferred);
}

#[test]
fn cancelled_new_basis_analysis_can_retry_the_unchanged_source() {
    use std::sync::{atomic::Ordering, Arc};
    let task = TaskFixture::new();
    let pause = Arc::new(product_studio::TestPause::default());
    pause.release.store(true, Ordering::Release);
    let (mut studio, mut h) = task.open_with(TestHooks {
        before_task_analysis: Some(pause.clone()),
        ..Default::default()
    });
    click(&mut h, &mut studio, "studio.tasks-back");
    settle(&mut h, &mut studio);
    studio.test_daily(fixture::add("New saved basis"));
    settle(&mut h, &mut studio);
    pause.reached.store(false, Ordering::Release);
    pause.release.store(false, Ordering::Release);
    click(&mut h, &mut studio, "studio.tasks");
    let start = Instant::now();
    while !pause.reached.load(Ordering::Acquire) {
        frame(&mut h, &mut studio);
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(studio.test_cancel());
    pause.release.store(true, Ordering::Release);
    settle(&mut h, &mut studio);
    studio.test_task_refresh();
    settle(&mut h, &mut studio);
    assert!(
        studio.test_task_can_review(),
        "A cancelled analysis may be retried without source edits or relinking"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn completed_stale_basis_stays_bound_across_same_tool_reopen_and_process_restart() {
    use std::sync::atomic::Ordering;
    let task = TaskFixture::new();
    let _harness = SyntheticHarness::new(&task);
    let hooks = TestHooks::default();
    let stopped = hooks.stopped.clone();
    let (mut studio, mut h) = task.open_with(hooks);
    studio.test_task_request("Never rebase this completed authorized request");
    settle(&mut h, &mut studio);
    let request = studio.test_prepared_request().unwrap().clone();
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.tasks-back");
    settle(&mut h, &mut studio);
    studio.test_daily(fixture::add("Later saved work"));
    settle(&mut h, &mut studio);
    let saved = task.store.load().unwrap();
    task.edit_label("Completed on stale data");
    product_studio::test_task_machine(&task.root, "submit", &task.record.id, &request.id).unwrap();
    studio.test_task_refresh();
    settle(&mut h, &mut studio);
    studio.test_open(task.root.join("tool"));
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.tasks");
    settle(&mut h, &mut studio);
    await_text(&mut h, &mut studio, "Completed on stale data");
    assert!(
        !studio.test_task_can_review(),
        "Same-tool reopen silently rebased the original completion"
    );
    drop(studio);
    let start = Instant::now();
    while !stopped.load(Ordering::Acquire) {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut studio = ProductStudio::testing(task.root.clone(), None, TestHooks::default());
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.tasks");
    settle(&mut h, &mut studio);
    await_text(&mut h, &mut studio, "Completed on stale data");
    assert!(
        !studio.test_task_can_review(),
        "Restart silently rebased the original completion"
    );
    assert_eq!(task.store.load().unwrap(), saved);
    assert!(
        product_studio::test_task_machine(&task.root, "submit", &task.record.id, &request.id)
            .is_err(),
        "Terminal completion cannot grant submission authority again"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn invalid_saved_current_basis_digest_refuses_every_machine_entry_without_writes() {
    use std::sync::atomic::Ordering;
    let task = TaskFixture::new();
    let _harness = SyntheticHarness::new(&task);
    let hooks = TestHooks::default();
    let stopped = hooks.stopped.clone();
    let (mut studio, mut h) = task.open_with(hooks);
    studio.test_task_request("Synthetic request for exact saved source binding");
    settle(&mut h, &mut studio);
    let request = studio.test_prepared_request().unwrap().clone();
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    drop(studio);
    let start = Instant::now();
    while !stopped.load(Ordering::Acquire) {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(10));
    }
    let path = task.root.join("studio/session.json");
    let mut journal: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    journal["task"]["external"]["basis"]["source"] = serde_json::to_value(
        product_contract::canonical_digest(
            product_contract::IdentityDomain::Source,
            &"different saved current capture",
        )
        .unwrap(),
    )
    .unwrap();
    let bytes = serde_json::to_vec(&journal).unwrap();
    fs::write(&path, &bytes).unwrap();
    let saved = task.store.load().unwrap();
    for operation in ["prepare", "inspect", "submit"] {
        assert!(product_studio::test_task_machine(
            &task.root,
            operation,
            &task.record.id,
            &request.id
        )
        .is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(task.store.load().unwrap(), saved);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn machine_submit_reports_success_when_exact_gui_intake_consumes_before_final_ack() {
    let task = TaskFixture::new();
    let _harness = SyntheticHarness::new(&task);
    let (mut studio, mut h) = task.open();
    studio.test_task_request("Exact consume versus submit acknowledgement race");
    settle(&mut h, &mut studio);
    let request = studio.test_prepared_request().unwrap().clone();
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    task.edit_label("Published and durably consumed source");
    let result = product_studio::test_task_machine_after_submit(
        &task.root,
        &task.record.id,
        &request.id,
        &mut || {
            studio.test_task_refresh();
            settle(&mut h, &mut studio);
            let journal: serde_json::Value =
                serde_json::from_slice(&fs::read(task.root.join("studio/session.json")).unwrap())
                    .unwrap();
            assert!(journal["task"]["external"].is_null(), "The real GUI worker must have consumed the published completion before the final machine read");
        },
    );
    assert!(
        result.is_ok(),
        "Actual GUI consumption must not be reported as abandoned authorization: {:?}",
        result
    );
}

#[cfg(target_os = "linux")]
#[test]
fn abandoned_intake_cannot_be_acknowledged_as_a_consumed_completion_or_replayed() {
    let task = TaskFixture::new();
    let harness = SyntheticHarness::new(&task);
    let (mut studio, mut h) = task.open();
    studio.test_task_request("Explicitly abandon this local intake");
    settle(&mut h, &mut studio);
    let request = studio.test_prepared_request().unwrap().clone();
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    harness.read();
    task.edit_label("Late independently authored source");
    let result = product_studio::test_task_machine_after_submit(
        &task.root,
        &task.record.id,
        &request.id,
        &mut || {
            studio.test_task_abandon(&request.id);
            settle(&mut h, &mut studio);
        },
    );
    assert!(result.is_err());
    let journal: serde_json::Value =
        serde_json::from_slice(&fs::read(task.root.join("studio/session.json")).unwrap()).unwrap();
    assert!(journal["task"]["external"].is_null());
    assert!(journal["task"]["completed"].is_null());
    for operation in ["prepare", "inspect", "submit"] {
        assert!(product_studio::test_task_machine(
            &task.root,
            operation,
            &task.record.id,
            &request.id
        )
        .is_err());
    }
    studio.test_open(task.root.join("tool"));
    settle(&mut h, &mut studio);
    assert!(
        product_studio::test_task_machine(&task.root, "submit", &task.record.id, &request.id)
            .is_err()
    );
    assert!(frame(&mut h, &mut studio)
        .text
        .iter()
        .all(|t| !t.contains("process stopped")));
}

#[cfg(target_os = "linux")]
#[test]
fn completed_terminal_request_is_preserved_on_reopen_and_rejects_tampered_or_replayed_proof() {
    use std::sync::atomic::Ordering;
    let task = TaskFixture::new();
    let harness = SyntheticHarness::new(&task);
    let hooks = TestHooks::default();
    let stopped = hooks.stopped.clone();
    let (mut studio, mut h) = task.open_with(hooks);
    studio.test_task_request("Preserve the exact completed request on every reopen");
    settle(&mut h, &mut studio);
    let request = studio.test_prepared_request().unwrap().clone();
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    harness.read();
    task.edit_label("Durable terminal source");
    product_studio::test_task_machine_after_submit(
        &task.root,
        &task.record.id,
        &request.id,
        &mut || {
            studio.test_task_refresh();
            settle(&mut h, &mut studio);
        },
    )
    .unwrap();
    let path = task.root.join("studio/session.json");
    let original: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        original["task"]["completed"]["external"]["pending"]["request"],
        serde_json::to_value(&request).unwrap()
    );
    assert!(original["task"]["external"].is_null());
    studio.test_open(task.root.join("tool"));
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.tasks");
    settle(&mut h, &mut studio);
    await_text(&mut h, &mut studio, "Durable terminal source");
    // The persisted last-complete label is intentionally visible while stale.
    await_review_control(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.task-review");
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    assert!(frame(&mut h, &mut studio)
        .text
        .iter()
        .any(|t| t == &format!("Requested change: {}", request.request)));
    drop(studio);
    let start = Instant::now();
    while !stopped.load(Ordering::Acquire) {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(10));
    }
    let saved = task.store.load().unwrap();
    for variant in [
        "replayed-pending",
        "reordered-request",
        "mismatched-receipt",
        "self-declared-capture",
    ] {
        let mut altered = original.clone();
        match variant {
            "replayed-pending" => {
                altered["task"]["external"] = altered["task"]["completed"]["external"].clone()
            }
            "reordered-request" => altered["task"]["completed"]["external"]["pending"]["request"]
                ["sources"]
                .as_array_mut()
                .unwrap()
                .reverse(),
            "mismatched-receipt" => {
                altered["task"]["completed"]["external"]["pending"]["ticket"]["request_digest"] =
                    serde_json::to_value(
                        product_contract::canonical_digest(
                            product_contract::IdentityDomain::Request,
                            &"another receipt",
                        )
                        .unwrap(),
                    )
                    .unwrap()
            }
            _ => {
                altered["task"]["completed"]["capture"] =
                    serde_json::to_value(&request.sources[0]).unwrap()
            }
        }
        let bytes = serde_json::to_vec(&altered).unwrap();
        fs::write(&path, &bytes).unwrap();
        for operation in ["prepare", "inspect", "submit"] {
            assert!(
                product_studio::test_task_machine(
                    &task.root,
                    operation,
                    &task.record.id,
                    &request.id
                )
                .is_err(),
                "{variant}"
            );
        }
        let hooks = TestHooks::default();
        let ended = hooks.stopped.clone();
        let mut reopened = ProductStudio::testing(task.root.clone(), None, hooks);
        settle(&mut h, &mut reopened);
        if reopened.test_page() == "daily" {
            click(&mut h, &mut reopened, "studio.tasks");
            settle(&mut h, &mut reopened);
        }
        assert!(
            !reopened.test_task_can_review(),
            "{variant} must not become a new ordinary source review"
        );
        assert_eq!(task.store.load().unwrap(), saved);
        assert_eq!(
            fs::read(&path).unwrap(),
            bytes,
            "{variant} must be retained rather than repaired silently"
        );
        drop(reopened);
        let start = Instant::now();
        while !ended.load(Ordering::Acquire) {
            assert!(start.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn oversized_terminal_capture_keeps_the_exact_pending_journal_and_saved_work() {
    let task = TaskFixture::new();
    let harness = SyntheticHarness::new(&task);
    let (mut studio, mut h) = task.open();
    studio.test_task_request("Bounded completed capture must fail closed");
    settle(&mut h, &mut studio);
    let request = studio.test_prepared_request().unwrap().clone();
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    harness.read();
    let path = task.root.join("studio/session.json");
    let pending = fs::read(&path).unwrap();
    let saved = task.store.load().unwrap();
    let mut bytes = serde_json::to_vec(&fixture::organizer()).unwrap();
    bytes.extend(std::iter::repeat_n(b' ', 450_000));
    fs::write(task.source.join("app.json"), bytes).unwrap();
    product_studio::test_task_machine(&task.root, "submit", &task.record.id, &request.id).unwrap();
    studio.test_task_refresh();
    settle(&mut h, &mut studio);
    assert_eq!(fs::read(&path).unwrap(), pending);
    assert_eq!(task.store.load().unwrap(), saved);
    assert!(!studio.test_task_can_review());
    assert!(frame(&mut h, &mut studio)
        .text
        .iter()
        .any(|t| t.contains("bounded restart record")));
    studio.test_task_abandon(&request.id);
    settle(&mut h, &mut studio);
    assert!(
        product_studio::test_task_machine(&task.root, "submit", &task.record.id, &request.id)
            .is_err()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn interrupted_terminal_publication_preserves_pending_ticket_and_restart_retries_only_intake() {
    use std::sync::{atomic::Ordering, Arc};
    let task = TaskFixture::new();
    let harness = SyntheticHarness::new(&task);
    let pause = Arc::new(product_studio::TestPause::default());
    let hooks = TestHooks {
        before_task_completion_commit: Some(pause.clone()),
        ..Default::default()
    };
    let stopped = hooks.stopped.clone();
    let (mut studio, mut h) = task.open_with(hooks);
    studio.test_task_request("Read cancellation before terminal publication");
    settle(&mut h, &mut studio);
    let request = studio.test_prepared_request().unwrap().clone();
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    harness.read();
    let launched = fs::read(&harness.log).unwrap();
    let path = task.root.join("studio/session.json");
    let before = fs::read(&path).unwrap();
    task.edit_label("Interrupted intake source");
    product_studio::test_task_machine(&task.root, "submit", &task.record.id, &request.id).unwrap();
    studio.test_task_refresh();
    let start = Instant::now();
    while !pause.reached.load(Ordering::Acquire) {
        frame(&mut h, &mut studio);
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(studio.test_cancel());
    pause.release.store(true, Ordering::Release);
    settle(&mut h, &mut studio);
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!studio.test_task_can_review());
    drop(studio);
    let start = Instant::now();
    while !stopped.load(Ordering::Acquire) {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut studio = ProductStudio::testing(task.root.clone(), None, TestHooks::default());
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.tasks");
    settle(&mut h, &mut studio);
    await_text(&mut h, &mut studio, "Interrupted intake source");
    await_review_control(&mut h, &mut studio);
    assert!(studio.test_task_can_review());
    let terminal: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(terminal["task"]["external"].is_null());
    assert_eq!(
        terminal["task"]["completed"]["external"]["pending"]["request"],
        serde_json::to_value(request).unwrap()
    );
    assert_eq!(
        fs::read(&harness.log).unwrap(),
        launched,
        "Restart may only reopen and intake, never invoke again"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_second_authorized_handoff_reuses_accepted_task_source_without_replaying_the_first_job() {
    let task = TaskFixture::new();
    let harness = SyntheticHarness::new(&task);
    let (mut studio, mut h) = task.open();
    studio.test_task_request("First synthetic authorized display change");
    settle(&mut h, &mut studio);
    let first = studio.test_prepared_request().unwrap().clone();
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    harness.read();
    task.edit_label("Accepted first external source");
    product_studio::test_task_machine(&task.root, "submit", &task.record.id, &first.id).unwrap();
    await_text(&mut h, &mut studio, "Accepted first external source");
    await_review_control(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.task-review");
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    studio.test_decide(product_contract::DecisionOutcome::Accept {
        artifact: task
            .store
            .load()
            .unwrap()
            .program()
            .unwrap()
            .artifact
            .program_digest
            .clone(),
    });
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    let accepted = task.store.load().unwrap().program().unwrap().clone();
    click(&mut h, &mut studio, "studio.tasks");
    settle(&mut h, &mut studio);
    await_text(&mut h, &mut studio, "Complete source captured");
    studio.test_task_request("Second explicit request with accepted current source");
    settle(&mut h, &mut studio);
    let second = studio.test_prepared_request().unwrap().clone();
    assert_ne!(second.id, first.id);
    assert_eq!(second.sources[0], accepted);
    assert_eq!(
        second
            .sources
            .iter()
            .filter(|s| s.binding == accepted.binding)
            .count(),
        1
    );
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    let start = Instant::now();
    loop {
        let logged: serde_json::Value =
            serde_json::from_slice(&fs::read(&harness.log).unwrap()).unwrap();
        if logged["journal"]["task"]["external"]["pending"]["request"]["id"] == second.id {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "The second authorized exact bundle was not invoked"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let journal: serde_json::Value =
        serde_json::from_slice(&fs::read(task.root.join("studio/session.json")).unwrap()).unwrap();
    assert!(journal["task"]["completed"].is_null());
    assert_eq!(
        journal["task"]["external"]["pending"]["request"],
        serde_json::to_value(second).unwrap()
    );
    assert!(
        product_studio::test_task_machine(&task.root, "submit", &task.record.id, &first.id)
            .is_err()
    );
}

#[cfg(target_os = "linux")]
fn assert_only_checked_external_lineage_added(before: &[u8], after: &[u8]) {
    product_studio::journal_testing::roundtrip(after)
        .expect("The new lineage must strictly correlate with the entire actual completion");
    let mut retained: serde_json::Value = serde_json::from_slice(after).unwrap();
    let lineage = retained
        .as_object_mut()
        .unwrap()
        .remove("discovery")
        .unwrap();
    assert!(lineage["original"].is_null());
    assert!(lineage["discover"].is_null());
    assert!(lineage["accepted_case"].is_null());
    assert_eq!(
        lineage["need"],
        retained["task"]["completed"]["external"]["original_need"]
    );
    assert_eq!(
        lineage["external"]["approval_operation"],
        retained["task"]["completed"]["external"]["approval_operation"]
    );
    assert_eq!(
        lineage["external"]["after"]["binding"],
        retained["task"]["completed"]["capture"]["binding"]
    );
    assert_eq!(
        lineage["external"]["after"]["artifact"],
        retained["task"]["completed"]["capture"]["artifact"]
    );
    assert_eq!(
        lineage["external"]["after"]["program"],
        retained["task"]["completed"]["capture"]["program"]
    );
    assert_eq!(
        serde_json::to_value(
            lineage["external"]["after"]["source_utf8"]
                .as_str()
                .unwrap()
                .as_bytes()
        )
        .unwrap(),
        retained["task"]["completed"]["capture"]["source_bytes"]
    );
    assert_eq!(product_contract::canonical_bytes(&retained).unwrap(), before,
        "Successful explicit Review may add only its exact uncommitted lineage; the entire earlier envelope remains byte-exact");
}

#[cfg(target_os = "linux")]
#[test]
fn cancelled_completed_analysis_retries_in_session_with_its_original_request() {
    use std::sync::{atomic::Ordering, Arc};
    let task = TaskFixture::new();
    let harness = SyntheticHarness::new(&task);
    let pause = Arc::new(product_studio::TestPause::default());
    pause.release.store(true, Ordering::Release);
    let (mut studio, mut h) = task.open_with(TestHooks {
        before_task_analysis: Some(pause.clone()),
        ..Default::default()
    });
    studio.test_task_request("Retry this exact completed request, without reauthorizing");
    settle(&mut h, &mut studio);
    let request = studio.test_prepared_request().unwrap().clone();
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    harness.read();
    let launched = fs::read(&harness.log).unwrap();
    let saved = task.store.load().unwrap();
    pause.reached.store(false, Ordering::Release);
    pause.release.store(false, Ordering::Release);
    task.edit_label("Completed analysis interrupted after publication");
    product_studio::test_task_machine(&task.root, "submit", &task.record.id, &request.id).unwrap();
    studio.test_task_refresh();
    let start = Instant::now();
    while !pause.reached.load(Ordering::Acquire) {
        frame(&mut h, &mut studio);
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(10));
    }
    let path = task.root.join("studio/session.json");
    let terminal = fs::read(&path).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&terminal).unwrap();
    assert!(value["task"]["external"].is_null());
    assert_eq!(
        value["task"]["completed"]["external"]["pending"]["request"],
        serde_json::to_value(&request).unwrap()
    );
    assert!(studio.test_cancel());
    pause.release.store(true, Ordering::Release);
    settle(&mut h, &mut studio);
    await_text(&mut h, &mut studio, "Complete source captured;");
    click(&mut h, &mut studio, "studio.tasks-back");
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.tasks");
    settle(&mut h, &mut studio);
    assert!(
        studio.test_task_can_review(),
        "Cancelled completed analysis must retry in-session on its original Basis"
    );
    click(&mut h, &mut studio, "studio.task-review");
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    assert!(frame(&mut h, &mut studio)
        .text
        .iter()
        .any(|t| t == &format!("Requested change: {}", request.request)));
    assert_only_checked_external_lineage_added(&terminal, &fs::read(&path).unwrap());
    assert_eq!(fs::read(&harness.log).unwrap(), launched);
    assert_eq!(task.store.load().unwrap(), saved);
}

#[cfg(target_os = "linux")]
#[test]
fn completed_analysis_recovers_when_source_returns_to_the_exact_authorized_capture() {
    let task = TaskFixture::new();
    let harness = SyntheticHarness::new(&task);
    let (mut studio, mut h) = task.open();
    studio.test_task_request("Keep the original request when the exact completed source returns");
    settle(&mut h, &mut studio);
    let request = studio.test_prepared_request().unwrap().clone();
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    harness.read();
    task.edit_label("Exact return of the completed source");
    product_studio::test_task_machine_after_submit(
        &task.root,
        &task.record.id,
        &request.id,
        &mut || {
            studio.test_task_refresh();
            settle(&mut h, &mut studio);
        },
    )
    .unwrap();
    await_text(&mut h, &mut studio, "Complete source captured;");
    assert!(studio.test_task_can_review());
    let terminal = fs::read(task.root.join("studio/session.json")).unwrap();
    let saved = task.store.load().unwrap();
    task.edit_label("Temporary later edit must invalidate evidence");
    await_text(
        &mut h,
        &mut studio,
        "Last complete source: Temporary later edit",
    );
    assert!(!studio.test_task_can_review());
    task.edit_label("Exact return of the completed source");
    await_text(&mut h, &mut studio, "Last complete source: Exact return");
    click(&mut h, &mut studio, "studio.tasks-back");
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.tasks");
    settle(&mut h, &mut studio);
    assert!(studio.test_task_can_review(), "Exact completed source may be independently checked again, retaining the original request and Basis");
    click(&mut h, &mut studio, "studio.task-review");
    settle(&mut h, &mut studio);
    assert!(frame(&mut h, &mut studio)
        .text
        .iter()
        .any(|t| t == &format!("Requested change: {}", request.request)));
    assert_only_checked_external_lineage_added(
        &terminal,
        &fs::read(task.root.join("studio/session.json")).unwrap(),
    );
    assert_eq!(task.store.load().unwrap(), saved);
}

#[cfg(target_os = "linux")]
#[test]
fn restarted_completed_source_can_be_revalidated_after_temporary_drift() {
    use std::sync::atomic::Ordering;
    let task = TaskFixture::new();
    let harness = SyntheticHarness::new(&task);
    let hooks = TestHooks::default();
    let stopped = hooks.stopped.clone();
    let (mut studio, mut h) = task.open_with(hooks);
    studio.test_task_request(
        "Revalidate only this original completion after interrupted source edits",
    );
    settle(&mut h, &mut studio);
    let request = studio.test_prepared_request().unwrap().clone();
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    harness.read();
    let launched = fs::read(&harness.log).unwrap();
    task.edit_label("Original completed source after restart");
    product_studio::test_task_machine_after_submit(
        &task.root,
        &task.record.id,
        &request.id,
        &mut || {
            studio.test_task_refresh();
            settle(&mut h, &mut studio);
        },
    )
    .unwrap();
    await_review_control(&mut h, &mut studio);
    let terminal = fs::read(task.root.join("studio/session.json")).unwrap();
    let saved = task.store.load().unwrap();
    task.edit_label("Temporary source while restarting");
    drop(studio);
    let start = Instant::now();
    while !stopped.load(Ordering::Acquire) {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut studio = ProductStudio::testing(task.root.clone(), None, TestHooks::default());
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.tasks");
    settle(&mut h, &mut studio);
    await_text(
        &mut h,
        &mut studio,
        "Last complete source: Temporary source while restarting",
    );
    assert!(!studio.test_task_can_review());
    task.edit_label("Original completed source after restart");
    await_text(
        &mut h,
        &mut studio,
        "Last complete source: Original completed source after restart",
    );
    click(
        &mut h,
        &mut studio,
        &format!("studio.task.{}", task.record.id),
    );
    settle(&mut h, &mut studio);
    assert!(studio.test_task_can_review(), "Explicit same-task selection must revalidate the exact receipt after source returns, without another restart");
    click(&mut h, &mut studio, "studio.task-review");
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    assert!(frame(&mut h, &mut studio)
        .text
        .iter()
        .any(|t| t == &format!("Requested change: {}", request.request)));
    assert_only_checked_external_lineage_added(
        &terminal,
        &fs::read(task.root.join("studio/session.json")).unwrap(),
    );
    assert_eq!(fs::read(&harness.log).unwrap(), launched);
    assert_eq!(task.store.load().unwrap(), saved);
}

#[cfg(target_os = "linux")]
fn restart_during_task_outage(mode: &str) {
    use std::sync::atomic::Ordering;
    let task = TaskFixture::new();
    let harness = SyntheticHarness::new(&task);
    let hooks = TestHooks::default();
    let stopped = hooks.stopped.clone();
    let (mut studio, mut h) = task.open_with(hooks);
    let request = if mode != "ordinary" {
        studio.test_task_request("Preserve this exact handoff through a temporary worktree outage");
        settle(&mut h, &mut studio);
        let request = studio.test_prepared_request().unwrap().clone();
        studio.test_task_authorize();
        settle(&mut h, &mut studio);
        harness.read();
        if mode == "completed" {
            task.edit_label("Completed source before worktree outage");
            product_studio::test_task_machine_after_submit(
                &task.root,
                &task.record.id,
                &request.id,
                &mut || {
                    studio.test_task_refresh();
                    settle(&mut h, &mut studio);
                },
            )
            .unwrap();
            await_review_control(&mut h, &mut studio);
        }
        Some(request)
    } else {
        None
    };
    let launched = request.as_ref().map(|_| fs::read(&harness.log).unwrap());
    let path = task.root.join("studio/session.json");
    let original = fs::read(&path).unwrap();
    let saved = task.store.load().unwrap();
    drop(studio);
    let start = Instant::now();
    while !stopped.load(Ordering::Acquire) {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(10));
    }
    let offline = task.source.with_file_name("temporarily-offline-source");
    fs::rename(&task.source, &offline).unwrap();
    let mut studio = ProductStudio::testing(task.root.clone(), None, TestHooks::default());
    settle(&mut h, &mut studio);
    assert_eq!(
        studio.test_page(),
        "daily",
        "An unavailable task must not block saved work"
    );
    click(&mut h, &mut studio, "studio.tasks");
    settle(&mut h, &mut studio);
    assert!(!studio.test_task_can_review());
    fs::rename(&offline, &task.source).unwrap();
    if mode == "pending" {
        // Pending task selection stays disabled; explicit page entry recovers
        // its same journal-bound read-only intake instead of replacing a link.
        click(&mut h, &mut studio, "studio.tasks-back");
        settle(&mut h, &mut studio);
        click(&mut h, &mut studio, "studio.tasks");
        settle(&mut h, &mut studio);
        assert_eq!(fs::read(&path).unwrap(), original);
        let request = request.as_ref().unwrap();
        let prepared =
            product_studio::test_task_machine(&task.root, "prepare", &task.record.id, &request.id)
                .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&prepared).unwrap(),
            serde_json::to_value(request).unwrap()
        );
        task.edit_label("Pending job completes after restored worktree");
        product_studio::test_task_machine(&task.root, "submit", &task.record.id, &request.id)
            .unwrap();
    } else {
        click(
            &mut h,
            &mut studio,
            &format!("studio.task.{}", task.record.id),
        );
        settle(&mut h, &mut studio);
        assert_eq!(fs::read(&path).unwrap(), original);
    }
    await_review_control(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.task-review");
    settle(&mut h, &mut studio);
    assert_eq!(
        studio.test_page(),
        "change",
        "{mode}: {}",
        studio.test_notice()
    );
    if let Some(request) = request {
        assert!(frame(&mut h, &mut studio)
            .text
            .iter()
            .any(|t| t == &format!("Requested change: {}", request.request)));
        assert_eq!(
            fs::read(&harness.log).unwrap(),
            launched.unwrap(),
            "Recovery must never launch again"
        );
        let current: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            current["task"]["completed"]["external"]["pending"]["request"],
            serde_json::to_value(request).unwrap()
        );
        if mode == "pending" {
            let before: serde_json::Value = serde_json::from_slice(&original).unwrap();
            assert_eq!(
                current["task"]["completed"]["external"],
                before["task"]["external"]
            );
        }
    }
    assert_eq!(task.store.load().unwrap(), saved);
}
#[cfg(target_os = "linux")]
#[test]
fn ordinary_task_flow_recovers_after_restart_during_worktree_outage() {
    restart_during_task_outage("ordinary");
}
#[cfg(target_os = "linux")]
#[test]
fn pending_task_flow_recovers_after_restart_during_worktree_outage_without_relaunch() {
    restart_during_task_outage("pending");
}
#[cfg(target_os = "linux")]
#[test]
fn completed_task_flow_recovers_after_restart_during_worktree_outage_without_rebasing() {
    restart_during_task_outage("completed");
}

#[test]
fn same_task_id_with_replaced_registry_identity_cannot_overwrite_the_original_link() {
    for changed in ["created", "repository", "worktree"] {
        let task = TaskFixture::new();
        let (mut studio, mut h) = task.open();
        let path = task.root.join("studio/session.json");
        let original = fs::read(&path).unwrap();
        let saved = task.store.load().unwrap();
        let other = task.data_home.join("different-registered-tree");
        let repo = git2::Repository::init(&other).unwrap();
        let tree = repo
            .find_tree(repo.index().unwrap().write_tree().unwrap())
            .unwrap();
        let who = git2::Signature::now("Synthetic fixture", "fixture@invalid.example").unwrap();
        repo.commit(
            Some("HEAD"),
            &who,
            &who,
            "Synthetic replacement",
            &tree,
            &[],
        )
        .unwrap();
        fs::create_dir(other.join(".gitmanager")).unwrap();
        fs::copy(
            task.source.join(".gitmanager/product.json"),
            other.join(".gitmanager/product.json"),
        )
        .unwrap();
        fs::copy(task.source.join("app.json"), other.join("app.json")).unwrap();
        let mut replacement = task.record.clone();
        match changed {
            "created" => replacement.created_at.push_str("-different-identity"),
            "repository" => replacement.repository_path = other.display().to_string(),
            _ => replacement.worktree_path = other.display().to_string(),
        }
        tasks::TaskRegistry::load().unlink(&task.record.id).unwrap();
        tasks::TaskRegistry::load().add(replacement).unwrap();
        click(
            &mut h,
            &mut studio,
            &format!("studio.task.{}", task.record.id),
        );
        settle(&mut h, &mut studio);
        assert_eq!(
            fs::read(&path).unwrap(),
            original,
            "A reused ID with different {changed} identity must not replace the retained link"
        );
        assert_eq!(task.store.load().unwrap(), saved);
        assert!(
            studio.test_notice().contains("identity") || studio.test_notice().contains("changed"),
            "{}",
            studio.test_notice()
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn retained_task_consent_cannot_launch_after_an_unresolved_ordinary_provider_outcome() {
    use std::os::unix::fs::PermissionsExt;
    let task = TaskFixture::new();
    let harness = SyntheticHarness::new(&task);
    let executable = task.root.join("unresolved-provider-fixture.py");
    let guard = product_provider::fixture_executable_write_guard();
    // Synthetic process fault: prevent the real transport from publishing or
    // reconciling its terminal receipt. No live provider is invoked.
    let script = include_str!("fixtures/provider_transport/fake_cli.py").replace(
        "output = pathlib.Path(args[args.index('--output-last-message')+1])",
        "output = pathlib.Path(args[args.index('--output-last-message')+1])\nreceipt = output.parent / 'receipt.json'\nreceipt.unlink()\nreceipt.mkdir()",
    );
    fs::write(&executable, script).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(executable.with_extension("json"), br#"{"mode":"good"}"#).unwrap();
    let home = task.root.join("unresolved-provider-home");
    fs::create_dir(&home).unwrap();
    let transport = product_provider::ProviderTransport::new_fixture(
        task.root.join("provider-jobs"),
        product_provider::ProviderKind::Codex,
        executable,
        home,
    )
    .unwrap();
    drop(guard);
    let (mut studio, mut h) = task.open_with_transport(TestHooks::default(), Some(transport));
    let saved = task.store.load().unwrap();
    studio.test_task_request(
        "Retained exact task consent must still respect current generation blockers",
    );
    settle(&mut h, &mut studio);
    let retained = studio.test_prepared_request().unwrap().clone();
    click(&mut h, &mut studio, "studio.tasks-back");
    settle(&mut h, &mut studio);
    studio.test_modify("Independent ordinary provider request that becomes unresolved");
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "consent", "{}", studio.test_notice());
    studio.test_consent();
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    assert!(
        studio.test_generation_blocked(),
        "The actual failed terminal publication must leave generation blocked"
    );
    let path = task.root.join("studio/session.json");
    let unresolved = fs::read(&path).unwrap();
    let journal: serde_json::Value = serde_json::from_slice(&unresolved).unwrap();
    assert_eq!(journal["provider"]["issued"], true);
    assert!(journal["task"]["external"].is_null());
    click(&mut h, &mut studio, "studio.tasks");
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_prepared_request(), Some(&retained));
    click(&mut h, &mut studio, "studio.external-authorize");
    settle(&mut h, &mut studio);
    assert_eq!(
        fs::read(&path).unwrap(),
        unresolved,
        "An unresolved ordinary job must prevent any new task request/ticket/launch write"
    );
    assert!(!task.root.join("external-task-jobs").exists());
    assert!(
        !harness.log.exists(),
        "No second Harness may be invoked while the earlier generation is unresolved"
    );
    assert_eq!(
        studio.test_prepared_request(),
        Some(&retained),
        "Refusal must not recreate or silently replace the task-specific consent"
    );
    assert_eq!(task.store.load().unwrap(), saved);
    assert!(
        studio.test_notice().contains("Resolve") || studio.test_notice().contains("unresolved"),
        "{}",
        studio.test_notice()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn restarted_stale_completion_allows_fresh_explicit_consent_for_new_settled_source() {
    fresh_consent_after_completed_restart(false);
}

#[cfg(target_os = "linux")]
#[test]
fn moved_program_after_completion_uses_new_authorized_flow_without_replaying_old_job() {
    fresh_consent_after_completed_restart(true);
}

#[cfg(target_os = "linux")]
fn fresh_consent_after_completed_restart(moved_program: bool) {
    use std::sync::atomic::Ordering;
    let task = TaskFixture::new();
    let harness = SyntheticHarness::new(&task);
    let hooks = TestHooks::default();
    let stopped = hooks.stopped.clone();
    let (mut studio, mut h) = task.open_with(hooks);
    studio.test_task_request("Original completed request must stay immutable");
    settle(&mut h, &mut studio);
    let first = studio.test_prepared_request().unwrap().clone();
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    harness.read();
    task.edit_label("Original completed source before later edits");
    product_studio::test_task_machine(&task.root, "submit", &task.record.id, &first.id).unwrap();
    await_review_control(&mut h, &mut studio);
    let path = task.root.join("studio/session.json");
    let terminal = fs::read(&path).unwrap();
    let launched = fs::read(&harness.log).unwrap();
    let saved = task.store.load().unwrap();
    task.edit_label("New settled source requiring separate consent");
    let program_path = if moved_program {
        let path = task.source.join("later-app.json");
        fs::rename(task.source.join("app.json"), &path).unwrap();
        let declaration = task.source.join(".gitmanager/product.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&declaration).unwrap()).unwrap();
        value["program_path"] = serde_json::json!("later-app.json");
        fs::write(declaration, serde_json::to_vec(&value).unwrap()).unwrap();
        path
    } else {
        task.source.join("app.json")
    };
    drop(studio);
    let start = Instant::now();
    while !stopped.load(Ordering::Acquire) {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut studio = ProductStudio::testing(task.root.clone(), None, TestHooks::default());
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.tasks");
    settle(&mut h, &mut studio);
    await_text(
        &mut h,
        &mut studio,
        "Last complete source: New settled source",
    );
    assert!(
        !studio.test_task_can_review(),
        "Historical completion cannot authorize later source review"
    );
    assert_eq!(fs::read(&path).unwrap(), terminal);
    assert_eq!(fs::read(&harness.log).unwrap(), launched);
    assert_eq!(task.store.load().unwrap(), saved);
    click(&mut h, &mut studio, "studio.task-need");
    // Restart truthfully restores the last request text. Replace it through
    // real text-edit input rather than silently appending to that saved need.
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
    h.text("A separate request for these later edits");
    frame(&mut h, &mut studio);
    let start = Instant::now();
    loop {
        let trace = frame(&mut h, &mut studio);
        if trace
            .controls
            .get("studio.external-request")
            .is_some_and(|c| c.enabled)
        {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(20), "A stale historical completion must not disable fresh explicit consent for settled current source: {:?}", trace.text);
        std::thread::sleep(Duration::from_millis(30));
    }
    click(&mut h, &mut studio, "studio.external-request");
    settle(&mut h, &mut studio);
    let second = studio.test_prepared_request().unwrap().clone();
    assert_ne!(second.id, first.id);
    assert_eq!(second.request, "A separate request for these later edits");
    assert_eq!(
        second.sources[0].binding.program_path,
        if moved_program {
            "later-app.json"
        } else {
            "app.json"
        }
    );
    assert_eq!(
        second.sources[0].program.label,
        "New settled source requiring separate consent"
    );
    assert_eq!(
        fs::read(&path).unwrap(),
        terminal,
        "Preparing fresh consent cannot alter the old completed association"
    );
    assert_eq!(
        fs::read(&harness.log).unwrap(),
        launched,
        "Disclosure alone never invokes the Harness"
    );
    assert!(!studio.test_task_can_review());
    click(&mut h, &mut studio, "studio.external-authorize");
    settle(&mut h, &mut studio);
    let start = Instant::now();
    loop {
        let logged: serde_json::Value =
            serde_json::from_slice(&fs::read(&harness.log).unwrap()).unwrap();
        if logged["journal"]["task"]["external"]["pending"]["request"]
            == serde_json::to_value(&second).unwrap()
        {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "Explicit fresh consent must invoke only its new exact bundle: {}",
            studio.test_notice()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        product_studio::test_task_machine(&task.root, "submit", &task.record.id, &first.id)
            .is_err()
    );
    let mut program = fixture::organizer();
    program["label"] = serde_json::json!("Second separately authorized completed source");
    fs::write(program_path, serde_json::to_vec(&program).unwrap()).unwrap();
    product_studio::test_task_machine(&task.root, "submit", &task.record.id, &second.id).unwrap();
    await_review_control(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.task-review");
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    assert!(frame(&mut h, &mut studio)
        .text
        .iter()
        .any(|t| t == &format!("Requested change: {}", second.request)));
    let journal: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        journal["task"]["completed"]["external"]["pending"]["request"],
        serde_json::to_value(&second).unwrap()
    );
    assert_eq!(task.store.load().unwrap(), saved);
}

#[cfg(target_os = "linux")]
#[test]
fn completed_tool_reopen_defers_analysis_and_explicit_entry_cancels_without_releasing_pause() {
    use std::sync::{atomic::Ordering, Arc};
    let task = TaskFixture::new();
    let harness = SyntheticHarness::new(&task);
    let pause = Arc::new(product_studio::TestPause::default());
    pause.release.store(true, Ordering::Release);
    let (mut studio, mut h) = task.open_with(TestHooks {
        before_task_analysis: Some(pause.clone()),
        ..Default::default()
    });
    studio.test_task_request("Retain exact completion while deferring cancellable copied analysis");
    settle(&mut h, &mut studio);
    let request = studio.test_prepared_request().unwrap().clone();
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    harness.read();
    task.edit_label("Completed program before explicit reopen");
    product_studio::test_task_machine(&task.root, "submit", &task.record.id, &request.id).unwrap();
    await_review_control(&mut h, &mut studio);
    let path = task.root.join("studio/session.json");
    let terminal = fs::read(&path).unwrap();
    let launched = fs::read(&harness.log).unwrap();
    let saved = task.store.load().unwrap();
    click(&mut h, &mut studio, "studio.tasks-back");
    settle(&mut h, &mut studio);
    pause.reached.store(false, Ordering::Release);
    pause.release.store(false, Ordering::Release);
    studio.test_open(task.root.join("tool"));
    let start = Instant::now();
    while (studio.is_busy() || studio.test_source_polling())
        && start.elapsed() < Duration::from_secs(20)
    {
        frame(&mut h, &mut studio);
        std::thread::sleep(Duration::from_millis(30));
    }
    let reopened = !studio.is_busy() && !studio.test_source_polling();
    if !reopened {
        // Release only to clean up a failing baseline; the assertion retains
        // the original 20-second gate and the fact that Open stayed blocked.
        pause.release.store(true, Ordering::Release);
        settle(&mut h, &mut studio);
    }
    assert!(
        reopened,
        "Opening daily work must not enter uncancellable copied task analysis"
    );
    assert!(!pause.reached.load(Ordering::Acquire));
    assert_eq!(studio.test_page(), "daily");
    assert_eq!(fs::read(&path).unwrap(), terminal);
    click(&mut h, &mut studio, "studio.tasks");
    let start = Instant::now();
    while !pause.reached.load(Ordering::Acquire) {
        frame(&mut h, &mut studio);
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "Explicit entry must still reach copied analysis"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(studio.test_cancel());
    let start = Instant::now();
    while (studio.is_busy() || studio.test_source_polling())
        && start.elapsed() < Duration::from_secs(20)
    {
        frame(&mut h, &mut studio);
        std::thread::sleep(Duration::from_millis(30));
    }
    let cancelled = !studio.is_busy() && !studio.test_source_polling();
    // Cancellation itself must release the worker, without fixture assistance.
    pause.release.store(true, Ordering::Release);
    settle(&mut h, &mut studio);
    assert!(
        cancelled,
        "The actual Tasks operation Gate must stop copied analysis"
    );
    assert!(
        !studio.test_task_can_review(),
        "Cancelled analysis cannot be admitted later"
    );
    assert_eq!(fs::read(&path).unwrap(), terminal);
    assert_eq!(fs::read(&harness.log).unwrap(), launched);
    assert_eq!(task.store.load().unwrap(), saved);
}

fn await_task_pause(
    h: &mut EguiHarness,
    studio: &mut ProductStudio,
    pause: &product_studio::TestPause,
) {
    use std::sync::atomic::Ordering;
    let start = Instant::now();
    while !pause.reached.load(Ordering::Acquire) {
        frame(h, studio);
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "Publication boundary was not reached: {}",
            studio.test_notice()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn cancelled_task_analysis_cannot_publish_its_queue_after_the_final_check() {
    task_publication_cancel(false);
}

#[cfg(target_os = "linux")]
#[test]
fn cancelled_task_review_cannot_publish_a_change_page_after_the_final_check() {
    task_publication_cancel(true);
}

#[cfg(target_os = "linux")]
fn task_publication_cancel(page: bool) {
    use std::sync::{atomic::Ordering, Arc};
    let task = TaskFixture::new();
    let harness = SyntheticHarness::new(&task);
    let before = Arc::new(product_studio::TestPause::default());
    let after = Arc::new(product_studio::TestPause::default());
    before.release.store(true, Ordering::Release);
    after.release.store(true, Ordering::Release);
    let (mut studio, mut h) = task.open_with(TestHooks {
        before_task_review_publish: Some(before.clone()),
        after_task_review_publish: Some(after.clone()),
        ..Default::default()
    });
    studio.test_task_request("Keep this exact completion across publication cancellation");
    settle(&mut h, &mut studio);
    let request = studio.test_prepared_request().unwrap().clone();
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    harness.read();
    task.edit_label("Publication boundary completed source");
    product_studio::test_task_machine(&task.root, "submit", &task.record.id, &request.id).unwrap();
    await_review_control(&mut h, &mut studio);
    let path = task.root.join("studio/session.json");
    let terminal = fs::read(&path).unwrap();
    let launched = fs::read(&harness.log).unwrap();
    let saved = task.store.load().unwrap();
    if !page {
        click(&mut h, &mut studio, "studio.tasks-back");
        settle(&mut h, &mut studio);
        studio.test_open(task.root.join("tool"));
        settle(&mut h, &mut studio);
    }
    before.reached.store(false, Ordering::Release);
    before.release.store(false, Ordering::Release);
    click(
        &mut h,
        &mut studio,
        if page {
            "studio.task-review"
        } else {
            "studio.tasks"
        },
    );
    await_task_pause(&mut h, &mut studio, &before);
    let accepted = studio.test_cancel();
    before.release.store(true, Ordering::Release);
    settle(&mut h, &mut studio);
    assert!(accepted, "Cancellation before publication must be accepted");
    if page {
        assert_eq!(
            studio.test_page(),
            "tasks",
            "A cancelled review cannot install its Change page"
        );
    } else {
        // A reopened watcher can still be Pending when cancellation returns.
        // Judge admission after the unchanged source settles, so that status
        // cannot mask a leaked review queue.
        await_text(&mut h, &mut studio, "Complete source captured;");
        assert!(
            !studio.test_task_can_review(),
            "Cancelled analysis cannot install a reviewable queue"
        );
    }
    assert_eq!(fs::read(&path).unwrap(), terminal);
    assert_eq!(task.store.load().unwrap(), saved);
    if !page {
        click(&mut h, &mut studio, "studio.tasks-back");
        settle(&mut h, &mut studio);
    }
    after.reached.store(false, Ordering::Release);
    after.release.store(false, Ordering::Release);
    click(
        &mut h,
        &mut studio,
        if page {
            "studio.task-review"
        } else {
            "studio.tasks"
        },
    );
    await_task_pause(&mut h, &mut studio, &after);
    let accepted = studio.test_cancel();
    after.release.store(true, Ordering::Release);
    settle(&mut h, &mut studio);
    assert!(
        !accepted,
        "Published evidence has crossed its atomic completion boundary"
    );
    if page {
        assert_eq!(studio.test_page(), "change");
    } else {
        assert!(studio.test_task_can_review());
    }
    if page {
        assert_only_checked_external_lineage_added(&terminal, &fs::read(&path).unwrap());
    } else {
        assert_eq!(fs::read(&path).unwrap(), terminal);
    }
    assert_eq!(fs::read(&harness.log).unwrap(), launched);
    assert_eq!(task.store.load().unwrap(), saved);
}

#[cfg(target_os = "linux")]
#[test]
fn task_abandonment_cancellation_is_accepted_only_before_durable_revocation() {
    use std::sync::{atomic::Ordering, Arc};
    let task = TaskFixture::new();
    let harness = SyntheticHarness::new(&task);
    let before = Arc::new(product_studio::TestPause::default());
    let writing = Arc::new(product_studio::TestPause::default());
    writing.release.store(true, Ordering::Release);
    let (mut studio, mut h) = task.open_with(TestHooks {
        before_abandon: Some(before.clone()),
        before_task_abandon_write: Some(writing.clone()),
        ..Default::default()
    });
    studio.test_task_request("Only explicit committed abandonment revokes this intake");
    settle(&mut h, &mut studio);
    let request = studio.test_prepared_request().unwrap().clone();
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    harness.read();
    let launched = fs::read(&harness.log).unwrap();
    let path = task.root.join("studio/session.json");
    let pending = fs::read(&path).unwrap();
    let saved = task.store.load().unwrap();
    click(&mut h, &mut studio, "studio.task-abandon");
    await_task_pause(&mut h, &mut studio, &before);
    assert!(studio.test_cancel());
    settle(&mut h, &mut studio);
    assert_eq!(fs::read(&path).unwrap(), pending);
    product_studio::test_task_machine(&task.root, "prepare", &task.record.id, &request.id).unwrap();
    before.release.store(true, Ordering::Release);
    writing.reached.store(false, Ordering::Release);
    writing.release.store(false, Ordering::Release);
    click(&mut h, &mut studio, "studio.task-abandon");
    await_task_pause(&mut h, &mut studio, &writing);
    let accepted = studio.test_cancel();
    writing.release.store(true, Ordering::Release);
    settle(&mut h, &mut studio);
    assert!(
        !accepted,
        "Cancellation cannot report success after durable revocation has begun"
    );
    assert!(
        product_studio::test_task_machine(&task.root, "prepare", &task.record.id, &request.id)
            .is_err()
    );
    let journal: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(journal["task"]["external"].is_null());
    assert_eq!(fs::read(&harness.log).unwrap(), launched);
    assert_eq!(task.store.load().unwrap(), saved);
}

#[test]
fn cancelled_task_disclosure_cannot_replace_the_exact_prepared_request() {
    use std::sync::{atomic::Ordering, Arc};
    let task = TaskFixture::new();
    let before = Arc::new(product_studio::TestPause::default());
    let after = Arc::new(product_studio::TestPause::default());
    before.release.store(true, Ordering::Release);
    after.release.store(true, Ordering::Release);
    let (mut studio, mut h) = task.open_with(TestHooks {
        before_task_review_publish: Some(before.clone()),
        after_task_review_publish: Some(after.clone()),
        ..Default::default()
    });
    studio.test_task_request("Original exact prepared request");
    settle(&mut h, &mut studio);
    let original = studio.test_prepared_request().unwrap().clone();
    let path = task.root.join("studio/session.json");
    let journal = fs::read(&path).unwrap();
    let saved = task.store.load().unwrap();
    before.reached.store(false, Ordering::Release);
    before.release.store(false, Ordering::Release);
    studio.test_task_request("This cancelled replacement must not be published");
    await_task_pause(&mut h, &mut studio, &before);
    let accepted = studio.test_cancel();
    before.release.store(true, Ordering::Release);
    settle(&mut h, &mut studio);
    assert!(accepted);
    assert_eq!(
        studio.test_prepared_request(),
        Some(&original),
        "Cancelled disclosure cannot replace previously prepared exact consent"
    );
    after.reached.store(false, Ordering::Release);
    after.release.store(false, Ordering::Release);
    studio.test_task_request("Explicit successfully published replacement");
    await_task_pause(&mut h, &mut studio, &after);
    let accepted = studio.test_cancel();
    after.release.store(true, Ordering::Release);
    settle(&mut h, &mut studio);
    assert!(
        !accepted,
        "Cancellation is too late after exact disclosure publication"
    );
    let replacement = studio.test_prepared_request().unwrap();
    assert_ne!(replacement.id, original.id);
    assert_eq!(
        replacement.request,
        "Explicit successfully published replacement"
    );
    assert_eq!(fs::read(&path).unwrap(), journal);
    assert_eq!(task.store.load().unwrap(), saved);
    assert!(
        !task.root.join("external-task-jobs").exists(),
        "Publishing consent must not create or launch a job"
    );
}

// A saved Pending choice retains its exact captured task provenance across a
// new draft operation and process restart; current source must never replace it.
fn resumed_task_choice_freshness(mode: &str, outcome: product_contract::DecisionOutcome) {
    use std::sync::{atomic::Ordering, Arc};
    let task = TaskFixture::new();
    task.store
        .apply(
            0,
            "seed-resumed-person",
            &fixture::add("Synthetic saved choice"),
            product_contract::RuntimeLimits::default(),
        )
        .unwrap();
    let mut candidate = fixture::organizer();
    candidate["actions"][2]["steps"][0]["items"] = fixture::query("person");
    fs::write(
        task.source.join("app.json"),
        serde_json::to_vec(&candidate).unwrap(),
    )
    .unwrap();
    let hooks = TestHooks::default();
    let stopped = hooks.stopped.clone();
    let (mut studio, mut h) = task.open_with(hooks);
    await_review_control(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.task-review");
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    studio.test_trial(fixture::invoke(
        "export_people",
        product_contract::Values::new(),
    ));
    settle(&mut h, &mut studio);
    studio.test_decide(product_contract::DecisionOutcome::Deferred);
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    let saved = task.store.load().unwrap();
    let choice = saved.decisions.decisions.last().unwrap().clone();
    assert_eq!(choice.status, product_contract::DecisionStatus::Pending);
    assert!(saved.programs.iter().any(|p| p
        .binding
        .task
        .as_ref()
        .is_some_and(|t| t.task_id == task.record.id)));
    drop(studio);
    let start = Instant::now();
    while !stopped.load(Ordering::Acquire) {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "First worker did not stop"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let pause = Arc::new(product_studio::TestPause::default());
    pause.release.store(mode != "final", Ordering::Release);
    let mut studio = ProductStudio::testing(
        task.root.clone(),
        None,
        TestHooks {
            before_commit: Some(pause.clone()),
            ..Default::default()
        },
    );
    settle(&mut h, &mut studio);
    studio.test_open(task.root.join("tool"));
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    if mode == "entry" {
        fs::write(
            task.source.join("unrelated-resumed-source.txt"),
            "Changed before retained choice entry",
        )
        .unwrap();
    }
    click(&mut h, &mut studio, &format!("studio.resume.{}", choice.id));
    settle(&mut h, &mut studio);
    if mode == "entry" {
        assert_eq!(
            studio.test_page(),
            "daily",
            "Stale retained task source must not publish a new copied review"
        );
        assert!(
            studio.test_notice().contains("Task source changed"),
            "{}",
            studio.test_notice()
        );
        assert_eq!(
            task.store.load().unwrap(),
            saved,
            "Refusal must retain the exact saved choice and work"
        );
        return;
    }
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    studio.test_trial(fixture::invoke(
        "export_people",
        product_contract::Values::new(),
    ));
    settle(&mut h, &mut studio);
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
    studio.test_decide(outcome.clone());
    if mode == "final" {
        let start = Instant::now();
        while !pause.reached.load(Ordering::Acquire) {
            frame(&mut h, &mut studio);
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "Final gate not reached: {}",
                studio.test_notice()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        fs::write(
            task.source.join("unrelated-resumed-source.txt"),
            "Changed after retained choice analysis",
        )
        .unwrap();
        pause.release.store(true, Ordering::Release);
    }
    settle(&mut h, &mut studio);
    let after = task.store.load().unwrap();
    if mode == "final" {
        assert_eq!(after, saved, "A resumed task choice must retain its original full-source freshness at the final recording gate");
        assert!(
            studio.test_notice().contains("Task source changed"),
            "{}",
            studio.test_notice()
        );
        let journal: serde_json::Value =
            serde_json::from_slice(&fs::read(task.root.join("studio/session.json")).unwrap())
                .unwrap();
        assert!(journal["pending"].is_null());
    } else {
        assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
        assert_eq!(after.data, saved.data);
        assert!(matches!(
            after
                .decisions
                .decisions
                .iter()
                .find(|d| d.id == choice.id)
                .unwrap()
                .status,
            product_contract::DecisionStatus::Superseded { .. }
        ));
        assert_eq!(after.decisions.decisions.last().unwrap().outcome, outcome);
        assert!(after
            .programs
            .iter()
            .any(|p| saved.programs.contains(p) && p.binding.task.is_some()));
    }
}
#[test]
fn unchanged_saved_task_choice_reopens_after_restart_and_resolves_without_source_substitution() {
    resumed_task_choice_freshness("control", product_contract::DecisionOutcome::KeepCurrent);
}
#[test]
fn changed_task_source_refuses_saved_choice_reentry_without_consuming_it() {
    resumed_task_choice_freshness("entry", product_contract::DecisionOutcome::KeepCurrent);
}
#[test]
fn resumed_task_keep_current_rechecks_source_at_final_recording_gate() {
    resumed_task_choice_freshness("final", product_contract::DecisionOutcome::KeepCurrent);
}
#[test]
fn resumed_task_accept_rechecks_source_at_final_recording_gate() {
    resumed_task_choice_freshness(
        "final",
        product_contract::DecisionOutcome::Accept {
            artifact: fixture::capture(fixture::organizer())
                .artifact
                .program_digest,
        },
    );
}
#[test]
fn resumed_task_either_acceptable_rechecks_source_at_final_recording_gate() {
    resumed_task_choice_freshness("final", product_contract::DecisionOutcome::EitherAcceptable);
}
#[test]
fn resumed_task_both_needed_rechecks_source_at_final_recording_gate() {
    resumed_task_choice_freshness("final", product_contract::DecisionOutcome::BothNeeded);
}
#[test]
fn resumed_task_neither_fits_rechecks_source_at_final_recording_gate() {
    resumed_task_choice_freshness("final", product_contract::DecisionOutcome::NeitherFits);
}
#[test]
fn resumed_task_deferred_rechecks_source_at_final_recording_gate() {
    resumed_task_choice_freshness("final", product_contract::DecisionOutcome::Deferred);
}

#[cfg(target_os = "linux")]
fn external_discovery_transport(task: &TaskFixture) -> product_provider::ProviderTransport {
    use std::os::unix::fs::PermissionsExt;
    let executable = task.root.join("external-discovery-synthetic.py");
    let guard = product_provider::fixture_executable_write_guard();
    let body = "{'version':1,'request_digest':json.loads(wire['prompt'])['request_digest'],'candidates':[],'hypotheses':[],'evolutions':[],'unsupported':['Synthetic external discovery has no hypotheses']}";
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
    let home = task.root.join("external-discovery-home");
    fs::create_dir(&home).unwrap();
    let transport = product_provider::ProviderTransport::new_fixture(
        task.root.join("discovery-jobs"),
        product_provider::ProviderKind::Codex,
        executable,
        home,
    )
    .unwrap();
    drop(guard);
    transport
}

#[cfg(target_os = "linux")]
#[test]
fn discover_uses_actual_external_capture_and_plain_original_need_without_fake_modify_receipt() {
    let task = TaskFixture::new();
    let harness = SyntheticHarness::new(&task);
    let transport = external_discovery_transport(&task);
    let (mut studio, mut h) = task.open_with_transport(TestHooks::default(), Some(transport));
    let need = "Keep my exact human request separate from inherited task guidance";
    studio.test_task_request(need);
    settle(&mut h, &mut studio);
    let original = studio.test_prepared_request().unwrap().clone();
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    assert_eq!(harness.read()["synthetic"], true);
    let mut edited = fixture::organizer();
    edited["label"] = serde_json::json!("Actual external discovery source");
    let kind = edited["views"][0]["kind"].clone();
    edited["views"].as_array_mut().unwrap().push(serde_json::json!({"id":"second_list","label":"Actual external view","kind":kind,"actions":[],"keys":[]}));
    fs::write(
        task.source.join("app.json"),
        serde_json::to_vec(&edited).unwrap(),
    )
    .unwrap();
    product_studio::test_task_machine(&task.root, "submit", &task.record.id, &original.id).unwrap();
    studio.test_task_refresh();
    settle(&mut h, &mut studio);
    await_text(&mut h, &mut studio, "Actual external discovery source");
    await_review_control(&mut h, &mut studio);
    studio.test_task_review();
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    let journal: serde_json::Value =
        serde_json::from_slice(&fs::read(task.root.join("studio/session.json")).unwrap()).unwrap();
    assert_eq!(
        journal["task"]["completed"]["external"]["original_need"],
        need
    );
    assert_eq!(journal["discovery"]["need"], need);
    assert!(
        journal["discovery"]["original"].is_null(),
        "ExternalAuthor must never acquire a fake Modify receipt or response"
    );
    let before = task.store.load().unwrap();
    click(&mut h, &mut studio, "studio.discover");
    settle(&mut h, &mut studio);
    let request = studio.test_prepared_request().unwrap().clone();
    assert_eq!(
        request.operation,
        product_contract::DevelopmentOperation::Discover
    );
    assert_ne!(request.id, original.id);
    assert_eq!(request.context, original.context);
    assert_eq!(request.sources[0], before.program().unwrap().clone());
    assert!(matches!(
        request.sources[1].binding.producer,
        product_contract::Producer::ExternalAuthor { .. }
    ));
    assert_eq!(
        request.sources[1].source_bytes,
        fs::read(task.source.join("app.json")).unwrap()
    );
    click(&mut h, &mut studio, "studio.consent");
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "discovery", "{}", studio.test_notice());
    assert_eq!(task.store.load().unwrap(), before);
}

#[cfg(unix)]
fn wide_fixture_transport(root: &Path) -> product_provider::ProviderTransport {
    use std::os::unix::fs::PermissionsExt;
    let executable = root.join("wide-discovery-fixture.py");
    let _guard = product_provider::fixture_executable_write_guard();
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
    let home = root.join("wide-fixture-home");
    fs::create_dir(&home).unwrap();
    product_provider::ProviderTransport::new_fixture(
        root.join("wide-fixture-jobs"),
        product_provider::ProviderKind::Codex,
        executable,
        home,
    )
    .unwrap()
}
fn wide_response(root: &Path, response: &product_contract::DevelopmentResponse) {
    fs::write(
        root.join("wide-discovery-fixture.json"),
        serde_json::to_vec(&serde_json::json!({"mode":"good", "response":response})).unwrap(),
    )
    .unwrap();
}
fn stop_wide(studio: ProductStudio, stopped: &std::sync::atomic::AtomicBool) {
    drop(studio);
    let start = Instant::now();
    while !stopped.load(std::sync::atomic::Ordering::Acquire) {
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "The sole writer must stop before fresh reopen"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[cfg(unix)]
#[test]
fn authentic_wide_managed_host_journal_fits_at_every_boundary_with_real_task_and_recovery() {
    use product_contract::*;
    use std::sync::{atomic::Ordering, Arc, Mutex};
    let task = TaskFixture::with_program(scope_fixture::program(false));
    let backup = task.root.join("original.gmbak");
    product_backup::VerifiedBackup::capture(&task.store)
        .unwrap()
        .export_new(&backup)
        .unwrap();
    product_studio::test_stage_daily(
        &task.root,
        &task.root.join("tool"),
        scope_fixture::invoke(
            "add",
            &[
                ("name", scope_fixture::text("Original unconfirmed work")),
                ("promised", DataValue::Date { days: 20020 }),
            ],
        ),
        false,
    );
    let journal_path = task.root.join("studio/session.json");
    let pending = serde_json::from_slice::<serde_json::Value>(&fs::read(&journal_path).unwrap())
        .unwrap()["pending"]
        .clone();
    fs::write(
        task.root.join("tool/CURRENT"),
        b"Synthetic damaged original pointer",
    )
    .unwrap();
    let hooks = TestHooks {
        folder_choice: Some(backup),
        ..Default::default()
    };
    let stopped = hooks.stopped.clone();
    let mut studio = ProductStudio::testing(task.root.clone(), None, hooks);
    let mut h = EguiHarness::new(egui::vec2(1500.0, 5000.0));
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.import");
    settle(&mut h, &mut studio);
    let recovered = studio
        .test_recovery_target()
        .expect(studio.test_notice())
        .to_path_buf();
    click(&mut h, &mut studio, "studio.recover");
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.open-recovered");
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_location(), Some(recovered.as_path()));
    let recovery = serde_json::from_slice::<serde_json::Value>(&fs::read(&journal_path).unwrap())
        .unwrap()["recovery_handoffs"]
        .clone();
    assert_eq!(recovery[0]["original"], pending);
    stop_wide(studio, &stopped);
    let store = ProductStore::open(&recovered).unwrap();
    let (_, _, candidate) = wide_fixture::managed(&store, false);
    let current = store.load().unwrap();
    assert_eq!(current.clock_day, 20003);
    let transport = wide_fixture_transport(&task.root);
    let hooks = TestHooks::default();
    hooks.today.store(20003, Ordering::Release);
    let stopped = hooks.stopped.clone();
    let mut studio = ProductStudio::testing(task.root.clone(), Some(transport.clone()), hooks);
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_location(), Some(recovered.as_path()));
    click(&mut h, &mut studio, "studio.tasks");
    settle(&mut h, &mut studio);
    click(
        &mut h,
        &mut studio,
        &format!("studio.task.{}", task.record.id),
    );
    settle(&mut h, &mut studio);
    let linked = serde_json::from_slice::<serde_json::Value>(&fs::read(&journal_path).unwrap())
        .unwrap()["task"]
        .clone();
    assert_eq!(linked["task_id"], task.record.id);
    assert!(linked["external"].is_null() && linked["completed"].is_null());
    click(&mut h, &mut studio, "studio.tasks-back");
    settle(&mut h, &mut studio);
    let publications = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let observer = product_studio::journal_testing::observe_synced(&task.root, {
        let publications = publications.clone();
        move |bytes| {
            eprintln!(
                "Observed authentic wide synced journal: {} bytes",
                bytes.len()
            );
            publications.lock().unwrap().push(bytes);
        }
    });
    let need = "Change how waiting contributes to production; preserve commitments";
    studio.test_modify(need);
    settle(&mut h, &mut studio);
    let modify = studio
        .test_prepared_request()
        .unwrap_or_else(|| panic!("Wide Modify disclosure: {}", studio.test_notice()))
        .clone();
    wide_response(
        &task.root,
        &DevelopmentResponse {
            version: 1,
            request_digest: modify.identity().unwrap(),
            candidates: vec![GeneratedCandidate {
                id: "actual-change".into(),
                source_json: String::from_utf8(candidate.source_bytes.clone()).unwrap(),
            }],
            hypotheses: vec![],
            evolutions: vec![],
            unsupported: vec![],
        },
    );
    studio.test_consent();
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    click(&mut h, &mut studio, "studio.scope.future");
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.discover");
    settle(&mut h, &mut studio);
    let discover = studio
        .test_prepared_request()
        .unwrap_or_else(|| panic!("Wide Discover disclosure: {}", studio.test_notice()))
        .clone();
    assert_eq!(discover.operation, DevelopmentOperation::Discover);
    let domain_size = serde_json::to_vec(&discover).unwrap().len();
    let wire = product_discovery::encode_request(&discover, &Default::default()).unwrap();
    assert!(
        domain_size > product_provider::MAX_PROMPT_BYTES,
        "The real wide domain must exercise lossless compact request intake: {domain_size}"
    );
    assert!(wire.prompt.len() <= product_provider::MAX_PROMPT_BYTES);
    let (original_scene, created) = wide_fixture::created_scene(&current);
    let mut scene = original_scene.clone();
    scene.inputs = vec![
        original_scene.inputs[0].clone(),
        scope_fixture::invoke("calculate", &[("row", scope_fixture::reference(&created))]),
        SemanticInput::Observe {
            point: "result".into(),
        },
    ];
    scene.validate(&current.program().unwrap().program).unwrap();
    let after = &discover.sources[1];
    wide_response(
        &task.root,
        &DevelopmentResponse {
            version: 1,
            request_digest: discover.identity().unwrap(),
            candidates: vec![
                GeneratedCandidate {
                    id: "before".into(),
                    source_json: String::from_utf8(discover.sources[0].source_bytes.clone())
                        .unwrap(),
                },
                GeneratedCandidate {
                    id: "after".into(),
                    source_json: String::from_utf8(after.source_bytes.clone()).unwrap(),
                },
            ],
            hypotheses: vec![ChoiceHypothesis {
                id: "waiting-choice".into(),
                statement: "New work shows an extra production day under the changed calculation"
                    .into(),
                kind: HypothesisKind::UnresolvedChoice,
                action: "calculate".into(),
                observable: "waiting_jobs".into(),
                sources: vec![SourceLocus {
                    relative_path: after.binding.program_path.clone(),
                    raw_digest: after.artifact.raw_digest.clone(),
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
    );
    click(&mut h, &mut studio, "studio.consent");
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "discovery", "{}", studio.test_notice());
    let trace = frame(&mut h, &mut studio);
    let copy = trace
        .controls
        .iter()
        .find(|(key, c)| key.starts_with("studio.discovery.copy.") && c.enabled)
        .map(|(key, _)| key.clone())
        .unwrap_or_else(|| {
            panic!(
                "The authentic wide rule needs a real checked business copy: {:?}",
                trace.text
            )
        });
    click(&mut h, &mut studio, &copy);
    settle(&mut h, &mut studio);
    assert_eq!(
        studio.test_page(),
        "change",
        "A fixed rule must use its real checked receiver: {}",
        studio.test_notice()
    );
    click(&mut h, &mut studio, "studio.either");
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    let saved = store.load().unwrap();
    assert_eq!(saved.active_revision, current.active_revision);
    assert_eq!(saved.data, current.data);
    assert!(matches!(
        saved.decisions.decisions.last().unwrap().outcome,
        DecisionOutcome::EitherAcceptable
    ));
    let terminal = fs::read(&journal_path).unwrap();
    stop_wide(studio, &stopped);
    let hooks = TestHooks::default();
    hooks.today.store(20003, Ordering::Release);
    let stopped = hooks.stopped.clone();
    let mut studio = ProductStudio::testing(task.root.clone(), None, hooks);
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.reopen-discovery");
    settle(&mut h, &mut studio);
    assert!(frame(&mut h, &mut studio)
        .text
        .iter()
        .any(|line| line.contains("report is stale")));
    assert!(
        store.load().unwrap() == saved,
        "Restart must not repeat the pending choice or business work"
    );
    let restart = fs::read(&journal_path).unwrap();
    stop_wide(studio, &stopped);
    drop(observer);
    let mut records = publications.lock().unwrap().clone();
    records.push(terminal);
    records.push(restart);
    let mut stages = std::collections::BTreeSet::new();
    let mut max_bytes = 0;
    for bytes in &records {
        max_bytes = max_bytes.max(bytes.len());
        assert!(bytes.len() <= MAX_WIRE_BYTES);
        assert_eq!(
            product_studio::journal_testing::roundtrip(bytes).unwrap(),
            *bytes
        );
        let value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        assert_eq!(value["task"], linked);
        assert_eq!(value["recovery_handoffs"], recovery);
        for (request, completion) in product_studio::journal_testing::invocations(bytes).unwrap() {
            assert!(
                request == modify || request == discover,
                "Retained requests must be exact host-prepared domains"
            );
            if let Some((raw, receipt)) = completion {
                let wire =
                    product_discovery::encode_request(&request, &Default::default()).unwrap();
                let actual = transport
                    .ingest(&request.id, &wire.digest().unwrap(), &wire.source_digest)
                    .unwrap()
                    .expect("The actual completed fixture response must remain correlated");
                assert_eq!(raw, actual.final_bytes);
                assert_eq!(
                    canonical_bytes(&receipt).unwrap(),
                    canonical_bytes(&actual.receipt).unwrap()
                );
            }
        }
        let origin = &value["discovery"]["original"];
        let disc = &value["discovery"]["discover"];
        if !origin["completed"].is_null() && value["provider"]["lineage_request"] == modify.id {
            stages.insert("modify-terminal");
        }
        if !origin["completed"].is_null() && value["provider"].is_null() {
            stages.insert("modify-cleared");
        }
        if !disc.is_null() && value["provider"]["issued"] == false {
            stages.insert("discover-prepared");
        }
        if disc["issued"] == true && disc["completed"].is_null() {
            stages.insert("discover-issued");
        }
        if !disc["completed"].is_null() && value["provider"].is_null() {
            stages.insert("discover-result");
        }
        if value["pending"]["kind"] == "change" {
            stages.insert("choice-prepared");
        }
        eprintln!("Authentic wide complete host publication: {} bytes; provider {:?}; pending {:?}; Modify raw {}; Discover raw {}",bytes.len(),value["provider"]["issued"],value["pending"]["kind"],origin["completed"]["raw"]["raw_bytes"],disc["completed"]["raw"]["raw_bytes"]);
    }
    assert_eq!(
        stages,
        [
            "modify-terminal",
            "modify-cleared",
            "discover-prepared",
            "discover-issued",
            "discover-result",
            "choice-prepared"
        ]
        .into()
    );
    let jobs: Vec<_> = fs::read_dir(task.root.join("wide-fixture-jobs"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_type().unwrap().is_dir())
        .collect();
    assert_eq!(jobs.len(), 2, "Restart cannot issue a third provider job");
    eprintln!("Authentic wide domain {domain_size}; exact prompt {}; maximum complete journal {max_bytes}; {} real synced publications plus terminal/restart reads",wire.prompt.len(),records.len()-2);
}

#[cfg(target_os = "linux")]
fn completed_external_discovery_origin(
    task: &TaskFixture,
    hooks: TestHooks,
) -> (ProductStudio, EguiHarness) {
    completed_external_discovery_origin_with_transport(task, hooks, None)
}
#[cfg(target_os = "linux")]
fn completed_external_discovery_origin_with_transport(
    task: &TaskFixture,
    hooks: TestHooks,
    transport: Option<product_provider::ProviderTransport>,
) -> (ProductStudio, EguiHarness) {
    let (mut studio, mut h) = task.open_with_transport(hooks, transport);
    studio.test_task_request("Exact original need for retained external discovery");
    settle(&mut h, &mut studio);
    let request = studio.test_prepared_request().unwrap().clone();
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    task.edit_label("Actual external origin for correlation");
    product_studio::test_task_machine(&task.root, "submit", &task.record.id, &request.id).unwrap();
    studio.test_task_refresh();
    settle(&mut h, &mut studio);
    await_text(
        &mut h,
        &mut studio,
        "Actual external origin for correlation",
    );
    await_review_control(&mut h, &mut studio);
    studio.test_task_review();
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    let bytes = fs::read(task.root.join("studio/session.json")).unwrap();
    let record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(record["discovery"]["external"].is_object());
    assert!(record["task"]["completed"].is_object());
    product_studio::journal_testing::roundtrip(&bytes)
        .expect("The actual untampered complete origin must decode");
    (studio, h)
}
#[cfg(target_os = "linux")]
#[test]
fn external_discovery_restart_rejects_approval_not_matching_actual_completed_handoff() {
    let task = TaskFixture::new();
    let harness = SyntheticHarness::new(&task);
    let hooks = TestHooks::default();
    let stopped = hooks.stopped.clone();
    let (studio, mut h) = completed_external_discovery_origin(&task, hooks);
    harness.read();
    let saved = task.store.load().unwrap();
    stop_wide(studio, &stopped);
    let path = task.root.join("studio/session.json");
    let mut journal: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let original = journal["task"]["completed"]["external"]["approval_operation"].clone();
    journal["discovery"]["external"]["approval_operation"] =
        serde_json::json!("different-valid-approval");
    assert_ne!(
        journal["discovery"]["external"]["approval_operation"],
        original
    );
    let tampered = serde_json::to_vec(&journal).unwrap();
    fs::write(&path, &tampered).unwrap();
    let mut restarted = ProductStudio::testing(task.root.clone(), None, TestHooks::default());
    settle(&mut h, &mut restarted);
    assert!(
        restarted.test_generation_blocked(),
        "Contradictory external approval was accepted on restart: {}",
        restarted.test_notice()
    );
    assert_eq!(fs::read(&path).unwrap(), tampered);
    assert_eq!(task.store.load().unwrap(), saved);
}
#[cfg(target_os = "linux")]
#[test]
fn external_discovery_cannot_replace_the_plain_need_even_when_both_local_copies_agree() {
    let task = TaskFixture::new();
    let harness = SyntheticHarness::new(&task);
    let hooks = TestHooks::default();
    let stopped = hooks.stopped.clone();
    let (studio, mut h) = completed_external_discovery_origin(&task, hooks);
    harness.read();
    let saved = task.store.load().unwrap();
    stop_wide(studio, &stopped);
    let path = task.root.join("studio/session.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["discovery"]["need"] =
        serde_json::json!("A different human request that was never authorized");
    value["task"]["completed"]["external"]["original_need"] = value["discovery"]["need"].clone();
    let tampered = serde_json::to_vec(&value).unwrap();
    product_studio::journal_testing::roundtrip(&tampered)
        .expect("The bounded correlated fields alone are insufficient to prove the original need");
    fs::write(&path, &tampered).unwrap();
    let mut restarted = ProductStudio::testing(task.root.clone(), None, TestHooks::default());
    settle(&mut h, &mut restarted);
    click(&mut h, &mut restarted, "studio.reopen-discovery");
    settle(&mut h, &mut restarted);
    assert_ne!(restarted.test_page(), "change");
    assert!(
        restarted.test_notice().contains("original human need"),
        "{}",
        restarted.test_notice()
    );
    assert_eq!(fs::read(&path).unwrap(), tampered);
    assert_eq!(task.store.load().unwrap(), saved);
}
#[cfg(target_os = "linux")]
#[test]
fn legacy_completed_task_without_plain_need_remains_reviewable_but_cannot_invent_discovery_lineage()
{
    let task = TaskFixture::new();
    let harness = SyntheticHarness::new(&task);
    let hooks = TestHooks::default();
    let stopped = hooks.stopped.clone();
    let (studio, mut h) = completed_external_discovery_origin(&task, hooks);
    harness.read();
    let saved = task.store.load().unwrap();
    stop_wide(studio, &stopped);
    let path = task.root.join("studio/session.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("discovery");
    value["task"]["completed"]["external"]
        .as_object_mut()
        .unwrap()
        .remove("original_need");
    let legacy = serde_json::to_vec(&value).unwrap();
    product_studio::journal_testing::roundtrip(&legacy).unwrap();
    fs::write(&path, &legacy).unwrap();
    let mut studio = ProductStudio::testing(task.root.clone(), None, TestHooks::default());
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.tasks");
    settle(&mut h, &mut studio);
    await_review_control(&mut h, &mut studio);
    studio.test_task_review();
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "change", "{}", studio.test_notice());
    assert!(frame(&mut h, &mut studio)
        .text
        .iter()
        .any(|text| text.contains("Original human need unavailable")));
    click(&mut h, &mut studio, "studio.discover");
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "change");
    assert!(studio.test_prepared_request().is_none());
    let retained: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(retained["discovery"].is_null());
    assert!(retained["task"]["completed"]["external"]["original_need"].is_null());
    assert_eq!(task.store.load().unwrap(), saved);
}
#[cfg(target_os = "linux")]
#[test]
fn new_explicit_task_authorization_replaces_only_its_older_external_discovery_lineage() {
    let task = TaskFixture::new();
    let harness = SyntheticHarness::new(&task);
    let (mut studio, mut h) = completed_external_discovery_origin(&task, TestHooks::default());
    harness.read();
    let saved = task.store.load().unwrap();
    click(&mut h, &mut studio, "studio.return");
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.tasks");
    settle(&mut h, &mut studio);
    await_text(
        &mut h,
        &mut studio,
        "Actual external origin for correlation",
    );
    studio.test_task_request("A separately authorized new external task request");
    settle(&mut h, &mut studio);
    assert!(
        studio.test_prepared_request().is_some(),
        "{}",
        studio.test_notice()
    );
    let path = task.root.join("studio/session.json");
    let prepared: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(
        prepared["discovery"]["external"].is_object(),
        "Disclosure alone must preserve the old exact lineage"
    );
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    let issued: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(
        issued["task"]["external"].is_object(),
        "{}",
        studio.test_notice()
    );
    assert!(issued["task"]["completed"].is_null());
    assert!(
        issued["discovery"].is_null(),
        "An explicitly replaced completion cannot leave its old Discover authority current"
    );
    assert_eq!(task.store.load().unwrap(), saved);
}

#[cfg(target_os = "linux")]
#[test]
fn explicit_relink_supersedes_uncommitted_external_analysis_but_background_edit_does_not() {
    let task = TaskFixture::new();
    let harness = SyntheticHarness::new(&task);
    let (mut studio, mut h) = completed_external_discovery_origin(&task, TestHooks::default());
    harness.read();
    let saved = task.store.load().unwrap();
    let path = task.root.join("studio/session.json");
    let retained: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let source = task.data_home.join("second-source");
    let repo = git2::Repository::init(&source).unwrap();
    let tree = repo
        .find_tree(repo.index().unwrap().write_tree().unwrap())
        .unwrap();
    let who = git2::Signature::now("Synthetic fixture", "fixture@invalid.example").unwrap();
    repo.commit(
        Some("HEAD"),
        &who,
        &who,
        "Synthetic second task",
        &tree,
        &[],
    )
    .unwrap();
    fs::create_dir(source.join(".gitmanager")).unwrap();
    fs::write(
        source.join(".gitmanager/product.json"),
        fs::read(task.source.join(".gitmanager/product.json")).unwrap(),
    )
    .unwrap();
    fs::write(
        source.join("app.json"),
        serde_json::to_vec(&fixture::organizer()).unwrap(),
    )
    .unwrap();
    let second = tasks::TaskRecord::from_worktree("Another synthetic task", &source).unwrap();
    tasks::TaskRegistry::load().add(second.clone()).unwrap();
    click(&mut h, &mut studio, "studio.return");
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.tasks");
    settle(&mut h, &mut studio);
    task.edit_label("Background edit does not supersede analysis");
    studio.test_task_refresh();
    settle(&mut h, &mut studio);
    await_text(
        &mut h,
        &mut studio,
        "Background edit does not supersede analysis",
    );
    let polled: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(polled["discovery"], retained["discovery"]);
    assert_eq!(polled["task"]["completed"], retained["task"]["completed"]);
    let warned = frame(&mut h, &mut studio)
        .text
        .iter()
        .any(|t| t.contains("supersede") && t.contains("Discover"));
    click(&mut h, &mut studio, &format!("studio.task.{}", second.id));
    settle(&mut h, &mut studio);
    let relinked: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(relinked["task"]["task_id"],second.id,"An explicit relink must atomically replace the task and supersede its old uncommitted analysis: {}",studio.test_notice());
    assert!(
        warned,
        "The ordinary task selector must disclose supersession before the explicit relink"
    );
    assert!(relinked["discovery"].is_null());
    assert!(relinked["task"]["external"].is_null());
    assert!(relinked["task"]["completed"].is_null());
    assert_eq!(task.store.load().unwrap(), saved);
    tasks::TaskRegistry::load().unlink(&second.id).unwrap();
}

#[cfg(target_os = "linux")]
fn task_lineage_publication_cancel(
    authorize: bool,
    after_publication: bool,
    changed_after_authorization: bool,
) {
    use std::sync::{atomic::Ordering, Arc};
    let task = TaskFixture::new();
    let harness = SyntheticHarness::new(&task);
    // Count actual synthetic process invocations, including identical retries.
    // The fixture still writes its usual exact argv/journal snapshot.
    let calls = harness._dir.path().join("calls.jsonl");
    let executable = harness._dir.path().join("codex");
    let mut script = fs::read_to_string(&executable).unwrap();
    script.push_str(&format!("\nwith pathlib.Path({:?}).open('a') as calls:\n    calls.write(json.dumps({{'argv': sys.argv, 'journal': journal}}) + '\\n')\n", calls.to_str().unwrap()));
    fs::write(&executable, script).unwrap();
    let before = Arc::new(product_studio::TestPause::default());
    before.release.store(true, Ordering::Release);
    let hooks = TestHooks {
        before_task_lineage_publish: Some(before.clone()),
        ..Default::default()
    };
    let stopped = hooks.stopped.clone();
    let (mut studio, mut h) = completed_external_discovery_origin_with_transport(
        &task,
        hooks,
        Some(external_discovery_transport(&task)),
    );
    harness.read();
    click(&mut h, &mut studio, "studio.discover");
    settle(&mut h, &mut studio);
    let discover = studio.test_prepared_request().unwrap().clone();
    click(&mut h, &mut studio, "studio.consent");
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "discovery", "{}", studio.test_notice());
    let report = format!(
        "{:?}",
        studio
            .test_discovery_report()
            .expect("Actual external Discover report")
    );
    let path = task.root.join("studio/session.json");
    let completed: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(completed["discovery"]["external"].is_object());
    assert!(completed["discovery"]["discover"]["completed"].is_object());
    assert_eq!(
        completed["discovery"]["discover"]["request"]["request"]["id"],
        discover.id
    );
    click(&mut h, &mut studio, "studio.return");
    settle(&mut h, &mut studio);

    let second = if authorize {
        None
    } else {
        let source = task.data_home.join("second-lineage-source");
        let repo = git2::Repository::init(&source).unwrap();
        let tree = repo
            .find_tree(repo.index().unwrap().write_tree().unwrap())
            .unwrap();
        let who = git2::Signature::now("Synthetic fixture", "fixture@invalid.example").unwrap();
        repo.commit(
            Some("HEAD"),
            &who,
            &who,
            "Synthetic lineage task",
            &tree,
            &[],
        )
        .unwrap();
        fs::create_dir(source.join(".gitmanager")).unwrap();
        fs::write(
            source.join(".gitmanager/product.json"),
            fs::read(task.source.join(".gitmanager/product.json")).unwrap(),
        )
        .unwrap();
        fs::write(
            source.join("app.json"),
            serde_json::to_vec(&fixture::organizer()).unwrap(),
        )
        .unwrap();
        let second =
            tasks::TaskRecord::from_worktree("Other synthetic lineage task", &source).unwrap();
        tasks::TaskRegistry::load().add(second.clone()).unwrap();
        Some(second)
    };
    click(&mut h, &mut studio, "studio.tasks");
    settle(&mut h, &mut studio);
    await_text(
        &mut h,
        &mut studio,
        "Actual external origin for correlation",
    );
    let request = if authorize {
        click(&mut h, &mut studio, "studio.task-need");
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
        h.text("Explicit replacement request at the lineage publication boundary");
        frame(&mut h, &mut studio);
        let started = Instant::now();
        loop {
            let trace = frame(&mut h, &mut studio);
            if !studio.test_source_polling()
                && trace
                    .controls
                    .get("studio.external-request")
                    .is_some_and(|c| c.enabled)
            {
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "Fresh request control unavailable: {}; {:?}",
                studio.test_notice(),
                trace.text
            );
            std::thread::sleep(Duration::from_millis(30));
        }
        click(&mut h, &mut studio, "studio.external-request");
        settle(&mut h, &mut studio);
        Some(
            studio
                .test_prepared_request()
                .expect("Actual explicit disclosure")
                .clone(),
        )
    } else {
        assert!(frame(&mut h, &mut studio)
            .text
            .iter()
            .any(|text| text.contains("supersede") && text.contains("Discover")));
        None
    };
    fn job_files(root: &Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
        fn visit(path: &Path, out: &mut std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>) {
            for entry in fs::read_dir(path).unwrap() {
                let entry = entry.unwrap();
                let kind = entry.file_type().unwrap();
                if kind.is_dir() {
                    visit(&entry.path(), out);
                } else {
                    assert!(kind.is_file());
                    out.insert(entry.path(), fs::read(entry.path()).unwrap());
                }
            }
        }
        let mut result = std::collections::BTreeMap::new();
        if root.exists() {
            visit(root, &mut result);
        }
        result
    }
    let original_jobs = job_files(&task.root.join("external-task-jobs"));
    let discover_jobs = job_files(&task.root.join("discovery-jobs"));
    assert!(!original_jobs.is_empty() && !discover_jobs.is_empty());
    let old_bytes = fs::read(&path).unwrap();
    let old: serde_json::Value = serde_json::from_slice(&old_bytes).unwrap();
    assert_eq!(
        old["discovery"], completed["discovery"],
        "Navigation and disclosure cannot supersede lineage"
    );
    let saved = task.store.load().unwrap();
    let launch_bytes = fs::read(&calls).unwrap();
    assert_eq!(
        std::str::from_utf8(&launch_bytes).unwrap().lines().count(),
        1
    );
    let actual_pause = if after_publication {
        Arc::new(product_studio::TestPause::default())
    } else {
        before.clone()
    };
    let observer = if after_publication {
        let pause = actual_pause.clone();
        let new_task = second.as_ref().map(|t| t.id.clone());
        let new_request = request.as_ref().map(|r| r.id.clone());
        Some(product_studio::journal_testing::after_publish(
            &task.root,
            move |bytes| {
                let value: serde_json::Value =
                    serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                let replaced = if let Some(id) = &new_task {
                    value["task"]["task_id"] == *id
                } else {
                    value["task"]["external"]["pending"]["request"]["id"]
                        == *new_request.as_ref().unwrap()
                };
                if replaced
                    && value["discovery"].is_null()
                    && !pause.reached.swap(true, Ordering::AcqRel)
                {
                    let started = Instant::now();
                    while !pause.release.load(Ordering::Acquire) {
                        if started.elapsed() > Duration::from_secs(120) {
                            return Err("Actual lineage publication was not released".into());
                        }
                        std::thread::sleep(Duration::from_millis(2));
                    }
                }
                Ok(())
            },
        ))
    } else {
        before.reached.store(false, Ordering::Release);
        before.release.store(false, Ordering::Release);
        None
    };
    let action = second
        .as_ref()
        .map(|t| format!("studio.task.{}", t.id))
        .unwrap_or_else(|| "studio.external-authorize".into());
    click(&mut h, &mut studio, &action);
    await_task_pause(&mut h, &mut studio, &actual_pause);
    let cancel_enabled = frame(&mut h, &mut studio).controls["studio.cancel"].enabled;
    let published_bytes = fs::read(&path).unwrap();
    let calls_at_boundary = fs::read(&calls).unwrap();
    if changed_after_authorization {
        assert!(authorize && after_publication);
        task.edit_label(
            "Actual source drift after durable authorization and before ticket preparation",
        );
    }
    let accepted = if after_publication {
        studio.test_cancel()
    } else {
        click(&mut h, &mut studio, "studio.cancel");
        true
    };
    let cancel_notice = studio.test_notice().to_owned();
    actual_pause.release.store(true, Ordering::Release);
    settle(&mut h, &mut studio);
    drop(observer);
    if let Some(second) = &second {
        tasks::TaskRegistry::load().unlink(&second.id).unwrap();
    }
    assert_eq!(
        calls_at_boundary, launch_bytes,
        "Relinking or publishing intake cannot itself invoke or repeat a Harness"
    );
    assert_eq!(task.store.load().unwrap(), saved);
    if after_publication {
        let published: serde_json::Value = serde_json::from_slice(&published_bytes).unwrap();
        assert!(published["discovery"].is_null());
        assert!(!accepted && !cancel_enabled, "Irreversible task lineage publication accepted cancellation: enabled={cancel_enabled}, accepted={accepted}, notice={cancel_notice}");
        assert!(
            cancel_notice.contains("completion boundary"),
            "{cancel_notice}"
        );
        assert!(
            !studio.test_notice().to_lowercase().contains("cancelled"),
            "{}",
            studio.test_notice()
        );
        let now: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(now["discovery"].is_null());
        assert!(now["task"]["completed"].is_null());
        if changed_after_authorization {
            assert_eq!(fs::read(&calls).unwrap(), launch_bytes);
            assert_eq!(
                now["task"]["external"]["pending"]["request"]["id"],
                request.as_ref().unwrap().id
            );
            assert_eq!(now["task"]["external"]["launch_attempted"], false);
            assert!(now["task"]["external"]["pending"]["ticket"].is_null());
            assert!(
                studio.test_notice().to_lowercase().contains("source"),
                "The actual source refusal must be reported: {}",
                studio.test_notice()
            );
            assert!(
                frame(&mut h, &mut studio)
                    .text
                    .iter()
                    .any(|text| text.contains("exact request is retained")
                        && text.contains("No automatic relaunch")),
                "The committed interrupted state must be visible"
            );
            let retained = fs::read(&path).unwrap();
            product_studio::journal_testing::roundtrip(&retained).unwrap();
            studio.test_task_refresh();
            settle(&mut h, &mut studio);
            assert_eq!(fs::read(&path).unwrap(), retained);
            assert_eq!(fs::read(&calls).unwrap(), launch_bytes);
            stop_wide(studio, &stopped);
            studio = ProductStudio::testing(task.root.clone(), None, TestHooks::default());
            settle(&mut h, &mut studio);
            click(&mut h, &mut studio, "studio.tasks");
            settle(&mut h, &mut studio);
            assert_eq!(fs::read(&path).unwrap(), retained, "Restart must retain the real committed unissued request without rollback or automatic ticket preparation");
            assert_eq!(
                fs::read(&calls).unwrap(),
                launch_bytes,
                "A failed committed authorization must never relaunch on restart"
            );
        } else if let Some(request) = &request {
            let started = Instant::now();
            loop {
                let bytes = fs::read(&calls).unwrap();
                let lines: Vec<serde_json::Value> = std::str::from_utf8(&bytes)
                    .unwrap()
                    .lines()
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect();
                if lines.len() == 2 {
                    assert_eq!(
                        lines[1]["journal"]["task"]["external"]["pending"]["request"]["id"],
                        request.id
                    );
                    break;
                }
                assert!(
                    started.elapsed() < Duration::from_secs(10),
                    "The separately explicitly authorized exact replacement was not invoked once"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(
                now["task"]["external"]["pending"]["request"]["id"],
                request.id
            );
            assert_eq!(now["task"]["external"]["launch_attempted"], true);
        } else {
            assert_eq!(fs::read(&calls).unwrap(), launch_bytes);
            assert_eq!(now["task"]["task_id"], second.as_ref().unwrap().id);
            assert!(now["task"]["external"].is_null());
        }
    } else {
        assert!(accepted && cancel_enabled);
        assert_eq!(fs::read(&path).unwrap(), old_bytes, "Accepted prepublication cancellation must retain exact original task and Discover lineage");
        assert_eq!(fs::read(&calls).unwrap(), launch_bytes);
        assert_eq!(studio.test_prepared_request(), request.as_ref());
        click(&mut h, &mut studio, "studio.tasks-back");
        settle(&mut h, &mut studio);
        click(&mut h, &mut studio, "studio.reopen-discovery");
        settle(&mut h, &mut studio);
        assert_eq!(
            format!(
                "{:?}",
                studio
                    .test_discovery_report()
                    .expect("The exact original same-run full report remains")
            ),
            report
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&fs::read(&path).unwrap()).unwrap()
                ["discovery"],
            old["discovery"]
        );
        assert_eq!(fs::read(&calls).unwrap(), launch_bytes);
    }
    assert_eq!(task.store.load().unwrap(), saved);
    for (path, bytes) in original_jobs {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    assert_eq!(
        job_files(&task.root.join("discovery-jobs")),
        discover_jobs,
        "No Discover invocation or resend is authorized by task navigation"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn cancelled_task_relink_before_publication_preserves_exact_discovery_lineage() {
    task_lineage_publication_cancel(false, false, false);
}
#[cfg(target_os = "linux")]
#[test]
fn published_task_relink_refuses_late_cancel_without_authorizing_a_harness() {
    task_lineage_publication_cancel(false, true, false);
}
#[cfg(target_os = "linux")]
#[test]
fn cancelled_task_authorization_before_publication_preserves_exact_discovery_lineage() {
    task_lineage_publication_cancel(true, false, false);
}
#[cfg(target_os = "linux")]
#[test]
fn published_task_authorization_refuses_late_cancel_and_launches_only_its_exact_request() {
    task_lineage_publication_cancel(true, true, false);
}

#[cfg(target_os = "linux")]
#[test]
fn source_drift_after_committed_authorization_retains_unissued_request_without_launch_or_resend() {
    task_lineage_publication_cancel(true, true, true);
}

#[cfg(unix)]
#[test]
fn actual_managed_design_new_feature_has_a_candidate_only_typed_acceptance_path() {
    use product_contract::*;
    use std::sync::atomic::Ordering;
    let task = TaskFixture::with_program(wide_fixture::compact_program(false));
    wide_fixture::managed(&task.store, true);
    let current = task.store.load().unwrap();
    assert!(current.editable_scope_context().unwrap().is_some());
    let mut raw =
        serde_json::to_value(change_adapter::baseline(&current).unwrap().program).unwrap();
    raw["state"].as_array_mut().unwrap().push(serde_json::json!({"id":"feature_count","label":"New feature count","value_type":{"kind":"integer"},"initial":{"kind":"integer","value":0}}));
    raw["actions"].as_array_mut().unwrap().push(serde_json::json!({"id":"new_feature","label":"Try the new feature","parameters":{},"guards":[],"steps":[{"kind":"set_state","state":"feature_count","value":{"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":1}}}],"ensures":[]}));
    raw["observables"].as_array_mut().unwrap().push(serde_json::json!({"id":"feature_count","label":"New feature count","value":{"kind":"state","state":"feature_count"}}));
    raw["views"][0]["actions"].as_array_mut().unwrap().push(serde_json::json!({"id":"new_feature_button","label":"Try the new feature","action":"new_feature","placement":"toolbar","arguments":{},"enabled":scope_fixture::boolean(true)}));
    let candidate = scope_fixture::capture(raw);
    assert!(!current
        .program()
        .unwrap()
        .program
        .actions
        .iter()
        .any(|a| a.id == "new_feature"));
    let transport = wide_fixture_transport(&task.root);
    let hooks = TestHooks::default();
    hooks.today.store(20003, Ordering::Release);
    let stopped = hooks.stopped.clone();
    let mut studio = ProductStudio::testing(task.root.clone(), Some(transport.clone()), hooks);
    let mut h = EguiHarness::new(egui::vec2(1500.0, 5000.0));
    settle(&mut h, &mut studio);
    studio.test_open(task.root.join("tool"));
    settle(&mut h, &mut studio);
    studio.test_modify(
        "Add a separately demonstrated new feature while preserving earlier scoped work",
    );
    settle(&mut h, &mut studio);
    let request = studio
        .test_prepared_request()
        .unwrap_or_else(|| panic!("{}", studio.test_notice()))
        .clone();
    wide_response(
        &task.root,
        &DevelopmentResponse {
            version: 1,
            request_digest: request.identity().unwrap(),
            candidates: vec![GeneratedCandidate {
                id: "new-feature".into(),
                source_json: String::from_utf8(candidate.source_bytes.clone()).unwrap(),
            }],
            hypotheses: vec![],
            evolutions: vec![],
            unsupported: vec![],
        },
    );
    studio.test_consent();
    settle(&mut h, &mut studio);
    assert_eq!(
        studio.test_page(),
        "change",
        "The real valid managed candidate must reach review: {}",
        studio.test_notice()
    );
    click(&mut h, &mut studio, "studio.feature-example");
    settle(&mut h, &mut studio);
    assert_eq!(
        studio.test_page(),
        "discovery-requirement",
        "A candidate-only example must not require Current to validate new state or actions: {}",
        studio.test_notice()
    );
    click(
        &mut h,
        &mut studio,
        "feature-candidate.action.new_feature_button",
    );
    settle(&mut h, &mut studio);
    click(
        &mut h,
        &mut studio,
        "studio.feature.accept.observed.feature_count",
    );
    settle(&mut h, &mut studio);
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(task.root.join("studio/session.json")).unwrap()).unwrap();
    assert_ne!(value["discovery"]["accepted_case"]["target_source"],value["discovery"]["accepted_case"]["authored_source"],"The accepted case must run the actual compiled managed target, never fall back to an uncompiled authored design");
    assert_eq!(
        value["discovery"]["accepted_case"]["required_actions"],
        serde_json::json!(["new_feature"])
    );
    assert_eq!(
        value["discovery"]["accepted_case"]["property"]["predicate"]["right"]["value"],
        serde_json::json!({"kind":"integer","value":1})
    );
    click(&mut h, &mut studio, "studio.feature.back");
    settle(&mut h, &mut studio);
    for id in [
        "studio.accept",
        "studio.keep-current",
        "studio.either",
        "studio.both",
        "studio.neither",
        "studio.defer",
    ] {
        assert!(
            !frame(&mut h, &mut studio)
                .controls
                .get(id)
                .is_some_and(|c| c.enabled),
            "Analysis-only target must not grant {id}"
        );
    }
    stop_wide(studio, &stopped);
    let hooks = TestHooks::default();
    hooks.today.store(20003, Ordering::Release);
    let mut studio = ProductStudio::testing(task.root.clone(), Some(transport), hooks);
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.reopen-discovery");
    settle(&mut h, &mut studio);
    assert_eq!(
        studio.test_page(),
        "change",
        "Restart must reconstruct the exact compiled analysis target: {}",
        studio.test_notice()
    );
    let reopened: serde_json::Value =
        serde_json::from_slice(&fs::read(task.root.join("studio/session.json")).unwrap()).unwrap();
    assert_eq!(
        reopened["discovery"]["accepted_case"],
        value["discovery"]["accepted_case"]
    );
    click(&mut h, &mut studio, "studio.discover");
    settle(&mut h, &mut studio);
    assert_eq!(
        studio.test_page(),
        "consent",
        "The real managed Discover disclosure must be reachable: {}",
        studio.test_notice()
    );
    let request = studio.test_prepared_request().unwrap();
    assert_eq!(
        serde_json::to_value(
            canonical_digest(IdentityDomain::Source, &request.sources[1]).unwrap()
        )
        .unwrap(),
        value["discovery"]["accepted_case"]["target_source"]
    );
    assert!(
        request
            .examples
            .iter()
            .any(|e| e.disclosure == Disclosure::ExplicitlySelected
                && !e.scenario.validity.is_empty())
    );
    assert_eq!(task.store.load().unwrap(), current);
}

#[cfg(target_os = "linux")]
#[test]
fn actual_external_new_state_design_reaches_candidate_only_acceptance_and_exact_disclosure() {
    let task = TaskFixture::new();
    let harness = SyntheticHarness::new(&task);
    let transport = wide_fixture_transport(&task.root);
    let (mut studio, mut h) = task.open_with_transport(TestHooks::default(), Some(transport));
    let saved = task.store.load().unwrap();
    assert!(saved.editable_scope_context().unwrap().is_none());
    studio.test_task_request("Add a separately checked new-state feature to my actual linked tool");
    settle(&mut h, &mut studio);
    let request = studio.test_prepared_request().unwrap().clone();
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    assert_eq!(harness.read()["synthetic"], true);
    let mut raw = fixture::organizer();
    raw["label"] = serde_json::json!("Actual ordinary new-state task completion");
    raw["state"].as_array_mut().unwrap().push(serde_json::json!({"id":"feature_count","label":"New feature count","value_type":{"kind":"integer"},"initial":{"kind":"integer","value":0}}));
    raw["actions"].as_array_mut().unwrap().push(serde_json::json!({"id":"new_feature","label":"Try the new feature","parameters":{},"guards":[],"steps":[{"kind":"set_state","state":"feature_count","value":{"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":1}}}],"ensures":[]}));
    raw["observables"].as_array_mut().unwrap().push(serde_json::json!({"id":"feature_count","label":"New feature count","value":{"kind":"state","state":"feature_count"}}));
    raw["views"][0]["actions"].as_array_mut().unwrap().push(serde_json::json!({"id":"new_feature_button","label":"Try the new feature","action":"new_feature","placement":"toolbar","arguments":{},"enabled":scope_fixture::boolean(true)}));
    fixture::capture(raw.clone()).validate().unwrap();
    fs::write(
        task.source.join("app.json"),
        serde_json::to_vec(&raw).unwrap(),
    )
    .unwrap();
    product_studio::test_task_machine(&task.root, "submit", &task.record.id, &request.id).unwrap();
    studio.test_task_refresh();
    settle(&mut h, &mut studio);
    await_text(
        &mut h,
        &mut studio,
        "Actual ordinary new-state task completion",
    );
    let path = task.root.join("studio/session.json");
    let completed: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        completed["task"]["completed"]["external"]["pending"]["request"],
        serde_json::to_value(&request).unwrap()
    );
    await_review_control(&mut h, &mut studio);
    studio.test_task_review();
    settle(&mut h, &mut studio);
    assert_eq!(
        studio.test_page(),
        "change",
        "Actual new-state ExternalAuthor review must not require a fictitious Current replay: {}",
        studio.test_notice()
    );
    click(&mut h, &mut studio, "studio.feature-example");
    settle(&mut h, &mut studio);
    click(
        &mut h,
        &mut studio,
        "feature-candidate.action.new_feature_button",
    );
    settle(&mut h, &mut studio);
    click(
        &mut h,
        &mut studio,
        "studio.feature.accept.observed.feature_count",
    );
    settle(&mut h, &mut studio);
    let accepted: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(accepted["discovery"]["original"].is_null());
    assert_eq!(
        accepted["discovery"]["accepted_case"]["required_actions"],
        serde_json::json!(["new_feature"])
    );
    click(&mut h, &mut studio, "studio.feature.back");
    settle(&mut h, &mut studio);
    assert!(!frame(&mut h, &mut studio)
        .controls
        .get("studio.accept")
        .is_some_and(|c| c.enabled));
    click(&mut h, &mut studio, "studio.discover");
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "consent", "{}", studio.test_notice());
    assert_eq!(
        studio.test_prepared_request().unwrap().operation,
        product_contract::DevelopmentOperation::Discover
    );
    assert_eq!(task.store.load().unwrap(), saved);
}

// Synthetic services, but actual registry completion, compiled managed sources,
// separate consent, ordinary copy/choice controls, durable rehearsal and restart.
#[cfg(target_os = "linux")]
fn managed_external_pair_retains_origin(mode: &str) {
    use product_contract::*;
    use std::sync::{atomic::Ordering, Arc};
    let task = TaskFixture::with_program(wide_fixture::compact_program(false));
    wide_fixture::managed(&task.store, true);
    let current = task.store.load().unwrap();
    assert!(current.editable_scope_context().unwrap().is_some());
    let harness = SyntheticHarness::new(&task);
    let transport = wide_fixture_transport(&task.root);
    let hooks = TestHooks::default();
    hooks.today.store(current.clock_day, Ordering::Release);
    let stopped = hooks.stopped.clone();
    let (mut studio, mut h) = task.open_with_transport(hooks, Some(transport));
    studio.test_task_request(
        "Explore how new work is named while keeping existing scoped commitments",
    );
    settle(&mut h, &mut studio);
    let original = studio.test_prepared_request().unwrap().clone();
    studio.test_task_authorize();
    settle(&mut h, &mut studio);
    assert_eq!(harness.read()["synthetic"], true);
    let mut authored =
        serde_json::to_value(change_adapter::baseline(&current).unwrap().program).unwrap();
    authored["label"] = serde_json::json!("Actual external managed naming design");
    authored["actions"][0]["steps"][0]["values"]["name"] = serde_json::to_value(
        scope_fixture::lit(scope_fixture::text("Authored new work name"), Type::Text),
    )
    .unwrap();
    let list = authored["views"][0]["kind"].clone();
    authored["views"].as_array_mut().unwrap().push(serde_json::json!({"id":"second_list","label":"External extra view","kind":list,"actions":[],"keys":[]}));
    fs::write(
        task.source.join("app.json"),
        serde_json::to_vec(&authored).unwrap(),
    )
    .unwrap();
    product_studio::test_task_machine(&task.root, "submit", &task.record.id, &original.id).unwrap();
    studio.test_task_refresh();
    settle(&mut h, &mut studio);
    await_review_control(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.task-review");
    settle(&mut h, &mut studio);
    assert_eq!(
        studio.test_page(),
        "change",
        "Managed authored review: {}",
        studio.test_notice()
    );
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(task.root.join("studio/session.json")).unwrap()).unwrap();
    assert!(
        record["discovery"]["original"].is_null(),
        "No fabricated Modify receipt"
    );
    let actual_authored: CapturedProgram =
        serde_json::from_value(record["task"]["completed"]["capture"].clone()).unwrap();
    assert_eq!(
        actual_authored.binding.task.as_ref().unwrap().task_id,
        task.record.id
    );
    click(&mut h, &mut studio, "studio.discover");
    settle(&mut h, &mut studio);
    assert_eq!(
        studio.test_page(),
        "consent",
        "Exact managed Discover disclosure: {}",
        studio.test_notice()
    );
    let request = studio.test_prepared_request().unwrap().clone();
    assert_eq!(request.operation, DevelopmentOperation::Discover);
    assert_eq!(&request.sources[0], current.program().unwrap());
    assert_ne!(
        request.sources[1], actual_authored,
        "Discover must use the actual compiled target"
    );
    assert!(request.sources[1].binding.task.is_none());
    assert!(request.sources[0].binding.task.is_none());
    let scene = ScenarioSpec {
        version: 1,
        id: "managed-naming-choice".into(),
        label: "Create new work with its supplied name".into(),
        seed: current.data.clone(),
        session: current.session.clone(),
        clock_day: current.clock_day,
        random_seed: 42,
        inputs: vec![
            scope_fixture::invoke(
                "add",
                &[
                    ("name", scope_fixture::text("Example new work")),
                    ("promised", DataValue::Date { days: 20020 }),
                ],
            ),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
        validity: vec![],
    };
    for source in &request.sources[..2] {
        scene.validate(&source.program).unwrap();
    }
    assert_ne!(
        request.sources[0].program.actions[0], request.sources[1].program.actions[0],
        "The checked action really changed"
    );
    wide_response(
        &task.root,
        &DevelopmentResponse {
            version: 1,
            request_digest: request.identity().unwrap(),
            candidates: vec![
                GeneratedCandidate {
                    id: "current".into(),
                    source_json: String::from_utf8(request.sources[0].source_bytes.clone())
                        .unwrap(),
                },
                GeneratedCandidate {
                    id: "captured".into(),
                    source_json: String::from_utf8(request.sources[1].source_bytes.clone())
                        .unwrap(),
                },
            ],
            hypotheses: vec![ChoiceHypothesis {
                id: "managed-external-pair".into(),
                statement: "Creating work can retain its entered name or use a fixed name".into(),
                kind: HypothesisKind::UnresolvedChoice,
                action: "add".into(),
                observable: "waiting_jobs".into(),
                sources: vec![SourceLocus {
                    relative_path: request.sources[1].binding.program_path.clone(),
                    raw_digest: request.sources[1].artifact.raw_digest.clone(),
                    pointer: "/actions/0/steps/0/values/name".into(),
                }],
                alternatives: vec!["current".into(), "captured".into()],
                related_decisions: vec![],
                scenario_json: serde_json::to_string(&scene).unwrap(),
                unknowns: vec![],
            }],
            evolutions: vec![],
            unsupported: vec![],
        },
    );
    click(&mut h, &mut studio, "studio.consent");
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "discovery", "{}", studio.test_notice());
    let report = studio
        .test_discovery_report()
        .expect("Complete current checked report");
    eprintln!("Managed origin checked dispositions: {:?}; defects: {:?}; unverified: {:?}; changed symbols: {:?}", report.log, report.defects, report.unverified, report.delta.changed_symbols);
    for run in report.runs.iter().take(8) {
        eprintln!(
            "Managed origin run {}: {:?}, task={:?}, errors={:?}, uncovered={:?}",
            run.id, run.state, run.binding.source.task, run.errors, run.uncovered
        );
    }
    let trace = frame(&mut h, &mut studio);
    let copies: Vec<_> = trace
        .controls
        .iter()
        .filter(|(key, c)| key.starts_with("studio.discovery.copy.") && c.enabled)
        .map(|(key, _)| key.clone())
        .collect();
    assert_eq!(
        copies.len(),
        1,
        "Expected one valid exact-candidate managed question: {:?}",
        trace.text
    );
    click(&mut h, &mut studio, &copies[0]);
    settle(&mut h, &mut studio);
    assert_eq!(
        studio.test_page(),
        "discovery-copy",
        "Whole-design pair required: {}",
        studio.test_notice()
    );
    click(&mut h, &mut studio, "studio.discovery.choose.5");
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    let saved = task.store.load().unwrap();
    let choice = saved.decisions.decisions.last().unwrap().clone();
    assert_eq!(choice.status, DecisionStatus::Pending);
    let scenes = product_decisions::IntentArchive::new(task.store.clone())
        .accepted_scenes(&choice)
        .unwrap();
    assert_eq!(scenes.len(), 2);
    assert!(
        scenes.iter().all(|s| s.program().binding.task.is_none()),
        "Compiled scene producers must retain their real identity"
    );
    assert!(scenes.iter().any(|s| s.program() == &request.sources[1]));
    assert!(
        saved.programs.contains(&actual_authored),
        "The existing archive must retain the exact authored capture"
    );
    assert!(
        saved
            .scope
            .rehearsals
            .values()
            .any(
                |proof| proof.witnesses.get(&choice.id) == Some(&choice.witness)
                    && proof.manifest.business
                        == canonical_digest(IdentityDomain::Source, &actual_authored).unwrap()
            ),
        "The checked rehearsal must bind the saved choice to its true authored source"
    );
    if mode == "control" {
        let backup = product_backup::VerifiedBackup::capture(&task.store).unwrap();
        let bytes = backup.to_bytes().unwrap();
        let verified = product_backup::VerifiedBackup::from_bytes(&bytes).unwrap();
        assert_eq!(verified.to_bytes().unwrap(), bytes);
        assert_eq!(verified.snapshot(), &saved);
        assert!(verified.snapshot().programs.contains(&actual_authored));
    }
    if matches!(mode, "replacement" | "replacement-control") {
        click(&mut h, &mut studio, "studio.tasks");
        settle(&mut h, &mut studio);
        click(&mut h, &mut studio, "studio.task-need");
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
        h.text("An explicitly separate later author request");
        frame(&mut h, &mut studio);
        let started = Instant::now();
        loop {
            let trace = frame(&mut h, &mut studio);
            if !studio.test_source_polling()
                && trace
                    .controls
                    .get("studio.external-request")
                    .is_some_and(|control| control.enabled)
            {
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "Actual settled-source request control never became available: {}; {:?}",
                studio.test_notice(),
                trace.text
            );
            std::thread::sleep(Duration::from_millis(30));
        }
        click(&mut h, &mut studio, "studio.external-request");
        settle(&mut h, &mut studio);
        let later = studio
            .test_prepared_request()
            .unwrap_or_else(|| {
                panic!(
                    "Explicit later request disclosure unavailable: {}",
                    studio.test_notice()
                )
            })
            .id
            .clone();
        studio.test_task_authorize();
        settle(&mut h, &mut studio);
        studio.test_task_abandon(&later);
        settle(&mut h, &mut studio);
        let record: serde_json::Value =
            serde_json::from_slice(&fs::read(task.root.join("studio/session.json")).unwrap())
                .unwrap();
        assert!(record["discovery"].is_null() && record["task"]["completed"].is_null());
        studio.test_open(task.root.join("tool"));
        settle(&mut h, &mut studio);
    }
    studio.test_daily(scope_fixture::invoke(
        "add",
        &[
            ("name", scope_fixture::text("Later independent saved work")),
            ("promised", DataValue::Date { days: 20021 }),
        ],
    ));
    settle(&mut h, &mut studio);
    let later_work = task.store.load().unwrap();
    assert!(later_work
        .data
        .records
        .iter()
        .any(|row| row.values.get("name")
            == Some(&scope_fixture::text("Later independent saved work"))));
    stop_wide(studio, &stopped);
    if mode == "entry" || mode == "replacement" {
        fs::write(
            task.source.join("unrelated-origin-drift.txt"),
            "Changed original full source after saving managed pair",
        )
        .unwrap();
    } else if mode == "removed" {
        tasks::TaskRegistry::load().unlink(&task.record.id).unwrap();
    } else if mode == "task-replacement" {
        let mut replacement = task.record.clone();
        replacement.created_at.push_str("-different-identity");
        tasks::TaskRegistry::load().unlink(&task.record.id).unwrap();
        tasks::TaskRegistry::load().add(replacement).unwrap();
    }
    let pause = Arc::new(product_studio::TestPause::default());
    pause.release.store(mode != "final", Ordering::Release);
    let hooks = TestHooks {
        before_commit: Some(pause.clone()),
        ..Default::default()
    };
    hooks.today.store(current.clock_day, Ordering::Release);
    let mut studio = ProductStudio::testing(task.root.clone(), None, hooks);
    settle(&mut h, &mut studio);
    studio.test_open(task.root.join("tool"));
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, &format!("studio.resume.{}", choice.id));
    settle(&mut h, &mut studio);
    if matches!(
        mode,
        "entry" | "removed" | "replacement" | "task-replacement"
    ) {
        assert_eq!(
            studio.test_page(),
            "daily",
            "An archived managed pair must retain its original authored source gate: {}",
            studio.test_notice()
        );
        assert_eq!(task.store.load().unwrap(), later_work);
        return;
    }
    assert_eq!(
        studio.test_page(),
        "discovery-copy",
        "Unchanged author origin must allow later business work: {}",
        studio.test_notice()
    );
    click(&mut h, &mut studio, "studio.discovery.choose.2");
    if mode == "final" {
        let start = Instant::now();
        while !pause.reached.load(Ordering::Acquire) {
            frame(&mut h, &mut studio);
            assert!(
                start.elapsed() < Duration::from_secs(60),
                "Final gate not reached: {}",
                studio.test_notice()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        fs::write(
            task.source.join("unrelated-origin-drift.txt"),
            "Changed original source at final recording",
        )
        .unwrap();
        pause.release.store(true, Ordering::Release);
    }
    settle(&mut h, &mut studio);
    let after = task.store.load().unwrap();
    if mode == "final" {
        assert_eq!(
            after, later_work,
            "Final recording must recheck original author, not just compiled scenes"
        );
    } else {
        assert_eq!(after.data, later_work.data);
        assert!(
            matches!(
                after
                    .decisions
                    .decisions
                    .iter()
                    .find(|d| d.id == choice.id)
                    .unwrap()
                    .status,
                DecisionStatus::Superseded { .. }
            ),
            "Unchanged-origin final resolution failed: {}",
            studio.test_notice()
        );
        assert_eq!(after.program().unwrap(), later_work.program().unwrap());
    }
}
#[cfg(target_os = "linux")]
#[test]
fn managed_external_pair_reopen_rejects_original_source_drift_after_later_work() {
    managed_external_pair_retains_origin("entry");
}
#[cfg(target_os = "linux")]
#[test]
fn managed_external_pair_reopen_rejects_removed_original_task() {
    managed_external_pair_retains_origin("removed");
}
#[cfg(target_os = "linux")]
#[test]
fn managed_external_pair_final_recording_rechecks_original_source() {
    managed_external_pair_retains_origin("final");
}
#[cfg(target_os = "linux")]
#[test]
fn unchanged_managed_external_pair_resolves_after_restart_and_later_business_work() {
    managed_external_pair_retains_origin("control");
}
#[cfg(target_os = "linux")]
#[test]
fn replaced_session_lineage_cannot_erase_managed_pending_pair_origin() {
    managed_external_pair_retains_origin("replacement");
}

#[cfg(target_os = "linux")]
#[test]
fn replaced_session_lineage_preserves_unchanged_managed_pair_resolution() {
    managed_external_pair_retains_origin("replacement-control");
}

#[cfg(target_os = "linux")]
#[test]
fn managed_external_pair_reopen_rejects_replaced_registered_task_identity() {
    managed_external_pair_retains_origin("task-replacement");
}

// Both edits use the ordinary request/disclosure/authorization/review controls.
// Services and author edits are synthetic; Current's first capture is genuinely
// adopted before the same registered task produces the second candidate.
#[cfg(target_os = "linux")]
fn second_external_edit_keeps_candidate_origin(mode: &str) {
    use product_contract::*;
    use std::sync::{atomic::Ordering, Arc};
    let task = TaskFixture::with_program(wide_fixture::compact_program(false));
    let initial = task.store.load().unwrap();
    let harness = SyntheticHarness::new(&task);
    let hooks = TestHooks::default();
    hooks.today.store(initial.clock_day, Ordering::Release);
    let stopped = hooks.stopped.clone();
    let (mut studio, mut h) =
        task.open_with_transport(hooks, Some(wide_fixture_transport(&task.root)));
    let authorize = |studio: &mut ProductStudio, h: &mut EguiHarness, need: &str| {
        click(h, studio, "studio.task-need");
        h.key(
            egui::Key::A,
            true,
            egui::Modifiers {
                ctrl: true,
                command: true,
                ..Default::default()
            },
        );
        frame(h, studio);
        h.key(egui::Key::A, false, egui::Modifiers::NONE);
        h.text(need);
        frame(h, studio);
        let started = Instant::now();
        loop {
            let trace = frame(h, studio);
            if !studio.test_source_polling()
                && trace
                    .controls
                    .get("studio.external-request")
                    .is_some_and(|c| c.enabled)
            {
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "Request control unavailable: {}; {:?}",
                studio.test_notice(),
                trace.text
            );
            std::thread::sleep(Duration::from_millis(30));
        }
        click(h, studio, "studio.external-request");
        settle(h, studio);
        let request = studio
            .test_prepared_request()
            .expect("Actual request disclosure")
            .clone();
        click(h, studio, "studio.external-authorize");
        settle(h, studio);
        let started = Instant::now();
        loop {
            let logged = fs::read(&harness.log)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
            if logged.as_ref().is_some_and(|value| {
                value["journal"]["task"]["external"]["pending"]["request"]["id"] == request.id
            }) {
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "Exact authorized request was not invoked"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        request
    };
    let first = authorize(
        &mut studio,
        &mut h,
        "Update the title while preserving existing behavior",
    );
    let mut authored = serde_json::to_value(&initial.program().unwrap().program).unwrap();
    authored["label"] = serde_json::json!("First adopted external design");
    fs::write(
        task.source.join("app.json"),
        serde_json::to_vec(&authored).unwrap(),
    )
    .unwrap();
    product_studio::test_task_machine(&task.root, "submit", &task.record.id, &first.id).unwrap();
    studio.test_task_refresh();
    settle(&mut h, &mut studio);
    await_review_control(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.task-review");
    settle(&mut h, &mut studio);
    assert_eq!(
        studio.test_page(),
        "change",
        "First review: {}",
        studio.test_notice()
    );
    let journal_path = task.root.join("studio/session.json");
    let first_journal: serde_json::Value =
        serde_json::from_slice(&fs::read(&journal_path).unwrap()).unwrap();
    let first_capture: CapturedProgram =
        serde_json::from_value(first_journal["task"]["completed"]["capture"].clone()).unwrap();
    assert_eq!(
        first_capture.binding.task.as_ref().unwrap().task_id,
        task.record.id
    );
    click(&mut h, &mut studio, "studio.accept");
    settle(&mut h, &mut studio);
    assert_eq!(
        studio.test_page(),
        "daily",
        "First adoption: {}",
        studio.test_notice()
    );
    let current = task.store.load().unwrap();
    assert_eq!(
        current.program().unwrap(),
        &first_capture,
        "Current must be the exact adopted first ExternalAuthor capture"
    );
    assert!(current.editable_scope_context().unwrap().is_none());
    click(&mut h, &mut studio, "studio.tasks");
    settle(&mut h, &mut studio);
    let second = authorize(
        &mut studio,
        &mut h,
        "Explore how new work is named while preserving existing work",
    );
    assert_ne!(first.id, second.id);
    assert!(second.sources.contains(&first_capture));
    authored["label"] = serde_json::json!("Second external naming design");
    authored["actions"][0]["steps"][0]["values"]["name"] = serde_json::to_value(
        scope_fixture::lit(scope_fixture::text("Authored new work name"), Type::Text),
    )
    .unwrap();
    let list = authored["views"][0]["kind"].clone();
    authored["views"].as_array_mut().unwrap().push(serde_json::json!({"id":"second_list","label":"Second design extra view","kind":list,"actions":[],"keys":[]}));
    fs::write(
        task.source.join("app.json"),
        serde_json::to_vec(&authored).unwrap(),
    )
    .unwrap();
    product_studio::test_task_machine(&task.root, "submit", &task.record.id, &second.id).unwrap();
    studio.test_task_refresh();
    settle(&mut h, &mut studio);
    await_review_control(&mut h, &mut studio);
    click(&mut h, &mut studio, "studio.task-review");
    settle(&mut h, &mut studio);
    assert_eq!(
        studio.test_page(),
        "change",
        "Second review: {}",
        studio.test_notice()
    );
    let second_journal: serde_json::Value =
        serde_json::from_slice(&fs::read(&journal_path).unwrap()).unwrap();
    let second_capture: CapturedProgram =
        serde_json::from_value(second_journal["task"]["completed"]["capture"].clone()).unwrap();
    assert!(
        second_journal["discovery"]["original"].is_null(),
        "No invented Modify receipt"
    );
    for field in ["task_id", "created_at", "repository", "worktree"] {
        assert!(!first_journal["task"][field].is_null());
        assert_eq!(
            first_journal["task"][field], second_journal["task"][field],
            "Both authorizations must retain the same full registered task identity"
        );
    }
    assert!(matches!(
        first_capture.binding.producer,
        Producer::ExternalAuthor { .. }
    ));
    assert!(matches!(
        second_capture.binding.producer,
        Producer::ExternalAuthor { .. }
    ));
    assert_eq!(
        second_capture.binding.task.as_ref().unwrap().task_id,
        task.record.id
    );
    assert_ne!(
        first_capture.binding.task, second_capture.binding.task,
        "Distinct full task-source fingerprints must survive both real completions"
    );
    assert_eq!(
        task.store.load().unwrap().program().unwrap(),
        &first_capture
    );
    click(&mut h, &mut studio, "studio.discover");
    settle(&mut h, &mut studio);
    assert_eq!(
        studio.test_page(),
        "consent",
        "Second Discover disclosure: {}",
        studio.test_notice()
    );
    let request = studio.test_prepared_request().unwrap().clone();
    assert_eq!(request.operation, DevelopmentOperation::Discover);
    assert_eq!(request.sources[0], first_capture);
    assert_eq!(request.sources[1], second_capture);
    let scene = ScenarioSpec {
        version: 1,
        id: "second-author-choice".into(),
        label: "Create work with its supplied name".into(),
        seed: current.data.clone(),
        session: current.session.clone(),
        clock_day: current.clock_day,
        random_seed: 42,
        inputs: vec![
            scope_fixture::invoke(
                "add",
                &[
                    ("name", scope_fixture::text("Example new work")),
                    ("promised", DataValue::Date { days: 20020 }),
                ],
            ),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
        validity: vec![],
    };
    for source in &request.sources[..2] {
        scene.validate(&source.program).unwrap();
    }
    assert_ne!(
        request.sources[0].program.actions[0],
        request.sources[1].program.actions[0]
    );
    wide_response(
        &task.root,
        &DevelopmentResponse {
            version: 1,
            request_digest: request.identity().unwrap(),
            candidates: vec![
                GeneratedCandidate {
                    id: "current".into(),
                    source_json: String::from_utf8(request.sources[0].source_bytes.clone())
                        .unwrap(),
                },
                GeneratedCandidate {
                    id: "captured".into(),
                    source_json: String::from_utf8(request.sources[1].source_bytes.clone())
                        .unwrap(),
                },
            ],
            hypotheses: vec![ChoiceHypothesis {
                id: "second-author-pair".into(),
                statement: "Creating work can retain its entered name or use a fixed name".into(),
                kind: HypothesisKind::UnresolvedChoice,
                action: "add".into(),
                observable: "waiting_jobs".into(),
                sources: vec![SourceLocus {
                    relative_path: second_capture.binding.program_path.clone(),
                    raw_digest: second_capture.artifact.raw_digest.clone(),
                    pointer: "/actions/0/steps/0/values/name".into(),
                }],
                alternatives: vec!["current".into(), "captured".into()],
                related_decisions: vec![],
                scenario_json: serde_json::to_string(&scene).unwrap(),
                unknowns: vec![],
            }],
            evolutions: vec![],
            unsupported: vec![],
        },
    );
    click(&mut h, &mut studio, "studio.consent");
    settle(&mut h, &mut studio);
    let report = studio
        .test_discovery_report()
        .expect("Checked second-edit report");
    eprintln!(
        "Second-author dispositions: {:?}; defects: {:?}; unverified: {:?}",
        report.log, report.defects, report.unverified
    );
    let trace = frame(&mut h, &mut studio);
    let copies: Vec<_> = trace
        .controls
        .iter()
        .filter(|(key, c)| key.starts_with("studio.discovery.copy.") && c.enabled)
        .map(|(key, _)| key.clone())
        .collect();
    assert_eq!(
        copies.len(),
        1,
        "Expected genuine Current/second-candidate question: {:?}; {}",
        trace.text,
        studio.test_notice()
    );
    click(&mut h, &mut studio, &copies[0]);
    settle(&mut h, &mut studio);
    assert_eq!(studio.test_page(), "discovery-copy");
    click(&mut h, &mut studio, "studio.discovery.choose.5");
    settle(&mut h, &mut studio);
    assert_eq!(
        studio.test_page(),
        "daily",
        "Save second-edit pending choice: {}",
        studio.test_notice()
    );
    let saved = task.store.load().unwrap();
    let choice = saved.decisions.decisions.last().unwrap().clone();
    assert_eq!(choice.status, DecisionStatus::Pending);
    let scenes = product_decisions::IntentArchive::new(task.store.clone())
        .accepted_scenes(&choice)
        .unwrap();
    assert_eq!(scenes.len(), 2);
    assert!(scenes.iter().any(|s| s.program() == &first_capture));
    assert!(scenes.iter().any(|s| s.program() == &second_capture));
    assert_eq!(saved.program().unwrap(), &first_capture);
    studio.test_daily(scope_fixture::invoke(
        "add",
        &[
            ("name", scope_fixture::text("Later independent saved work")),
            ("promised", DataValue::Date { days: 20021 }),
        ],
    ));
    settle(&mut h, &mut studio);
    let later_work = task.store.load().unwrap();
    assert!(later_work
        .data
        .records
        .iter()
        .any(|row| row.values.get("name")
            == Some(&scope_fixture::text("Later independent saved work"))));
    let backup = product_backup::VerifiedBackup::capture(&task.store)
        .unwrap()
        .to_bytes()
        .unwrap();
    assert_eq!(
        product_backup::VerifiedBackup::from_bytes(&backup)
            .unwrap()
            .snapshot(),
        &later_work
    );
    stop_wide(studio, &stopped);
    if mode == "entry" {
        fs::write(
            task.source.join("second-source-drift.txt"),
            "Changed second author source after pending choice",
        )
        .unwrap();
    }
    if mode == "removed" {
        tasks::TaskRegistry::load().unlink(&task.record.id).unwrap();
    }
    let pause = Arc::new(product_studio::TestPause::default());
    pause.release.store(mode != "final", Ordering::Release);
    let hooks = TestHooks {
        before_commit: Some(pause.clone()),
        ..Default::default()
    };
    hooks.today.store(initial.clock_day, Ordering::Release);
    let mut studio = ProductStudio::testing(task.root.clone(), None, hooks);
    settle(&mut h, &mut studio);
    studio.test_open(task.root.join("tool"));
    settle(&mut h, &mut studio);
    click(&mut h, &mut studio, &format!("studio.resume.{}", choice.id));
    settle(&mut h, &mut studio);
    if matches!(mode, "entry" | "removed") {
        assert_eq!(
            studio.test_page(),
            "daily",
            "Second candidate must retain its original source gate: {}",
            studio.test_notice()
        );
        assert_eq!(task.store.load().unwrap(), later_work);
        return;
    }
    assert_eq!(studio.test_page(), "discovery-copy", "The unchanged second author must reopen after later work; adopted Current is historical: {}", studio.test_notice());
    click(&mut h, &mut studio, "studio.discovery.choose.2");
    if mode == "final" {
        let started = Instant::now();
        while !pause.reached.load(Ordering::Acquire) {
            frame(&mut h, &mut studio);
            assert!(
                started.elapsed() < Duration::from_secs(60),
                "Final second-author gate not reached: {}",
                studio.test_notice()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        fs::write(
            task.source.join("second-source-drift.txt"),
            "Second source changed at final recording",
        )
        .unwrap();
        pause.release.store(true, Ordering::Release);
    }
    settle(&mut h, &mut studio);
    let after = task.store.load().unwrap();
    if mode == "final" {
        assert_eq!(
            after, later_work,
            "Final recording must recheck the second candidate's real source"
        );
    } else {
        assert_eq!(after.data, later_work.data);
        assert_eq!(after.program().unwrap(), &first_capture);
        assert!(
            matches!(
                after
                    .decisions
                    .decisions
                    .iter()
                    .find(|d| d.id == choice.id)
                    .unwrap()
                    .status,
                DecisionStatus::Superseded { .. }
            ),
            "Unchanged second-author resolution: {}",
            studio.test_notice()
        );
    }
}
#[cfg(target_os = "linux")]
#[test]
fn second_authorized_edit_reopens_with_historical_external_current() {
    second_external_edit_keeps_candidate_origin("control");
}
#[cfg(target_os = "linux")]
#[test]
fn second_authorized_edit_reopen_rejects_candidate_source_drift() {
    second_external_edit_keeps_candidate_origin("entry");
}
#[cfg(target_os = "linux")]
#[test]
fn second_authorized_edit_reopen_rejects_candidate_task_removal() {
    second_external_edit_keeps_candidate_origin("removed");
}
#[cfg(target_os = "linux")]
#[test]
fn second_authorized_edit_final_recording_rechecks_candidate_source() {
    second_external_edit_keeps_candidate_origin("final");
}
