use chrono::{SecondsFormat, Utc};
use git2::{ErrorCode, Repository};
use ring::digest::{Context, SHA256};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT_SECONDS: u64 = 30 * 60;
const MAX_OUTPUT_PER_STREAM: usize = 16 * 1024;
const OUTPUT_DRAIN_WAIT: Duration = Duration::from_secs(2);
const FINGERPRINT_FORMAT: &[u8] = b"git-manager-task-source-v2";
static NEXT_RUN_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationCommand {
    pub executable: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u64,
}

impl VerificationCommand {
    pub fn validate(&self) -> Result<(), String> {
        if self.executable.trim().is_empty() {
            return Err("Enter an executable for the verification command".into());
        }
        if self.executable.contains('\0') || self.args.iter().any(|arg| arg.contains('\0')) {
            return Err("The executable and arguments cannot contain a NUL character".into());
        }
        if !(1..=24 * 60 * 60).contains(&self.timeout_seconds) {
            return Err("Set the timeout between 1 second and 24 hours".into());
        }
        Ok(())
    }
}

fn default_timeout_seconds() -> u64 {
    DEFAULT_TIMEOUT_SECONDS
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationState {
    Running,
    Passed,
    Failed,
    Cancelled,
    TimedOut,
    Error,
}

impl VerificationState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Passed => "Passed",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
            Self::TimedOut => "Timed out",
            Self::Error => "Could not run",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationResult {
    pub run_id: String,
    pub command: VerificationCommand,
    pub state: VerificationState,
    pub started_at: String,
    #[serde(default)]
    pub finished_at: Option<String>,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub source_fingerprint: Option<String>,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    #[serde(default)]
    pub output_truncated: bool,
}

impl VerificationResult {
    pub fn running(run_id: String, command: VerificationCommand) -> Self {
        Self {
            run_id,
            command,
            state: VerificationState::Running,
            started_at: now(),
            finished_at: None,
            exit_code: None,
            source_fingerprint: None,
            stdout: String::new(),
            stderr: String::new(),
            output_truncated: false,
        }
    }

    fn finish(&mut self, state: VerificationState) {
        self.state = state;
        self.finished_at = Some(now());
    }
}

pub fn next_run_id() -> String {
    let sequence = NEXT_RUN_ID.fetch_add(1, Ordering::Relaxed);
    let nanos = Utc::now().timestamp_nanos_opt().unwrap_or_default();
    format!("verification-{nanos:x}-{:x}-{:x}", std::process::id(), sequence)
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Fingerprint the Git version and a recursive snapshot of the task worktree.
/// The filesystem walk includes ignored and empty directories that Git status omits.
pub fn source_fingerprint(
    worktree_path: &Path,
    expected_repository_path: &Path,
) -> Result<String, String> {
    source_fingerprint_inner(worktree_path, expected_repository_path, None)
}

pub fn source_fingerprint_with_cancel(
    worktree_path: &Path,
    expected_repository_path: &Path,
    cancel_requested: &AtomicBool,
) -> Result<String, String> {
    source_fingerprint_inner(
        worktree_path,
        expected_repository_path,
        Some(cancel_requested),
    )
}

fn source_fingerprint_inner(
    worktree_path: &Path,
    expected_repository_path: &Path,
    cancel_requested: Option<&AtomicBool>,
) -> Result<String, String> {
    check_cancelled(cancel_requested)?;
    let repository = Repository::open(worktree_path)
        .map_err(|error| format!("Could not open the task worktree: {error}"))?;
    let workdir = repository
        .workdir()
        .ok_or_else(|| "The task repository has no working directory".to_string())?;
    let workdir = fs::canonicalize(workdir)
        .map_err(|error| format!("Could not resolve the task worktree: {error}"))?;
    let requested_path = fs::canonicalize(worktree_path)
        .map_err(|error| format!("Could not resolve the task worktree path: {error}"))?;
    if requested_path != workdir {
        return Err("The task path no longer points to its own Git worktree".into());
    }
    let repository_root = crate::tasks::repository_root(&repository, &workdir)?;
    let expected_repository_path = fs::canonicalize(expected_repository_path)
        .map_err(|error| format!("Could not resolve the task's saved repository path: {error}"))?;
    if repository_root != expected_repository_path {
        return Err("The task worktree is no longer linked to its saved repository".into());
    }

    let mut context = Context::new(&SHA256);
    context.update(FINGERPRINT_FORMAT);

    let head = repository
        .find_reference("HEAD")
        .map_err(|error| format!("Could not read task HEAD: {error}"))?;
    let head_name = head.symbolic_target().or_else(|| head.name()).unwrap_or("HEAD");
    update_field(&mut context, b"head-name", head_name.as_bytes());
    match head.resolve() {
        Ok(resolved) => {
            let target = resolved
                .target()
                .ok_or_else(|| "Task HEAD does not resolve to a commit".to_string())?;
            update_field(&mut context, b"head-commit", target.as_bytes());
        }
        Err(error) if error.code() == ErrorCode::UnbornBranch => {
            update_field(&mut context, b"head-commit", b"unborn");
        }
        Err(error) => return Err(format!("Could not resolve task HEAD: {error}")),
    }

    let index = repository
        .index()
        .map_err(|error| format!("Could not read the task index: {error}"))?;
    for entry in index.iter() {
        update_field(&mut context, b"index-path", &entry.path);
        update_field(&mut context, b"index-object", entry.id.as_bytes());
        update_field(&mut context, b"index-mode", &entry.mode.to_be_bytes());
        update_field(&mut context, b"index-flags", &entry.flags.to_be_bytes());
        update_field(
            &mut context,
            b"index-extended-flags",
            &entry.flags_extended.to_be_bytes(),
        );
    }
    fingerprint_path(
        &mut context,
        &workdir,
        Path::new("."),
        cancel_requested,
    )?;

    let digest = context.finish();
    Ok(digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn update_field(context: &mut Context, label: &[u8], value: &[u8]) {
    context.update(&(label.len() as u64).to_be_bytes());
    context.update(label);
    context.update(&(value.len() as u64).to_be_bytes());
    context.update(value);
}

fn fingerprint_path(
    context: &mut Context,
    root: &Path,
    relative_path: &Path,
    cancel_requested: Option<&AtomicBool>,
) -> Result<(), String> {
    check_cancelled(cancel_requested)?;
    reject_symlink_parents(root, relative_path)?;
    let path = root.join(relative_path);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            update_field(context, b"worktree-kind", b"missing");
            return Ok(());
        }
        Err(error) => {
            return Err(format!("Could not inspect {}: {error}", relative_path.display()))
        }
    };

    if metadata.file_type().is_symlink() {
        update_field(context, b"worktree-kind", b"symlink");
        let before_modified = metadata.modified().ok();
        let target = fs::read_link(&path)
            .map_err(|error| format!("Could not read symlink {}: {error}", relative_path.display()))?;
        update_field(context, b"symlink-target", &os_str_bytes(target.as_os_str()));
        let after = fs::symlink_metadata(&path)
            .map_err(|error| format!("Could not recheck {}: {error}", relative_path.display()))?;
        let after_target = fs::read_link(&path)
            .map_err(|error| format!("Could not recheck symlink {}: {error}", relative_path.display()))?;
        if after.modified().ok() != before_modified || after_target != target {
            return Err(format!(
                "{} changed while its source fingerprint was being calculated",
                relative_path.display()
            ));
        }
    } else if metadata.is_file() {
        update_field(context, b"worktree-kind", b"file");
        update_field(context, b"worktree-mode", &file_mode(&metadata).to_be_bytes());
        update_field(context, b"worktree-size", &metadata.len().to_be_bytes());
        let before_modified = metadata.modified().ok();
        let mut file = File::open(&path)
            .map_err(|error| format!("Could not read {}: {error}", relative_path.display()))?;
        let mut buffer = [0_u8; 16 * 1024];
        loop {
            check_cancelled(cancel_requested)?;
            let count = file
                .read(&mut buffer)
                .map_err(|error| format!("Could not read {}: {error}", relative_path.display()))?;
            if count == 0 {
                break;
            }
            context.update(&buffer[..count]);
        }
        let after = fs::symlink_metadata(&path)
            .map_err(|error| format!("Could not recheck {}: {error}", relative_path.display()))?;
        if after.len() != metadata.len()
            || after.modified().ok() != before_modified
            || file_mode(&after) != file_mode(&metadata)
        {
            return Err(format!(
                "{} changed while its source fingerprint was being calculated",
                relative_path.display()
            ));
        }
        if relative_path.file_name() == Some(std::ffi::OsStr::new(".git"))
            && relative_path.parent() != Some(Path::new("."))
        {
            if let Some(repository_path) = path.parent() {
                fingerprint_nested_repository(context, repository_path)?;
            }
        }
    } else if metadata.is_dir() {
        if relative_path.file_name() == Some(std::ffi::OsStr::new(".git"))
            && relative_path.parent() != Some(Path::new("."))
        {
            if let Some(repository_path) = path.parent() {
                if fingerprint_nested_repository(context, repository_path)? {
                    update_field(context, b"worktree-kind", b"nested-git-directory");
                    update_field(context, b"worktree-mode", &file_mode(&metadata).to_be_bytes());
                    return Ok(());
                }
            }
        }
        update_field(context, b"worktree-kind", b"directory");
        update_field(context, b"worktree-mode", &file_mode(&metadata).to_be_bytes());
        let before_modified = metadata.modified().ok();
        let mut children = fs::read_dir(&path)
            .map_err(|error| format!("Could not read {}: {error}", relative_path.display()))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<io::Result<Vec<_>>>()
            .map_err(|error| format!("Could not read {}: {error}", relative_path.display()))?;
        children.retain(|child| {
            relative_path != Path::new(".")
                || child.file_name().and_then(|name| name.to_str()) != Some(".git")
        });
        children.sort();
        for child in &children {
            let child_name = child
                .file_name()
                .ok_or_else(|| format!("Could not get a child name in {}", path.display()))?;
            let child_relative = relative_path.join(child_name);
            update_field(
                context,
                b"directory-child",
                &os_str_bytes(child_relative.as_os_str()),
            );
            fingerprint_path(context, root, &child_relative, cancel_requested)?;
        }
        let mut after_children = fs::read_dir(&path)
            .map_err(|error| format!("Could not recheck {}: {error}", relative_path.display()))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<io::Result<Vec<_>>>()
            .map_err(|error| format!("Could not recheck {}: {error}", relative_path.display()))?;
        after_children.retain(|child| {
            relative_path != Path::new(".")
                || child.file_name().and_then(|name| name.to_str()) != Some(".git")
        });
        after_children.sort();
        let after = fs::symlink_metadata(&path)
            .map_err(|error| format!("Could not recheck {}: {error}", relative_path.display()))?;
        if after_children != children
            || after.len() != metadata.len()
            || after.modified().ok() != before_modified
            || file_mode(&after) != file_mode(&metadata)
        {
            return Err(format!(
                "{} changed while its source fingerprint was being calculated",
                relative_path.display()
            ));
        }
    } else {
        return Err(format!(
            "Cannot fingerprint non-regular task path {}",
            relative_path.display()
        ));
    }
    Ok(())
}

fn fingerprint_nested_repository(context: &mut Context, path: &Path) -> Result<bool, String> {
    let repository = match Repository::open(path) {
        Ok(repository) => repository,
        Err(_) => return Ok(false),
    };
    let Some(workdir) = repository.workdir() else {
        return Ok(false);
    };
    let workdir = fs::canonicalize(workdir)
        .map_err(|error| format!("Could not resolve nested repository {}: {error}", path.display()))?;
    let expected_path = fs::canonicalize(path)
        .map_err(|error| format!("Could not resolve nested repository {}: {error}", path.display()))?;
    if workdir != expected_path {
        return Ok(false);
    }

    let head = repository
        .find_reference("HEAD")
        .map_err(|error| format!("Could not read nested repository HEAD: {error}"))?;
    let head_name = head.symbolic_target().or_else(|| head.name()).unwrap_or("HEAD");
    update_field(context, b"nested-head-name", head_name.as_bytes());
    match head.resolve() {
        Ok(resolved) => {
            let target = resolved
                .target()
                .ok_or_else(|| "Nested repository HEAD does not resolve to a commit".to_string())?;
            update_field(context, b"nested-head-commit", target.as_bytes());
        }
        Err(error) if error.code() == ErrorCode::UnbornBranch => {
            update_field(context, b"nested-head-commit", b"unborn");
        }
        Err(error) => return Err(format!("Could not resolve nested repository HEAD: {error}")),
    }

    let index = repository
        .index()
        .map_err(|error| format!("Could not read nested repository index: {error}"))?;
    for entry in index.iter() {
        update_field(context, b"nested-index-path", &entry.path);
        update_field(context, b"nested-index-object", entry.id.as_bytes());
        update_field(context, b"nested-index-mode", &entry.mode.to_be_bytes());
        update_field(context, b"nested-index-flags", &entry.flags.to_be_bytes());
        update_field(
            context,
            b"nested-index-extended-flags",
            &entry.flags_extended.to_be_bytes(),
        );
    }
    Ok(true)
}

fn check_cancelled(cancel_requested: Option<&AtomicBool>) -> Result<(), String> {
    if cancel_requested.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
        Err("Task verification was cancelled before the command started".into())
    } else {
        Ok(())
    }
}

fn reject_symlink_parents(root: &Path, relative_path: &Path) -> Result<(), String> {
    let components = relative_path.components().collect::<Vec<_>>();
    let mut parent = root.to_path_buf();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        parent.push(component.as_os_str());
        match fs::symlink_metadata(&parent) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "Cannot fingerprint {} through a symlinked parent",
                    relative_path.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(format!("Could not inspect {}: {error}", parent.display())),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn file_mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode()
}

#[cfg(not(unix))]
fn file_mode(metadata: &fs::Metadata) -> u32 {
    u32::from(metadata.permissions().readonly())
}

#[cfg(unix)]
fn os_str_bytes(value: &std::ffi::OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    value.as_bytes().to_vec()
}

#[cfg(windows)]
fn os_str_bytes(value: &std::ffi::OsStr) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().flat_map(u16::to_be_bytes).collect()
}

