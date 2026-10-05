//! Filesystem-backed source regressions. Fixtures are not live AI authorship.
#[path = "support/product_contract_fixture.rs"]
mod fixture;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_provider/mod.rs"]
mod product_provider;
#[path = "../src/product_sources/mod.rs"]
mod product_sources;
#[path = "../src/task_delivery.rs"]
mod task_delivery;
#[path = "../src/task_verification.rs"]
mod task_verification;
#[path = "../src/tasks.rs"]
mod tasks;

use product_contract::*;
use product_sources::*;
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

struct Task {
    _dir: tempfile::TempDir,
    root: PathBuf,
    record: tasks::TaskRecord,
    adapter: TaskSourceAdapter,
}
// TaskRegistry remains the actual production authority, but tests must never
// use the caller's own task configuration. Initialize once before any fixture
// can start a registry read or spawn a transport/watch worker.
fn isolate_registry() {
    static CONFIG: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    CONFIG.get_or_init(|| {
        let directory = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(directory.path()).unwrap();
        #[cfg(windows)]
        std::env::set_var("APPDATA", &root);
        #[cfg(not(windows))]
        std::env::set_var("XDG_CONFIG_HOME", &root);
        directory
    });
}
impl Task {
    fn new() -> Self {
        isolate_registry();
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let repo = git2::Repository::init(&root).unwrap();
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let signature =
            git2::Signature::now("GitManager CI", "gitmanager-ci@users.noreply.invalid").unwrap();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "Fixture baseline",
            &tree,
            &[],
        )
        .unwrap();
        fs::create_dir(root.join(".gitmanager")).unwrap();
        fs::write(
            root.join(DECLARATION_PATH),
            serde_json::to_vec(&json!({
                "version":1,"project_id":"project","artifact_kind":"generated_app",
                "program_path":"app.json"
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("app.json"),
            serde_json::to_vec(&fixture::organizer()).unwrap(),
        )
        .unwrap();
        let record = tasks::TaskRecord::from_worktree("Source test", &root).unwrap();
        tasks::TaskRegistry::load().add(record.clone()).unwrap();
        let adapter = TaskSourceAdapter::link("project", &record.id).unwrap();
        Self {
            _dir: dir,
            root,
            record,
            adapter,
        }
    }
    fn capture(&self) -> CapturedProgram {
        self.adapter
            .capture("project", Some(&self.record.id))
            .unwrap()
    }
    fn change(&self) {
        let mut program = fixture::organizer();
        program["label"] = json!("New actual source");
        fs::write(
            self.root.join("app.json"),
            serde_json::to_vec(&program).unwrap(),
        )
        .unwrap();
    }
}
impl Drop for Task {
    fn drop(&mut self) {
        let _ = tasks::TaskRegistry::load().unlink(&self.record.id);
    }
}
fn next(watcher: &mut SourceWatcher) -> SourceUpdate {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let update = watcher.poll();
        if matches!(
            update,
            SourceUpdate::Captured(_) | SourceUpdate::Unavailable(_)
        ) {
            return update;
        }
        assert!(
            Instant::now() < deadline,
            "watcher never settled: {update:?}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}
fn request(task: &Task) -> DevelopmentRequest {
    DevelopmentRequest {
        version: 1,
        id: "change-request".into(),
        project_id: "project".into(),
        operation: DevelopmentOperation::Modify,
        request: "Change the tool".into(),
        sources: vec![task.capture()],
        context: DevelopmentContext {
            view: None,
            selected: vec![],
            recent_inputs: vec![],
            data_digest: None,
            session_digest: None,
        },
        examples: vec![],
        decisions: DecisionGraph {
            version: 1,
            revision: 0,
            decisions: vec![],
        },
        unknowns: vec![],
        required_capabilities: Default::default(),
    }
}
fn completion(job: &ExternalHandoff, capture: &CapturedProgram) -> Vec<u8> {
    serde_json::to_vec(&ExternalCompletion {
        version: 1,
        request_id: job.request().id.clone(),
        request_digest: job.request().identity().unwrap(),
        project_id: "project".into(),
        task_id: capture.binding.task.as_ref().unwrap().task_id.clone(),
        source_fingerprint: capture
            .binding
            .task
            .as_ref()
            .unwrap()
            .source_fingerprint
            .clone(),
        baseline_source_fingerprint: job
            .baseline()
            .binding
            .task
            .as_ref()
            .unwrap()
            .source_fingerprint
            .clone(),
        program_path: capture.binding.program_path.clone(),
        raw_digest: capture.artifact.raw_digest.clone(),
        program_digest: capture.artifact.program_digest.clone(),
    })
    .unwrap()
}

#[test]
fn actual_task_capture_pins_separate_identities_without_touching_registry() {
    let task = Task::new();
    let before = tasks::TaskRegistry::load()
        .entries()
        .iter()
        .find(|r| r.id == task.record.id)
        .unwrap()
        .clone();
    let a = task.capture();
    assert_eq!(
        a.binding.task.as_ref().unwrap().source_fingerprint,
        task_verification::source_fingerprint(&task.root, &task.root).unwrap()
    );
    fs::write(
        task.root.join("app.json"),
        serde_json::to_vec_pretty(&fixture::organizer()).unwrap(),
    )
    .unwrap();
    let b = task.capture();
    assert_ne!(a.artifact.raw_digest, b.artifact.raw_digest);
    assert_eq!(a.artifact.program_digest, b.artifact.program_digest);
    assert_ne!(a.binding.task, b.binding.task);
    assert_eq!(
        before,
        *tasks::TaskRegistry::load()
            .entries()
            .iter()
            .find(|r| r.id == task.record.id)
            .unwrap()
    );
    assert!(matches!(
        b.binding.producer,
        Producer::ExternalAuthor { .. }
    ));
}
#[test]
fn watcher_ingests_real_file_edits_and_ignored_source_changes() {
    let task = Task::new();
    fs::write(task.root.join(".gitignore"), "ignored\n").unwrap();
    let mut watcher = SourceWatcher::new(task.adapter.clone()).unwrap();
    let SourceUpdate::Captured(first) = next(&mut watcher) else {
        panic!("initial capture")
    };
    task.change();
    let SourceUpdate::Captured(second) = next(&mut watcher) else {
        panic!("edited capture")
    };
    assert_ne!(
        first.artifact.program_digest,
        second.artifact.program_digest
    );
    fs::write(task.root.join("ignored"), "changed ignored content").unwrap();
    let SourceUpdate::Captured(third) = next(&mut watcher) else {
        panic!("ignored capture")
    };
    assert_eq!(second.artifact, third.artifact);
    assert_ne!(second.binding.task, third.binding.task);
}
#[test]
fn partial_writes_are_pending_and_cannot_replace_last_complete_capture() {
    let task = Task::new();
    let mut watcher = SourceWatcher::new(task.adapter.clone()).unwrap();
    let SourceUpdate::Captured(first) = next(&mut watcher) else {
        panic!()
    };
    fs::write(task.root.join("app.json"), b"{\"version\":").unwrap();
    thread::sleep(Duration::from_millis(150));
    for _ in 0..3 {
        assert!(!matches!(watcher.poll(), SourceUpdate::Captured(_)));
        thread::sleep(Duration::from_millis(80));
    }
    assert_eq!(watcher.last_capture().unwrap(), first.as_ref());
    task.change();
    assert!(matches!(next(&mut watcher), SourceUpdate::Captured(_)));
}
#[test]
fn freshness_checks_detect_mid_analysis_and_pre_adoption_mutation() {
    let task = Task::new();
    let captured = task.capture();
    let result = task.adapter.with_fresh_source(&captured, || {
        task.change();
        Ok(42)
    });
    assert!(matches!(result, Err(AdapterError::Stale(_))));
    assert!(matches!(
        task.adapter.ensure_fresh(&captured),
        Err(AdapterError::Stale(_))
    ));
    assert_eq!(captured.program.label, "Club organizer");
}
#[test]
fn missing_moved_or_relinked_tasks_cannot_keep_old_evidence_current() {
    let task = Task::new();
    let captured = task.capture();
    tasks::TaskRegistry::load().unlink(&task.record.id).unwrap();
    assert!(task.adapter.ensure_fresh(&captured).is_err());
    let mut moved = task.record.clone();
    moved.worktree_path = task.root.join("missing").to_string_lossy().into();
    tasks::TaskRegistry::load().add(moved).unwrap();
    assert!(task.adapter.ensure_fresh(&captured).is_err());
}
#[test]
fn wrong_project_parent_escape_and_non_regular_program_are_rejected() {
    let task = Task::new();
    assert!(task
        .adapter
        .capture("other", Some(&task.record.id))
        .is_err());
    for path in [
        "../outside.json",
        "/outside.json",
        ".git/config",
        "sub/../../app.json",
    ] {
        let value = json!({"version":1,"project_id":"project","artifact_kind":"generated_app","program_path":path});
        fs::write(
            task.root.join(DECLARATION_PATH),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(
            task.adapter
                .capture("project", Some(&task.record.id))
                .is_err(),
            "{path}"
        );
    }
    let value = json!({"version":1,"project_id":"other","artifact_kind":"generated_app","program_path":"app.json"});
    fs::write(
        task.root.join(DECLARATION_PATH),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    assert!(task
        .adapter
        .capture("project", Some(&task.record.id))
        .is_err());
}
#[cfg(unix)]
#[test]
fn symlinked_program_and_parent_are_rejected() {
    use std::os::unix::fs::symlink;
    let task = Task::new();
    let outside = tempfile::tempdir().unwrap();
    fs::write(
        outside.path().join("app.json"),
        serde_json::to_vec(&fixture::organizer()).unwrap(),
    )
    .unwrap();
    fs::remove_file(task.root.join("app.json")).unwrap();
    symlink(outside.path().join("app.json"), task.root.join("app.json")).unwrap();
    assert!(task
        .adapter
        .capture("project", Some(&task.record.id))
        .is_err());
    fs::remove_file(task.root.join("app.json")).unwrap();
    symlink(outside.path(), task.root.join("linked")).unwrap();
    fs::write(task.root.join(DECLARATION_PATH),serde_json::to_vec(&json!({"version":1,"project_id":"project","artifact_kind":"generated_app","program_path":"linked/app.json"})).unwrap()).unwrap();
    assert!(task
        .adapter
        .capture("project", Some(&task.record.id))
        .is_err());
}
#[test]
fn undeclared_and_external_repositories_are_explicitly_unsupported() {
    let task = Task::new();
    fs::remove_file(task.root.join(DECLARATION_PATH)).unwrap();
    assert!(matches!(
        task.adapter.capture("project", Some(&task.record.id)),
        Err(AdapterError::Unsupported(_))
    ));
    fs::write(task.root.join(DECLARATION_PATH),serde_json::to_vec(&json!({"version":1,"project_id":"project","artifact_kind":"external_web","program_path":"app.json"})).unwrap()).unwrap();
    assert!(matches!(
        task.adapter.capture("project", Some(&task.record.id)),
        Err(AdapterError::Unsupported(_))
    ));
}
#[test]
fn external_handoff_automatically_correlates_completed_program_and_request() {
    let task = Task::new();
    let jobs = tempfile::tempdir().unwrap();
    let req = request(&task);
    let job =
        ExternalHandoff::prepare(&task.adapter, &fs::canonicalize(jobs.path()).unwrap(), &req)
            .unwrap();
    assert!(job.ingest().unwrap().is_none());
    fs::write(job.completion_path(), b"{").unwrap();
    assert!(job.ingest().unwrap().is_none());
    task.change();
    let capture = task.capture();
    fs::write(job.completion_path(), completion(&job, &capture)).unwrap();
    let imported = job.ingest().unwrap().unwrap();
    assert_eq!(imported.program, capture.program);
    assert!(matches!(
        imported.binding.producer,
        Producer::ExternalAuthor { .. }
    ));
    task.adapter.ensure_fresh(&imported).unwrap();
    assert!(
        ExternalHandoff::prepare(&task.adapter, &fs::canonicalize(jobs.path()).unwrap(), &req)
            .is_err()
    );
}
#[test]
fn external_results_reject_wrong_request_project_stale_program_and_bundle_changes() {
    let task = Task::new();
    let jobs = tempfile::tempdir().unwrap();
    let job = ExternalHandoff::prepare(
        &task.adapter,
        &fs::canonicalize(jobs.path()).unwrap(),
        &request(&task),
    )
    .unwrap();
    task.change();
    let capture = task.capture();
    let valid = completion(&job, &capture);
    for (key, value) in [
        ("request_id", json!("other")),
        ("project_id", json!("other")),
        ("task_id", json!("other")),
        ("program_path", json!("../app.json")),
        ("baseline_source_fingerprint", json!("0".repeat(64))),
    ] {
        let mut v: serde_json::Value = serde_json::from_slice(&valid).unwrap();
        v[key] = value;
        fs::write(job.completion_path(), serde_json::to_vec(&v).unwrap()).unwrap();
        assert!(job.ingest().is_err(), "{key}");
    }
    fs::write(job.completion_path(), &valid).unwrap();
    fs::write(
        task.root.join("app.json"),
        serde_json::to_vec_pretty(&capture.program).unwrap(),
    )
    .unwrap();
    assert!(job.ingest().is_err());
    fs::write(job.request_path(), b"{}").unwrap();
    assert!(job.ingest().is_err());
}
#[test]
fn intake_rejects_duplicates_oversize_and_jobs_inside_source_tree() {
    let task = Task::new();
    assert!(ExternalHandoff::prepare(&task.adapter, &task.root, &request(&task)).is_err());
    fs::write(task.root.join(DECLARATION_PATH), br#"{"version":1,"version":1,"project_id":"project","artifact_kind":"generated_app","program_path":"app.json"}"#).unwrap();
    assert!(task
        .adapter
        .capture("project", Some(&task.record.id))
        .is_err());
    fs::write(
        task.root.join(DECLARATION_PATH),
        vec![b' '; MAX_DECLARATION_BYTES + 1],
    )
    .unwrap();
    assert!(task
        .adapter
        .capture("project", Some(&task.record.id))
        .is_err());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn opaque_transport_ingestion_checks_fresh_source_and_keeps_fixture_origin() {
    use product_provider::*;
    use std::os::unix::fs::PermissionsExt;
    let task = Task::new();
    let baseline = task.capture();
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let bin = root.join("fake.py");
    {
        let _guard = fixture_executable_write_guard();
        fs::write(
            &bin,
            include_bytes!("fixtures/provider_transport/fake_cli.py"),
        )
        .unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(
            bin.with_extension("json"),
            br#"{"provider":"codex","mode":"good"}"#,
        )
        .unwrap();
    }
    let home = root.join("home");
    fs::create_dir(&home).unwrap();
    let transport =
        ProviderTransport::new_fixture(root.join("jobs"), ProviderKind::Codex, bin, home).unwrap();
    let request = ProviderRequest {
        request_id: "opaque-request".into(),
        provider: ProviderKind::Codex,
        source_digest: baseline.binding.identity().unwrap().as_str().into(),
        prompt: b"Fictional example".to_vec(),
        schema: br#"{"type":"object"}"#.to_vec(),
        purpose: "Synthetic regression".into(),
        data_categories: vec!["synthetic".into()],
        profile: CapabilityProfile::DataOnly,
        limits: JobLimits::default(),
    };
    let prepared = transport.prepare(request.clone()).unwrap();
    let approval = ConsentReceipt {
        disclosure_digest: prepared.disclosure.digest(),
        approval_reference: "fixture-only".into(),
        expires_at_unix_ms: unix_ms() + 60_000,
    };
    let mut job = transport.submit(prepared, &approval).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while job.poll().unwrap().is_none() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(20));
    }
    let result = task
        .adapter
        .ingest_transport(&transport, &request, &baseline)
        .unwrap()
        .unwrap();
    assert_eq!(
        result.receipt.provenance,
        InvocationProvenance::TransportFixture
    );
    assert!(
        serde_json::from_slice::<serde_json::Value>(&result.final_bytes).unwrap()["passed"]
            .as_bool()
            .unwrap()
    );
    task.change();
    assert!(matches!(
        task.adapter
            .ingest_transport(&transport, &request, &baseline),
        Err(AdapterError::Stale(_))
    ));
}

#[test]
fn machine_submission_needs_no_json_copy_and_stales_on_any_completed_source_change() {
    let task = Task::new();
    let jobs = tempfile::tempdir().unwrap();
    let job = ExternalHandoff::prepare(
        &task.adapter,
        &fs::canonicalize(jobs.path()).unwrap(),
        &request(&task),
    )
    .unwrap();
    task.change();
    job.submit_current_program().unwrap();
    let captured = job.ingest().unwrap().unwrap();
    assert_eq!(captured.program.label, "New actual source");
    assert!(job.submit_current_program().is_err());
    fs::write(task.root.join("other-source.txt"), "new behavior context").unwrap();
    assert!(matches!(job.ingest(), Err(AdapterError::Stale(_))));
}

#[cfg(unix)]
#[test]
fn non_regular_hardlinked_and_symlinked_completion_files_are_rejected() {
    use std::os::unix::fs::symlink;
    let task = Task::new();
    let jobs = tempfile::tempdir().unwrap();
    let job = ExternalHandoff::prepare(
        &task.adapter,
        &fs::canonicalize(jobs.path()).unwrap(),
        &request(&task),
    )
    .unwrap();
    let outside = jobs.path().join("outside.json");
    fs::write(&outside, completion(&job, &task.capture())).unwrap();
    symlink(&outside, job.completion_path()).unwrap();
    assert!(job.ingest().is_err());
    fs::remove_file(job.completion_path()).unwrap();
    fs::hard_link(&outside, job.completion_path()).unwrap();
    assert!(job.ingest().is_err());
    fs::remove_file(task.root.join("app.json")).unwrap();
    fs::create_dir(task.root.join("app.json")).unwrap();
    assert!(task.adapter.capture_current().is_err());
}

#[test]
fn hardlinked_program_and_completion_are_rejected_on_each_supported_platform() {
    let task = Task::new();
    let jobs = tempfile::tempdir().unwrap();
    let job = ExternalHandoff::prepare(
        &task.adapter,
        &fs::canonicalize(jobs.path()).unwrap(),
        &request(&task),
    )
    .unwrap();
    let outside = jobs.path().join("outside.json");
    fs::write(&outside, completion(&job, &task.capture())).unwrap();
    fs::hard_link(&outside, job.completion_path()).unwrap();
    assert!(job.ingest().is_err());
    fs::hard_link(
        task.root.join("app.json"),
        jobs.path().join("program-link.json"),
    )
    .unwrap();
    assert!(task.adapter.capture_current().is_err());
}
