use crate::task_merge_preview::PreviewWorkspace;
use crate::task_verification::{self, VerificationCommand, VerificationResult, VerificationState};
use crate::tasks::{TaskDependency, TaskRecord, TaskRegistry};
use git2::{BranchType, Repository};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegratedVerificationStatus {
    Running,
    Passed,
    Partial,
    Failed,
    Cancelled,
    TimedOut,
    Error,
    Stale,
    Unavailable,
}

impl IntegratedVerificationStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Passed => "Passed",
            Self::Partial => "Partial coverage",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
            Self::TimedOut => "Timed out",
            Self::Error => "Could not run",
            Self::Stale => "Stale",
            Self::Unavailable => "Unavailable",
        }
    }
}

#[derive(Clone, Debug)]
pub struct IntegratedCommandCoverage {
    pub command: VerificationCommand,
    pub task_ids: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct IntegratedVerificationCoverage {
    pub commands: Vec<IntegratedCommandCoverage>,
    pub unconfigured_task_ids: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct IntegratedTaskCoverage {
    pub task_id: String,
    pub title: String,
    pub command: Option<VerificationCommand>,
    pub source_fingerprint: String,
}

#[derive(Clone, Debug)]
pub struct IntegratedCommandOutcome {
    pub command: VerificationCommand,
    pub task_ids: Vec<String>,
    pub result: Option<VerificationResult>,
    pub not_run_reason: Option<String>,
}

#[derive(Clone, Debug)]
pub struct IntegratedVerificationResult {
    pub run_id: String,
    pub status: IntegratedVerificationStatus,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub repository_path: String,
    pub base_ref: String,
    pub base_oid: String,
    pub preview_tree_oid: String,
    pub ordered_task_ids: Vec<String>,
    pub tasks: Vec<IntegratedTaskCoverage>,
    pub commands: Vec<IntegratedCommandOutcome>,
    pub unconfigured_task_ids: Vec<String>,
    pub stale_reason: Option<String>,
}

struct TaskInput {
    id: String,
    title: String,
    repository_path: String,
    worktree_path: String,
    base_commit: Option<String>,
    dependencies: Vec<TaskDependency>,
    command: Option<VerificationCommand>,
    source_fingerprint: String,
}

#[derive(Clone)]
struct VerificationInputs {
    repository_path: String,
    base_ref: String,
    base_oid: String,
    preview_tree_oid: String,
    preview_path: PathBuf,
    object_repository_path: PathBuf,
    tasks: Vec<TaskInput>,
}

impl Clone for TaskInput {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            title: self.title.clone(),
            repository_path: self.repository_path.clone(),
            worktree_path: self.worktree_path.clone(),
            base_commit: self.base_commit.clone(),
            dependencies: self.dependencies.clone(),
            command: self.command.clone(),
            source_fingerprint: self.source_fingerprint.clone(),
        }
    }
}

struct PendingIntegratedVerification {
    cancel_requested: Arc<AtomicBool>,
    receiver: mpsc::Receiver<IntegratedVerificationResult>,
    worker: std::thread::JoinHandle<()>,
    _workspace: Arc<PreviewWorkspace>,
}

#[derive(Default)]
pub struct TaskIntegratedVerificationController {
    result: Option<IntegratedVerificationResult>,
    inputs: Option<VerificationInputs>,
    run: Option<PendingIntegratedVerification>,
    status_before_freshness_unavailable: Option<(IntegratedVerificationStatus, Option<String>)>,
}

impl TaskIntegratedVerificationController {
    pub fn result(&self) -> Option<&IntegratedVerificationResult> {
        self.result.as_ref()
    }

    pub fn is_running(&self) -> bool {
        self.run.is_some()
    }

    pub fn cancel(&self) {
        if let Some(run) = &self.run {
            run.cancel_requested.store(true, Ordering::Relaxed);
        }
    }

    pub fn shutdown(&mut self) {
        if let Some(run) = self.run.take() {
            run.cancel_requested.store(true, Ordering::Relaxed);
            let _ = run.worker.join();
        }
    }