#[cfg(not(any(unix, windows)))]
fn os_str_bytes(value: &std::ffi::OsStr) -> Vec<u8> {
    value.to_string_lossy().as_bytes().to_vec()
}

pub fn run_and_record(
    task_id: &str,
    worktree_path: PathBuf,
    repository_path: PathBuf,
    run_id: String,
    command: VerificationCommand,
    cancel_requested: Arc<AtomicBool>,
) -> Result<(), String> {
    let mut result = VerificationResult::running(run_id.clone(), command.clone());
    match source_fingerprint_inner(
        &worktree_path,
        &repository_path,
        Some(&cancel_requested),
    ) {
        Ok(fingerprint) => {
            result.source_fingerprint = Some(fingerprint.clone());
            let mut registry = crate::tasks::TaskRegistry::load();
            registry
                .record_verification_fingerprint(task_id, &run_id, &fingerprint)
                .map_err(|error| format!("Could not bind verification to its source state: {error}"))?;
        }
        Err(error) if cancel_requested.load(Ordering::Relaxed) => {
            result.stderr = error;
            result.finish(VerificationState::Cancelled);
            return record_result(task_id, &run_id, result);
        }
        Err(error) => {
            result.stderr = format!("The verification command was not started because source freshness could not be established: {error}");
            result.finish(VerificationState::Error);
            return record_result(task_id, &run_id, result);
        }
    }

    if cancel_requested.load(Ordering::Relaxed) {
        result.finish(VerificationState::Cancelled);
        return record_result(task_id, &run_id, result);
    }

    result = execute_command(&worktree_path, result, cancel_requested);
    record_result(task_id, &run_id, result)
}

