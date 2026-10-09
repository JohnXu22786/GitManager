//! Synthetic task/source worker flow. No live author, provider or GUI acceptance.
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
#[path = "../src/product_studio/task_source_flow.rs"]
mod task_source_flow;
#[path = "../src/task_verification.rs"]
mod task_verification;
#[path = "../src/tasks.rs"]
mod tasks;

use product_contract::*;
use product_sources::{TaskSourceAdapter, DECLARATION_PATH};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, MutexGuard, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};
use task_source_flow::*;

static REGISTRY: Mutex<()> = Mutex::new(());
struct Task {
    _dir: tempfile::TempDir,
    root: PathBuf,
    record: tasks::TaskRecord,
    _lease: MutexGuard<'static, ()>,
}
impl Task {
    fn new() -> Self {
        let lease = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
        static HOME: OnceLock<tempfile::TempDir> = OnceLock::new();
        HOME.get_or_init(|| {
            let home = tempfile::tempdir().unwrap();
            let path = fs::canonicalize(home.path()).unwrap();
            std::env::set_var("HOME", &path);
            std::env::set_var("USERPROFILE", &path);
            std::env::set_var("XDG_CONFIG_HOME", &path);
            std::env::set_var("APPDATA", &path);
            home
        });
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let repo = git2::Repository::init(&root).unwrap();
        let tree = repo
            .find_tree(repo.index().unwrap().write_tree().unwrap())
            .unwrap();
        let who = git2::Signature::now("Fixture", "fixture@invalid.example").unwrap();
        repo.commit(Some("HEAD"), &who, &who, "Fixture", &tree, &[])
            .unwrap();
        fs::create_dir(root.join(".gitmanager")).unwrap();
        fs::write(root.join(DECLARATION_PATH), serde_json::to_vec(&json!({
            "version":1,"project_id":"project","artifact_kind":"generated_app","program_path":"app.json"
        })).unwrap()).unwrap();
        fs::write(
            root.join("app.json"),
            serde_json::to_vec(&fixture::organizer()).unwrap(),
        )
        .unwrap();
        let record = tasks::TaskRecord::from_worktree("Existing development task", &root).unwrap();
        tasks::TaskRegistry::load().add(record.clone()).unwrap();
        Self {
            _dir: dir,
            root,
            record,
            _lease: lease,
        }
    }
    fn flow(&self) -> TaskSourceFlow {
        TaskSourceFlow::link("project", &self.record.id).unwrap()
    }
    fn capture(&self) -> CapturedProgram {
        TaskSourceAdapter::link("project", &self.record.id)
            .unwrap()
            .capture_current()
            .unwrap()
    }
    fn edit(&self, label: &str) {
        let mut program = fixture::organizer();
        program["label"] = json!(label);
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
fn active() -> AtomicBool {
    AtomicBool::new(false)
}
fn settle(flow: &mut TaskSourceFlow) -> CapturedProgram {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let FlowUpdate::Captured(capture) = flow.poll() {
            return *capture;
        }
        assert!(
            Instant::now() < deadline,
            "source never settled: {:?}",
            flow.view()
        );
        thread::sleep(Duration::from_millis(25));
    }
}
fn poll_until(flow: &mut TaskSourceFlow, predicate: impl Fn(&SourceStatus) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        flow.poll();
        if predicate(&flow.view().source_status) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "status not observed: {:?}",
            flow.view()
        );
        thread::sleep(Duration::from_millis(25));
    }
}
fn request(task: &Task, id: &str) -> DevelopmentRequest {
    DevelopmentRequest {
        version: 1,
        id: id.into(),
        project_id: "project".into(),
        operation: DevelopmentOperation::Modify,
        request: "Use my current work and saved decisions".into(),
        sources: vec![task.capture()],
        context: DevelopmentContext {
            view: Some("contacts".into()),
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
    }
}
struct Jobs {
    _dir: tempfile::TempDir,
    root: PathBuf,
}
impl Jobs {
    fn path(&self) -> &std::path::Path {
        &self.root
    }
}
fn job_root() -> Jobs {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    Jobs { _dir: dir, root }
}

#[test]
fn linking_uses_only_existing_registry_identity_and_does_not_register_another_task() {
    let task = Task::new();
    let before = tasks::TaskRegistry::load().entries().to_vec();
    let choices = task_choices().unwrap();
    assert_eq!(choices.len(), 1);
    assert_eq!(choices[0].task_id, task.record.id);
    let mut flow = task.flow();
    let capture = settle(&mut flow);
    assert_eq!(capture, task.capture());
    assert_eq!(flow.view().task_id, task.record.id);
    assert!(matches!(flow.view().source_status, SourceStatus::Current));
    assert_eq!(before, tasks::TaskRegistry::load().entries());
    assert!(TaskSourceFlow::link("project", "not-registered").is_err());
}

#[test]
fn watcher_coalesces_real_edits_and_emits_only_a_stable_complete_capture() {
    let task = Task::new();
    let mut flow = task.flow();
    let old = settle(&mut flow);
    for label in ["First write", "Second write", "Complete write"] {
        task.edit(label);
    }
    let changed = settle(&mut flow);
    assert_eq!(changed.program.label, "Complete write");
    assert_ne!(old.artifact.program_digest, changed.artifact.program_digest);
    assert_eq!(flow.last_capture(), Some(&changed));
    assert!(matches!(flow.poll(), FlowUpdate::Unchanged));
}

#[test]
fn partial_or_invalid_writes_leave_last_capture_visible_but_stale_then_recover() {
    let task = Task::new();
    let original = fs::read(task.root.join("app.json")).unwrap();
    let mut flow = task.flow();
    let old = settle(&mut flow);
    for bytes in [b"{\"version\":".as_slice(), b"{\"wrong\":true}".as_slice()] {
        fs::write(task.root.join("app.json"), bytes).unwrap();
        poll_until(&mut flow, |s| matches!(s, SourceStatus::Pending(_)));
        assert_eq!(flow.last_capture(), Some(&old));
        assert!(flow.view().last_capture_stale);
        assert!(flow.analyze(&old, &active(), |_| Ok(())).is_err());
    }
    fs::write(task.root.join("app.json"), original).unwrap();
    poll_until(&mut flow, |s| matches!(s, SourceStatus::Current));
    assert!(!flow.view().last_capture_stale);
    assert_eq!(flow.last_capture(), Some(&old));
}

#[test]
fn periodic_reconciliation_detects_unwatched_registry_removal_and_moved_task() {
    let task = Task::new();
    let mut flow = task.flow();
    let old = settle(&mut flow);
    // The registry lives outside the watched worktree: no native file event.
    tasks::TaskRegistry::load().unlink(&task.record.id).unwrap();
    poll_until(&mut flow, |s| matches!(s, SourceStatus::Unavailable(_)));
    assert!(flow.view().last_capture_stale);
    assert_eq!(flow.last_capture(), Some(&old));
    let other = tempfile::tempdir().unwrap();
    let mut moved = task.record.clone();
    moved.worktree_path = fs::canonicalize(other.path())
        .unwrap()
        .to_string_lossy()
        .into();
    tasks::TaskRegistry::load().add(moved).unwrap();
    assert!(flow.analyze(&old, &active(), |_| Ok(())).is_err());
    assert!(matches!(flow.poll(), FlowUpdate::StatusChanged));
}

#[test]
fn ignored_and_nonprogram_edits_stale_full_source_even_when_program_is_unchanged() {
    let task = Task::new();
    fs::write(task.root.join(".gitignore"), "ignored.txt\n").unwrap();
    let mut flow = task.flow();
    let original = settle(&mut flow);
    let analysis = flow
        .analyze(&original, &active(), |_| Ok("checked"))
        .unwrap();
    fs::write(task.root.join("ignored.txt"), "meaningful hidden context").unwrap();
    assert!(flow.check_before_adoption(&analysis, &active()).is_err());
    let changed = settle(&mut flow);
    assert_eq!(original.artifact, changed.artifact);
    assert_ne!(original.binding.task, changed.binding.task);
    fs::write(task.root.join("readme.txt"), "more source context").unwrap();
    assert!(flow.analyze(&changed, &active(), |_| Ok(())).is_err());
}

#[test]
fn analysis_rechecks_source_before_after_and_immediately_before_adoption() {
    let task = Task::new();
    let flow = task.flow();
    let old = task.capture();
    let result = flow.analyze(&old, &active(), |source| {
        assert_eq!(source, &old);
        task.edit("Edited while analyzing");
        Ok(123)
    });
    assert!(matches!(result, Err(AdapterError::Stale(_))));
    let called = AtomicBool::new(false);
    assert!(flow
        .analyze(&old, &active(), |_| {
            called.store(true, Ordering::Release);
            Ok(())
        })
        .is_err());
    assert!(!called.load(Ordering::Acquire));
    let current = task.capture();
    let good = flow.analyze(&current, &active(), |_| Ok(456)).unwrap();
    assert_eq!(*good.value(), 456);
    flow.check_before_adoption(&good, &active()).unwrap();
    task.edit("Changed before adoption");
    assert!(flow.check_before_adoption(&good, &active()).is_err());
}

#[test]
fn exact_authorized_bundle_source_edit_machine_submit_and_intake_preserve_origin() {
    let task = Task::new();
    let jobs = job_root();
    let mut flow = task.flow();
    let mut request = request(&task, "external-edit");
    // Synthetic graph bytes test envelope preservation, not accepted-scene proof.
    let scenario = canonical_digest(IdentityDomain::Scenario, &"fixture-scene").unwrap();
    request.decisions = DecisionGraph {
        version: 1,
        revision: 1,
        decisions: vec![ScopedDecision {
            id: "fixture-decision".into(),
            revision: 1,
            request: "Keep this unresolved business choice".into(),
            rationale: None,
            scope: DecisionScope {
                operations: ["export_people".into()].into(),
                population: Population::All,
                conditions: Values::new(),
                excluded_records: vec![],
                unknowns: vec![],
            },
            outcome: DecisionOutcome::Deferred,
            status: DecisionStatus::Pending,
            obligations: vec![],
            scenarios: vec![scenario.clone()],
            witness: scenario,
            supersedes: vec![],
        }],
    };
    request.unknowns = vec![UnknownBoundary {
        id: "future-selection".into(),
        operations: ["collect".into()].into(),
        description: "The next selection change is not decided".into(),
    }];
    request.context.view = Some("people".into());
    request.context.recent_inputs = vec![SemanticInput::Observe {
        point: "current-selection".into(),
    }];
    let invocation = flow
        .prepare_external(jobs.path(), &request, &active())
        .unwrap();
    assert_eq!(invocation.request(), &request);
    assert_eq!(
        serde_json::from_slice::<DevelopmentRequest>(&fs::read(invocation.request_path()).unwrap())
            .unwrap(),
        request
    );
    assert_eq!(
        flow.inspect_decisions(&request.id).unwrap(),
        &request.decisions
    );
    assert!(flow
        .intake_external(&request.id, &active())
        .unwrap()
        .is_none());
    task.edit("External authored change");
    flow.submit_external(&request.id, &active()).unwrap();
    assert!(flow.submit_external(&request.id, &active()).is_err());
    let edit = flow
        .intake_external(&request.id, &active())
        .unwrap()
        .unwrap();
    assert_eq!(edit.request(), &request);
    assert_eq!(edit.baseline(), &request.sources[0]);
    assert_eq!(edit.capture().program.label, "External authored change");
    assert!(matches!(
        edit.capture().binding.producer,
        Producer::ExternalAuthor { .. }
    ));
    let analysis = flow
        .analyze(edit.capture(), &active(), |_| Ok("independent analysis"))
        .unwrap();
    flow.check_before_adoption(&analysis, &active()).unwrap();
    assert!(matches!(
        flow.view().external,
        ExternalStatus::Captured { .. }
    ));
    assert!(flow.intake_external(&request.id, &active()).is_err());
}

#[test]
fn truncated_completion_remains_pending_and_forged_completion_is_not_capture() {
    let task = Task::new();
    let jobs = job_root();
    let mut flow = task.flow();
    let request = request(&task, "partial-completion");
    let invocation = flow
        .prepare_external(jobs.path(), &request, &active())
        .unwrap();
    let complete = invocation
        .request_path()
        .parent()
        .unwrap()
        .join("complete.json");
    fs::write(&complete, b"{\"version\":").unwrap();
    assert!(flow
        .intake_external(&request.id, &active())
        .unwrap()
        .is_none());
    assert!(matches!(
        flow.view().external,
        ExternalStatus::AwaitingCompletion { .. }
    ));
    fs::remove_file(&complete).unwrap();
    task.edit("Edited program");
    flow.submit_external(&request.id, &active()).unwrap();
    let mut forged: serde_json::Value =
        serde_json::from_slice(&fs::read(&complete).unwrap()).unwrap();
    forged["request_id"] = json!("another-request");
    fs::write(&complete, serde_json::to_vec(&forged).unwrap()).unwrap();
    assert!(flow.intake_external(&request.id, &active()).is_err());
    assert!(matches!(
        flow.view().external,
        ExternalStatus::Rejected { .. }
    ));
}

#[test]
fn completed_source_changed_before_intake_is_rejected_without_adoption() {
    let task = Task::new();
    let jobs = job_root();
    let mut flow = task.flow();
    let request = request(&task, "changed-completion");
    flow.prepare_external(jobs.path(), &request, &active())
        .unwrap();
    task.edit("First completion");
    flow.submit_external(&request.id, &active()).unwrap();
    fs::write(task.root.join("outside-program.txt"), "later source").unwrap();
    assert!(matches!(
        flow.intake_external(&request.id, &active()),
        Err(AdapterError::Stale(_))
    ));
}

#[test]
fn duplicate_wrong_id_and_cancelled_handoffs_cannot_submit_or_deliver_late_results() {
    let task = Task::new();
    let jobs = job_root();
    let mut flow = task.flow();
    let request = request(&task, "cancelled-edit");
    let invocation = flow
        .prepare_external(jobs.path(), &request, &active())
        .unwrap();
    let bytes = fs::read(invocation.request_path()).unwrap();
    assert!(flow
        .prepare_external(jobs.path(), &request, &active())
        .is_err());
    assert!(flow.submit_external("wrong-id", &active()).is_err());
    assert!(flow.inspect_decisions("wrong-id").is_err());
    assert!(flow.cancel_external("wrong-id").is_err());
    flow.cancel_external(&request.id).unwrap();
    assert!(flow.submit_external(&request.id, &active()).is_err());
    assert!(flow.intake_external(&request.id, &active()).is_err());
    assert!(flow
        .prepare_external(jobs.path(), &request, &active())
        .is_err());
    assert_eq!(fs::read(invocation.request_path()).unwrap(), bytes);
    assert!(!invocation
        .request_path()
        .parent()
        .unwrap()
        .join("complete.json")
        .exists());
    let next = request_for_next(&task);
    flow.prepare_external(jobs.path(), &next, &active())
        .unwrap();
}
fn request_for_next(task: &Task) -> DevelopmentRequest {
    request(task, "fresh-next-request")
}

#[test]
fn interrupted_handoff_stays_blocked_without_reprepare_and_allows_explicit_new_job() {
    let task = Task::new();
    let jobs = job_root();
    let request = request(&task, "interrupted-edit");
    let mut old = task.flow();
    let invocation = old
        .prepare_external(jobs.path(), &request, &active())
        .unwrap();
    let pending = old.pending_external().unwrap().clone();
    let original = fs::read(invocation.request_path()).unwrap();
    drop(old);
    task.edit("Edited while app was closed");
    let mut resumed = TaskSourceFlow::interrupted("project", &task.record.id, pending).unwrap();
    assert!(matches!(
        resumed.view().external,
        ExternalStatus::Interrupted { .. }
    ));
    assert!(resumed.submit_external(&request.id, &active()).is_err());
    assert!(resumed.intake_external(&request.id, &active()).is_err());
    assert!(resumed
        .prepare_external(jobs.path(), &request_for_next(&task), &active())
        .is_err());
    assert_eq!(
        resumed.inspect_decisions(&request.id).unwrap(),
        &request.decisions
    );
    assert_eq!(
        settle(&mut resumed).program.label,
        "Edited while app was closed"
    );
    assert_eq!(fs::read(invocation.request_path()).unwrap(), original);
    resumed.cancel_external(&request.id).unwrap();
    resumed
        .prepare_external(jobs.path(), &request_for_next(&task), &active())
        .unwrap();
}

#[test]
fn unsupported_repository_never_runs_arbitrary_scripts_or_claims_current_capture() {
    let task = Task::new();
    fs::remove_file(task.root.join(DECLARATION_PATH)).unwrap();
    fs::write(
        task.root.join("run.sh"),
        "this is not an authorized interpreter input",
    )
    .unwrap();
    let mut flow = task.flow();
    poll_until(&mut flow, |s| matches!(s, SourceStatus::Unsupported(_)));
    assert!(flow.last_capture().is_none());
    assert!(!flow.view().last_capture_stale);
    fs::write(task.root.join(DECLARATION_PATH), serde_json::to_vec(&json!({
        "version":1,"project_id":"project","artifact_kind":"external_web","program_path":"run.sh"
    })).unwrap()).unwrap();
    poll_until(&mut flow, |s| matches!(s, SourceStatus::Unsupported(_)));
    assert!(flow.last_capture().is_none());
}

#[test]
fn cancellation_before_during_and_after_analysis_prevents_result_admission() {
    let task = Task::new();
    let flow = task.flow();
    let capture = task.capture();
    let cancel = AtomicBool::new(true);
    assert!(matches!(
        flow.analyze::<()>(&capture, &cancel, |_| panic!("must not execute")),
        Err(AdapterError::Cancelled)
    ));
    cancel.store(false, Ordering::Release);
    assert!(matches!(
        flow.analyze(&capture, &cancel, |_| {
            cancel.store(true, Ordering::Release);
            Ok(())
        }),
        Err(AdapterError::Cancelled)
    ));
    cancel.store(false, Ordering::Release);
    let checked = flow.analyze(&capture, &cancel, |_| Ok(())).unwrap();
    cancel.store(true, Ordering::Release);
    assert!(matches!(
        flow.check_before_adoption(&checked, &cancel),
        Err(AdapterError::Cancelled)
    ));
    let mut flow = task.flow();
    let jobs = job_root();
    assert!(matches!(
        flow.prepare_external(jobs.path(), &request(&task, "never-prepared"), &cancel),
        Err(AdapterError::Cancelled)
    ));
    assert_eq!(fs::read_dir(jobs.path()).unwrap().count(), 0);
}

#[derive(serde::Serialize, serde::Deserialize)]
struct RestartInput {
    root: PathBuf,
    request: DevelopmentRequest,
    ticket: product_sources::ExternalHandoffTicket,
}
fn job_files(root: &std::path::Path) -> Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> {
    fn walk(root: &std::path::Path, rows: &mut Vec<(PathBuf, Vec<u8>, std::time::SystemTime)>) {
        for item in fs::read_dir(root).unwrap() {
            let path = item.unwrap().path();
            if path.is_dir() {
                walk(&path, rows);
            } else {
                rows.push((
                    path.clone(),
                    fs::read(&path).unwrap(),
                    fs::metadata(&path).unwrap().modified().unwrap(),
                ));
            }
        }
    }
    let mut rows = vec![];
    walk(root, &mut rows);
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

#[test]
fn exact_job_reopens_and_ingests_in_a_fresh_process_without_rewriting_files() {
    const INPUT: &str = "GIT_MANAGER_TASK_SOURCE_RESTART_FIXTURE";
    if let Some(path) = std::env::var_os(INPUT) {
        let input: RestartInput = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        let task_id = &input.request.sources[0]
            .binding
            .task
            .as_ref()
            .unwrap()
            .task_id;
        let adapter = TaskSourceAdapter::link(&input.request.project_id, task_id).unwrap();
        let before = job_files(&input.root);
        let job = product_sources::ExternalHandoff::reopen(
            &adapter,
            &input.root,
            &input.request,
            &input.ticket,
        )
        .unwrap();
        let capture = job.ingest().unwrap().unwrap();
        assert_eq!(
            capture.program.label,
            "Completed before fresh-process intake"
        );
        assert!(matches!(
            capture.binding.producer,
            Producer::ExternalAuthor { .. }
        ));
        assert_eq!(job.request(), &input.request);
        assert_eq!(before, job_files(&input.root));
        return;
    }
    let task = Task::new();
    let jobs = job_root();
    let root = fs::canonicalize(jobs.path()).unwrap();
    let request = request(&task, "restart-complete");
    let adapter = TaskSourceAdapter::link("project", &task.record.id).unwrap();
    let job = product_sources::ExternalHandoff::prepare(&adapter, &root, &request).unwrap();
    let ticket = job.ticket().unwrap();
    task.edit("Completed before fresh-process intake");
    job.submit_current_program().unwrap();
    drop(job);
    let input_dir = tempfile::tempdir().unwrap();
    let input_path = input_dir.path().join("restart.json");
    fs::write(
        &input_path,
        serde_json::to_vec(&RestartInput {
            root: root.clone(),
            request,
            ticket,
        })
        .unwrap(),
    )
    .unwrap();
    let before = job_files(&root);
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "exact_job_reopens_and_ingests_in_a_fresh_process_without_rewriting_files",
            "--nocapture",
        ])
        .env(INPUT, input_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "fresh process failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(before, job_files(&root));
}

#[test]
fn reopened_pending_job_keeps_partial_completion_pending_and_accepts_later_submission() {
    let task = Task::new();
    let jobs = job_root();
    let root = fs::canonicalize(jobs.path()).unwrap();
    let request = request(&task, "restart-pending");
    let mut flow = task.flow();
    let invocation = flow.prepare_external(&root, &request, &active()).unwrap();
    let pending_bytes = serde_json::to_vec(flow.pending_external().unwrap()).unwrap();
    drop(flow);
    task.edit("External changes while closed");
    let completion = invocation
        .request_path()
        .parent()
        .unwrap()
        .join("complete.json");
    fs::write(&completion, b"{\"version\":").unwrap();
    let pending: PendingExternal = serde_json::from_slice(&pending_bytes).unwrap();
    let mut flow = TaskSourceFlow::interrupted("project", &task.record.id, pending).unwrap();
    let before = job_files(&root);
    flow.reopen_external(&root, &active(), &active()).unwrap();
    assert_eq!(before, job_files(&root));
    assert!(flow
        .intake_external(&request.id, &active())
        .unwrap()
        .is_none());
    fs::remove_file(completion).unwrap();
    flow.submit_external(&request.id, &active()).unwrap();
    let edit = flow
        .intake_external(&request.id, &active())
        .unwrap()
        .unwrap();
    assert_eq!(
        edit.capture().program.label,
        "External changes while closed"
    );
    assert_eq!(edit.request(), &request);
}

#[test]
fn reopen_rejects_wrong_ticket_request_root_and_changed_request_bytes_without_writes() {
    let task = Task::new();
    let jobs = job_root();
    let root = fs::canonicalize(jobs.path()).unwrap();
    let adapter = TaskSourceAdapter::link("project", &task.record.id).unwrap();
    let request = request(&task, "pinned-request");
    let first = product_sources::ExternalHandoff::prepare(&adapter, &root, &request).unwrap();
    let ticket = first.ticket().unwrap();
    let next = request_for_next(&task);
    let second = product_sources::ExternalHandoff::prepare(&adapter, &root, &next).unwrap();
    let before = job_files(&root);
    assert!(product_sources::ExternalHandoff::reopen(
        &adapter,
        &root,
        &request,
        &second.ticket().unwrap()
    )
    .is_err());
    let mut changed = request.clone();
    changed.request = "Different disclosure and purpose".into();
    assert!(product_sources::ExternalHandoff::reopen(&adapter, &root, &changed, &ticket).is_err());
    let elsewhere = job_root();
    assert!(product_sources::ExternalHandoff::reopen(
        &adapter,
        &fs::canonicalize(elsewhere.path()).unwrap(),
        &request,
        &ticket
    )
    .is_err());
    assert_eq!(before, job_files(&root));
    // Even semantically equal JSON is not the exact original request bytes.
    fs::write(
        first.request_path(),
        serde_json::to_vec_pretty(&request).unwrap(),
    )
    .unwrap();
    let tampered = job_files(&root);
    assert!(product_sources::ExternalHandoff::reopen(&adapter, &root, &request, &ticket).is_err());
    assert_eq!(tampered, job_files(&root));
}

#[test]
fn reopen_rejects_replaced_registry_identity_and_changed_program_path() {
    let task = Task::new();
    let jobs = job_root();
    let root = fs::canonicalize(jobs.path()).unwrap();
    let adapter = TaskSourceAdapter::link("project", &task.record.id).unwrap();
    let request = request(&task, "registry-pinned");
    let job = product_sources::ExternalHandoff::prepare(&adapter, &root, &request).unwrap();
    let ticket = job.ticket().unwrap();
    tasks::TaskRegistry::load().unlink(&task.record.id).unwrap();
    assert!(product_sources::ExternalHandoff::reopen(&adapter, &root, &request, &ticket).is_err());
    let mut replacement = task.record.clone();
    replacement.created_at = "2099-01-01T00:00:00Z".into();
    tasks::TaskRegistry::load().add(replacement).unwrap();
    let relinked = TaskSourceAdapter::link("project", &task.record.id).unwrap();
    assert!(product_sources::ExternalHandoff::reopen(&relinked, &root, &request, &ticket).is_err());
    tasks::TaskRegistry::load().unlink(&task.record.id).unwrap();
    tasks::TaskRegistry::load()
        .add(task.record.clone())
        .unwrap();
    fs::copy(task.root.join("app.json"), task.root.join("moved.json")).unwrap();
    fs::write(task.root.join(DECLARATION_PATH), serde_json::to_vec(&json!({"version":1,"project_id":"project","artifact_kind":"generated_app","program_path":"moved.json"})).unwrap()).unwrap();
    assert!(product_sources::ExternalHandoff::reopen(&adapter, &root, &request, &ticket).is_err());
}

#[test]
fn reopen_never_recaptures_stale_completion_as_current_and_absent_ticket_stays_interrupted() {
    let task = Task::new();
    let jobs = job_root();
    let root = fs::canonicalize(jobs.path()).unwrap();
    let request = request(&task, "stale-restart");
    let mut flow = task.flow();
    flow.prepare_external(&root, &request, &active()).unwrap();
    let pending = flow.pending_external().unwrap().clone();
    task.edit("Completed source");
    flow.submit_external(&request.id, &active()).unwrap();
    drop(flow);
    fs::write(
        task.root.join("later-file"),
        "source changed after completion",
    )
    .unwrap();
    let mut flow =
        TaskSourceFlow::interrupted("project", &task.record.id, pending.clone()).unwrap();
    flow.reopen_external(&root, &active(), &active()).unwrap();
    assert!(matches!(
        flow.intake_external(&request.id, &active()),
        Err(AdapterError::Stale(_))
    ));
    let mut missing = serde_json::to_value(pending).unwrap();
    missing.as_object_mut().unwrap().remove("ticket");
    let mut flow = TaskSourceFlow::interrupted(
        "project",
        &task.record.id,
        serde_json::from_value(missing).unwrap(),
    )
    .unwrap();
    assert!(flow.reopen_external(&root, &active(), &active()).is_err());
    assert!(matches!(
        flow.view().external,
        ExternalStatus::Interrupted { .. }
    ));
    assert_eq!(settle(&mut flow).program.label, "Completed source");
}

#[cfg(unix)]
#[test]
fn reopen_refuses_symlinked_roots_jobs_and_request_files() {
    use std::os::unix::fs::symlink;
    let task = Task::new();
    let jobs = job_root();
    let root = fs::canonicalize(jobs.path()).unwrap();
    let adapter = TaskSourceAdapter::link("project", &task.record.id).unwrap();
    let request = request(&task, "nofollow-reopen");
    let job = product_sources::ExternalHandoff::prepare(&adapter, &root, &request).unwrap();
    let ticket = job.ticket().unwrap();
    let aliases = tempfile::tempdir().unwrap();
    let alias = fs::canonicalize(aliases.path()).unwrap().join("root-alias");
    symlink(&root, &alias).unwrap();
    assert!(product_sources::ExternalHandoff::reopen(&adapter, &alias, &request, &ticket).is_err());
    let request_copy = aliases.path().join("request-copy.json");
    fs::rename(job.request_path(), &request_copy).unwrap();
    symlink(&request_copy, job.request_path()).unwrap();
    assert!(product_sources::ExternalHandoff::reopen(&adapter, &root, &request, &ticket).is_err());
    fs::remove_file(job.request_path()).unwrap();
    fs::rename(&request_copy, job.request_path()).unwrap();
    let directory = root.join(&request.id);
    let moved = root.join("moved-job");
    fs::rename(&directory, &moved).unwrap();
    symlink(&moved, &directory).unwrap();
    assert!(product_sources::ExternalHandoff::reopen(&adapter, &root, &request, &ticket).is_err());
}

#[test]
fn external_cancellation_flag_revokes_pending_intake_and_preserves_completed_files() {
    let task = Task::new();
    let jobs = job_root();
    let mut flow = task.flow();
    let request = request(&task, "cancel-flag");
    flow.prepare_external(jobs.path(), &request, &active())
        .unwrap();
    task.edit("Completed before cancellation");
    flow.submit_external(&request.id, &active()).unwrap();
    let before = job_files(jobs.path());
    assert!(matches!(
        flow.intake_external(&request.id, &AtomicBool::new(true)),
        Err(AdapterError::Cancelled)
    ));
    assert!(matches!(
        flow.view().external,
        ExternalStatus::Cancelled { .. }
    ));
    assert!(flow.intake_external(&request.id, &active()).is_err());
    assert_eq!(before, job_files(jobs.path()));
}

#[test]
fn terminal_jobs_are_not_returned_as_restart_pending_but_decisions_remain_inspectable() {
    let task = Task::new();
    let jobs = job_root();
    let mut flow = task.flow();
    for terminal in ["cancel", "capture", "reject"] {
        let request = request(&task, &format!("terminal-{terminal}"));
        let invocation = flow
            .prepare_external(jobs.path(), &request, &active())
            .unwrap();
        assert!(flow.pending_external().is_some());
        flow.submit_external(&request.id, &active()).unwrap();
        match terminal {
            "cancel" => flow.cancel_external(&request.id).unwrap(),
            "capture" => assert!(flow
                .intake_external(&request.id, &active())
                .unwrap()
                .is_some()),
            "reject" => {
                fs::write(
                    invocation
                        .request_path()
                        .parent()
                        .unwrap()
                        .join("complete.json"),
                    b"{}",
                )
                .unwrap();
                assert!(flow.intake_external(&request.id, &active()).is_err());
            }
            _ => unreachable!(),
        }
        // The existing journal can clear its pending slot instead of restoring
        // a cancelled, already delivered, or rejected completion after restart.
        assert_eq!(
            serde_json::to_vec(&flow.pending_external()).unwrap(),
            b"null"
        );
        assert_eq!(
            flow.inspect_decisions(&request.id).unwrap(),
            &request.decisions
        );
    }
}

#[test]
fn cancelled_reopen_is_terminal_and_cannot_be_journaled_or_resumed_again() {
    let task = Task::new();
    let jobs = job_root();
    let request = request(&task, "cancel-reopen");
    let mut original = task.flow();
    original
        .prepare_external(jobs.path(), &request, &active())
        .unwrap();
    original.submit_external(&request.id, &active()).unwrap();
    let pending = original.pending_external().unwrap().clone();
    drop(original);
    let mut resumed = TaskSourceFlow::interrupted("project", &task.record.id, pending).unwrap();
    let before = job_files(jobs.path());
    assert!(matches!(
        resumed.reopen_external(jobs.path(), &active(), &AtomicBool::new(true)),
        Err(AdapterError::Cancelled)
    ));
    assert!(matches!(
        resumed.view().external,
        ExternalStatus::Cancelled { .. }
    ));
    assert!(resumed.pending_external().is_none());
    assert!(resumed
        .reopen_external(jobs.path(), &active(), &active())
        .is_err());
    assert!(resumed.intake_external(&request.id, &active()).is_err());
    assert_eq!(before, job_files(jobs.path()));
}

// The saved ticket comes from the controller's own exact pending association.
// This synthetic machine participant continues independently of the UI read.
fn external_machine(
    task: &Task,
    root: &std::path::Path,
    pending: &PendingExternal,
) -> product_sources::ExternalHandoff {
    let value = serde_json::to_value(pending).unwrap();
    let ticket: product_sources::ExternalHandoffTicket =
        serde_json::from_value(value["ticket"].clone()).unwrap();
    product_sources::ExternalHandoff::reopen(
        &TaskSourceAdapter::link("project", &task.record.id).unwrap(),
        root,
        pending.request(),
        &ticket,
    )
    .unwrap()
}

#[test]
fn read_cancellation_preserves_recovery_for_late_completion_after_restart() {
    let task = Task::new();
    let jobs = job_root();
    let request = request(&task, "cancel-only-read");
    let mut original = task.flow();
    original
        .prepare_external(jobs.path(), &request, &active())
        .unwrap();
    let pending = original.pending_external().unwrap().clone();
    let machine = external_machine(&task, jobs.path(), &pending);
    let saved = serde_json::to_vec(&pending).unwrap();
    drop(original);
    let mut resumed = TaskSourceFlow::interrupted("project", &task.record.id, pending).unwrap();
    let before = job_files(jobs.path());
    assert!(matches!(
        resumed.reopen_external(jobs.path(), &AtomicBool::new(true), &active()),
        Err(AdapterError::Cancelled)
    ));
    assert!(matches!(
        resumed.view().external,
        ExternalStatus::Interrupted { .. }
    ));
    assert!(resumed.view().can_reopen);
    assert_eq!(
        serde_json::to_vec(resumed.pending_external().unwrap()).unwrap(),
        saved
    );
    assert_eq!(job_files(jobs.path()), before);
    // Cancelling this read neither abandoned intake nor stopped the author.
    task.edit("Author finished after the cancelled read");
    machine.submit_current_program().unwrap();
    let persisted = serde_json::to_vec(resumed.pending_external().unwrap()).unwrap();
    drop(resumed);
    let mut restarted = TaskSourceFlow::interrupted(
        "project",
        &task.record.id,
        serde_json::from_slice(&persisted).unwrap(),
    )
    .unwrap();
    restarted
        .reopen_external(jobs.path(), &active(), &active())
        .unwrap();
    let edit = restarted
        .intake_external(&request.id, &active())
        .unwrap()
        .unwrap();
    assert_eq!(edit.request(), &request);
    assert_eq!(
        edit.capture().program.label,
        "Author finished after the cancelled read"
    );
    assert!(matches!(
        edit.capture().binding.producer,
        Producer::ExternalAuthor { .. }
    ));
    assert!(restarted.pending_external().is_none());
}

#[test]
fn simultaneous_read_and_handoff_cancellation_rejects_late_completion_after_restart() {
    let task = Task::new();
    let jobs = job_root();
    let request = request(&task, "cancel-read-and-job");
    let mut original = task.flow();
    original
        .prepare_external(jobs.path(), &request, &active())
        .unwrap();
    let pending = original.pending_external().unwrap().clone();
    let machine = external_machine(&task, jobs.path(), &pending);
    drop(original);
    let mut resumed = TaskSourceFlow::interrupted("project", &task.record.id, pending).unwrap();
    let before = job_files(jobs.path());
    assert!(matches!(
        resumed.reopen_external(jobs.path(), &AtomicBool::new(true), &AtomicBool::new(true)),
        Err(AdapterError::Cancelled)
    ));
    assert!(matches!(
        resumed.view().external,
        ExternalStatus::Cancelled { .. }
    ));
    assert!(!resumed.view().can_reopen);
    assert_eq!(job_files(jobs.path()), before);
    assert!(resumed.pending_external().is_none());
    assert!(resumed
        .reopen_external(jobs.path(), &active(), &active())
        .is_err());
    // The independent author can still finish; local abandonment is not a kill.
    task.edit("Late external completion after local abandonment");
    machine.submit_current_program().unwrap();
    assert!(resumed.intake_external(&request.id, &active()).is_err());
    let persisted = serde_json::to_vec(&resumed.pending_external()).unwrap();
    assert_eq!(persisted, b"null");
    let restored: Option<PendingExternal> = serde_json::from_slice(&persisted).unwrap();
    assert!(restored.is_none());
    drop(resumed);
    let mut restarted = task.flow();
    assert!(restarted.intake_external(&request.id, &active()).is_err());
    assert_eq!(
        settle(&mut restarted).program.label,
        "Late external completion after local abandonment"
    );
}