    pub fn start(
        &mut self,
        ctx: &egui::Context,
        repository_path: String,
        base_ref: String,
        base_oid: String,
        tasks: &[TaskRecord],
        source_fingerprints: &HashMap<String, String>,
        workspace: Arc<PreviewWorkspace>,
    ) -> Result<(), String> {
        if self.is_running() {
            return Err("Integrated verification is already running".into());
        }
        if tasks.is_empty() {
            return Err("Select at least one task before running integrated verification".into());
        }
        let coverage = resolve_command_coverage(tasks)?;
        let inputs = VerificationInputs {
            repository_path,
            base_ref,
            base_oid,
            preview_tree_oid: workspace.tree_oid.clone(),
            preview_path: workspace.path.clone(),
            object_repository_path: workspace.object_repository_path.clone(),
            tasks: tasks
                .iter()
                .map(|task| {
                    let source_fingerprint = source_fingerprints.get(&task.id).cloned().ok_or_else(
                        || format!("{} has no current source fingerprint", task.title),
                    )?;
                    Ok(TaskInput {
                        id: task.id.clone(),
                        title: task.title.clone(),
                        repository_path: task.repository_path.clone(),
                        worktree_path: task.worktree_path.clone(),
                        base_commit: task.base_commit.clone(),
                        dependencies: task.dependencies.clone(),
                        command: task.verification_command.clone(),
                        source_fingerprint,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?,
        };
        let run_id = task_verification::next_run_id();
        let mut result = result_for_run(&inputs, &coverage, run_id);
        self.status_before_freshness_unavailable = None;
        self.inputs = Some(inputs.clone());
        if coverage.commands.is_empty() {
            result.status = IntegratedVerificationStatus::Unavailable;
            result.stale_reason = Some(
                "None of the selected tasks has a configured verification command.".into(),
            );
            finish(&mut result);
            self.result = Some(result);
            return Ok(());
        }
        result.status = IntegratedVerificationStatus::Running;
        let worker_result = result.clone();
        self.result = Some(result);

        let cancel_requested = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel_requested.clone();
        let worker_workspace = workspace.clone();
        let repaint = ctx.clone();
        let (sender, receiver) = mpsc::channel();
        let worker = match std::thread::Builder::new()
            .name("task-integrated-verification".into())
            .spawn(move || {
                let result = run_verification(
                    worker_result,
                    &inputs,
                    &coverage,
                    &worker_workspace,
                    worker_cancel,
                );
                let _ = sender.send(result);
                repaint.request_repaint();
            })
        {
            Ok(worker) => worker,
            Err(error) => {
                if let Some(result) = self.result.as_mut() {
                    result.status = IntegratedVerificationStatus::Error;
                    result.stale_reason = Some(format!(
                        "Could not start the integrated verification worker: {error}"
                    ));
                    finish(result);
                }
                return Ok(());
            }
        };
        self.run = Some(PendingIntegratedVerification {
            cancel_requested,
            receiver,
            worker,
            _workspace: workspace,
        });
        Ok(())
    }

    pub fn poll(&mut self) {
        let completion = self.run.as_ref().map(|run| run.receiver.try_recv());
        match completion {
            Some(Ok(result)) => {
                if let Some(run) = self.run.take() {
                    let _ = run.worker.join();
                }
                self.result = Some(result);
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                if let Some(run) = self.run.take() {
                    let _ = run.worker.join();
                }
                if let Some(result) = self.result.as_mut() {
                    result.status = IntegratedVerificationStatus::Error;
                    result.stale_reason = Some(
                        "The integrated verification worker stopped before returning a result."
                            .into(),
                    );
                    finish(result);
                }
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    pub fn refresh_current_state(
        &mut self,
        repository_path: &str,
        base_ref: &str,
        base_oid: Option<&str>,
        preview_tree_oid: Option<&str>,
        preview_is_current: Option<bool>,
        tasks: &[TaskRecord],
        source_fingerprints: &HashMap<String, String>,
    ) {
        if self.is_running() {
            return;
        }
        if preview_is_current.is_none() {
            return;
        }
        let (Some(result), Some(inputs)) = (self.result.as_mut(), self.inputs.as_ref()) else {
            return;
        };
        if result.status == IntegratedVerificationStatus::Running {
            return;
        }
        let stale_reason = if preview_is_current == Some(false) {
            Some("The combined preview is no longer current. Run it again.".to_string())
        } else if repository_path != inputs.repository_path
            || base_ref != inputs.base_ref
            || base_oid != Some(inputs.base_oid.as_str())
            || preview_tree_oid != Some(inputs.preview_tree_oid.as_str())
        {
            Some("The repository base or combined preview tree changed after verification.".into())
        } else if tasks.len() != inputs.tasks.len()
            || tasks
                .iter()
                .zip(&inputs.tasks)
                .any(|(task, input)| task.id != input.id)
        {
            Some("The selected task IDs or their dependency order changed after verification.".into())
        } else {
            let mut reason = None;
            for (task, input) in tasks.iter().zip(&inputs.tasks) {
                if task.repository_path != input.repository_path
                    || task.worktree_path != input.worktree_path
                    || task.base_commit != input.base_commit
                    || task.dependencies != input.dependencies
                    || task.verification_command != input.command
                {
                    reason = Some(format!(
                        "{} source or verification configuration changed after the run.",
                        input.title
                    ));
                    break;
                }
                match source_fingerprints.get(&input.id) {
                    Some(current) if current == &input.source_fingerprint => {}
                    Some(_) => {
                        reason = Some(format!(
                            "{} source changed after the run.",
                            input.title
                        ));
                        break;
                    }
                    None => {
                        self.status_before_freshness_unavailable
                            .get_or_insert((result.status, result.stale_reason.clone()));
                        result.status = IntegratedVerificationStatus::Unavailable;
                        result.stale_reason = Some(format!(
                            "{} source freshness is unavailable.",
                            input.title
                        ));
                        finish(result);
                        return;
                    }
                }
            }
            reason
        };
        if let Some(reason) = stale_reason {
            self.status_before_freshness_unavailable = None;
            result.status = IntegratedVerificationStatus::Stale;
            result.stale_reason = Some(reason);
        } else if let Some((status, stale_reason)) =
            self.status_before_freshness_unavailable.take()
        {
            result.status = status;
            result.stale_reason = stale_reason;
        }
    }
}

pub fn resolve_command_coverage(
    tasks: &[TaskRecord],
) -> Result<IntegratedVerificationCoverage, String> {
    let mut coverage = IntegratedVerificationCoverage::default();
    for task in tasks {
        let Some(command) = task.verification_command.as_ref() else {
            coverage.unconfigured_task_ids.push(task.id.clone());
            continue;
        };
        command.validate().map_err(|error| {
            format!("{} has an invalid verification command: {error}", task.title)
        })?;
        if let Some(existing) = coverage
            .commands
            .iter_mut()
            .find(|existing| existing.command == *command)
        {
            existing.task_ids.push(task.id.clone());
        } else {
            coverage.commands.push(IntegratedCommandCoverage {
                command: command.clone(),
                task_ids: vec![task.id.clone()],
            });
        }
    }
    Ok(coverage)
}

fn result_for_run(
    inputs: &VerificationInputs,
    coverage: &IntegratedVerificationCoverage,
    run_id: String,
) -> IntegratedVerificationResult {
    IntegratedVerificationResult {
        run_id,
        status: IntegratedVerificationStatus::Running,
        started_at: now(),
        finished_at: None,
        repository_path: inputs.repository_path.clone(),
        base_ref: inputs.base_ref.clone(),
        base_oid: inputs.base_oid.clone(),
        preview_tree_oid: inputs.preview_tree_oid.clone(),
        ordered_task_ids: inputs.tasks.iter().map(|task| task.id.clone()).collect(),
        tasks: inputs
            .tasks
            .iter()
            .map(|task| IntegratedTaskCoverage {
                task_id: task.id.clone(),
                title: task.title.clone(),
                command: task.command.clone(),
                source_fingerprint: task.source_fingerprint.clone(),
            })
            .collect(),
        commands: coverage
            .commands
            .iter()
            .map(|entry| IntegratedCommandOutcome {
                command: entry.command.clone(),
                task_ids: entry.task_ids.clone(),
                result: None,
                not_run_reason: None,
            })
            .collect(),
        unconfigured_task_ids: coverage.unconfigured_task_ids.clone(),
        stale_reason: None,
    }
}

fn run_verification(
    mut result: IntegratedVerificationResult,
    inputs: &VerificationInputs,
    coverage: &IntegratedVerificationCoverage,
    workspace: &PreviewWorkspace,
    cancel_requested: Arc<AtomicBool>,
) -> IntegratedVerificationResult {
    let mut cancelled_before_all_commands_finished = false;
    let mut command_preview_unavailable = None;
    if cancel_requested.load(Ordering::Relaxed) {
        result.status = IntegratedVerificationStatus::Cancelled;
        result.stale_reason = Some("Cancelled before configured commands started.".into());
        finish(&mut result);
        return result;
    }
    if let Err(failure) = check_inputs(inputs, Some(cancel_requested.as_ref())) {
        apply_input_failure(&mut result, failure);
        finish(&mut result);
        return result;
    }
    for index in 0..coverage.commands.len() {
        if cancel_requested.load(Ordering::Relaxed) {
            cancelled_before_all_commands_finished = true;
            mark_not_run(&mut result, index, "Cancellation was requested.");
            break;
        }
        if let Err(failure) = check_inputs(inputs, Some(cancel_requested.as_ref())) {
            if failure.status == IntegratedVerificationStatus::Cancelled {
                cancelled_before_all_commands_finished = true;
                mark_not_run(&mut result, index, "Cancellation was requested.");
            } else {
                apply_input_failure(&mut result, failure);
                mark_not_run(
                    &mut result,
                    index,
                    "The verification inputs changed or became unavailable.",
                );
            }
            break;
        }
        let planned = &coverage.commands[index];
        let command_workspace = match workspace.materialize_for_command(cancel_requested.as_ref()) {
            Ok(command_workspace) => command_workspace,
            Err(error) => {
                if cancel_requested.load(Ordering::Relaxed) {
                    cancelled_before_all_commands_finished = true;
                    mark_not_run(&mut result, index, "Cancellation was requested.");
                    break;
                }
                command_preview_unavailable = Some(format!(
                    "Could not prepare an isolated combined preview for {}: {error}",
                    planned.command.executable
                ));
                result.commands[index].not_run_reason = command_preview_unavailable.clone();
                mark_not_run(
                    &mut result,
                    index + 1,
                    "An earlier command preview could not be prepared.",
                );
                break;
            }
        };
        if cancel_requested.load(Ordering::Relaxed) {
            cancelled_before_all_commands_finished = true;
            mark_not_run(&mut result, index, "Cancellation was requested.");
            break;
        }
        let command_result = task_verification::execute_in_directory(
            &command_workspace.path,
            format!("{}-{:02}", result.run_id, index + 1),
            planned.command.clone(),
            cancel_requested.clone(),
        );
        let command_state = command_result.state;
        result.commands[index].result = Some(command_result);
        match command_state {
            VerificationState::Cancelled => {
                mark_not_run(&mut result, index + 1, "An earlier command was cancelled.");
                break;
            }
            VerificationState::Error => {
                mark_not_run(
                    &mut result,
                    index + 1,
                    "An earlier command could not be completed safely.",
                );
                break;
            }
            _ => {}
        }
    }
    let input_failure_already_detected = matches!(
        result.status,
        IntegratedVerificationStatus::Stale | IntegratedVerificationStatus::Unavailable
    );
    let command_error = result.commands.iter().any(|outcome| {
        outcome
            .result
            .as_ref()
            .is_some_and(|command| command.state == VerificationState::Error)
    });
    let final_input_check = check_inputs(inputs, Some(cancel_requested.as_ref()));
    let cancellation_requested =
        cancelled_before_all_commands_finished || cancel_requested.load(Ordering::Relaxed);
    match final_input_check {
        Err(failure) if failure.status == IntegratedVerificationStatus::Cancelled => {
            if !input_failure_already_detected {
                if command_error {
                    result.status = IntegratedVerificationStatus::Error;
                } else {
                    apply_input_failure(&mut result, failure);
                }
            }
        }
        Err(failure) => apply_input_failure(&mut result, failure),
        Ok(()) if input_failure_already_detected => {}
        Ok(()) if cancellation_requested => {
            if command_error {
                result.status = IntegratedVerificationStatus::Error;
            } else {
                result.status = IntegratedVerificationStatus::Cancelled;
            }
        }
        Ok(()) if command_preview_unavailable.is_some() => {
            result.status = IntegratedVerificationStatus::Unavailable;
            result.stale_reason = command_preview_unavailable;
        }
        Ok(()) => result.status = aggregate_status(&result, coverage),
    }
    finish(&mut result);
    result
}

fn mark_not_run(result: &mut IntegratedVerificationResult, from: usize, reason: &str) {
    for outcome in result.commands.iter_mut().skip(from) {
        if outcome.result.is_none() {
            outcome.not_run_reason = Some(reason.into());
        }
    }
}

fn aggregate_status(
    result: &IntegratedVerificationResult,
    coverage: &IntegratedVerificationCoverage,
) -> IntegratedVerificationStatus {
    let states = result
        .commands
        .iter()
        .filter_map(|outcome| outcome.result.as_ref().map(|result| result.state))
        .collect::<Vec<_>>();
    if states.contains(&VerificationState::Cancelled) {
        return IntegratedVerificationStatus::Cancelled;
    }
    if states.contains(&VerificationState::Error) {
        return IntegratedVerificationStatus::Error;
    }
    if states.contains(&VerificationState::TimedOut) {
        return IntegratedVerificationStatus::TimedOut;
    }
    if states.contains(&VerificationState::Failed) {
        return IntegratedVerificationStatus::Failed;
    }
    if states.iter().any(|state| *state != VerificationState::Passed) {
        return IntegratedVerificationStatus::Error;
    }
    if coverage.unconfigured_task_ids.is_empty()
        && coverage.commands.len() == result.commands.len()
        && result.commands.iter().all(|outcome| {
            outcome
                .result
                .as_ref()
                .is_some_and(|result| result.state == VerificationState::Passed)
        })
    {
        IntegratedVerificationStatus::Passed
    } else {
        IntegratedVerificationStatus::Partial
    }
}

fn check_inputs(
    inputs: &VerificationInputs,
    cancel_requested: Option<&AtomicBool>,
) -> Result<(), InputCheckFailure> {
    let preview_path = fs::canonicalize(&inputs.preview_path).map_err(|error| {
        InputCheckFailure::unavailable(format!("The combined preview tree is unavailable: {error}"))
    })?;
    if preview_path != inputs.preview_path {
        return Err(InputCheckFailure::changed(
            "The combined preview tree path changed during verification.",
        ));
    }
    let object_repository =
        Repository::open_bare(&inputs.object_repository_path).map_err(|error| {
            InputCheckFailure::unavailable(format!(
                "Could not recheck the combined preview tree: {error}"
            ))
        })?;
    let tree_oid = git2::Oid::from_str(&inputs.preview_tree_oid).map_err(|error| {
        InputCheckFailure::unavailable(format!("The combined preview tree ID is invalid: {error}"))
    })?;
    let tree = object_repository.find_tree(tree_oid).map_err(|error| {
        InputCheckFailure::unavailable(format!("The combined preview tree is unavailable: {error}"))
    })?;
    if tree.id().to_string() != inputs.preview_tree_oid {
        return Err(InputCheckFailure::changed(
            "The combined preview tree identity changed during verification.",
        ));
    }

    let repository = Repository::open(&inputs.repository_path).map_err(|error| {
        InputCheckFailure::unavailable(format!("Could not recheck the preview base: {error}"))
    })?;
    let current_base = repository
        .find_branch(&inputs.base_ref, BranchType::Local)
        .and_then(|branch| branch.get().peel_to_commit())
        .map(|commit| commit.id().to_string())
        .map_err(|error| {
            InputCheckFailure::unavailable(format!(
                "Could not recheck base {}: {error}",
                inputs.base_ref
            ))
        })?;
    if current_base != inputs.base_oid {
        return Err(InputCheckFailure::changed(
            "The selected base branch moved during verification.",
        ));
    }

    let registry = TaskRegistry::load();
    if let Some(error) = registry.load_error() {
        return Err(InputCheckFailure::unavailable(format!(
            "Could not recheck task command configuration: {error}"
        )));
    }
    let selected_ids = inputs
        .tasks
        .iter()
        .map(|task| task.id.as_str())
        .collect::<HashSet<_>>();
    let selected_tasks = registry
        .entries()
        .iter()
        .filter(|task| selected_ids.contains(task.id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if selected_tasks.len() != inputs.tasks.len() {
        return Err(InputCheckFailure::unavailable(
            "One or more selected tasks are no longer linked to the task registry.",
        ));
    }
    let (current_order, unavailable) = crate::tasks::dependency_order(&selected_tasks);
    if !unavailable.is_empty() {
        return Err(InputCheckFailure::unavailable(
            "The selected tasks no longer have a complete dependency order.",
        ));
    }
    let expected_order = inputs
        .tasks
        .iter()
        .map(|task| task.id.clone())
        .collect::<Vec<_>>();
    if current_order != expected_order {
        return Err(InputCheckFailure::changed(
            "The selected task dependency order changed during verification.",
        ));
    }
    for input in &inputs.tasks {
        let task = registry
            .entries()
            .iter()
            .find(|task| task.id == input.id)
            .ok_or_else(|| {
                InputCheckFailure::unavailable(format!(
                    "{} is no longer linked to the task registry.",
                    input.title
                ))
            })?;
        if task.repository_path != input.repository_path
            || task.worktree_path != input.worktree_path
            || task.base_commit != input.base_commit
            || task.dependencies != input.dependencies
        {
            return Err(InputCheckFailure::changed(format!(
                "{} task source metadata changed during verification.",
                input.title
            )));
        }
        if task.verification_command != input.command {
            return Err(InputCheckFailure::changed(format!(
                "{} verification command configuration changed during the run.",
                input.title
            )));
        }
        let fingerprint = match cancel_requested {
            Some(cancel_requested) => task_verification::source_fingerprint_with_cancel(
                Path::new(&input.worktree_path),
                Path::new(&input.repository_path),
                cancel_requested,
            ),
            None => task_verification::source_fingerprint(
                Path::new(&input.worktree_path),
                Path::new(&input.repository_path),
            ),
        };
        let current_fingerprint = match fingerprint {
            Ok(fingerprint) => fingerprint,
            Err(error) if error == "Task verification was cancelled before the command started" => {
                return Err(InputCheckFailure::cancelled(
                    "Cancellation was requested while rechecking task sources.",
                ));
            }
            Err(error) => {
                return Err(InputCheckFailure::unavailable(format!(
                    "Could not recheck {} source: {error}",
                    input.title
                )));
            }
        };
        if current_fingerprint != input.source_fingerprint {
            return Err(InputCheckFailure::changed(format!(
                "{} source changed during verification.",
                input.title
            )));
        }
    }
    Ok(())
}

struct InputCheckFailure {
    status: IntegratedVerificationStatus,
    reason: String,
}

impl InputCheckFailure {
    fn changed(reason: impl Into<String>) -> Self {
        Self {
            status: IntegratedVerificationStatus::Stale,
            reason: reason.into(),
        }
    }

    fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            status: IntegratedVerificationStatus::Unavailable,
            reason: reason.into(),
        }
    }

    fn cancelled(reason: impl Into<String>) -> Self {
        Self {
            status: IntegratedVerificationStatus::Cancelled,
            reason: reason.into(),
        }
    }
}

fn apply_input_failure(result: &mut IntegratedVerificationResult, failure: InputCheckFailure) {
    result.status = failure.status;
    result.stale_reason = Some(failure.reason);
}

fn finish(result: &mut IntegratedVerificationResult) {
    result.finished_at = Some(now());
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