fn record_result(task_id: &str, run_id: &str, result: VerificationResult) -> Result<(), String> {
    let mut registry = crate::tasks::TaskRegistry::load();
    registry
        .record_verification_result(task_id, run_id, result)
        .map_err(|error| format!("Could not save the task verification result: {error}"))
}

/// Run a configured verification command in a caller-selected isolated directory.
/// This shares task verification's process-tree control, timeout, cancellation,
/// and bounded output capture without saving the result to an individual task.
pub fn execute_in_directory(
    working_directory: &Path,
    run_id: String,
    command: VerificationCommand,
    cancel_requested: Arc<AtomicBool>,
) -> VerificationResult {
    let result = VerificationResult::running(run_id, command);
    execute_command(working_directory, result, cancel_requested)
}

fn execute_command(
    worktree_path: &Path,
    mut result: VerificationResult,
    cancel_requested: Arc<AtomicBool>,
) -> VerificationResult {
    if let Err(error) = result.command.validate() {
        result.stderr = error;
        result.finish(VerificationState::Error);
        return result;
    }

    let mut command = Command::new(&result.command.executable);
    command
        .args(&result.command.args)
        .current_dir(worktree_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut process_tree = match ProcessTreeControl::prepare(&mut command) {
        Ok(process_tree) => process_tree,
        Err(error) => {
            result.stderr = format!("Could not prepare process-tree control: {error}");
            result.finish(VerificationState::Error);
            return result;
        }
    };
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            result.stderr = format!("Could not start the configured executable: {error}");
            result.finish(VerificationState::Error);
            return result;
        }
    };
    if let Err(error) = process_tree.attach(&child) {
        stop_direct_child(&mut child);
        result.stderr = format!("Could not attach the verification process to its process tree: {error}");
        result.finish(VerificationState::Error);
        return result;
    }

    let stdout = child.stdout.take().map(spawn_output_capture);
    let stderr = child.stderr.take().map(spawn_output_capture);
    let timeout = Duration::from_secs(result.command.timeout_seconds);
    let started = Instant::now();
    let mut state = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                result.exit_code = status.code();
                let command_state = if status.success() {
                    VerificationState::Passed
                } else {
                    VerificationState::Failed
                };
                #[cfg(unix)]
                if let Err(error) = process_tree.terminate(&mut child) {
                    result.stderr = format!(
                        "The verification command exited, but remaining processes could not be stopped: {error}"
                    );
                    break VerificationState::Error;
                }
                break command_state;
            }
            Ok(None) => {}
            Err(error) => {
                let stop_error = process_tree.terminate(&mut child).err();
                result.stderr = match stop_error {
                    Some(stop_error) => format!(
                        "Could not check the verification process: {error}. Could not stop its process tree: {stop_error}"
                    ),
                    None => format!("Could not check the verification process: {error}"),
                };
                break VerificationState::Error;
            }
        }

        if cancel_requested.load(Ordering::Relaxed) {
            match process_tree.terminate(&mut child) {
                Ok(()) => break VerificationState::Cancelled,
                Err(error) => {
                    result.stderr = format!(
                        "Cancellation was requested, but the verification process tree could not be stopped: {error}"
                    );
                    break VerificationState::Error;
                }
            }
        }
        if started.elapsed() >= timeout {
            match process_tree.terminate(&mut child) {
                Ok(()) => break VerificationState::TimedOut,
                Err(error) => {
                    result.stderr = format!(
                        "The verification timed out, but its process tree could not be stopped: {error}"
                    );
                    break VerificationState::Error;
                }
            }
        }
        thread::sleep(Duration::from_millis(50));
    };

    if let Some(stdout) = stdout {
        if let Some(captured) = collect_output(
            stdout,
            &mut child,
            &mut process_tree,
            &cancel_requested,
            &mut state,
            &mut result,
            "Standard output",
        ) {
            result.stdout = String::from_utf8_lossy(&captured.bytes).into_owned();
            result.output_truncated |= captured.truncated || captured.read_error;
        } else {
            result.output_truncated = true;
        }
    }
    if let Some(stderr) = stderr {
        if let Some(captured) = collect_output(
            stderr,
            &mut child,
            &mut process_tree,
            &cancel_requested,
            &mut state,
            &mut result,
            "Standard error",
        ) {
            if !result.stderr.is_empty() && !captured.bytes.is_empty() {
                result.stderr.push('\n');
            }
            result.stderr.push_str(&String::from_utf8_lossy(&captured.bytes));
            result.output_truncated |= captured.truncated || captured.read_error;
        } else {
            result.output_truncated = true;
        }
    }
    result.finish(state);
    result
}

fn collect_output(
    receiver: mpsc::Receiver<CapturedOutput>,
    child: &mut Child,
    process_tree: &mut ProcessTreeControl,
    cancel_requested: &AtomicBool,
    state: &mut VerificationState,
    result: &mut VerificationResult,
    stream_name: &str,
) -> Option<CapturedOutput> {
    let mut deadline = Instant::now() + OUTPUT_DRAIN_WAIT;
    let mut cleanup_attempted = matches!(
        *state,
        VerificationState::Cancelled | VerificationState::TimedOut
    );
    loop {
        if cancel_requested.load(Ordering::Relaxed)
            && !matches!(
                *state,
                VerificationState::Cancelled
                    | VerificationState::TimedOut
                    | VerificationState::Error
            )
        {
            if !cleanup_attempted {
                cleanup_attempted = true;
                match process_tree.terminate(child) {
                    Ok(()) => *state = VerificationState::Cancelled,
                    Err(error) => {
                        *state = VerificationState::Error;
                        result.stderr.push_str(&format!(
                            "\nCancellation was requested during {stream_name} capture, but the process tree could not be stopped: {error}"
                        ));
                    }
                }
                deadline = Instant::now() + OUTPUT_DRAIN_WAIT;
            } else {
                *state = VerificationState::Cancelled;
            }
        }

        if Instant::now() >= deadline {
            if !cleanup_attempted {
                cleanup_attempted = true;
                match process_tree.terminate(child) {
                    Ok(()) => result.stderr.push_str(&format!(
                        "\n{stream_name} capture did not close after the command exited; remaining processes were stopped."
                    )),
                    Err(error) => {
                        *state = VerificationState::Error;
                        result.stderr.push_str(&format!(
                            "\n{stream_name} capture did not close after the command exited, and the process tree could not be stopped: {error}"
                        ));
                    }
                }
                result.output_truncated = true;
                deadline = Instant::now() + OUTPUT_DRAIN_WAIT;
            } else {
                result.output_truncated = true;
                result.stderr.push_str(&format!(
                    "\n{stream_name} capture remained open after process cleanup."
                ));
                return None;
            }
        }

        let wait = deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(50));
        match receiver.recv_timeout(wait) {
            Ok(captured) => return Some(captured),
            Err(mpsc::RecvTimeoutError::Disconnected) => return None,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

struct ProcessTreeControl {
    #[cfg(unix)]
    process_group: Option<i32>,
    #[cfg(windows)]
    job: WindowsJob,
}

impl ProcessTreeControl {
    fn prepare(command: &mut Command) -> Result<Self, String> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
            return Ok(Self { process_group: None });
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;

            command.creation_flags(CREATE_SUSPENDED);
            let job = WindowsJob::new()?;
            return Ok(Self { job });
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = command;
            Ok(Self)
        }
    }

    fn attach(&mut self, child: &Child) -> Result<(), String> {
        #[cfg(unix)]
        {
            self.process_group = Some(
                i32::try_from(child.id())
                    .map_err(|_| "the process ID does not fit the process-group type".to_string())?,
            );
            Ok(())
        }
        #[cfg(windows)]
        {
            self.job.assign(child)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = child;
            Ok(())
        }
    }

    fn terminate(&mut self, child: &mut Child) -> Result<(), String> {
        #[cfg(unix)]
        {
            use std::os::raw::c_int;

            extern "C" {
                fn kill(pid: c_int, signal: c_int) -> c_int;
            }

            let process_group = self
                .process_group
                .ok_or_else(|| "the verification process group was not initialized".to_string())?;
            // The command is started in its own process group, so a negative PID targets its descendants too.
            let kill_result = unsafe { kill(-process_group, 9) };
            if kill_result != 0 {
                let error = io::Error::last_os_error();
                stop_direct_child(child);
                // ESRCH (3 on the supported Unix targets) means the group is already empty.
                if error.raw_os_error() == Some(3) {
                    return Ok(());
                }
                return Err(format!("could not signal process group {process_group}: {error}"));
            }
            child
                .wait()
                .map(|_| ())
                .map_err(|error| format!("could not reap the verification process: {error}"))
        }
        #[cfg(windows)]
        {
            self.job.terminate(child)
        }
        #[cfg(not(any(unix, windows)))]
        {
            child
                .kill()
                .map_err(|error| format!("could not stop the verification process: {error}"))?;
            child
                .wait()
                .map(|_| ())
                .map_err(|error| format!("could not reap the verification process: {error}"))
        }
    }
}

#[cfg(windows)]
struct WindowsJob {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl WindowsJob {
    fn new() -> Result<Self, String> {
        use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
        use windows_sys::Win32::System::JobObjects::{
            CreateJobObjectW, SetInformationJobObject, JobObjectExtendedLimitInformation,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };

        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(format!(
                "could not create a Windows process job: {}",
                windows_error(unsafe { GetLastError() })
            ));
        }
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let set_result = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if set_result == 0 {
            let error = windows_error(unsafe { GetLastError() });
            unsafe { CloseHandle(handle) };
            return Err(format!("could not configure the Windows process job: {error}"));
        }
        Ok(Self { handle })
    }

    fn assign(&self, child: &Child) -> Result<(), String> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::{GetLastError, HANDLE};
        use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;

        let process = child.as_raw_handle() as HANDLE;
        if unsafe { AssignProcessToJobObject(self.handle, process) } == 0 {
            return Err(format!(
                "could not assign the verification process to its Windows job: {}",
                windows_error(unsafe { GetLastError() })
            ));
        }
        resume_suspended_process(child.id())
    }

    fn terminate(&self, child: &mut Child) -> Result<(), String> {
        use windows_sys::Win32::Foundation::GetLastError;
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;

        if unsafe { TerminateJobObject(self.handle, 1) } == 0 {
            let error = windows_error(unsafe { GetLastError() });
            stop_direct_child(child);
            return Err(format!("could not terminate the Windows process job: {error}"));
        }
        child
            .wait()
            .map(|_| ())
            .map_err(|error| format!("could not reap the verification process: {error}"))
    }
}

#[cfg(windows)]
fn resume_suspended_process(process_id: u32) -> Result<(), String> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, THREADENTRY32, TH32CS_SNAPTHREAD,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(format!(
            "could not inspect the suspended verification process: {}",
            windows_error(unsafe { GetLastError() })
        ));
    }

    let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
    let mut thread_id = None;
    if unsafe { Thread32First(snapshot, &mut entry) } != 0 {
        loop {
            if entry.th32OwnerProcessID == process_id {
                thread_id = Some(entry.th32ThreadID);
                break;
            }
            if unsafe { Thread32Next(snapshot, &mut entry) } == 0 {
                break;
            }
        }
    }
    unsafe { CloseHandle(snapshot) };

    let thread_id = thread_id.ok_or_else(|| {
        "could not find the suspended verification process's primary thread".to_string()
    })?;
    let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, thread_id) };
    if thread.is_null() {
        return Err(format!(
            "could not open the suspended verification process's primary thread: {}",
            windows_error(unsafe { GetLastError() })
        ));
    }

    let previous_suspend_count = unsafe { ResumeThread(thread) };
    let resume_error = if previous_suspend_count == u32::MAX {
        Some(windows_error(unsafe { GetLastError() }))
    } else if previous_suspend_count != 1 {
        Some(io::Error::new(
            io::ErrorKind::Other,
            format!("unexpected thread suspend count {previous_suspend_count}"),
        ))
    } else {
        None
    };
    unsafe { CloseHandle(thread) };
    match resume_error {
        Some(error) => Err(format!(
            "could not resume the verification process after job assignment: {error}"
        )),
        None => Ok(()),
    }
}

#[cfg(windows)]
impl Drop for WindowsJob {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        unsafe { CloseHandle(self.handle) };
    }
}

#[cfg(windows)]
fn windows_error(code: u32) -> io::Error {
    io::Error::from_raw_os_error(code as i32)
}

fn stop_direct_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

struct CapturedOutput {
    bytes: Vec<u8>,
    truncated: bool,
    read_error: bool,
}

fn spawn_output_capture(stream: impl Read + Send + 'static) -> mpsc::Receiver<CapturedOutput> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(drain_output(stream, MAX_OUTPUT_PER_STREAM));
    });
    receiver
}

fn drain_output(mut stream: impl Read, limit: usize) -> CapturedOutput {
    let mut captured = Vec::with_capacity(limit.min(8 * 1024));
    let mut truncated = false;
    let mut read_error = false;
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                let remaining = limit.saturating_sub(captured.len());
                let keep = remaining.min(count);
                captured.extend_from_slice(&buffer[..keep]);
                truncated |= keep < count;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => {
                read_error = true;
                break;
            }
        }
    }
    CapturedOutput { bytes: captured, truncated, read_error }
}
