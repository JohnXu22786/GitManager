use crate::git_ops::*;
use crate::recent::{path_name, RecentRepos};
use crate::tasks::{TaskRecord, TaskRegistry, TaskVerificationRunLock};
use crate::task_delivery::{
    self, PullRequestAction, PullRequestActionMessage, PullRequestSnapshot, PullRequestStatusView,
};
use crate::task_verification::{
    self, VerificationCommand, VerificationResult, VerificationState,
};
use crate::updater::{self, UpdateState};
use eframe::egui;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

const ABOUT_BUTTON_LABEL: &str = "ℹ";
const APP_VERSION: &str = crate::version_info::VERSION;
const TASK_PR_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const TASK_FINGERPRINT_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const MAX_TASK_PR_REFRESHES: usize = 2;
const MAX_TASK_SOURCE_FINGERPRINT_PROBES: usize = 2;

fn cleanup_change_label(change: &WorktreeChange) -> String {
    let mut labels = Vec::new();
    if change.conflicted {
        labels.push("conflict");
    }
    if change.staged {
        labels.push("staged");
    }
    if change.unstaged {
        labels.push("modified");
    }
    if change.untracked {
        labels.push("untracked");
    }
    if change.ignored {
        labels.push("ignored");
    }
    labels.join(", ")
}

fn escaped_preview_path(path: &Path) -> String {
    format!("{:?}", path.to_string_lossy())
}

fn worktree_cleanup_preview_message(
    worktree: &WorktreeInfo,
    force: bool,
    branch_used_elsewhere: bool,
) -> String {
    let status = &worktree.status;
    let mut lines = vec![format!("Path: {}", escaped_preview_path(&worktree.path))];

    if status.directory_missing {
        lines.push("The worktree directory is already missing.".into());
    } else if status.change_path_count == 0 {
        lines.push("Working tree is clean; no staged, modified, untracked, or ignored paths were found.".into());
    } else {
        lines.push(format!(
            "Git reports {} affected path(s): {} staged, {} modified, {} untracked, {} ignored, {} conflicted.",
            status.change_path_count,
            status.staged_changes,
            status.unstaged_changes,
            status.untracked_paths,
            status.ignored_paths,
            status.conflicted_paths,
        ));
        if !status.change_paths.is_empty() {
            lines.push("Affected paths at preview time:".into());
            for change in &status.change_paths {
                lines.push(format!(
                    "  {}  {}",
                    cleanup_change_label(change),
                    escaped_preview_path(&change.path),
                ));
            }
            if status.omitted_path_count > 0 {
                lines.push(format!("  ... and {} more path(s)", status.omitted_path_count));
            }
        }
    }

    if let Some(branch) = worktree.branch.as_deref() {
        lines.push(format!("Branch: {}", branch));
        lines.push("The branch and its committed history remain after worktree removal.".into());
        if branch_used_elsewhere {
            lines.push("The branch is also checked out in another listed worktree.".into());
        }
    } else {
        lines.push(match status.merged_into_main {
            Some(true) => "Detached HEAD: this commit is already reachable from the main worktree.",
            Some(false) => "Detached HEAD: no branch ref protects commits unique to this worktree.",
            None => "Detached HEAD: merge status is unknown and no branch ref protects this commit.",
        }.into());
    }
    if !worktree.sha.is_empty() {
        lines.push(format!("HEAD commit: {}", &worktree.sha[..worktree.sha.len().min(12)]));
    }
    match status.merged_into_main {
        Some(true) => lines.push("Commits are merged into the main worktree HEAD.".into()),
        Some(false) => lines.push("Commits are not merged into the main worktree HEAD.".into()),
        None => lines.push("Merge status relative to the main worktree is unavailable.".into()),
    }
    if worktree.branch.is_none() {
        match status.merged_into_main {
            Some(true) => {}
            Some(false) => lines.push(if status.directory_missing {
                "No branch ref protects this HEAD; its commits may become unreachable when stale metadata is removed.".into()
            } else {
                "Create a branch before cleanup; otherwise unique commits may become unreachable.".into()
            }),
            None => lines.push(if status.directory_missing {
                "Merge status could not be verified; detached commits may become unreachable when stale metadata is removed.".into()
            } else {
                "Merge status could not be verified; protect detached commits with a branch before cleanup.".into()
            }),
        }
    }
    if status.directory_missing {
        lines.push("Remote tracking status is unavailable because the worktree directory is missing.".into());
    } else {
        match status.upstream.as_deref() {
            Some(upstream) => match (status.ahead, status.behind) {
                (Some(ahead), Some(behind)) => lines.push(format!(
                    "Tracking {}: {} ahead (not pushed), {} behind.",
                    upstream, ahead, behind
                )),
                _ => lines.push(format!("Tracking {}: ahead/behind status unavailable.", upstream)),
            },
            None => lines.push("No upstream is configured; remote commit status is unknown.".into()),
        }
    }
    if status.locked {
        lines.push(match status.lock_reason.as_deref() {
            Some(reason) if !reason.is_empty() => format!("Worktree is locked: {}", reason),
            _ => "Worktree is locked.".into(),
        });
    }

    if status.directory_missing {
        lines.push(if force {
            "Force Remove bypasses the worktree lock and removes its stale Git metadata.".into()
        } else {
            "Only stale Git worktree metadata is removed; any local branch ref is kept.".into()
        });
    } else if force {
        lines.push("Force Remove bypasses any worktree lock and deletes the worktree directory, including ignored and uncommitted files. Changes made after this preview are removed too.".into());
    } else {
        lines.push("Only this worktree directory and its Git metadata are removed; the branch ref is kept.".into());
    }
    lines.join("\n")
}

fn update_asset_download_path(
    download_dir: &Path,
    file_name: &str,
) -> Result<std::path::PathBuf, String> {
    let mut components = Path::new(file_name).components();
    if file_name.contains('/')
        || file_name.contains('\\')
        || !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || components.next().is_some()
    {
        return Err("Invalid update asset filename".to_string());
    }

    Ok(download_dir.join(file_name))
}

fn begin_update_request_if(
    state: &Mutex<UpdateState>,
    generation: &AtomicU64,
    allowed: impl FnOnce(&UpdateState) -> bool,
    next_state: UpdateState,
) -> Option<u64> {
    let mut current_state = state.lock().unwrap();
    if !allowed(&current_state) {
        return None;
    }

    let request_id = generation.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
    *current_state = next_state;
    Some(request_id)
}

fn commit_update_state_if_current(
    state: &Mutex<UpdateState>,
    generation: &AtomicU64,
    request_id: u64,
    active: impl FnOnce(&UpdateState) -> bool,
    next_state: UpdateState,
) -> bool {
    let mut current_state = state.lock().unwrap();
    if generation.load(Ordering::Acquire) != request_id || !active(&current_state) {
        return false;
    }

    *current_state = next_state;
    true
}

fn clone_credential(
    url: &str,
    username_from_url: Option<&str>,
    allowed_types: git2::CredentialType,
) -> Result<git2::Cred, git2::Error> {
    let config = git2::Config::open_default().ok();
    clone_credential_with_config(config.as_ref(), url, username_from_url, allowed_types)
}

fn clone_credential_with_config(
    config: Option<&git2::Config>,
    url: &str,
    username_from_url: Option<&str>,
    allowed_types: git2::CredentialType,
) -> Result<git2::Cred, git2::Error> {
    credential_from_config(config, url, username_from_url, allowed_types)
}

struct PendingConfirmation {
    title: String,
    message: String,
    confirm_label: String,
    description: String,
    operation: GitOperation,
    repo_generation: u64,
}


const WORKTREE_CLEANUP_PREVIEW_OPERATION: &str = "Preparing worktree cleanup preview";

struct PendingWorktreeCleanup {
    path: std::path::PathBuf,
    expected_git_link: Option<WorktreeFileIdentity>,
    directory_missing_at_request: bool,
    force_requested: bool,
    repo_generation: u64,
    ready: bool,
}

pub struct TaskVerificationCommandDraft {
    pub task_id: String,
    pub title: String,
    pub executable: String,
    pub arguments: String,
    pub timeout_minutes: String,
}

impl TaskVerificationCommandDraft {
    pub(crate) fn for_task(task: &TaskRecord) -> Self {
        let command = task.verification_command.as_ref();
        Self {
            task_id: task.id.clone(),
            title: task.title.clone(),
            executable: command.map(|command| command.executable.clone()).unwrap_or_default(),
            arguments: command
                .and_then(|command| serde_json::to_string_pretty(&command.args).ok())
                .unwrap_or_else(|| "[]".into()),
            timeout_minutes: command
                .map(|command| (command.timeout_seconds.saturating_add(59) / 60).to_string())
                .unwrap_or_else(|| "30".into()),
        }
    }
}

struct PendingTaskVerification {
    task_id: String,
    cancel_requested: Arc<AtomicBool>,
    receiver: mpsc::Receiver<Result<(), String>>,
    worker: std::thread::JoinHandle<()>,
    _run_lock: TaskVerificationRunLock,
}

struct TaskFingerprintProbe {
    result: Option<Result<String, String>>,
    checked_at: Option<Instant>,
    receiver: Option<mpsc::Receiver<Result<String, String>>>,
}

struct TaskPrSourceWatcher {
    _watcher: RecommendedWatcher,
    invalidated: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
}

struct TaskPrSourceFingerprintProbe {
    worktree_path: String,
    repository_path: String,
    result: Option<String>,
    receiver: Option<mpsc::Receiver<Result<String, String>>>,
    watcher: Option<TaskPrSourceWatcher>,
    watcher_receiver: Option<mpsc::Receiver<Result<TaskPrSourceWatcher, String>>>,
    watch_error: Option<String>,
}

impl TaskPrSourceFingerprintProbe {
    fn for_task(task: &TaskRecord) -> Self {
        Self {
            worktree_path: task.worktree_path.clone(),
            repository_path: task.repository_path.clone(),
            result: None,
            receiver: None,
            watcher: None,
            watcher_receiver: None,
            watch_error: None,
        }
    }
}

fn task_pr_source_watcher(
    worktree_path: &Path,
    expected_repository_path: &Path,
    ctx: &egui::Context,
) -> Result<TaskPrSourceWatcher, String> {
    let repository = git2::Repository::open(worktree_path)
        .map_err(|error| {
            format!("Could not open the task worktree to watch for source changes: {error}")
        })?;
    let worktree_path = std::fs::canonicalize(worktree_path)
        .map_err(|error| {
            format!("Could not resolve the task worktree to watch for source changes: {error}")
        })?;
    let repository_root = crate::tasks::repository_root(&repository, &worktree_path)?;
    let expected_repository_path = std::fs::canonicalize(expected_repository_path)
        .map_err(|error| {
            format!("Could not resolve the task's saved repository path: {error}")
        })?;
    if repository_root != expected_repository_path {
        return Err("The task worktree is no longer linked to its saved repository".into());
    }
    let worktree_git_dir = repository.path().to_path_buf();
    let common_git_dir = repository.commondir().to_path_buf();
    let invalidated = Arc::new(AtomicBool::new(true));
    let error = Arc::new(Mutex::new(None));
    let callback_invalidated = Arc::clone(&invalidated);
    let callback_error = Arc::clone(&error);
    let callback_context = ctx.clone();
    let mut watcher = RecommendedWatcher::new(
        move |event: notify::Result<notify::Event>| match event {
            Ok(_) => {
                if !callback_invalidated.swap(true, Ordering::AcqRel) {
                    callback_context.request_repaint();
                }
            }
            Err(error) => {
                *callback_error
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(error.to_string());
                callback_context.request_repaint();
            }
        },
        notify::Config::default(),
    )
    .map_err(|error| {
        format!("Could not watch the task worktree for source changes: {error}")
    })?;
    watcher
        .watch(&worktree_path, RecursiveMode::Recursive)
        .map_err(|error| {
            format!("Could not watch the task worktree for source changes: {error}")
        })?;
    if !worktree_git_dir.starts_with(&worktree_path) {
        watcher
            .watch(&worktree_git_dir, RecursiveMode::NonRecursive)
            .map_err(|error| {
                format!("Could not watch the task Git metadata for source changes: {error}")
            })?;
    }
    let refs_dir = common_git_dir.join("refs");
    if !refs_dir.starts_with(&worktree_path) && refs_dir.is_dir() {
        watcher
            .watch(&refs_dir, RecursiveMode::Recursive)
            .map_err(|error| {
                format!("Could not watch the repository references for source changes: {error}")
            })?;
    }
    if common_git_dir != worktree_git_dir && !common_git_dir.starts_with(&worktree_path) {
        watcher
            .watch(&common_git_dir, RecursiveMode::NonRecursive)
            .map_err(|error| {
                format!("Could not watch the repository Git metadata for source changes: {error}")
            })?;
    }
    Ok(TaskPrSourceWatcher {
        _watcher: watcher,
        invalidated,
        error,
    })
}

struct TaskReviewQueueProbe {
    result: Option<TaskReviewQueueState>,
    checked_at: Option<Instant>,
    receiver: Option<mpsc::Receiver<TaskReviewQueueState>>,
}

struct TaskPullRequestProbe {
    snapshot: Option<PullRequestSnapshot>,
    error: Option<String>,
    checked_at: Option<Instant>,
    receiver: Option<mpsc::Receiver<Result<PullRequestSnapshot, task_delivery::PullRequestActionError>>>,
    action: Option<PullRequestAction>,
    action_message: Option<PullRequestActionMessage>,
}

impl Default for TaskPullRequestProbe {
    fn default() -> Self {
        Self {
            snapshot: None,
            error: None,
            checked_at: None,
            receiver: None,
            action: None,
            action_message: None,
        }
    }
}


/// Tracks a Git operation running in a background thread.
struct PendingOp {
    description: String,
    receiver: mpsc::Receiver<OpResult>,
    repo_generation: u64,
    started_at: Instant,
    /// Real-time progress text updated by the background thread (e.g. "Receiving objects: 45%").
    progress: Arc<Mutex<String>>,
    /// Tracks the last time the progress text changed (watchdog timer).
    last_progress_update: Instant,
    /// The last progress value we read (to detect changes).
    last_seen_progress: String,
    /// Whether the watchdog timed out while the worker was still running.
    timed_out: bool,
    task_diff_identity: Option<(String, u64)>,
    form_submission: Option<FormSubmission>,
}

pub(crate) enum FormSubmission {
    Commit { message: String, amend: bool },
    CreateBranch { name: String, base: String },
    MergeBranch { name: String },
    RenameBranch { old: String, new: String },
    CreateWorktree {
        name: String,
        path: String,
        branch: String,
        create_branch: bool,
    },
    Stash { message: String },
}

impl FormSubmission {
    fn clear_if_unchanged(self, app: &mut App) {
        match self {
            Self::Commit { message, amend } => {
                if app.commit_msg == message {
                    app.commit_msg.clear();
                }
                if app.commit_amend == amend {
                    app.commit_amend = false;
                }
            }
            Self::CreateBranch { name, base } => {
                if app.new_branch_name == name {
                    app.new_branch_name.clear();
                }
                if app.new_branch_base == base {
                    app.new_branch_base.clear();
                }
            }
            Self::MergeBranch { name } => {
                if app.merge_branch_name == name {
                    app.merge_branch_name.clear();
                }
            }
            Self::RenameBranch { old, new } => {
                if app.rename_branch_old == old {
                    app.rename_branch_old.clear();
                }
                if app.rename_branch_new == new {
                    app.rename_branch_new.clear();
                }
            }
            Self::CreateWorktree {
                name,
                path,
                branch,
                create_branch,
            } => {
                if app.new_worktree_name == name {
                    app.new_worktree_name.clear();
                }
                if app.new_worktree_path == path {
                    app.new_worktree_path.clear();
                }
                if app.new_worktree_branch == branch {
                    app.new_worktree_branch.clear();
                }
                if app.new_worktree_create_branch == create_branch {
                    app.new_worktree_create_branch = false;
                }
            }
            Self::Stash { message } => {
                if app.stash_message == message {
                    app.stash_message.clear();
                }
            }
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum Tab {
    Tasks,
    ReviewQueue,
    Status,
    Branches,
    Worktrees,
    Log,
    Stash,
    Remotes,
}

pub struct App {
    pub git: GitRepo,
    pub current_tab: Tab,
    pub repo_path: String,
    /// Single status message for the bottom bar — shows the latest operation,
    /// concise success/error. Replaces old error_message + success_message.
    pub status_message: String,
    /// Whether the status_message represents an error (for coloring).
    pub status_is_error: bool,
    /// Accumulated real-time log of the latest operation (progress + final result).
    /// Used in the expandable bottom panel so users see detailed CLI-like output.
    pub last_operation_log: String,

    pub status_entries: Vec<StatusEntry>,
    pub branches: Vec<BranchInfo>,
    pub worktrees: Vec<WorktreeInfo>,
    pub commits: Vec<CommitInfo>,
    pub stashes: Vec<StashEntry>,
    pub remote_list: Vec<RemoteInfo>,

    pub commit_msg: String,
    pub commit_amend: bool,

    pub branch_filter: String,
    pub new_branch_name: String,
    pub new_branch_base: String,
    pub rename_branch_old: String,
    pub rename_branch_new: String,
    pub merge_branch_name: String,

    pub new_worktree_path: String,
    pub new_worktree_branch: String,
    pub new_worktree_name: String,
    pub new_worktree_create_branch: bool,

    pub stash_message: String,
    pub remote_name: String,
    /// Whether the remote field has been edited by the user.
    pub(crate) remote_name_user_edited: bool,
    pub push_branch: String,
    /// Whether the push branch field has been edited by the user.
    pub(crate) push_branch_user_edited: bool,
    pub push_force: bool,
    pub pull_rebase: bool,

    pub diff_content: Vec<DiffLine>,
    pub diff_path: String,
    pub show_diff: bool,
    pub log_search: String,
    /// Monotonic identity of the newest log search request.
    log_search_request_id: u64,
    /// Monotonic identity of the currently open repository.
    repo_generation: u64,

    pub last_refresh: std::time::Instant,

    pub show_about: bool,
    show_clone_dialog: bool,
    clone_url: String,
    clone_destination: String,
    pub update_state: Arc<Mutex<UpdateState>>,
    update_request_id: Arc<AtomicU64>,
    pub show_update_dialog: bool,
    pub auto_check_done: bool,
    /// Set to true when the user dismisses the update dialog to prevent it from reopening.
    pub update_dialog_dismissed: bool,
    /// Download progress from 0.0 to 1.0 for the current download.
    pub download_progress: f32,
    /// Pending Git operations running in background threads.
    pending_ops: Vec<PendingOp>,
    #[cfg(test)]
    test_before_operation: Option<Arc<dyn Fn() + Send + Sync>>,
    /// A destructive Git operation awaiting explicit user confirmation.
    pending_confirmation: Option<PendingConfirmation>,
    /// A cleanup request waiting for a fresh worktree snapshot.
    pending_worktree_cleanup: Option<PendingWorktreeCleanup>,
    /// Whether an asynchronous repository refresh is queued.
    needs_refresh: bool,
    pub recent_repos: RecentRepos,
    pub task_registry: TaskRegistry,
    pub task_form_open: bool,
    pub task_title: String,
    pub task_worktree_path: String,
    pub task_diff_review: Option<TaskDiffReviewView>,
    pub task_verification_editor: Option<TaskVerificationCommandDraft>,
    task_verification_run: Option<PendingTaskVerification>,
    task_fingerprint_probes: HashMap<String, TaskFingerprintProbe>,
    task_pr_source_fingerprint_probes: HashMap<String, TaskPrSourceFingerprintProbe>,
    task_review_queue_probes: HashMap<String, TaskReviewQueueProbe>,
    task_pull_request_probes: HashMap<String, TaskPullRequestProbe>,
    task_pull_request_inputs: HashMap<String, String>,
    task_diff_request_id: u64,
    pub status_expanded: bool,
    /// Excel-style resizable column widths for tables.
    pub column_widths: crate::ui::ColumnWidthStore,
}

impl App {
    const FONT_SIZE: f32 = 14.0;

    pub fn new() -> Self {
        Self {
            git: GitRepo::new(),
            current_tab: Tab::Worktrees,
            repo_path: String::new(),
            status_message: String::new(),
            status_is_error: false,
            last_operation_log: String::new(),

            status_entries: Vec::new(),
            branches: Vec::new(),
            worktrees: Vec::new(),
            commits: Vec::new(),
            stashes: Vec::new(),
            remote_list: Vec::new(),

            commit_msg: String::new(),
            commit_amend: false,

            branch_filter: String::new(),
            new_branch_name: String::new(),
            new_branch_base: String::new(),
            rename_branch_old: String::new(),
            rename_branch_new: String::new(),
            merge_branch_name: String::new(),

            new_worktree_path: String::new(),
            new_worktree_branch: String::new(),
            new_worktree_name: String::new(),
            new_worktree_create_branch: false,

            stash_message: String::new(),
            remote_name: String::new(),
            remote_name_user_edited: false,
            push_branch: String::new(),
            push_branch_user_edited: false,
            push_force: false,
            pull_rebase: false,

            diff_content: Vec::new(),
            diff_path: String::new(),
            show_diff: false,
            log_search: String::new(),
            log_search_request_id: 0,
            repo_generation: 0,

            last_refresh: std::time::Instant::now(),

            show_about: false,
            show_clone_dialog: false,
            clone_url: String::new(),
            clone_destination: String::new(),

            update_state: Arc::new(Mutex::new(UpdateState::Idle)),
            update_request_id: Arc::new(AtomicU64::new(0)),
            show_update_dialog: false,
            auto_check_done: false,
            update_dialog_dismissed: false,
            download_progress: 0.0,
            pending_ops: Vec::new(),
            #[cfg(test)]
            test_before_operation: None,
            pending_confirmation: None,
            pending_worktree_cleanup: None,
            needs_refresh: false,
            recent_repos: RecentRepos::load(),
            task_registry: TaskRegistry::load(),
            task_form_open: false,
            task_title: String::new(),
            task_worktree_path: String::new(),
            task_diff_review: None,
            task_verification_editor: None,
            task_verification_run: None,
            task_fingerprint_probes: HashMap::new(),
            task_pr_source_fingerprint_probes: HashMap::new(),
            task_review_queue_probes: HashMap::new(),
            task_pull_request_probes: HashMap::new(),
            task_pull_request_inputs: HashMap::new(),
            task_diff_request_id: 0,
            status_expanded: false,
            column_widths: crate::ui::init_column_widths(),
        }
    }

    pub fn trigger_update_check(&mut self) {
        let current_version = APP_VERSION.to_string();
        let state = self.update_state.clone();
        let generation = self.update_request_id.clone();
        let Some(request_id) = begin_update_request_if(
            &state,
            &generation,
            |current| {
                !matches!(current, UpdateState::Downloading { .. } | UpdateState::Downloaded { .. })
            },
            UpdateState::Checking,
        ) else {
            return;
        };
        self.update_dialog_dismissed = false;
        self.show_update_dialog = false;

        std::thread::spawn(move || {
            let result = updater::check_for_update(&current_version);
            commit_update_state_if_current(
                &state,
                &generation,
                request_id,
                |current| matches!(current, UpdateState::Checking),
                result,
            );
        });
    }

    fn dismiss_update_dialog(&mut self) {
        self.show_update_dialog = false;
        self.update_dialog_dismissed = true;
    }

    fn update_download_progress_if_active(
        state: &Mutex<UpdateState>,
        generation: &AtomicU64,
        request_id: u64,
        progress: f32,
        file_name: &str,
    ) -> bool {
        commit_update_state_if_current(
            state,
            generation,
            request_id,
            |current| {
                matches!(
                    current,
                    UpdateState::Downloading {
                        file_name: active_file,
                        ..
                    } if active_file.as_str() == file_name
                )
            },
            UpdateState::Downloading {
                progress,
                file_name: file_name.to_string(),
            },
        )
    }

    /// Start downloading the update asset in a background thread.
    /// Updates `update_state` with progress as the download proceeds.
    pub fn trigger_download(&mut self, asset: updater::ReleaseAsset) {
        if !asset.has_valid_sha256_digest() {
            let state = self.update_state.clone();
            let generation = self.update_request_id.clone();
            begin_update_request_if(
                &state,
                &generation,
                |current| !matches!(current, UpdateState::Downloading { .. }),
                UpdateState::Error(
                    "This release asset has no valid SHA-256 digest. Download it from the release page instead."
                        .to_string(),
                ),
            );
            return;
        }
        let expected_digest = asset.digest.expect("validated release digest");
        let url = asset.browser_download_url;
        let file_name = asset.name;
        let dest_dir = updater::get_default_download_dir();
        let dest_path = match update_asset_download_path(Path::new(&dest_dir), &file_name) {
            Ok(path) => path,
            Err(error) => {
                let state = self.update_state.clone();
                let generation = self.update_request_id.clone();
                begin_update_request_if(
                    &state,
                    &generation,
                    |current| !matches!(current, UpdateState::Downloading { .. }),
                    UpdateState::Error(error),
                );
                return;
            }
        };

        let state = self.update_state.clone();
        let generation = self.update_request_id.clone();
        let progress = Arc::new(Mutex::new(0.0f32));
        let prog = progress.clone();
        let state_for_progress = state.clone();
        let generation_for_progress = generation.clone();
        let Some(request_id) = begin_update_request_if(
            &state,
            &generation,
            |current| !matches!(current, UpdateState::Downloading { .. }),
            UpdateState::Downloading {
                progress: 0.0,
                file_name: file_name.clone(),
            },
        ) else {
            return;
        };
        self.download_progress = 0.0;

        std::thread::spawn(move || {
            // Update progress in real-time from the background thread
            let prog_clone = prog.clone();
            let progress_file_name = file_name.clone();
            let _prog_update_handle = std::thread::spawn(move || {
                loop {
                    let p = *prog_clone.lock().unwrap();
                    if !App::update_download_progress_if_active(
                        &state_for_progress,
                        &generation_for_progress,
                        request_id,
                        p,
                        &progress_file_name,
                    ) {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            });

            let result = updater::download_file_with_progress(
                &url,
                &dest_path,
                &expected_digest,
                prog,
            );

            let next_state = match result {
                Ok(()) => {
                    let path_str = dest_path.to_string_lossy().to_string();
                    UpdateState::Downloaded {
                        file_path: path_str,
                    }
                }
                Err(error) => UpdateState::Error(error),
            };
            commit_update_state_if_current(
                &state,
                &generation,
                request_id,
                |current| matches!(current, UpdateState::Downloading { .. }),
                next_state,
            );
        });
    }

    /// Extract the binary from the downloaded archive, create a self-update
    /// script, launch it, and exit the current process to complete the update.
    pub fn install_and_restart(&mut self) {
        if self.task_verification_any_running() {
            self.show_error("Finish or cancel task verification before installing an update".into());
            return;
        }
        let current_state = self.update_state.lock().unwrap().clone();
        let archive_path = match &current_state {
            UpdateState::Downloaded { file_path } => file_path.clone(),
            _ => return,
        };

        let archive_path = std::path::Path::new(&archive_path).to_path_buf();

        // Extract binary from archive
        let new_binary = match updater::extract_binary_from_archive(&archive_path) {
            Ok(p) => p,
            Err(e) => {
                self.status_message = format!("Failed to extract update: {}", e);
                self.status_is_error = true;
                return;
            }
        };

        // Get current executable path
        let current_binary = match std::env::current_exe() {
            Ok(p) => p,
            Err(e) => {
                self.status_message = format!("Failed to get current exe path: {}", e);
                self.status_is_error = true;
                return;
            }
        };

        // Create self-update script
        let script_path = match updater::create_self_update_script(&new_binary, &current_binary) {
            Ok(p) => p,
            Err(e) => {
                self.status_message = format!("Failed to create update script: {}", e);
                self.status_is_error = true;
                return;
            }
        };

        // Launch the script (detached from parent process)
        if !self.try_launch_update_script(&script_path) {
            return;
        }

        // Exit the current process immediately
        std::process::exit(0);
    }

    fn try_launch_update_script(&mut self, script_path: &Path) -> bool {
        match updater::launch_self_update_script(script_path) {
            Ok(()) => true,
            Err(e) => {
                self.show_error(e);
                false
            }
        }
    }

    pub fn open_repo(&mut self, path: &str) {
        self.open_repo_path(Path::new(path));
    }

    pub(crate) fn open_repo_path(&mut self, path: &Path) {
        // Pending operations publish results back into this App, so changing
        // repositories before they finish could apply stale data to the new one.
        if self.is_busy() {
            return;
        }

        let path_display = path.to_string_lossy().into_owned();
        self.status_message.clear();
        self.status_is_error = false;
        match self.git.open(path) {
            Ok(()) => {
                self.log_search_request_id = self.log_search_request_id.wrapping_add(1);
                self.repo_generation = self.repo_generation.wrapping_add(1);
                self.pending_worktree_cleanup = None;
                self.repo_path = path_display.clone();
                self.remote_name_user_edited = false;
                self.push_branch.clear();
                self.push_branch_user_edited = false;
                self.diff_content.clear();
                self.diff_path.clear();
                self.show_diff = false;
                self.status_entries.clear();
                self.branches.clear();
                self.worktrees.clear();
                self.commits.clear();
                self.stashes.clear();
                self.remote_list.clear();
                self.needs_refresh = true;
                if let Err(error) = self.recent_repos.add(&path_display) {
                    self.status_message = format!(
                        "Opened repository at {} (failed to save recent history: {})",
                        path_display, error
                    );
                    self.status_is_error = true;
                }
            }
            Err(e) => {
                self.status_message = format!("Failed to open repo: {}", e);
                self.status_is_error = true;
            }
        }
    }

    fn remove_recent_repo(&mut self, index: usize) {
        if let Err(error) = self.recent_repos.remove(index) {
            self.show_error(format!("Failed to save recent history: {}", error));
        }
    }

    fn show_welcome_screen(&mut self, ui: &mut egui::Ui) -> egui::Response {
        let mut clone_response = None;
        ui.vertical_centered(|ui| {
            ui.add_space(100.0);
            ui.heading("Git Manager");
            ui.label("Open a Git repository to get started.");
            ui.add_space(20.0);
            if crate::ui::add_enabled_ellipsis(ui, !self.is_busy(), "📂 Open Repository").clicked() {
                let path = crate::native_file_dialog();
                if let Some(p) = path {
                    self.open_repo(&p);
                }
            }
            ui.add_space(10.0);
            ui.label("Or drag & drop a folder");
            let response = crate::ui::ellipsis_button(ui, "Clone Repository...");
            if response.clicked() {
                self.show_clone_dialog = true;
            }
            clone_response = Some(response);

            if self.status_is_error && !self.status_message.is_empty() {
                ui.add_space(4.0);
                ui.colored_label(
                    App::adaptive_red(ui.style().visuals.dark_mode),
                    &self.status_message,
                );
            }

            if !self.recent_repos.is_empty() {
                ui.add_space(30.0);
                ui.separator();
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new("📁 Recent Repositories")
                        .heading(),
                );
                ui.add_space(5.0);

                let mut to_delete: Option<usize> = None;
                let entries = self.recent_repos.entries().to_vec();
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .show(ui, |ui| {
                        for (i, entry) in entries.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.set_min_width(400.0);
                                let repo_name = format!("📂 {}", entry.name);
                                if ui
                                    .selectable_label(false, egui::RichText::new(&repo_name).size(14.0))
                                    .clicked()
                                {
                                    self.open_repo(&entry.path);
                                }
                                ui.label(
                                    egui::RichText::new(&entry.path)
                                        .size(10.0)
                                        .color(egui::Color32::GRAY),
                                );
                                if crate::ui::ellipsis_button(ui, "🗑 Delete").clicked() {
                                    to_delete = Some(i);
                                }
                            });
                        }
                    });
                if let Some(idx) = to_delete {
                    self.remove_recent_repo(idx);
                }
            }
        });
        clone_response.expect("welcome screen clone button is always rendered")
    }

    fn start_clone(&mut self, ctx: &egui::Context, url: String, destination: String) {
        let url = url.trim().to_string();
        if url.is_empty() {
            self.show_error("Repository URL required".into());
            return;
        }
        let destination = std::path::PathBuf::from(destination.trim());
        if destination.as_os_str().is_empty() {
            self.show_error("Clone destination required".into());
            return;
        }
        if self.is_busy() {
            return;
        }

        self.status_message.clear();
        self.status_is_error = false;
        let (tx, rx) = mpsc::channel::<OpResult>();
        let progress = Arc::new(Mutex::new(String::new()));
        let operation_progress = progress.clone();
        let operation_destination = destination.clone();

        std::thread::spawn(move || {
            let mut callbacks = git2::RemoteCallbacks::new();
            callbacks.credentials(clone_credential);
            callbacks.transfer_progress(move |stats| {
                let message = format!(
                    "Receiving objects: {}/{} ({} bytes)",
                    stats.received_objects(),
                    stats.total_objects(),
                    stats.received_bytes()
                );
                *operation_progress.lock().unwrap() = message;
                true
            });
            let mut fetch_options = git2::FetchOptions::new();
            fetch_options.remote_callbacks(callbacks);
            let mut builder = git2::build::RepoBuilder::new();
            builder.fetch_options(fetch_options);

            let result = match builder.clone(&url, &operation_destination) {
                Ok(_) => OpResult::CloneSuccess(operation_destination),
                Err(error) => OpResult::Error(format!("Failed to clone repository: {}", error)),
            };
            let _ = tx.send(result);
        });

        self.pending_ops.push(PendingOp {
            description: "Cloning repository".to_string(),
            receiver: rx,
            repo_generation: self.repo_generation,
            started_at: Instant::now(),
            progress,
            last_progress_update: Instant::now(),
            last_seen_progress: String::new(),
            timed_out: false,
            task_diff_identity: None,
            form_submission: None,

        });
        self.last_operation_log =
            "▶ Operation: Cloning repository\n  (waiting for progress...)\n".to_string();
        ctx.request_repaint();
    }

    fn render_clone_dialog(&mut self, ctx: &egui::Context) -> Option<egui::Response> {
        if !self.show_clone_dialog {
            return None;
        }

        let busy = self.is_busy();
        let mut open = true;
        let mut start_clone = false;
        let mut clone_button_response = None;
        let mut window = egui::Window::new("Clone Repository")
            .collapsible(false)
            .resizable(false);
        if !busy {
            window = window.open(&mut open);
        }
        window.show(ctx, |ui| {
                ui.label("Repository URL");
                ui.add_enabled(
                    !busy,
                    egui::TextEdit::singleline(&mut self.clone_url).desired_width(360.0),
                );
                ui.add_space(8.0);
                ui.label("Destination folder");
                ui.horizontal(|ui| {
                    ui.add_enabled(
                        !busy,
                        egui::TextEdit::singleline(&mut self.clone_destination)
                            .desired_width(280.0),
                    );
                    if crate::ui::add_enabled_ellipsis(ui, !busy, "Browse...").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_title("Select clone destination")
                            .pick_folder()
                        {
                            self.clone_destination = path.to_string_lossy().into_owned();
                        }
                    }
                });

                if busy {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(self.current_operation());
                    });
                    if let Some(operation) = self.pending_ops.first() {
                        let progress = operation.progress.lock().unwrap().clone();
                        if !progress.is_empty() {
                            ui.label(progress);
                        }
                    }
                }

                if self.status_is_error && !self.status_message.is_empty() {
                    ui.add_space(4.0);
                    ui.colored_label(
                        App::adaptive_red(ctx.style().visuals.dark_mode),
                        &self.status_message,
                    );
                }

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if crate::ui::add_enabled_ellipsis(ui, !busy, "Cancel").clicked() {
                        self.show_clone_dialog = false;
                    }
                    let can_clone = !busy
                        && !self.clone_url.trim().is_empty()
                        && !self.clone_destination.trim().is_empty();
                    let response = crate::ui::add_enabled_ellipsis(ui, can_clone, "Clone");
                    if response.clicked() {
                        start_clone = true;
                    }
                    clone_button_response = Some(response);
                });
            });

        if !busy {
            self.show_clone_dialog &= open;
        }
        if start_clone {
            self.start_clone(
                ctx,
                self.clone_url.clone(),
                self.clone_destination.clone(),
            );
        }
        clone_button_response
    }

    /// Checks if an operation is running or awaits destructive-action confirmation.
    pub fn is_busy(&self) -> bool {
        !self.pending_ops.is_empty()
            || self.pending_confirmation.is_some()
            || self.pending_worktree_cleanup.is_some()
            || self.task_verification_run.is_some()
    }

    /// Returns the description of the current/last operation.
    /// For the status bar: just the operation name (concise).
    pub fn current_operation(&self) -> String {
        self.pending_ops
            .first()
            .map(|op| op.description.clone())
            .or_else(|| {
                self.task_verification_run
                    .as_ref()
                    .map(|_| "Running task verification".into())
            })
            .or_else(|| {
                self.pending_confirmation
                    .as_ref()
                    .map(|request| format!("Awaiting confirmation: {}", request.description))
            })
            .unwrap_or_default()
    }

    pub fn request_confirmation(
        &mut self,
        ctx: &egui::Context,
        title: impl Into<String>,
        message: impl Into<String>,
        confirm_label: impl Into<String>,
        description: impl Into<String>,
        operation: GitOperation,
    ) {
        if self.is_busy() {
            return;
        }
        if self.git.path().is_none() {
            self.show_error("No repository open".into());
            return;
        }

        self.pending_confirmation = Some(PendingConfirmation {
            title: title.into(),
            message: message.into(),
            confirm_label: confirm_label.into(),
            description: description.into(),
            operation,
            repo_generation: self.repo_generation,
        });
        ctx.request_repaint();
    }

    pub(crate) fn preview_worktree_cleanup(
        &mut self,
        ctx: &egui::Context,
        worktree: &WorktreeInfo,
        force_requested: bool,
    ) {
        if self.is_busy() {
            return;
        }
        if !self.git.is_open() {
            self.show_error("No repository open".into());
            return;
        }

        self.pending_worktree_cleanup = Some(PendingWorktreeCleanup {
            path: worktree.path.clone(),
            expected_git_link: worktree.git_link_identity.clone(),
            directory_missing_at_request: worktree.status.directory_missing,
            force_requested,
            repo_generation: self.repo_generation,
            ready: false,
        });
        self.status_message.clear();
        self.status_is_error = false;
        self.needs_refresh = false;
        self.start_operation(
            ctx,
            WORKTREE_CLEANUP_PREVIEW_OPERATION,
            GitOperation::RefreshWorktreeCleanup(worktree.path.clone()),
        );
        if self.pending_ops.is_empty() {
            self.pending_worktree_cleanup = None;
        }
    }

    fn confirm_pending_operation(&mut self, ctx: &egui::Context) {
        let Some(request) = self.pending_confirmation.take() else {
            return;
        };
        if request.repo_generation != self.repo_generation {
            self.show_error("Repository changed; confirmation cancelled".into());
            return;
        }
        self.start_operation(ctx, &request.description, request.operation);
    }

    fn cancel_pending_confirmation(&mut self) {
        self.pending_confirmation = None;
    }

    fn finish_worktree_cleanup_preview(&mut self, ctx: &egui::Context) {
        let Some(request) = self.pending_worktree_cleanup.take() else {
            return;
        };
        if request.repo_generation != self.repo_generation {
            return;
        }

        let Some(worktree) = self
            .worktrees
            .iter()
            .find(|worktree| worktree.path == request.path)
            .cloned()
        else {
            self.show_error("Worktree disappeared before cleanup could be previewed".into());
            return;
        };
        if let Some(error) = worktree.status.inspection_error.as_deref() {
            self.show_error(format!("Cannot safely preview worktree cleanup: {}", error));
            return;
        }
        if request.directory_missing_at_request {
            if !worktree.status.directory_missing || request.expected_git_link.is_some() {
                self.show_error(
                    "Worktree directory changed since it was listed; refresh and retry".into(),
                );
                return;
            }
        } else {
            let Some(expected_git_link) = request.expected_git_link.as_ref() else {
                self.show_error(
                    "Cannot safely preview cleanup because the listed worktree identity is unavailable".into(),
                );
                return;
            };
            if worktree.git_link_identity.is_none()
                || !expected_git_link.matches_path(&request.path)
            {
                self.show_error(
                    "Worktree identity changed since it was listed; refresh and retry".into(),
                );
                return;
            }
        }

        let Some(expected_head) = worktree.head_snapshot.clone() else {
            self.show_error("Cannot safely preview cleanup because worktree HEAD is unavailable".into());
            return;
        };
        let branch_used_elsewhere = worktree.branch.as_ref().is_some_and(|branch| {
            self.worktrees.iter().any(|other| {
                other.path != worktree.path && other.branch.as_ref() == Some(branch)
            })
        });
        let force = request.force_requested || worktree.status.has_removal_blockers();
        let label = if force {
            "Force Remove worktree"
        } else {
            "Remove worktree"
        };
        self.request_confirmation(
            ctx,
            "Review worktree cleanup",
            worktree_cleanup_preview_message(&worktree, force, branch_used_elsewhere),
            label,
            format!("{} {:?}", label, worktree.path),
            GitOperation::RemoveWorktree {
                path: worktree.path,
                force,
                expected_git_link: request.expected_git_link,
                expected_head,
                require_git_link_identity: true,
            },
        );
    }

    fn render_confirmation_dialog(&mut self, ctx: &egui::Context) {
        let Some(request) = self.pending_confirmation.as_ref() else {
            return;
        };
        let title = request.title.clone();
        let message = request.message.clone();
        let confirm_label = request.confirm_label.clone();
        let mut open = true;
        let mut confirm = false;
        let mut cancel = false;

        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_max_width(540.0);
                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .show(ui, |ui| ui.label(message));
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    if ui.button(&confirm_label).clicked() {
                        confirm = true;
                    }
                });
            });

        if cancel || !open {
            self.cancel_pending_confirmation();
        } else if confirm {
            self.confirm_pending_operation(ctx);
        }
    }

    /// Spawn a Git operation in a background thread.
    /// Returns immediately. Results will be processed in `process_pending_ops()`.
    pub fn start_operation(&mut self, ctx: &egui::Context, description: &str, op: GitOperation) {
        self.start_operation_inner_at(ctx, description, op, None, None);
    }

    pub(crate) fn start_operation_with_form_submission(
        &mut self,
        ctx: &egui::Context,
        description: &str,
        op: GitOperation,
        form_submission: FormSubmission,
    ) {
        self.start_operation_inner_at(ctx, description, op, Some(form_submission), None);
    }

    pub fn start_task_diff_review(&mut self, ctx: &egui::Context, task: &TaskRecord) {
        if self.is_busy() {
            return;
        }
        self.task_diff_request_id = self.task_diff_request_id.wrapping_add(1);
        let request_id = self.task_diff_request_id;
        self.task_diff_review = Some(TaskDiffReviewView {
            task_id: task.id.clone(),
            title: task.title.clone(),
            state: TaskDiffReviewState::Loading,
        });
        self.start_operation_inner_at(
            ctx,
            "Reading task diff",
            GitOperation::TaskDiffReview {
                task_id: task.id.clone(),
                request_id,
                base_commit: task.base_commit.clone(),
                repository_path: PathBuf::from(&task.repository_path),
            },
            None,
            Some(std::path::PathBuf::from(&task.worktree_path)),
        );
    }

    pub fn start_task_verification(&mut self, ctx: &egui::Context, task: &TaskRecord) {
        if self.is_busy() {
            return;
        }
        let Some(command) = task.verification_command.clone() else {
            self.show_error("Configure a verification command before running it".into());
            return;
        };
        if let Err(error) = command.validate() {
            self.show_error(error);
            return;
        }

        let run_id = task_verification::next_run_id();
        let running = VerificationResult::running(run_id.clone(), command.clone());
        let run_lock = match self
            .task_registry
            .start_verification(&task.id, &command, running)
        {
            Ok(run_lock) => run_lock,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                self.show_error(
                    "Task verification is already running in another Git Manager instance".into(),
                );
                return;
            }
            Err(error) => {
                self.show_error(format!("Could not start task verification: {error}"));
                return;
            }
        };

        let cancel_requested = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel_requested.clone();
        let task_id = task.id.clone();
        let worker_task_id = task_id.clone();
        let worktree_path = PathBuf::from(&task.worktree_path);
        let worker_path = worktree_path.clone();
        let repository_path = PathBuf::from(&task.repository_path);
        let worker_repository_path = repository_path.clone();
        let worker_run_id = run_id.clone();
        let worker_command = command.clone();
        let (sender, receiver) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = task_verification::run_and_record(
                &worker_task_id,
                worker_path,
                worker_repository_path,
                worker_run_id,
                worker_command,
                worker_cancel,
            );
            let _ = sender.send(result);
        });

        self.task_fingerprint_probes.remove(&task_id);
        self.task_pr_source_fingerprint_probes.remove(&task_id);
        self.task_review_queue_probes.remove(&task_id);
        self.task_verification_run = Some(PendingTaskVerification {
            task_id,
            cancel_requested,
            receiver,
            worker,
            _run_lock: run_lock,
        });
        ctx.request_repaint();
    }

    pub fn cancel_task_verification(&mut self, task_id: &str) {
        if let Some(run) = self
            .task_verification_run
            .as_ref()
            .filter(|run| run.task_id.as_str() == task_id)
        {
            run.cancel_requested.store(true, Ordering::Relaxed);
        }
    }

    pub fn task_verification_is_running(&self, task_id: &str) -> bool {
        self.task_verification_run
            .as_ref()
            .is_some_and(|run| run.task_id == task_id)
    }

    pub fn task_verification_any_running(&self) -> bool {
        self.task_verification_run.is_some()
    }

    pub fn forget_task_fingerprint_probe(&mut self, task_id: &str) {
        self.task_fingerprint_probes.remove(task_id);
        self.task_pr_source_fingerprint_probes.remove(task_id);
        self.task_review_queue_probes.remove(task_id);
    }

    pub fn forget_task_pull_request_probe(&mut self, task_id: &str) {
        self.task_pull_request_probes.remove(task_id);
        self.task_pr_source_fingerprint_probes.remove(task_id);
        self.task_pull_request_inputs.remove(task_id);
    }

    pub fn task_pull_request_input(&mut self, task_id: &str) -> &mut String {
        self.task_pull_request_inputs
            .entry(task_id.to_string())
            .or_default()
    }

    pub fn task_pull_request_action_running(&self, task_id: &str) -> bool {
        self.task_pull_request_probes
            .get(task_id)
            .is_some_and(|probe| probe.receiver.is_some())
    }

    pub fn start_task_pull_request_action(
        &mut self,
        ctx: &egui::Context,
        task: &TaskRecord,
        action: PullRequestAction,
        identifier: Option<String>,
        source_fingerprint: Option<String>,
    ) {
        if action == PullRequestAction::Create
            && self
                .task_registry
                .entries()
                .iter()
                .find(|entry| entry.id == task.id)
                .map_or(true, |entry| entry.pull_request_url.is_some())
        {
            return;
        }
        if self
            .task_pull_request_probes
            .get(&task.id)
            .is_some_and(|probe| probe.receiver.is_some())
        {
            return;
        }

        let worker_task = task.clone();
        let worker_identifier = identifier;
        let worker_source_fingerprint = source_fingerprint;
        let worker_ctx = ctx.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let result = task_delivery::perform_action(
                &worker_task,
                action,
                worker_identifier.as_deref(),
            )
            .map(|mut snapshot| {
                snapshot.source_fingerprint_at_fetch = worker_source_fingerprint;
                snapshot
            });
            let _ = sender.send(result);
            worker_ctx.request_repaint();
        });

        let probe = self
            .task_pull_request_probes
            .entry(task.id.clone())
            .or_default();
        probe.receiver = Some(receiver);
        probe.action = Some(action);
        probe.action_message = None;
        if action == PullRequestAction::Refresh {
            probe.error = None;
        }
        ctx.request_repaint();
    }

    fn process_task_pull_request_actions(&mut self) {
        let task_ids = self
            .task_pull_request_probes
            .iter()
            .filter_map(|(task_id, probe)| probe.receiver.as_ref().map(|_| task_id.clone()))
            .collect::<Vec<_>>();
        for task_id in task_ids {
            self.process_task_pull_request_action(&task_id);
        }
    }

    fn process_task_pull_request_action(&mut self, task_id: &str) {
        let completion = self
            .task_pull_request_probes
            .get(task_id)
            .and_then(|probe| {
                let receiver = probe.receiver.as_ref()?;
                let action = probe.action.unwrap_or(PullRequestAction::Refresh);
                match receiver.try_recv() {
                    Ok(result) => Some((action, result)),
                    Err(mpsc::TryRecvError::Disconnected) => Some((
                        action,
                        Err(task_delivery::PullRequestActionError {
                            message: "The GitHub status worker stopped before returning a result"
                                .into(),
                            created_url: None,
                        }),
                    )),
                    Err(mpsc::TryRecvError::Empty) => None,
                }
            });
        let Some((action, result)) = completion else {
            return;
        };

        let result = if action != PullRequestAction::Refresh {
            match result {
                Ok(snapshot) => match self
                    .task_registry
                    .record_pull_request(task_id, &snapshot.url)
                {
                    Ok(()) => {
                        let action_text = match action {
                            PullRequestAction::Associate => "Associated",
                            PullRequestAction::Create => "Created",
                            PullRequestAction::Refresh => unreachable!(),
                        };
                        if action == PullRequestAction::Associate {
                            self.task_pull_request_inputs.remove(task_id);
                        }
                        let message = PullRequestActionMessage {
                            succeeded: true,
                            text: format!(
                                "{action_text} PR #{} · {}",
                                snapshot.number, snapshot.url
                            ),
                        };
                        if let Some(probe) = self.task_pull_request_probes.get_mut(task_id) {
                            probe.action_message = Some(message);
                        }
                        Ok(snapshot)
                    }
                    Err(error) => Err(format!(
                        "PR #{} is available at {}, but its link could not be saved: {error}",
                        snapshot.number, snapshot.url
                    )),
                },
                Err(error) => {
                    let mut message = error.message;
                    if action == PullRequestAction::Create {
                        if let Some(url) = error.created_url {
                            match self.task_registry.record_pull_request(task_id, &url) {
                                Ok(()) => {
                                    message = format!("{message}. The created PR is linked at {url}.");
                                }
                                Err(save_error) => {
                                    message = format!(
                                        "{message}. The created PR is at {url}, but its link could not be saved: {save_error}. Use Associate PR with this URL."
                                    );
                                }
                            }
                        }
                    }
                    Err(message)
                }
            }
        } else {
            result.map_err(|error| error.message)
        };

        if let Some(probe) = self.task_pull_request_probes.get_mut(task_id) {
            probe.receiver = None;
            probe.action = None;
            match result {
                Ok(snapshot) => {
                    probe.snapshot = Some(snapshot.clone());
                    probe.error = None;
                    probe.checked_at = Some(Instant::now());
                    if action != PullRequestAction::Refresh {
                        probe.action_message.get_or_insert_with(|| PullRequestActionMessage {
                            succeeded: true,
                            text: format!("Refreshed PR #{}", snapshot.number),
                        });
                    }
                }
                Err(error) if action == PullRequestAction::Refresh => {
                    probe.error = Some(error);
                    probe.checked_at = Some(Instant::now());
                }
                Err(error) => {
                    probe.action_message = Some(PullRequestActionMessage {
                        succeeded: false,
                        text: error,
                    });
                }
            }
        }
    }

    pub fn current_task_pull_request_status(
        &mut self,
        ctx: &egui::Context,
        task: &TaskRecord,
    ) -> PullRequestStatusView {
        self.process_task_pull_request_action(&task.id);

        let current_task = self
            .task_registry
            .entries()
            .iter()
            .find(|entry| entry.id == task.id)
            .cloned()
            .unwrap_or_else(|| task.clone());
        let linked = current_task.pull_request_url.is_some();
        let current_source_fingerprint = if linked {
            self.current_task_pr_source_fingerprint(ctx, &current_task)
        } else {
            None
        };
        let active_refreshes = self
            .task_pull_request_probes
            .values()
            .filter(|probe| {
                probe.action == Some(PullRequestAction::Refresh) && probe.receiver.is_some()
            })
            .count();
        let should_refresh = linked
            && active_refreshes < MAX_TASK_PR_REFRESHES
            && self.task_pull_request_probes.get(&task.id).map_or(true, |probe| {
                probe.receiver.is_none()
                    && (probe.checked_at.map_or(true, |checked_at| {
                        checked_at.elapsed() >= TASK_PR_REFRESH_INTERVAL
                    }) || (current_source_fingerprint.is_some()
                        && probe.snapshot.as_ref().is_some_and(|snapshot| {
                            snapshot.source_fingerprint_at_fetch.is_none()
                        })))
            });
        if should_refresh {
            self.start_task_pull_request_action(
                ctx,
                &current_task,
                PullRequestAction::Refresh,
                None,
                current_source_fingerprint.clone(),
            );
        }

        let Some(probe) = self.task_pull_request_probes.get(&task.id) else {
            return PullRequestStatusView {
                remote: if linked {
                    task_delivery::PullRequestRemoteView::Checking {
                        action: "Waiting for a GitHub status slot",
                    }
                } else {
                    task_delivery::PullRequestRemoteView::Unlinked
                },
                action_message: None,
            };
        };
        let remote = if let Some(action) = probe.action.filter(|_| probe.receiver.is_some()) {
            if action == PullRequestAction::Refresh {
                if probe.snapshot.is_some() || probe.error.is_some() {
                    task_delivery::PullRequestRemoteView::Refreshing {
                        previous: probe.snapshot.clone(),
                        previous_at: probe
                            .snapshot
                            .as_ref()
                            .map(|snapshot| snapshot.fetched_at.clone()),
                    }
                } else {
                    task_delivery::PullRequestRemoteView::Checking {
                        action: action.label(),
                    }
                }
            } else {
                task_delivery::PullRequestRemoteView::Checking {
                    action: action.label(),
                }
            }
        } else if !linked {
            task_delivery::PullRequestRemoteView::Unlinked
        } else if probe.checked_at.map_or(true, |checked_at| {
            checked_at.elapsed() >= TASK_PR_REFRESH_INTERVAL
        }) {
            task_delivery::PullRequestRemoteView::Refreshing {
                previous: probe.snapshot.clone(),
                previous_at: probe
                    .snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.fetched_at.clone()),
            }
        } else if let Some(error) = &probe.error {
            task_delivery::PullRequestRemoteView::Unavailable {
                message: error.clone(),
                previous: probe.snapshot.clone(),
                previous_at: probe
                    .snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.fetched_at.clone()),
            }
        } else if let Some(snapshot) = &probe.snapshot {
            task_delivery::PullRequestRemoteView::Ready {
                snapshot: task_delivery::refresh_snapshot_freshness(
                    snapshot,
                    current_source_fingerprint.as_deref(),
                ),
            }
        } else {
            task_delivery::PullRequestRemoteView::Checking {
                action: PullRequestAction::Refresh.label(),
            }
        };
        if probe.receiver.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        PullRequestStatusView {
            remote,
            action_message: probe.action_message.clone(),
        }
    }

    pub fn task_verification_cancel_requested(&self, task_id: &str) -> bool {
        self.task_verification_run.as_ref().is_some_and(|run| {
            run.task_id.as_str() == task_id && run.cancel_requested.load(Ordering::Relaxed)
        })
    }

    /// Returns the current task fingerprint when a recent asynchronous check is ready.
    /// `None` means a check is in progress or the task has no saved source snapshot.
    pub fn current_task_fingerprint(
        &mut self,
        ctx: &egui::Context,
        task: &TaskRecord,
    ) -> Option<Result<String, String>> {
        let saved_fingerprint = task
            .verification_result
            .as_ref()
            .and_then(|result| result.source_fingerprint.as_ref());
        if saved_fingerprint.is_none() {
            return None;
        }

        self.request_task_source_fingerprint(ctx, task, TASK_FINGERPRINT_REFRESH_INTERVAL)
    }

    fn current_task_pr_source_fingerprint(
        &mut self,
        ctx: &egui::Context,
        task: &TaskRecord,
    ) -> Option<String> {
        let probe_matches_task = self
            .task_pr_source_fingerprint_probes
            .get(&task.id)
            .is_some_and(|probe| {
                probe.worktree_path == task.worktree_path
                    && probe.repository_path == task.repository_path
            });
        if !probe_matches_task {
            self.task_pr_source_fingerprint_probes.insert(
                task.id.clone(),
                TaskPrSourceFingerprintProbe::for_task(task),
            );
        }

        let watcher_setup = self
            .task_pr_source_fingerprint_probes
            .get_mut(&task.id)
            .and_then(|probe| probe.watcher_receiver.as_ref())
            .map(|receiver| receiver.try_recv());
        if let Some(result) = watcher_setup {
            match result {
                Ok(Ok(watcher)) => {
                    if let Some(probe) = self.task_pr_source_fingerprint_probes.get_mut(&task.id) {
                        probe.watcher = Some(watcher);
                        probe.watcher_receiver = None;
                    }
                }
                Ok(Err(error)) => {
                    if let Some(probe) = self.task_pr_source_fingerprint_probes.get_mut(&task.id) {
                        probe.watcher_receiver = None;
                        probe.watch_error = Some(error);
                    }
                    return None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    if let Some(probe) = self.task_pr_source_fingerprint_probes.get_mut(&task.id) {
                        probe.watcher_receiver = None;
                        probe.watch_error = Some(
                            "Task source watcher stopped before it was initialized".into(),
                        );
                    }
                    return None;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(250));
                    return None;
                }
            }
        }

        let watch_error = self
            .task_pr_source_fingerprint_probes
            .get(&task.id)
            .and_then(|probe| probe.watch_error.as_ref())
            .is_some();
        if watch_error {
            return None;
        }

        let watcher_ready = self
            .task_pr_source_fingerprint_probes
            .get(&task.id)
            .is_some_and(|probe| probe.watcher.is_some());
        if !watcher_ready {
            let active_probes = self
                .task_pr_source_fingerprint_probes
                .values()
                .filter(|probe| probe.receiver.is_some() || probe.watcher_receiver.is_some())
                .count()
                + self
                    .task_fingerprint_probes
                    .values()
                    .filter(|probe| probe.receiver.is_some())
                    .count();
            if active_probes >= MAX_TASK_SOURCE_FINGERPRINT_PROBES {
                ctx.request_repaint_after(Duration::from_millis(250));
                return None;
            }
            let worktree_path = PathBuf::from(&task.worktree_path);
            let repository_path = PathBuf::from(&task.repository_path);
            let worker_context = ctx.clone();
            let (sender, receiver) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = sender.send(task_pr_source_watcher(
                    &worktree_path,
                    &repository_path,
                    &worker_context,
                ));
            });
            if let Some(probe) = self.task_pr_source_fingerprint_probes.get_mut(&task.id) {
                probe.watcher_receiver = Some(receiver);
            }
            ctx.request_repaint_after(Duration::from_millis(250));
            return None;
        }

        let active_probes = self
            .task_pr_source_fingerprint_probes
            .values()
            .filter(|probe| probe.receiver.is_some() || probe.watcher_receiver.is_some())
            .count()
            + self
                .task_fingerprint_probes
                .values()
                .filter(|probe| probe.receiver.is_some())
                .count();
        let Some(probe) = self.task_pr_source_fingerprint_probes.get_mut(&task.id) else {
            return None;
        };
        let Some(watcher) = probe.watcher.as_ref() else {
            return None;
        };
        if watcher
            .error
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some()
        {
            probe.result = None;
            probe.receiver = None;
            return None;
        }

        if watcher.invalidated.load(Ordering::Acquire) {
            probe.result = None;
        }
        if let Some(receiver) = &probe.receiver {
            match receiver.try_recv() {
                Ok(Ok(result)) => {
                    let source_changed_during_check =
                        watcher.invalidated.load(Ordering::Acquire);
                    probe.receiver = None;
                    probe.result = (!source_changed_during_check).then_some(result);
                }
                Ok(Err(_)) | Err(mpsc::TryRecvError::Disconnected) => {
                    probe.receiver = None;
                    probe.result = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }

        let needs_check = probe.receiver.is_none()
            && watcher.invalidated.load(Ordering::Acquire)
            && probe.result.is_none();
        if needs_check {
            if active_probes >= MAX_TASK_SOURCE_FINGERPRINT_PROBES {
                ctx.request_repaint_after(Duration::from_millis(250));
                return None;
            }
            watcher.invalidated.store(false, Ordering::Release);
            let path = PathBuf::from(&task.worktree_path);
            let repository_path = PathBuf::from(&task.repository_path);
            let (sender, receiver) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = sender.send(task_verification::source_fingerprint(
                    &path,
                    &repository_path,
                ));
            });
            probe.receiver = Some(receiver);
            probe.result = None;
        }

        if probe.receiver.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
            None
        } else if watcher.invalidated.load(Ordering::Acquire) {
            None
        } else {
            probe.result.clone()
        }
    }

    fn request_task_source_fingerprint(
        &mut self,
        ctx: &egui::Context,
        task: &TaskRecord,
        refresh_interval: Duration,
    ) -> Option<Result<String, String>> {
        let active_probes = self
            .task_fingerprint_probes
            .values()
            .filter(|probe| probe.receiver.is_some())
            .count();
        let probe = self
            .task_fingerprint_probes
            .entry(task.id.clone())
            .or_insert_with(|| TaskFingerprintProbe {
                result: None,
                checked_at: None,
                receiver: None,
            });

        if let Some(receiver) = &probe.receiver {
            match receiver.try_recv() {
                Ok(result) => {
                    probe.result = Some(result);
                    probe.checked_at = Some(Instant::now());
                    probe.receiver = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    probe.result = Some(Err("Fingerprint check stopped before it completed".into()));
                    probe.checked_at = Some(Instant::now());
                    probe.receiver = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }

        let needs_check = probe.receiver.is_none()
            && probe
                .checked_at
                .map_or(true, |checked_at| checked_at.elapsed() >= refresh_interval);
        if needs_check {
            if active_probes >= 2 {
                ctx.request_repaint_after(Duration::from_millis(250));
                return None;
            }
            let path = PathBuf::from(&task.worktree_path);
            let repository_path = PathBuf::from(&task.repository_path);
            let (sender, receiver) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = sender.send(task_verification::source_fingerprint(
                    &path,
                    &repository_path,
                ));
            });
            probe.receiver = Some(receiver);
            probe.result = None;
        }

        if probe.receiver.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
            None
        } else {
            if let Some(checked_at) = probe.checked_at {
                ctx.request_repaint_after(refresh_interval.saturating_sub(checked_at.elapsed()));
            }
            probe.result.clone()
        }
    }

    pub fn current_task_review_queue_state(
        &mut self,
        ctx: &egui::Context,
        task: &TaskRecord,
    ) -> Option<TaskReviewQueueState> {
        let active_probes = self
            .task_review_queue_probes
            .values()
            .filter(|probe| probe.receiver.is_some())
            .count();
        let probe = self
            .task_review_queue_probes
            .entry(task.id.clone())
            .or_insert_with(|| TaskReviewQueueProbe {
                result: None,
                checked_at: None,
                receiver: None,
            });

        if let Some(receiver) = &probe.receiver {
            match receiver.try_recv() {
                Ok(result) => {
                    probe.result = Some(result);
                    probe.checked_at = Some(Instant::now());
                    probe.receiver = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    probe.result = Some(TaskReviewQueueState::Error(
                        "Task review status check stopped before it completed".into(),
                    ));
                    probe.checked_at = Some(Instant::now());
                    probe.receiver = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }

        let needs_check = probe.receiver.is_none()
            && probe
                .checked_at
                .map_or(true, |checked_at| checked_at.elapsed() >= Duration::from_secs(5));
        if needs_check {
            if active_probes >= 2 {
                ctx.request_repaint_after(Duration::from_millis(250));
                return None;
            }
            let worktree_path = PathBuf::from(&task.worktree_path);
            let repository_path = PathBuf::from(&task.repository_path);
            let base_commit = task.base_commit.clone();
            let (sender, receiver) = mpsc::channel();
            std::thread::spawn(move || {
                let mut repo = GitRepo::new();
                let state = match repo.open(&worktree_path) {
                    Ok(()) => repo.task_review_queue_state(base_commit.as_deref(), &repository_path),
                    Err(error) => TaskReviewQueueState::Error(format!(
                        "Could not open the task worktree: {error}"
                    )),
                };
                let _ = sender.send(state);
            });
            probe.receiver = Some(receiver);
            probe.result = None;
        }

        if probe.receiver.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
            None
        } else {
            if let Some(checked_at) = probe.checked_at {
                ctx.request_repaint_after(
                    Duration::from_secs(5).saturating_sub(checked_at.elapsed()),
                );
            }
            probe.result.clone()
        }
    }

    fn process_task_verification(&mut self, ctx: &egui::Context) {
        let completion = self
            .task_verification_run
            .as_ref()
            .map(|run| run.receiver.try_recv());
        match completion {
            Some(Ok(result)) => {
                let run = self.task_verification_run.take();
                if let Some(run) = run {
                    let _ = run.worker.join();
                    self.task_registry.reload();
                    self.task_fingerprint_probes.remove(&run.task_id);
                    self.task_review_queue_probes.remove(&run.task_id);
                    match result {
                        Ok(()) => self.show_success("Task verification finished; its saved result is bound to the captured source state.".into()),
                        Err(error) => self.show_error(error),
                    }
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                let run = self.task_verification_run.take();
                if let Some(run) = run {
                    let _ = run.worker.join();
                    self.task_registry.reload();
                    self.task_fingerprint_probes.remove(&run.task_id);
                    self.task_review_queue_probes.remove(&run.task_id);
                }
                self.show_error("Task verification worker stopped before it could save a result".into());
            }
            Some(Err(mpsc::TryRecvError::Empty)) => {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
            None => {}
        }
    }

    fn start_operation_inner_at(
        &mut self,
        ctx: &egui::Context,
        description: &str,
        op: GitOperation,
        form_submission: Option<FormSubmission>,
        repo_path_override: Option<std::path::PathBuf>,
    ) {
        if self.pending_confirmation.is_some()
            || (self.pending_worktree_cleanup.is_some()
                && description != WORKTREE_CLEANUP_PREVIEW_OPERATION)
        {
            return;
        }

        // Get the repo path to pass to the thread
        let repo_path = match repo_path_override {
            Some(path) => path,
            None => match self.git.path() {
                Some(path) => path.to_path_buf(),
                None => {
                    self.show_error("No repository open".into());
                    return;
                }
            },
        };

        let (tx, rx) = mpsc::channel::<OpResult>();
        let desc = description.to_string();
        let repo_generation = self.repo_generation;
        let task_diff_identity = match &op {
            GitOperation::TaskDiffReview {
                task_id,
                request_id,
                ..
            } => Some((task_id.clone(), *request_id)),
            _ => None,
        };
        let progress = Arc::new(Mutex::new(String::new()));
        let op_progress = progress.clone();
        #[cfg(test)]
        let test_before_operation = self.test_before_operation.clone();

        std::thread::spawn(move || {
            #[cfg(test)]
            if let Some(test_before_operation) = test_before_operation {
                test_before_operation();
            }
            let result = execute_operation(&repo_path, op, op_progress);
            let _ = tx.send(result);
        });

        self.pending_ops.push(PendingOp {
            description: desc,
            receiver: rx,
            repo_generation,
            started_at: Instant::now(),
            progress,
            last_progress_update: Instant::now(),
            last_seen_progress: String::new(),
            timed_out: false,
            task_diff_identity,
            form_submission,
        });

        // Initialize the operation log with description
        self.last_operation_log = format!("▶ Operation: {}\n", description);
        // Ensure log has a trailing placeholder so progress can accumulate
        self.last_operation_log += "  (waiting for progress...)\n";

        ctx.request_repaint();
    }

    /// Start a log search with a request identity so out-of-order responses can be ignored.
    pub fn start_log_search(&mut self, ctx: &egui::Context, filter: String) {
        self.log_search_request_id = self.log_search_request_id.wrapping_add(1);
        let request_id = self.log_search_request_id;
        self.start_operation(
            ctx,
            "Searching commits",
            GitOperation::LogSearch { filter, request_id },
        );
    }

    /// Process completed background operations.
    /// Must be called at the start of each `update()` frame.
    /// Uses a progress-watchdog timeout: keeps waiting while the progress string keeps
    /// changing (operation is still alive). Times out when progress stops for too long,
    /// or when no progress was ever received beyond a reasonable limit.
    pub fn process_pending_ops(&mut self, ctx: &egui::Context) {
        let mut i = 0;
        while i < self.pending_ops.len() {
            // --- Watchdog timeout: check if the operation is still making progress ---
            let (
                description,
                started_at,
                current_progress,
                last_seen_progress,
                last_progress_update,
                timed_out,
                task_diff_identity,
            ) = {
                let op = &self.pending_ops[i];
                let description = op.description.clone();
                let started_at = op.started_at;
                let current_progress = op.progress.lock().unwrap().clone();
                let last_seen_progress = op.last_seen_progress.clone();
                let last_progress_update = op.last_progress_update;
                let timed_out = op.timed_out;
                let task_diff_identity = op.task_diff_identity.clone();
                (
                    description,
                    started_at,
                    current_progress,
                    last_seen_progress,
                    last_progress_update,
                    timed_out,
                    task_diff_identity,
                )
            };

            // If progress text changed, reset the watchdog timer and accumulate to log
            if !timed_out && current_progress != last_seen_progress {
                if let Some(mut_op) = self.pending_ops.get_mut(i) {
                    mut_op.last_progress_update = Instant::now();
                    mut_op.last_seen_progress = current_progress.clone();
                }
                if !current_progress.is_empty() {
                    self.last_operation_log += &format!("  {}\n", current_progress);
                }
            }

            let receive_result = self.pending_ops[i].receiver.try_recv();
            if !timed_out
                && current_progress == last_seen_progress
                && receive_result.is_err()
            {
                if current_progress.is_empty() {
                    // No progress ever received: give 60 seconds total
                    if started_at.elapsed().as_secs() > 60 {
                        if description == WORKTREE_CLEANUP_PREVIEW_OPERATION {
                            self.pending_worktree_cleanup = None;
                        }
                        let msg = format!(
                            "Operation '{}' timed out (no progress in 60s)",
                            description
                        );
                        self.status_message = msg.clone();
                        self.status_is_error = true;
                        self.last_operation_log += &format!("  ✗ {}\n", msg);
                        self.fail_task_diff_review(
                            task_diff_identity.clone(),
                            "The task diff worker timed out before returning a complete patch.".into(),
                        );
                        self.pending_ops[i].timed_out = true;
                        i += 1;
                        continue;
                    }
                } else {
                    // Progress was received but stopped: 30 second stall threshold
                    let stall_secs = last_progress_update.elapsed().as_secs();
                    if stall_secs > 30 {
                        if description == WORKTREE_CLEANUP_PREVIEW_OPERATION {
                            self.pending_worktree_cleanup = None;
                        }
                        let msg = format!(
                            "Operation '{}' timed out (stalled {}s)\nLast: {}",
                            description, stall_secs, current_progress
                        );
                        self.status_message = msg.clone();
                        self.status_is_error = true;
                        self.last_operation_log += &format!("  ✗ {}\n", msg);
                        self.fail_task_diff_review(
                            task_diff_identity.clone(),
                            "The task diff worker stalled before returning a complete patch.".into(),
                        );
                        self.pending_ops[i].timed_out = true;
                        i += 1;
                        continue;
                    }
                }
            }

            let op = &self.pending_ops[i];
            match receive_result {
                Ok(result) => {
                    let op = self.pending_ops.swap_remove(i);
                    if op.timed_out {
                        // Keep the UI blocked until the timed-out worker has finished.
                        if matches!(&result, OpResult::Success(_)) {
                            if op.repo_generation == self.repo_generation {
                                if let Some(form_submission) = op.form_submission {
                                    form_submission.clear_if_unchanged(self);
                                }
                            }
                            self.needs_refresh = true;
                        } else if matches!(&result, OpResult::CloneSuccess(_)) {
                            self.handle_op_result(op.description, result);
                        } else if let OpResult::TaskDiffReview {
                            task_id,
                            request_id,
                            state,
                        } = result
                        {
                            self.update_task_diff_review(task_id, request_id, state);
                        }
                        continue;
                    }
                    if op.repo_generation != self.repo_generation
                        && matches!(&result, OpResult::RefreshData { .. })
                    {
                        if self
                            .pending_worktree_cleanup
                            .as_ref()
                            .is_some_and(|request| request.repo_generation == op.repo_generation)
                        {
                            self.pending_worktree_cleanup = None;
                        }
                        continue;
                    }
                    // Append final progress to log before handling result
                    let final_progress = current_progress.clone();
                    if !final_progress.is_empty() {
                        self.last_operation_log += &format!("  {}\n", final_progress);
                    }
                    if op.repo_generation == self.repo_generation
                        && matches!(&result, OpResult::Success(_))
                    {
                        if let Some(form_submission) = op.form_submission {
                            form_submission.clear_if_unchanged(self);
                        }
                    }
                    self.handle_op_result(op.description, result);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    i += 1; // Still pending
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    if self.pending_ops[i].description == WORKTREE_CLEANUP_PREVIEW_OPERATION {
                        self.pending_worktree_cleanup = None;
                    }
                    if self.pending_ops[i].timed_out {
                        self.fail_task_diff_review(
                            task_diff_identity.clone(),
                            "The task diff worker exited before returning a complete patch.".into(),
                        );
                        self.pending_ops.swap_remove(i);
                        continue;
                    }
                    let last_prog = op.progress.lock().unwrap().clone();
                    let fail_msg = if last_prog.is_empty() {
                        format!("Operation '{}' failed unexpectedly", op.description)
                    } else {
                        format!("Operation '{}' failed unexpectedly\nLast progress: {}", op.description, last_prog)
                    };
                    self.status_message = fail_msg.clone();
                    self.status_is_error = true;
                    self.last_operation_log += &format!("  ✗ {}\n", fail_msg);
                    self.fail_task_diff_review(task_diff_identity, fail_msg);
                    self.pending_ops.swap_remove(i);
                }
            }
        }

        let cleanup_preview_ready = self
            .pending_worktree_cleanup
            .as_ref()
            .is_some_and(|request| request.ready);
        if cleanup_preview_ready && self.pending_ops.is_empty() {
            self.finish_worktree_cleanup_preview(ctx);
        }

        // Start a queued refresh once other operations and confirmations are clear.
        if self.needs_refresh
            && self.pending_ops.is_empty()
            && self.pending_confirmation.is_none()
        {
            self.needs_refresh = false;
            if self.git.is_open() {
                self.start_operation(ctx, "Refreshing", GitOperation::RefreshAll);
            }
        }
    }

    fn handle_op_result(&mut self, description: String, result: OpResult) {
        match result {
            OpResult::Success(msg) => {
                self.last_operation_log += &format!("  ✓ {}\n", msg);
                // Set status_message to the concise success message
                self.status_message = msg;
                self.status_is_error = false;
                // Auto-refresh after mutation operations
                self.needs_refresh = true;
            }
            OpResult::Error(e) => {
                if description == WORKTREE_CLEANUP_PREVIEW_OPERATION {
                    self.pending_worktree_cleanup = None;
                }
                let err_msg = format!("{}: {}", description, e);
                self.last_operation_log += &format!("  ✗ {}\n", err_msg);
                // Set status_message to concise error message
                self.status_message = err_msg;
                self.status_is_error = true;
            }
            OpResult::CloneSuccess(path) => {
                self.last_operation_log += &format!("  ✓ Cloned repository into {}\n", path.display());
                let path = path.to_string_lossy().into_owned();
                self.show_clone_dialog = false;
                self.clone_url.clear();
                self.clone_destination.clear();
                self.open_repo(&path);
                if self.repo_path == path {
                    self.current_tab = Tab::Worktrees;
                    if !self.status_is_error {
                        self.show_success(format!("Cloned repository into {}", path));
                    }
                }
            }
            OpResult::DiffContent { path, lines } => {
                self.diff_path = path;
                self.diff_content = lines;
                self.show_diff = true;
            }
            OpResult::TaskDiffReview {
                task_id,
                request_id,
                state,
            } => self.update_task_diff_review(task_id, request_id, state),
            OpResult::SearchResults { request_id, filter, commits } => {
                if request_id == self.log_search_request_id && filter == self.log_search {
                    self.commits = commits;
                }
            }
            OpResult::RefreshData {
                status_entries,
                branches,
                worktrees,
                commits,
                stashes,
                remote_list,
                errors,
            } => {
                self.status_entries = status_entries;
                self.branches = branches;
                self.worktrees = worktrees;
                if description == WORKTREE_CLEANUP_PREVIEW_OPERATION {
                    if let Some(request) = &mut self.pending_worktree_cleanup {
                        request.ready = true;
                    }
                }
                self.commits = filter_commits(commits, &self.log_search);
                self.stashes = stashes;
                self.remote_list = remote_list;
                self.last_refresh = Instant::now();
                if !errors.is_empty() {
                    let msg = errors.join("; ");
                    self.last_operation_log += &format!("  ✗ {}\n", msg);
                    self.status_message = msg;
                    self.status_is_error = true;
                }
            }
        }
    }

    fn update_task_diff_review(
        &mut self,
        task_id: String,
        request_id: u64,
        state: TaskDiffReviewState,
    ) {
        if request_id == self.task_diff_request_id {
            if let Some(view) = self.task_diff_review.as_mut() {
                if view.task_id == task_id {
                    view.state = state;
                }
            }
        }
    }

    fn fail_task_diff_review(&mut self, identity: Option<(String, u64)>, message: String) {
        if let Some((task_id, request_id)) = identity {
            self.update_task_diff_review(
                task_id,
                request_id,
                TaskDiffReviewState::Error(message),
            );
        }
    }

    pub fn refresh_all(&mut self, ctx: &egui::Context) {
        if !self.git.is_open() || self.is_busy() || self.pending_confirmation.is_some() {
            return;
        }
        self.status_message.clear();
        self.status_is_error = false;
        self.needs_refresh = false;
        self.start_operation(ctx, "Refreshing", GitOperation::RefreshAll);
    }

    pub fn show_error(&mut self, msg: String) {
        self.status_message = msg;
        self.status_is_error = true;
    }

    pub fn show_success(&mut self, msg: String) {
        self.status_message = msg;
        self.status_is_error = false;
    }

    /// Returns the project folder name extracted from the repo path.
    /// e.g. "/home/user/projects/my-repo" → "my-repo"
    pub fn repo_name(&self) -> String {
        path_name(&self.repo_path)
    }

    /// Format elapsed seconds into a human-readable string.
    /// <60s → "Just updated", <3600s → "Updated Xm ago", ≥3600s → "Updated Xh ago"
    pub fn format_elapsed(elapsed_secs: u64) -> String {
        if elapsed_secs < 60 {
            "Just updated".to_string()
        } else if elapsed_secs < 3600 {
            format!("Updated {}m ago", elapsed_secs / 60)
        } else {
            format!("Updated {}h ago", elapsed_secs / 3600)
        }
    }

    /// Return an adaptive green color suitable for both dark and light mode.
    pub fn adaptive_green(dark: bool) -> egui::Color32 {
        if dark {
            egui::Color32::from_rgb(80, 220, 80)   // Bright green on dark bg
        } else {
            egui::Color32::from_rgb(0, 120, 0)       // Dark green on light bg
        }
    }

    /// Return an adaptive yellow/amber color suitable for both dark and light mode.
    pub fn adaptive_yellow(dark: bool) -> egui::Color32 {
        if dark {
            egui::Color32::from_rgb(220, 200, 50)    // Bright yellow on dark bg
        } else {
            egui::Color32::from_rgb(180, 130, 0)      // Dark amber on light bg
        }
    }

    /// Return an adaptive red color suitable for both dark and light mode.
    pub fn adaptive_red(dark: bool) -> egui::Color32 {
        if dark {
            egui::Color32::from_rgb(240, 80, 80)      // Bright red on dark bg
        } else {
            egui::Color32::from_rgb(180, 30, 30)      // Dark red on light bg
        }
    }

    pub fn status_color_by_type(s: char, dark: bool) -> egui::Color32 {
        match s {
            'M' | 'A' | 'R' => Self::adaptive_green(dark),
            'D' => Self::adaptive_red(dark),
            'U' => Self::adaptive_yellow(dark),  // Conflicted
            _ => egui::Color32::GRAY,            // '?' untracked, '!' ignored, or unknown
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Auto-check for updates on startup (first frame only)
        if !self.auto_check_done {
            self.auto_check_done = true;
            self.trigger_update_check();
            ctx.request_repaint();
        }

        // Poll update state from background thread
        {
            let state = self.update_state.lock().unwrap();
            match &*state {
                UpdateState::UpdateAvailable { latest_version, download_url: _, assets: _ } => {
                    if !self.update_dialog_dismissed && !self.show_update_dialog {
                        self.show_update_dialog = true;
                    }
                    let _ = latest_version;
                }
                UpdateState::Checking => {
                    ctx.request_repaint();
                }
                UpdateState::Downloading { progress, file_name: _ } => {
                    self.download_progress = *progress;
                    ctx.request_repaint();
                }
                UpdateState::Downloaded { file_path: _ } => {
                    self.download_progress = 1.0;
                }
                _ => {}
            }
        }
        self.process_pending_ops(ctx);
        self.process_task_verification(ctx);
        self.process_task_pull_request_actions();

        let dark = ctx.style().visuals.dark_mode;
        if dark {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        // Apply font size via text styles
        let fs = Self::FONT_SIZE;
        ctx.style_mut(|style| {
            style.text_styles = [
                (egui::TextStyle::Body, egui::FontId::proportional(fs)),
                (egui::TextStyle::Button, egui::FontId::proportional(fs)),
                (egui::TextStyle::Heading, egui::FontId::proportional(fs + 4.0)),
                (egui::TextStyle::Small, egui::FontId::proportional(fs - 2.0)),
                (egui::TextStyle::Monospace, egui::FontId::monospace(fs)),
            ].into();
        });

        // --- Top Bar ---
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if crate::ui::ellipsis_button(ui, "☷ Tasks").clicked() {
                    self.current_tab = Tab::Tasks;
                }
                if crate::ui::add_enabled_ellipsis(ui, !self.is_busy(), "📂").clicked() {
                    let path = crate::native_file_dialog();
                    if let Some(p) = path {
                        self.open_repo(&p);
                    }
                }

                // Recent repos dropdown
                let recent_repos_enabled = !self.is_busy();
                ui.add_enabled_ui(recent_repos_enabled, |ui| {
                    egui::menu::menu_button(ui, "🕒", |ui| {
                        if self.recent_repos.is_empty() {
                            ui.label("No recent repositories");
                        } else {
                            let mut to_delete: Option<usize> = None;
                            let entries = self.recent_repos.entries().to_vec();
                            ui.label(
                                egui::RichText::new("Recent Repositories")
                                    .strong()
                                    .size(14.0),
                            );
                            ui.separator();
                            for (i, entry) in entries.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.set_min_width(300.0);
                                    if ui
                                        .selectable_label(false, &entry.name)
                                        .clicked()
                                    {
                                        self.open_repo(&entry.path);
                                        ui.close_menu();
                                    }
                                    ui.label(
                                        egui::RichText::new(&entry.path)
                                            .size(10.0)
                                            .color(egui::Color32::GRAY),
                                    );
                                    if crate::ui::ellipsis_button(ui, "🗑").clicked() {
                                        to_delete = Some(i);
                                    }
                                });
                            }
                            if let Some(idx) = to_delete {
                                self.remove_recent_repo(idx);
                            }
                        }
                    });
                });

                if self.git.is_open() {
                    ui.separator();
                    let project_name = self.repo_name();

                    let mut job = egui::text::LayoutJob::default();
                    job.append(
                        &project_name,
                        0.0,
                        egui::TextFormat {
                            font_id: egui::FontId::proportional(14.0),
                            color: egui::Color32::from_rgb(100, 150, 255),
                            ..Default::default()
                        },
                    );
                    job.append(
                        "  ",
                        0.0,
                        egui::TextFormat::default(),
                    );
                    job.append(
                        &self.repo_path,
                        0.0,
                        egui::TextFormat {
                            font_id: egui::FontId::proportional(11.0),
                            color: egui::Color32::GRAY,
                            ..Default::default()
                        },
                    );
                    ui.add(
                        egui::Label::new(job).truncate(),
                    )
                    .on_hover_text(format!("{}\n{}", project_name, self.repo_path));

                    // Right-side elements anchored to right edge: version, about, update indicator
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Show update indicator if an update is available
                        {
                            let state = self.update_state.lock().unwrap();
                            if matches!(*state, UpdateState::UpdateAvailable { .. }) {
                                ui.label(
                                    egui::RichText::new("⬆ Update")
                                        .color(App::adaptive_yellow(dark)),
                                );
                            }
                        }
                        // About button
                        if ui.button(ABOUT_BUTTON_LABEL).clicked() {
                            self.show_about = !self.show_about;
                        }
                        // Version label (truncatable so it doesn't push buttons off-screen)
                        let version_text = format!("v{}", APP_VERSION);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&version_text)
                                    .color(egui::Color32::GRAY)
                                    .text_style(egui::TextStyle::Small),
                            )
                            .truncate(),
                        )
                        .on_hover_text(version_text);
                    });

                    ui.separator();
                    let branch = self.git.current_branch().unwrap_or_default();
                    let branch_text = format!("🔀 {}", branch);
                    ui.add(
                        egui::Label::new(&branch_text)
                            .truncate(),
                    )
                    .on_hover_text(&branch_text);

                    let status_count = self.status_entries.len();
                    if status_count > 0 {
                        let status_text = format!("{} changes", status_count);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&status_text)
                                    .color(egui::Color32::YELLOW),
                            )
                            .truncate(),
                        )
                        .on_hover_text(&status_text);
                    }

                    if !self.remote_list.is_empty() {
                        let remote = &self.remote_list[0];
                        let remote_text = format!("🌐 {}", remote.name);
                        ui.add(
                            egui::Label::new(&remote_text)
                                .truncate(),
                        )
                        .on_hover_text(&remote_text);
                    }
                } else {
                    // No repo open: show version + about on the right
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(ABOUT_BUTTON_LABEL).clicked() {
                            self.show_about = !self.show_about;
                        }
                        let version_text = format!("v{}", APP_VERSION);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&version_text)
                                    .color(egui::Color32::GRAY)
                                    .text_style(egui::TextStyle::Small),
                            )
                            .truncate(),
                        )
                        .on_hover_text(version_text);
                    });
                }
            });
        });

        // --- Bottom Bar (simplified: single message + "..." expand + timestamp) ---
        if self.git.is_open() {
            let bottom_height = if self.status_expanded { 200.0 } else { 24.0 };
            egui::TopBottomPanel::bottom("bottom_bar")
                .resizable(false)
                .min_height(bottom_height)
                .show(ctx, |ui| {
                    let dark = ctx.style().visuals.dark_mode;
                    ui.horizontal(|ui| {
                        // --- Right side: timestamp + "..." expand/collapse button ---
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let elapsed = self.last_refresh.elapsed().as_secs();
                            let elapsed_text = App::format_elapsed(elapsed);
                            ui.add(
                                egui::Label::new(&elapsed_text)
                                    .truncate(),
                            )
                            .on_hover_text(elapsed_text);

                            // Expand button: "..." when collapsed, "✕" when expanded
                            let btn_label = if self.status_expanded { "✕" } else { "···" };
                            if ui.button(btn_label).clicked() {
                                self.status_expanded = !self.status_expanded;
                            }
                        });

                        // --- Left side: single status message (operation or result) ---
                        if self.is_busy() {
                            // Show concise operation name while busy
                            let op_text = self.current_operation();
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&op_text)
                                        .color(App::adaptive_yellow(dark))
                                        .size(13.0),
                                )
                                .truncate(),
                            );
                        } else if !self.status_message.is_empty() {
                            let color = if self.status_is_error {
                                App::adaptive_red(dark)
                            } else {
                                App::adaptive_green(dark)
                            };
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&self.status_message)
                                        .color(color)
                                        .size(13.0),
                                )
                                .truncate(),
                            );
                        }
                    });

                    // --- Expanded area: full command output log ---
                    if self.status_expanded {
                        ui.separator();
                        egui::ScrollArea::vertical()
                            .max_height(150.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(self.last_operation_log.clone())
                                        .monospace()
                                        .size(12.0),
                                );
                                ui.allocate_space(ui.available_size());
                            });
                    }
                });
            }

        // --- Central Panel ---
        egui::CentralPanel::default().show(ctx, |ui| {
            if !self.git.is_open() {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui.button("Tasks").clicked() {
                                self.current_tab = Tab::Tasks;
                            }
                            if ui.button("Review queue").clicked() {
                                self.current_tab = Tab::ReviewQueue;
                            }
                        });
                        if self.current_tab == Tab::ReviewQueue {
                            crate::ui::task_review_queue::show(self, ui, ctx);
                        } else {
                            crate::ui::task_panel::show(self, ui, ctx);
                        }
                        ui.add_space(12.0);
                        ui.separator();
                        self.show_welcome_screen(ui);
                    });
                return;
            }

            // Tab bar — wrapped in horizontal ScrollArea so tabs don't overflow when window is narrow
            egui::ScrollArea::horizontal()
                .id_salt("tab_bar_scroll")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let tabs = [
                            (Tab::Tasks, "☷ Tasks"),
                            (Tab::ReviewQueue, "☷ Review queue"),
                            (Tab::Status, "📊 Status"),
                            (Tab::Branches, "🔀 Branches"),
                            (Tab::Worktrees, "📂 Worktrees"),
                            (Tab::Log, "📋 Log"),
                            (Tab::Stash, "📦 Stash"),
                            (Tab::Remotes, "🌐 Remotes"),
                        ];

                        for (tab, label) in &tabs {
                            let selected = self.current_tab == *tab;
                            let btn = egui::Button::new(*label)
                                .fill(if selected {
                                    ui.style().visuals.selection.bg_fill
                                } else {
                                    egui::Color32::TRANSPARENT
                                });
                            if ui.add(btn).clicked() {
                                self.current_tab = tab.clone();
                            }
                        }
                    });
                });

            ui.separator();

            // Wrap tab panel in a vertical ScrollArea so content is scrollable
            // when the expanded bottom bar takes up vertical space.
            egui::ScrollArea::vertical()
                .id_salt("main_content_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    // Render the active tab panel
                    match self.current_tab {
                        Tab::Tasks => crate::ui::task_panel::show(self, ui, ctx),
                        Tab::ReviewQueue => crate::ui::task_review_queue::show(self, ui, ctx),
                        Tab::Status => crate::ui::status_panel::show(self, ui, ctx),
                        Tab::Branches => crate::ui::branch_panel::show(self, ui, ctx),
                        Tab::Worktrees => crate::ui::worktree_panel::show(self, ui, ctx),
                        Tab::Log => crate::ui::log_panel::show(self, ui, ctx),
                        Tab::Stash => crate::ui::stash_panel::show(self, ui, ctx),
                        Tab::Remotes => crate::ui::remote_panel::show(self, ui, ctx),
                    }
                    ui.allocate_space(ui.available_size());
                });
        });

        let _ = self.render_clone_dialog(ctx);
        self.render_confirmation_dialog(ctx);

        // About window
        if self.show_about {
            egui::Window::new("About Git Manager")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.heading("Git Manager");
                        ui.add_space(4.0);
                        ui.label(format!("Version: {}", APP_VERSION));
                        ui.label(format!("Commit: {}", crate::version_info::GIT_HASH));
                        ui.label(format!("Tag: {}", crate::version_info::GIT_DESCRIBE));
                        ui.label(format!("Build: {}", crate::version_info::BUILD_DATE));
                        ui.add_space(8.0);
                        ui.hyperlink("https://github.com/JohnXu22786/GitManager");
                        ui.add_space(8.0);
                        ui.label("A dedicated Git branch & worktree manager.");
                        ui.add_space(4.0);
                        ui.label("Built with Rust + egui + libgit2");
                        ui.add_space(12.0);

                        // Check for Updates button
                        {
                            let state = self.update_state.lock().unwrap().clone();
                            match state {
                                UpdateState::Idle | UpdateState::UpToDate => {
                                    if crate::ui::ellipsis_button(ui, "Check for Updates").clicked() {
                                        self.trigger_update_check();
                                    }
                                    if state == UpdateState::UpToDate {
                                        ui.add_space(4.0);
                                        ui.label(egui::RichText::new("✓ You're up to date!").color(App::adaptive_green(dark)));
                                    }
                                }
                                UpdateState::Checking => {
                                    ui.add(egui::Spinner::new());
                                    ui.label("Checking for updates...");
                                }
                                UpdateState::UpdateAvailable { latest_version, download_url, ref assets } => {
                                    ui.colored_label(App::adaptive_yellow(dark), format!("Update available: {}!", latest_version));
                                    let asset = updater::find_asset_for_current_platform(assets);
                                    let can_auto_download = asset
                                        .as_ref()
                                        .is_some_and(updater::ReleaseAsset::has_valid_sha256_digest);
                                    if can_auto_download {
                                        if crate::ui::ellipsis_button(ui, "Download & Install").clicked() {
                                            self.trigger_download(asset.expect("verified asset"));
                                        }
                                    } else {
                                        if asset.is_some() {
                                            ui.label("Automatic installation requires a valid SHA-256 release digest.");
                                        }
                                        // Fallback: open browser
                                        if crate::ui::ellipsis_button(ui, "Download (Browser)").clicked() {
                                            let _ = open::that(&download_url);
                                        }
                                    }
                                }
                                UpdateState::Downloading { progress, file_name } => {
                                    ui.colored_label(App::adaptive_yellow(dark), format!("Downloading: {} ({:.0}%)", file_name, progress * 100.0));
                                    ui.add(
                                        egui::ProgressBar::new(progress)
                                            .show_percentage()
                                            .animate(true),
                                    );
                                }
                UpdateState::Downloaded { file_path } => {
                                    ui.colored_label(App::adaptive_green(dark), "✓ Download complete!");
                                    ui.label(egui::RichText::new(&file_path).size(10.0).color(egui::Color32::GRAY));
                                }
                                UpdateState::Error(ref msg) => {
                                    ui.colored_label(App::adaptive_red(dark), msg.as_str());
                                    if crate::ui::ellipsis_button(ui, "Retry").clicked() {
                                        self.trigger_update_check();
                                    }
                                }
                            }
                        }

                        ui.add_space(8.0);
                        if crate::ui::ellipsis_button(ui, "Close").clicked() {
                            self.show_about = false;
                        }
                    });
                });
        }

        // Auto-update notification dialog
        // Note: we intentionally keep show_update_dialog = true during download
        // so the dialog shows the Downloading → Downloaded state transitions.
        // The dialog only closes when the user explicitly dismisses it.
        if self.show_update_dialog {
            let current_state = self.update_state.lock().unwrap().clone();
            match &current_state {
                UpdateState::UpdateAvailable { latest_version, download_url, assets } => {
                    // If download is in progress (triggered from About window), switch to download view
                    if matches!(current_state, UpdateState::Downloading { .. }) {
                        // Let the next match arm handle it
                    } else {
                        egui::Window::new("Update Available")
                            .collapsible(false)
                            .resizable(false)
                            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                            .show(ctx, |ui| {
                                ui.vertical_centered(|ui| {
                                    ui.heading("🚀 Update Available!");
                                    ui.add_space(8.0);
                                    ui.label(format!(
                                        "Version {} is now available (you have {}).",
                                        latest_version,
                                        APP_VERSION,
                                    ));
                                    ui.add_space(8.0);
                                    let asset = updater::find_asset_for_current_platform(assets);
                                    let can_auto_download = asset
                                        .as_ref()
                                        .is_some_and(updater::ReleaseAsset::has_valid_sha256_digest);
                                    ui.label(if can_auto_download {
                                        "The download will be verified before installation."
                                    } else {
                                        "Automatic installation is unavailable because no valid SHA-256 digest is published for this platform."
                                    });
                                    ui.add_space(12.0);
                                    ui.horizontal(|ui| {
                                        // Try auto-download first
                                        if can_auto_download {
                                            if crate::ui::ellipsis_button(ui, "Auto Download").clicked() {
                                                self.trigger_download(asset.expect("verified asset"));
                                                // Keep dialog open to show progress
                                            }
                                        }
                                        // Fallback: open browser
                                        if crate::ui::ellipsis_button(ui, "Open in Browser").clicked() {
                                            let _ = open::that(download_url);
                                            self.dismiss_update_dialog();
                                        }
                                        if crate::ui::ellipsis_button(ui, "Remind Later").clicked() {
                                            self.dismiss_update_dialog();
                                        }
                                    });
                                });
                            });
                    }
                }
                UpdateState::Downloading { progress, file_name } => {
                    egui::Window::new("Downloading Update")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(ctx, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.heading("⬇ Downloading Update");
                                ui.add_space(8.0);
                                ui.label(format!("Downloading: {}...", file_name));
                                ui.add_space(8.0);
                                ui.add(
                                    egui::ProgressBar::new(*progress)
                                        .show_percentage()
                                        .animate(true),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new(format!("{:.1}%", *progress * 100.0))
                                        .color(App::adaptive_yellow(ctx.style().visuals.dark_mode)),
                                );
                            });
                        });
                }
                UpdateState::Downloaded { file_path } => {
                    egui::Window::new("Download Complete")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(ctx, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.heading("✅ Download Complete!");
                                ui.add_space(8.0);
                                ui.label("The update has been downloaded successfully.");
                                ui.add_space(4.0);
                                    ui.label(
                                        egui::RichText::new(file_path.clone())
                                            .size(10.0)
                                            .color(egui::Color32::GRAY),
                                    ).on_hover_text(file_path.clone());
                                ui.add_space(12.0);
                                ui.horizontal(|ui| {
                                    if crate::ui::add_enabled_ellipsis(
                                        ui,
                                        !self.task_verification_any_running(),
                                        "Install & Restart",
                                    )
                                    .clicked()
                                    {
                                        self.install_and_restart();
                                    }
                                    if crate::ui::ellipsis_button(ui, "Dismiss").clicked() {
                                        *self.update_state.lock().unwrap() = UpdateState::Idle;
                                    }
                                });
                                if self.task_verification_any_running() {
                                    ui.label("Finish or cancel task verification before installing an update.");
                                }
                            });
                        });
                }
                UpdateState::Error(message) => {
                    egui::Window::new("Update Download Failed")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(ctx, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.heading("Update Download Failed");
                                ui.add_space(8.0);
                                ui.colored_label(
                                    App::adaptive_red(ctx.style().visuals.dark_mode),
                                    message.as_str(),
                                );
                                ui.add_space(12.0);
                                ui.horizontal(|ui| {
                                    if crate::ui::ellipsis_button(ui, "Retry").clicked() {
                                        self.trigger_update_check();
                                    }
                                    if crate::ui::ellipsis_button(ui, "Dismiss").clicked() {
                                        self.dismiss_update_dialog();
                                    }
                                });
                            });
                        });
                }
                _ => {
                    // State changed while dialog was open (or no longer relevant)
                    self.show_update_dialog = false;
                }
            }
        }

        // Keep repainting while background operations are in progress.
        if !self.pending_ops.is_empty() {
            ctx.request_repaint();
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if let Some(run) = self.task_verification_run.take() {
            run.cancel_requested.store(true, Ordering::Relaxed);
            let _ = run.worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for_background_operations(app: &mut App, ctx: &egui::Context) {
        for _ in 0..200 {
            app.process_pending_ops(ctx);
            if !app.is_busy() && !app.needs_refresh {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("background repository operations should finish");
    }

    #[test]
    fn app_version_matches_cargo_package_version() {
        assert_eq!(APP_VERSION, env!("CARGO_PKG_VERSION"));
    }

    // --- New tests for simplified status bar (Requirement 3 & 4) ---

    #[test]
    fn test_status_message_starts_empty() {
        let app = App::new();
        assert!(app.status_message.is_empty());
        assert!(!app.status_is_error);
    }

    #[test]
    fn test_show_error_sets_status_message_and_is_error() {
        let mut app = App::new();
        app.show_error("Cannot delete branch 'feature-x'".into());
        assert_eq!(app.status_message, "Cannot delete branch 'feature-x'");
        assert!(app.status_is_error);
    }

    #[test]
    fn test_show_success_sets_status_message_and_not_error() {
        let mut app = App::new();
        app.show_success("Deleted 'feature-x' successfully".into());
        assert_eq!(app.status_message, "Deleted 'feature-x' successfully");
        assert!(!app.status_is_error);
    }

    #[test]
    fn test_show_error_overwrites_status_message() {
        let mut app = App::new();
        app.show_success("old success".into());
        assert!(!app.status_is_error);
        app.show_error("new error".into());
        assert_eq!(app.status_message, "new error");
        assert!(app.status_is_error);
    }

    #[test]
    fn test_show_success_overwrites_status_message() {
        let mut app = App::new();
        app.show_error("old error".into());
        assert!(app.status_is_error);
        app.show_success("new success".into());
        assert_eq!(app.status_message, "new success");
        assert!(!app.status_is_error);
    }

    fn test_commit(message: &str, author: &str) -> CommitInfo {
        CommitInfo {
            sha: format!("{message}-sha"),
            short_sha: "1234567".to_string(),
            author: author.to_string(),
            time: "2026-01-01 00:00:00".to_string(),
            message: message.to_string(),
            summary: message.to_string(),
        }
    }

    fn queue_form_result(app: &mut App, result: OpResult, form_submission: FormSubmission) {
        let (sender, receiver) = mpsc::channel();
        sender.send(result).expect("queue operation result");
        app.pending_ops.push(PendingOp {
            description: "Form operation".to_string(),
            receiver,
            repo_generation: app.repo_generation,
            started_at: Instant::now(),
            progress: Arc::new(Mutex::new(String::new())),
            last_progress_update: Instant::now(),
            last_seen_progress: String::new(),
            timed_out: false,
            task_diff_identity: None,
            form_submission: Some(form_submission),
        });
    }

    #[test]
    fn failed_form_operation_preserves_commit_input() {
        let mut app = App::new();
        app.commit_msg = "fix the parser".to_string();
        app.commit_amend = true;
        let form_submission = FormSubmission::Commit {
            message: app.commit_msg.clone(),
            amend: app.commit_amend,
        };
        queue_form_result(
            &mut app,
            OpResult::Error("hook rejected commit".to_string()),
            form_submission,
        );

        app.process_pending_ops(&egui::Context::default());

        assert_eq!(app.commit_msg, "fix the parser");
        assert!(app.commit_amend);
    }

    #[test]
    fn successful_form_operation_clears_only_unchanged_values() {
        let mut app = App::new();
        app.new_branch_name = "feature".to_string();
        app.new_branch_base = "main".to_string();
        let form_submission = FormSubmission::CreateBranch {
            name: app.new_branch_name.clone(),
            base: app.new_branch_base.clone(),
        };
        queue_form_result(
            &mut app,
            OpResult::Success("Created branch 'feature'".to_string()),
            form_submission,
        );
        app.new_branch_name = "next feature".to_string();

        app.process_pending_ops(&egui::Context::default());

        assert_eq!(app.new_branch_name, "next feature");
        assert!(app.new_branch_base.is_empty());
    }

    #[test]
    fn successful_form_reset_covers_each_form() {
        let mut app = App::new();
        app.commit_msg = "commit".to_string();
        app.commit_amend = true;
        app.new_branch_name = "branch".to_string();
        app.new_branch_base = "base".to_string();
        app.merge_branch_name = "merge".to_string();
        app.rename_branch_old = "old".to_string();
        app.rename_branch_new = "new".to_string();
        app.new_worktree_name = "worktree".to_string();
        app.new_worktree_path = "/tmp/worktree".to_string();
        app.new_worktree_branch = "branch".to_string();
        app.new_worktree_create_branch = true;
        app.stash_message = "stash".to_string();

        FormSubmission::Commit {
            message: "commit".to_string(),
            amend: true,
        }
        .clear_if_unchanged(&mut app);
        FormSubmission::CreateBranch {
            name: "branch".to_string(),
            base: "base".to_string(),
        }
        .clear_if_unchanged(&mut app);
        FormSubmission::MergeBranch {
            name: "merge".to_string(),
        }
        .clear_if_unchanged(&mut app);
        FormSubmission::RenameBranch {
            old: "old".to_string(),
            new: "new".to_string(),
        }
        .clear_if_unchanged(&mut app);
        FormSubmission::CreateWorktree {
            name: "worktree".to_string(),
            path: "/tmp/worktree".to_string(),
            branch: "branch".to_string(),
            create_branch: true,
        }
        .clear_if_unchanged(&mut app);
        FormSubmission::Stash {
            message: "stash".to_string(),
        }
        .clear_if_unchanged(&mut app);

        assert!(app.commit_msg.is_empty());
        assert!(!app.commit_amend);
        assert!(app.new_branch_name.is_empty());
        assert!(app.new_branch_base.is_empty());
        assert!(app.merge_branch_name.is_empty());
        assert!(app.rename_branch_old.is_empty());
        assert!(app.rename_branch_new.is_empty());
        assert!(app.new_worktree_name.is_empty());
        assert!(app.new_worktree_path.is_empty());
        assert!(app.new_worktree_branch.is_empty());
        assert!(!app.new_worktree_create_branch);
        assert!(app.stash_message.is_empty());
    }

    fn init_repo_with_branch(path: &std::path::Path, branch_name: &str) {
        let repo = git2::Repository::init(path).expect("initialize repository");
        let signature = git2::Signature::now("test", "test@example.com").expect("signature");
        let tree_oid = {
            let mut index = repo.index().expect("index");
            index.write_tree().expect("write tree")
        };
        let tree = repo.find_tree(tree_oid).expect("tree");
        let commit_oid = repo
            .commit(Some("HEAD"), &signature, &signature, "initial", &tree, &[])
            .expect("initial commit");
        let commit = repo.find_commit(commit_oid).expect("commit");
        repo.branch(branch_name, &commit, false)
            .expect("create test branch");
    }

    fn create_linked_test_worktree(
        main_path: &std::path::Path,
        worktree_path: &std::path::Path,
        branch_name: &str,
    ) {
        let repo = git2::Repository::open(main_path).expect("open main repository");
        let branch = repo
            .find_branch(branch_name, git2::BranchType::Local)
            .expect("find test branch");
        let reference = repo
            .find_reference(&format!("refs/heads/{}", branch_name))
            .expect("find branch reference");
        let name = worktree_path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("worktree name");
        let mut options = git2::WorktreeAddOptions::new();
        options.reference(Some(&reference));
        repo.worktree(name, worktree_path, Some(&options))
            .expect("create linked worktree");
        drop(reference);
        drop(branch);
        drop(repo);

        git2::Repository::open(worktree_path)
            .expect("open linked worktree")
            .set_head(&format!("refs/heads/{}", branch_name))
            .expect("checkout linked branch");
    }

    fn setup_linked_worktree() -> (
        tempfile::TempDir,
        tempfile::TempDir,
        std::path::PathBuf,
    ) {
        let main_dir = tempfile::tempdir().expect("main repository directory");
        init_repo_with_branch(main_dir.path(), "feature");
        let worktree_root = tempfile::tempdir().expect("worktree root");
        let worktree_path = worktree_root.path().join("linked-wt");
        create_linked_test_worktree(main_dir.path(), &worktree_path, "feature");
        (main_dir, worktree_root, worktree_path)
    }

    fn setup_dirty_linked_worktree() -> (
        tempfile::TempDir,
        tempfile::TempDir,
        std::path::PathBuf,
    ) {
        let (main_dir, worktree_root, worktree_path) = setup_linked_worktree();
        std::fs::write(worktree_path.join("dirty.txt"), "uncommitted data\n")
            .expect("write untracked file");
        (main_dir, worktree_root, worktree_path)
    }

    fn start_cleanup_preview(
        app: &mut App,
        ctx: &egui::Context,
        path: &std::path::Path,
        force_requested: bool,
    ) {
        app.worktrees = app.git.worktrees().expect("list worktrees");
        let worktree = app
            .worktrees
            .iter()
            .find(|worktree| {
                !worktree.is_main && worktree.path.file_name() == path.file_name()
            })
            .cloned()
            .expect("listed worktree");
        app.preview_worktree_cleanup(ctx, &worktree, force_requested);
    }

    fn wait_for_cleanup_preview(app: &mut App, ctx: &egui::Context) {
        for _ in 0..200 {
            app.process_pending_ops(ctx);
            if app.pending_ops.is_empty() && app.pending_worktree_cleanup.is_none() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("worktree cleanup preview should finish");
    }

    #[test]
    fn cleanup_preview_rejects_worktree_identity_changed_after_listing() {
        let (main_dir, _worktree_root, _worktree_path) = setup_linked_worktree();
        let repo = git2::Repository::open(main_dir.path()).expect("open main repository");
        let commit = repo
            .head()
            .expect("main HEAD")
            .peel_to_commit()
            .expect("main commit");
        repo.branch("other", &commit, false)
            .expect("create comparison branch");
        drop(commit);
        drop(repo);

        let other_root = tempfile::tempdir().expect("comparison worktree root");
        let other_path = other_root.path().join("other-wt");
        create_linked_test_worktree(main_dir.path(), &other_path, "other");

        let mut app = App::new();
        app.git.open(main_dir.path()).expect("open main repository");
        let worktrees = app.git.worktrees().expect("list worktrees");
        let target = worktrees
            .iter()
            .find(|worktree| !worktree.is_main && worktree.branch.as_deref() == Some("feature"))
            .expect("target worktree");
        let expected_git_link = worktrees
            .iter()
            .find(|worktree| worktree.branch.as_deref() == Some("other"))
            .and_then(|worktree| worktree.git_link_identity.clone())
            .expect("capture a different worktree identity");

        app.worktrees = vec![target.clone()];
        app.pending_worktree_cleanup = Some(PendingWorktreeCleanup {
            path: target.path.clone(),
            expected_git_link: Some(expected_git_link),
            directory_missing_at_request: false,
            force_requested: false,
            repo_generation: app.repo_generation,
            ready: true,
        });
        app.finish_worktree_cleanup_preview(&egui::Context::default());

        assert!(app.pending_confirmation.is_none());
        assert!(app.status_is_error);
        assert!(app.status_message.contains("identity changed"));
    }

    #[test]
    fn cleanup_confirmation_rejects_a_new_detached_head_commit() {
        let (main_dir, _worktree_root, worktree_path) = setup_linked_worktree();
        let worktree_repo = git2::Repository::open(&worktree_path).expect("open linked worktree");
        let original_head = worktree_repo
            .head()
            .expect("worktree HEAD")
            .target()
            .expect("worktree commit");
        worktree_repo
            .set_head_detached(original_head)
            .expect("detach worktree HEAD");
        drop(worktree_repo);

        let mut app = App::new();
        app.git.open(main_dir.path()).expect("open main repository");
        let ctx = egui::Context::default();
        start_cleanup_preview(&mut app, &ctx, &worktree_path, false);
        wait_for_cleanup_preview(&mut app, &ctx);
        assert!(app.pending_confirmation.is_some());

        let worktree_repo = git2::Repository::open(&worktree_path).expect("reopen linked worktree");
        let parent = worktree_repo
            .head()
            .expect("detached HEAD")
            .peel_to_commit()
            .expect("parent commit");
        let tree = parent.tree().expect("parent tree");
        let signature = worktree_repo.signature().expect("signature");
        let new_head = worktree_repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "external detached commit",
                &tree,
                &[&parent],
            )
            .expect("advance detached HEAD");
        drop(tree);
        drop(parent);
        drop(signature);
        drop(worktree_repo);

        app.confirm_pending_operation(&ctx);
        wait_for_background_operations(&mut app, &ctx);

        assert!(worktree_path.exists());
        assert!(app.status_is_error);
        assert!(app.status_message.contains("HEAD changed"));
        let refreshed = app.git.worktrees().expect("list worktrees after rejection");
        let worktree = refreshed
            .iter()
            .find(|worktree| !worktree.is_main)
            .expect("linked worktree remains registered");
        assert_eq!(worktree.sha, new_head.to_string());
        assert!(worktree.branch.is_none());
    }

    #[test]
    fn cleanup_confirmation_rejects_attached_branch_advance() {
        let (main_dir, _worktree_root, worktree_path) = setup_linked_worktree();
        let mut app = App::new();
        app.git.open(main_dir.path()).expect("open main repository");
        let ctx = egui::Context::default();
        start_cleanup_preview(&mut app, &ctx, &worktree_path, false);
        wait_for_cleanup_preview(&mut app, &ctx);

        let worktree_repo = git2::Repository::open(&worktree_path).expect("open linked worktree");
        let parent = worktree_repo
            .head()
            .expect("attached HEAD")
            .peel_to_commit()
            .expect("parent commit");
        let tree = parent.tree().expect("parent tree");
        let signature = worktree_repo.signature().expect("signature");
        let new_head = worktree_repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "external attached commit",
                &tree,
                &[&parent],
            )
            .expect("advance attached branch");
        drop(tree);
        drop(parent);
        drop(signature);
        drop(worktree_repo);

        app.confirm_pending_operation(&ctx);
        wait_for_background_operations(&mut app, &ctx);

        assert!(worktree_path.exists());
        assert!(app.status_is_error);
        assert!(app.status_message.contains("HEAD changed"));
        let refreshed = app.git.worktrees().expect("list worktrees after rejection");
        let worktree = refreshed
            .iter()
            .find(|worktree| !worktree.is_main)
            .expect("linked worktree remains registered");
        assert_eq!(worktree.sha, new_head.to_string());
        assert_eq!(worktree.branch.as_deref(), Some("feature"));
    }

    #[test]
    fn cleanup_preview_fails_closed_when_worktree_status_is_unknown() {
        let mut app = App::new();
        let path = std::path::PathBuf::from("/tmp/uninspectable-worktree");
        app.worktrees = vec![WorktreeInfo {
            path: path.clone(),
            branch: Some("feature".into()),
            sha: "1234567890".into(),
            head_snapshot: None,
            is_main: false,
            git_link_identity: None,
            status: WorktreeStatusSummary {
                inspection_error: Some("permission denied".into()),
                ..WorktreeStatusSummary::default()
            },
        }];
        app.pending_worktree_cleanup = Some(PendingWorktreeCleanup {
            path,
            expected_git_link: None,
            directory_missing_at_request: false,
            force_requested: false,
            repo_generation: app.repo_generation,
            ready: true,
        });

        app.finish_worktree_cleanup_preview(&egui::Context::default());

        assert!(app.pending_confirmation.is_none());
        assert!(app.status_is_error);
        assert!(app.status_message.contains("permission denied"));
    }

    #[test]
    fn cleanup_preview_can_prune_missing_directory_without_losing_branch() {
        let (main_dir, _worktree_root, worktree_path) = setup_linked_worktree();
        let mut app = App::new();
        app.git.open(main_dir.path()).expect("open main repository");
        std::fs::remove_dir_all(&worktree_path).expect("remove linked worktree directory");
        let ctx = egui::Context::default();

        start_cleanup_preview(&mut app, &ctx, &worktree_path, false);
        wait_for_cleanup_preview(&mut app, &ctx);
        let confirmation = app
            .pending_confirmation
            .as_ref()
            .expect("missing-directory cleanup preview");
        assert!(confirmation.message.contains("directory is already missing"));
        assert!(confirmation.message.contains("Branch: feature"));
        assert!(confirmation.message.contains("HEAD commit:"));
        assert!(confirmation.message.contains("branch ref is kept"));

        app.confirm_pending_operation(&ctx);
        wait_for_background_operations(&mut app, &ctx);

        let repo = git2::Repository::open(main_dir.path()).expect("reopen main repository");
        assert!(repo.find_branch("feature", git2::BranchType::Local).is_ok());
        assert!(repo.worktrees().expect("list worktrees").is_empty());
    }

    #[test]
    fn missing_detached_preview_does_not_warn_for_a_merged_commit() {
        let (main_dir, _worktree_root, worktree_path) = setup_linked_worktree();
        let worktree_repo = git2::Repository::open(&worktree_path).expect("open linked worktree");
        let head = worktree_repo
            .head()
            .expect("worktree HEAD")
            .target()
            .expect("worktree commit");
        worktree_repo
            .set_head_detached(head)
            .expect("detach worktree HEAD");
        drop(worktree_repo);

        let mut app = App::new();
        app.git.open(main_dir.path()).expect("open main repository");
        std::fs::remove_dir_all(&worktree_path).expect("remove linked worktree directory");
        let ctx = egui::Context::default();
        start_cleanup_preview(&mut app, &ctx, &worktree_path, false);
        wait_for_cleanup_preview(&mut app, &ctx);

        let confirmation = app
            .pending_confirmation
            .as_ref()
            .expect("missing detached cleanup preview");
        assert!(confirmation
            .message
            .contains("this commit is already reachable from the main worktree"));
        assert!(confirmation
            .message
            .contains("Commits are merged into the main worktree HEAD"));
        assert!(!confirmation.message.contains("may become unreachable"));
        app.cancel_pending_confirmation();
    }

    #[test]
    fn cleanup_preview_lists_dirty_path_and_cancel_preserves_worktree() {
        let (main_dir, _worktree_root, worktree_path) = setup_dirty_linked_worktree();
        std::fs::create_dir_all(worktree_path.join("nested"))
            .expect("create nested untracked directory");
        std::fs::write(worktree_path.join("nested/deep.txt"), "nested untracked data\n")
            .expect("write nested untracked file");
        let mut app = App::new();
        app.git.open(main_dir.path()).expect("open main repository");
        let ctx = egui::Context::default();

        start_cleanup_preview(&mut app, &ctx, &worktree_path, false);
        wait_for_cleanup_preview(&mut app, &ctx);

        let confirmation = app
            .pending_confirmation
            .as_ref()
            .expect("cleanup preview confirmation");
        assert_eq!(confirmation.confirm_label, "Force Remove worktree");
        assert!(confirmation.message.contains("dirty.txt"));
        assert!(confirmation.message.contains("deep.txt"));
        assert!(confirmation.message.contains("untracked"));
        assert!(confirmation.message.contains("committed history remain"));
        assert!(confirmation.message.contains("Changes made after this preview"));

        app.cancel_pending_confirmation();
        assert!(!app.is_busy());
        assert!(worktree_path.exists());
        assert!(worktree_path.join("dirty.txt").exists());
    }

    #[test]
    fn normal_cleanup_preview_exposes_force_for_locked_worktree() {
        let (main_dir, _worktree_root, worktree_path) = setup_linked_worktree();
        let main_repo = git2::Repository::open(main_dir.path()).expect("open main repository");
        let lock_path = main_repo.path().join("worktrees").join("linked-wt").join("locked");
        std::fs::write(lock_path, "kept by test").expect("lock linked worktree");
        drop(main_repo);

        let mut app = App::new();
        app.git.open(main_dir.path()).expect("open main repository");
        let ctx = egui::Context::default();

        start_cleanup_preview(&mut app, &ctx, &worktree_path, false);
        wait_for_cleanup_preview(&mut app, &ctx);

        let confirmation = app
            .pending_confirmation
            .as_ref()
            .expect("locked cleanup preview confirmation");
        assert_eq!(confirmation.confirm_label, "Force Remove worktree");
        assert!(confirmation.message.contains("Worktree is locked: kept by test"));
        assert!(worktree_path.exists());
        app.cancel_pending_confirmation();
        assert!(worktree_path.exists());
    }

    #[test]
    fn confirming_cleanup_preview_removes_worktree_but_keeps_branch() {
        let (main_dir, _worktree_root, worktree_path) = setup_dirty_linked_worktree();
        let mut app = App::new();
        app.git.open(main_dir.path()).expect("open main repository");
        let ctx = egui::Context::default();

        start_cleanup_preview(&mut app, &ctx, &worktree_path, false);
        wait_for_cleanup_preview(&mut app, &ctx);
        app.confirm_pending_operation(&ctx);
        wait_for_background_operations(&mut app, &ctx);

        assert!(!worktree_path.exists());
        let repo = git2::Repository::open(main_dir.path()).expect("reopen main repository");
        assert!(repo
            .find_branch("feature", git2::BranchType::Local)
            .is_ok());
        assert!(!repo
            .worktrees()
            .expect("list remaining worktrees")
            .iter()
            .flatten()
            .any(|name| name == "linked-wt"));
    }

    #[test]
    fn cancelling_destructive_confirmation_keeps_target_and_does_not_dispatch() {
        let repo_dir = tempfile::tempdir().expect("repository directory");
        init_repo_with_branch(repo_dir.path(), "feature");
        let mut app = App::new();
        app.git.open(repo_dir.path()).expect("open repository");
        let ctx = egui::Context::default();

        app.request_confirmation(
            &ctx,
            "Confirm branch deletion",
            "Delete local branch 'feature'?".to_string(),
            "Delete branch",
            "Delete branch 'feature'",
            GitOperation::DeleteBranch {
                name: "feature".to_string(),
                force: false,
            },
        );
        assert!(app.pending_ops.is_empty());
        assert_eq!(
            app.current_operation(),
            "Awaiting confirmation: Delete branch 'feature'"
        );
        app.start_operation(&ctx, "Unconfirmed operation", GitOperation::StageAll);
        assert!(app.pending_ops.is_empty());

        app.cancel_pending_confirmation();

        assert!(app.pending_confirmation.is_none());
        assert!(app.pending_ops.is_empty());
        let repo = git2::Repository::open(repo_dir.path()).expect("reopen repository");
        assert!(repo
            .find_branch("feature", git2::BranchType::Local)
            .is_ok());
    }

    #[test]
    fn confirming_destructive_operation_dispatches_it() {
        let repo_dir = tempfile::tempdir().expect("repository directory");
        init_repo_with_branch(repo_dir.path(), "feature");
        let mut app = App::new();
        app.git.open(repo_dir.path()).expect("open repository");
        let ctx = egui::Context::default();

        app.request_confirmation(
            &ctx,
            "Confirm branch deletion",
            "Delete local branch 'feature'?".to_string(),
            "Delete branch",
            "Delete branch 'feature'",
            GitOperation::DeleteBranch {
                name: "feature".to_string(),
                force: false,
            },
        );
        assert!(app.pending_ops.is_empty());

        app.confirm_pending_operation(&ctx);

        assert!(app.pending_confirmation.is_none());
        assert_eq!(app.pending_ops.len(), 1);
        app.pending_ops[0]
            .receiver
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("confirmed operation should complete");
        let repo = git2::Repository::open(repo_dir.path()).expect("reopen repository");
        assert!(repo
            .find_branch("feature", git2::BranchType::Local)
            .is_err());
    }

    #[test]
    fn test_stale_search_results_do_not_replace_current_log() {
        let mut app = App::new();
        app.log_search = "alice".to_string();
        app.log_search_request_id = 1;
        app.commits = vec![test_commit("alice changed the parser", "alice")];

        app.handle_op_result(
            "Searching commits".to_string(),
            OpResult::SearchResults {
                request_id: 0,
                filter: "bob".to_string(),
                commits: vec![test_commit("bob changed the parser", "bob")],
            },
        );

        assert_eq!(app.commits.len(), 1);
        assert_eq!(app.commits[0].author, "alice");
    }

    #[test]
    fn test_refresh_preserves_active_log_filter() {
        let mut app = App::new();
        app.log_search = "alice".to_string();

        app.handle_op_result(
            "Refreshing".to_string(),
            OpResult::RefreshData {
                status_entries: Vec::new(),
                branches: Vec::new(),
                worktrees: Vec::new(),
                commits: vec![
                    test_commit("alice changed the parser", "alice"),
                    test_commit("unrelated change", "bob"),
                ],
                stashes: Vec::new(),
                remote_list: Vec::new(),
                errors: Vec::new(),
            },
        );

        assert_eq!(app.commits.len(), 1);
        assert_eq!(app.commits[0].author, "alice");
    }

    #[test]
    fn test_stale_refresh_result_does_not_replace_data_after_repo_switch() {
        fn init_repo(path: &std::path::Path, message: &str) {
            let repo = git2::Repository::init(path).expect("init repo");
            let signature = git2::Signature::now("test", "test@example.com").expect("signature");
            let tree_oid = {
                let mut index = repo.index().expect("index");
                index.write_tree().expect("write tree")
            };
            let tree = repo.find_tree(tree_oid).expect("tree");
            repo.commit(Some("HEAD"), &signature, &signature, message, &tree, &[])
                .expect("commit");
        }

        let old_repo = tempfile::tempdir().expect("old repo dir");
        let new_repo = tempfile::tempdir().expect("new repo dir");
        init_repo(old_repo.path(), "old repository commit");
        init_repo(new_repo.path(), "new repository commit");

        let recent_file = tempfile::NamedTempFile::new().expect("recent repos file");
        let mut app = App::new();
        app.recent_repos = RecentRepos::load_from(recent_file.path().to_path_buf());
        app.open_repo(old_repo.path().to_str().expect("old repo path"));

        let old_generation = app.repo_generation;
        app.open_repo(new_repo.path().to_str().expect("new repo path"));

        let (tx, rx) = mpsc::channel();
        tx.send(OpResult::RefreshData {
            status_entries: Vec::new(),
            branches: Vec::new(),
            worktrees: Vec::new(),
            commits: vec![test_commit("stale old repository data", "old")],
            stashes: Vec::new(),
            remote_list: Vec::new(),
            errors: Vec::new(),
        })
        .expect("send stale refresh result");
        app.pending_ops.push(PendingOp {
            description: "Refreshing".to_string(),
            receiver: rx,
            repo_generation: old_generation,
            started_at: Instant::now(),
            progress: Arc::new(Mutex::new(String::new())),
            last_progress_update: Instant::now(),
            last_seen_progress: String::new(),
            timed_out: false,
            task_diff_identity: None,
            form_submission: None,

        });

        let ctx = egui::Context::default();
        app.process_pending_ops(&ctx);
        wait_for_background_operations(&mut app, &ctx);

        assert_eq!(app.repo_path, new_repo.path().to_string_lossy());
        assert_eq!(app.commits.len(), 1);
        assert_eq!(app.commits[0].summary, "new repository commit");
    }

    #[test]
    fn test_open_repo_clears_diff_from_previous_repository() {
        let first_repo = tempfile::tempdir().expect("first repo dir");
        let second_repo = tempfile::tempdir().expect("second repo dir");
        git2::Repository::init(first_repo.path()).expect("init first repo");
        git2::Repository::init(second_repo.path()).expect("init second repo");

        let recent_file = tempfile::NamedTempFile::new().expect("recent repos file");
        let mut app = App::new();
        app.recent_repos = RecentRepos::load_from(recent_file.path().to_path_buf());
        app.open_repo(first_repo.path().to_str().expect("first repo path"));

        app.handle_op_result(
            "Show diff".to_string(),
            OpResult::DiffContent {
                path: "repo-a.txt".to_string(),
                lines: vec![DiffLine {
                    origin: '+',
                    content: "repo A content".to_string(),
                }],
            },
        );
        assert!(app.show_diff);
        assert_eq!(app.diff_path, "repo-a.txt");
        assert_eq!(app.diff_content.len(), 1);

        app.open_repo(second_repo.path().to_str().expect("second repo path"));

        assert!(app.diff_content.is_empty());
        assert!(app.diff_path.is_empty());
        assert!(!app.show_diff);
    }

    #[test]
    fn refresh_all_returns_while_git_worker_is_blocked() {
        let dir = tempfile::tempdir().expect("temp dir");
        drop(git2::Repository::init(dir.path()).expect("init repo"));
        let mut app = App::new();
        app.git.open(dir.path()).expect("open repo");
        let ctx = egui::Context::default();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = Arc::new(Mutex::new(release_rx));
        app.test_before_operation = Some(Arc::new(move || {
            let _ = started_tx.send(());
            let _ = release_rx
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(2));
        }));

        app.refresh_all(&ctx);

        let worker_started = started_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .is_ok();
        let refresh_is_pending = app.is_busy();
        let _ = release_tx.send(());
        assert!(worker_started, "refresh Git work should start on a worker thread");
        assert!(refresh_is_pending, "the UI should retain a pending refresh");
        app.test_before_operation = None;
        wait_for_background_operations(&mut app, &ctx);
    }

    #[test]
    fn test_refresh_all_preserves_active_log_filter() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = git2::Repository::init(dir.path()).expect("init repo");
        let alice = git2::Signature::now("alice", "alice@example.com").expect("signature");
        let bob = git2::Signature::now("bob", "bob@example.com").expect("signature");
        let tree_oid = {
            let mut index = repo.index().expect("index");
            index.write_tree().expect("write tree")
        };
        let tree = repo.find_tree(tree_oid).expect("tree");
        let first_oid = repo
            .commit(Some("HEAD"), &alice, &alice, "alice change", &tree, &[])
            .expect("alice commit");
        let first = repo.find_commit(first_oid).expect("first commit");
        repo.commit(Some("HEAD"), &bob, &bob, "bob change", &tree, &[&first])
            .expect("bob commit");
        drop(first);
        drop(tree);
        drop(repo);

        let mut app = App::new();
        app.git.open(dir.path()).expect("open repo");
        app.log_search = "alice".to_string();
        let ctx = egui::Context::default();
        app.refresh_all(&ctx);
        wait_for_background_operations(&mut app, &ctx);

        assert_eq!(app.commits.len(), 1);
        assert_eq!(app.commits[0].author, "alice");
    }

    #[test]
    fn test_repeated_search_query_rejects_stale_response() {
        let mut app = App::new();
        let ctx = egui::Context::default();

        app.log_search = "alice".to_string();
        let filter = app.log_search.clone();
        app.start_log_search(&ctx, filter);
        let first_a_request_id = app.log_search_request_id;
        app.log_search = "bob".to_string();
        let filter = app.log_search.clone();
        app.start_log_search(&ctx, filter);
        app.log_search = "alice".to_string();
        let filter = app.log_search.clone();
        app.start_log_search(&ctx, filter);
        let second_a_request_id = app.log_search_request_id;
        app.commits = vec![test_commit("latest alice result", "alice")];

        app.handle_op_result(
            "Searching commits".to_string(),
            OpResult::SearchResults {
                request_id: first_a_request_id,
                filter: "alice".to_string(),
                commits: vec![test_commit("stale alice result", "alice")],
            },
        );

        assert_eq!(app.commits[0].message, "latest alice result");

        app.handle_op_result(
            "Searching commits".to_string(),
            OpResult::SearchResults {
                request_id: second_a_request_id,
                filter: "alice".to_string(),
                commits: vec![test_commit("current alice result", "alice")],
            },
        );

        assert_eq!(app.commits[0].message, "current alice result");
    }

    #[test]
    fn test_status_expanded_defaults_to_false() {
        let app = App::new();
        assert!(!app.status_expanded);
    }

    #[test]
    fn test_current_operation_empty_when_not_busy() {
        let app = App::new();
        assert_eq!(app.current_operation(), "");
    }

    #[test]
    fn test_open_repo_clears_status_message() {
        let repo_dir = tempfile::tempdir().expect("repo dir");
        let repo = git2::Repository::init(repo_dir.path()).expect("init repo");
        let signature = git2::Signature::now("test", "test@example.com").expect("signature");
        let tree_oid = {
            let mut index = repo.index().expect("index");
            index.write_tree().expect("write tree")
        };
        let tree = repo.find_tree(tree_oid).expect("tree");
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "initial commit",
            &tree,
            &[],
        )
        .expect("commit");
        drop(tree);
        drop(repo);

        let recent_repos_dir = tempfile::tempdir().expect("recent repos dir");
        let repo_path = repo_dir.path().to_str().expect("repo path");

        let mut app = App::new();
        app.recent_repos = RecentRepos::load_from(recent_repos_dir.path().join("recent.json"));
        app.status_message = "old message".into();
        app.status_is_error = true;

        app.open_repo(repo_path);

        assert_eq!(app.repo_path, repo_path);
        assert!(app.status_message.is_empty());
        assert!(!app.status_is_error);
    }

    #[test]
    fn test_open_repo_is_ignored_while_operation_is_pending() {
        let first_repo = tempfile::tempdir().unwrap();
        let second_repo = tempfile::tempdir().unwrap();
        git2::Repository::init(first_repo.path()).unwrap();
        git2::Repository::init(second_repo.path()).unwrap();

        let mut app = App::new();
        let recent_repos_dir = tempfile::tempdir().unwrap();
        app.recent_repos = RecentRepos::load_from(recent_repos_dir.path().join("recent.json"));
        let first_path = first_repo.path().to_string_lossy().to_string();
        let second_path = second_repo.path().to_string_lossy().to_string();
        app.open_repo(&first_path);

        let (_tx, receiver) = mpsc::channel::<OpResult>();
        app.pending_ops.push(PendingOp {
            description: "Fetching".to_string(),
            receiver,
            repo_generation: app.repo_generation,
            started_at: Instant::now(),
            progress: Arc::new(Mutex::new(String::new())),
            last_progress_update: Instant::now(),
            last_seen_progress: String::new(),
            timed_out: false,
            task_diff_identity: None,
            form_submission: None,

        });

        app.open_repo(&second_path);

        assert_eq!(app.repo_path, first_path);
        assert_eq!(app.git.path().unwrap(), first_repo.path());
    }

    #[cfg(unix)]
    #[test]
    fn test_update_script_launch_failure_is_reported() {
        let mut app = App::new();

        assert!(!app.try_launch_update_script(Path::new(
            "/path/that/does/not/exist/update_git_manager.sh",
        )));
        assert!(app.status_is_error);
        assert!(app.status_message.contains("Failed to launch update script"));
    }

    // --- Legacy tests (unchanged) ---

    #[test]
    fn test_font_size_constant_is_14() {
        assert_eq!(App::FONT_SIZE, 14.0);
    }

    #[test]
    fn test_app_new_defaults() {
        let app = App::new();
        assert!(!app.show_about);
        assert!(!app.git.is_open());
        assert_eq!(app.current_tab, Tab::Worktrees);
        assert!(!app.show_clone_dialog);
    }

    #[test]
    fn welcome_clone_button_opens_clone_dialog_without_a_repository() {
        let mut app = App::new();
        let recent_dir = tempfile::tempdir().expect("recent directory");
        app.recent_repos = RecentRepos::load_from(recent_dir.path().join("recent.json"));
        let ctx = egui::Context::default();
        let screen_rect = egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(800.0, 600.0),
        );
        let mut clone_button_rect = egui::Rect::NOTHING;

        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    clone_button_rect = app.show_welcome_screen(ui).rect;
                });
            },
        );

        let position = clone_button_rect.center();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                events: vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::default(),
                    },
                ],
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.show_welcome_screen(ui);
                });
            },
        );
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                events: vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::default(),
                    },
                ],
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.show_welcome_screen(ui);
                });
            },
        );

        assert!(!app.git.is_open());
        assert!(app.show_clone_dialog);
    }

    #[test]
    fn welcome_screen_displays_recent_history_save_error() {
        let recent_dir = tempfile::tempdir().expect("recent directory");
        let history_path = recent_dir.path().join("recent.json");
        let mut app = App::new();
        app.recent_repos = RecentRepos::load_from(history_path.clone());
        for index in 0..20 {
            app.recent_repos
                .add(&format!("repository-{index}"))
                .expect("save initial history");
        }

        std::fs::remove_file(&history_path).expect("remove history file");
        std::fs::create_dir(&history_path).expect("replace history file with directory");
        app.remove_recent_repo(0);

        assert!(app.status_is_error);
        assert!(app.status_message.contains("Failed to save recent history"));

        let ctx = egui::Context::default();
        let screen_rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(600.0, 400.0));
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.show_welcome_screen(ui);
                });
            },
        );

        assert!(output.shapes.iter().any(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.job.text == app.status_message => {
                clipped.clip_rect.contains_rect(text.visual_bounding_rect())
            }
            _ => false,
        }));
    }

    fn run_clone_dialog_frame(
        app: &mut App,
        ctx: &egui::Context,
        screen_rect: egui::Rect,
        events: Vec<egui::Event>,
    ) -> (egui::Rect, bool) {
        let mut clone_button_rect = egui::Rect::NOTHING;
        let mut clone_button_clicked = false;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |_ui| {});
                let response = app.render_clone_dialog(ctx).expect("clone dialog is open");
                clone_button_rect = response.rect;
                clone_button_clicked = response.clicked();
            },
        );
        (clone_button_rect, clone_button_clicked)
    }

    #[test]
    fn clone_dialog_button_opens_repository_after_success() {
        let source_dir = tempfile::tempdir().expect("source directory");
        let source_repo = git2::Repository::init(source_dir.path()).expect("initialize source");
        let signature = git2::Signature::now("test", "test@example.com").expect("signature");
        let tree_oid = {
            let mut index = source_repo.index().expect("source index");
            index.write_tree().expect("write source tree")
        };
        let tree = source_repo.find_tree(tree_oid).expect("source tree");
        source_repo
            .commit(Some("HEAD"), &signature, &signature, "initial", &tree, &[])
            .expect("create source commit");
        drop(tree);
        drop(source_repo);

        let destination_parent = tempfile::tempdir().expect("destination parent");
        let destination = destination_parent.path().join("cloned");
        let recent_dir = tempfile::tempdir().expect("recent directory");
        let mut app = App::new();
        app.recent_repos = RecentRepos::load_from(recent_dir.path().join("recent.json"));
        app.show_clone_dialog = true;
        app.clone_url = source_dir.path().to_string_lossy().into_owned();
        app.clone_destination = destination.to_string_lossy().into_owned();
        let ctx = egui::Context::default();
        let screen_rect = egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(800.0, 600.0),
        );
        run_clone_dialog_frame(&mut app, &ctx, screen_rect, Vec::new());
        let (clone_rect, _) = run_clone_dialog_frame(&mut app, &ctx, screen_rect, Vec::new());
        let position = clone_rect.center();
        let press = egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        };
        run_clone_dialog_frame(
            &mut app,
            &ctx,
            screen_rect,
            vec![egui::Event::PointerMoved(position), press],
        );
        let release = egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::default(),
        };
        let (_, clone_clicked) = run_clone_dialog_frame(
            &mut app,
            &ctx,
            screen_rect,
            vec![egui::Event::PointerMoved(position), release],
        );
        assert!(clone_clicked, "clone dialog button should report a click");
        assert!(app.is_busy(), "clone button should start the operation");

        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        while app.is_busy() && Instant::now() < deadline {
            app.process_pending_ops(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        assert!(!app.is_busy(), "clone operation should complete");
        assert!(app.git.is_open());
        assert_eq!(app.git.path(), Some(destination.as_path()));
        assert_eq!(app.current_tab, Tab::Worktrees);
        assert!(!app.show_clone_dialog);
        assert!(!app.status_is_error);
    }

    #[test]
    fn clone_success_preserves_recent_history_error() {
        let clone_dir = tempfile::tempdir().expect("clone directory");
        drop(git2::Repository::init(clone_dir.path()).expect("initialize cloned repository"));
        let recent_dir = tempfile::tempdir().expect("recent directory");
        let recent_path = recent_dir.path().join("invalid-history");
        std::fs::create_dir(&recent_path).expect("make history path a directory");
        let mut app = App::new();
        app.recent_repos = RecentRepos::load_from(recent_path);

        app.handle_op_result(
            "Cloning repository".to_string(),
            OpResult::CloneSuccess(clone_dir.path().to_path_buf()),
        );

        assert!(app.git.is_open());
        assert!(app.status_is_error);
        assert!(app.status_message.contains("failed to save recent history"));
    }

    #[test]
    fn clone_credentials_support_username_only_transports() {
        let credential = clone_credential_with_config(
            None,
            "ssh://git.example.com/repo.git",
            Some("test-user"),
            git2::CredentialType::USERNAME,
        )
        .expect("username credential");

        assert!(credential.has_username());
    }

    #[cfg(unix)]
    #[test]
    fn clone_credentials_use_configured_git_helper() {
        let temp_dir = tempfile::tempdir().expect("credential helper directory");
        let credentials_path = temp_dir.path().join("credentials");
        let config_path = temp_dir.path().join("config");
        std::fs::write(
            &credentials_path,
            "https://test-user:test-password@git.example.com\n",
        )
        .expect("write test credentials");
        std::fs::write(&config_path, "").expect("create credential config");
        let mut config = git2::Config::open(&config_path).expect("credential config");
        config
            .set_str(
                "credential.helper",
                &format!("store --file={}", credentials_path.display()),
            )
            .expect("configure credential helper");

        let credential = clone_credential_with_config(
            Some(&config),
            "https://git.example.com/repo.git",
            None,
            git2::CredentialType::USER_PASS_PLAINTEXT,
        )
        .expect("credential helper result");

        assert!(credential.has_username());
    }

    #[test]
    fn failed_clone_can_be_retried_from_the_dialog() {
        let source_dir = tempfile::tempdir().expect("source directory");
        let source_repo = git2::Repository::init(source_dir.path()).expect("initialize source");
        let signature = git2::Signature::now("test", "test@example.com").expect("signature");
        let tree_oid = {
            let mut index = source_repo.index().expect("source index");
            index.write_tree().expect("write source tree")
        };
        let tree = source_repo.find_tree(tree_oid).expect("source tree");
        source_repo
            .commit(Some("HEAD"), &signature, &signature, "initial", &tree, &[])
            .expect("create source commit");
        drop(tree);
        drop(source_repo);

        let temp_dir = tempfile::tempdir().expect("clone parent");
        let recent_dir = tempfile::tempdir().expect("recent directory");
        let mut app = App::new();
        app.recent_repos = RecentRepos::load_from(recent_dir.path().join("recent.json"));
        app.show_clone_dialog = true;
        let ctx = egui::Context::default();
        app.start_clone(
            &ctx,
            temp_dir.path().join("missing-source").to_string_lossy().into_owned(),
            temp_dir.path().join("failed-clone").to_string_lossy().into_owned(),
        );
        let screen_rect = egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(800.0, 600.0),
        );
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                ..Default::default()
            },
            |ctx| {
                let _ = app.render_clone_dialog(ctx);
            },
        );
        assert!(app.show_clone_dialog, "dialog should stay open while cloning");

        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        while app.is_busy() && Instant::now() < deadline {
            app.process_pending_ops(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!app.is_busy(), "failed clone should finish");
        assert!(app.status_is_error);
        assert!(app.show_clone_dialog);
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                ..Default::default()
            },
            |ctx| {
                let _ = app.render_clone_dialog(ctx);
            },
        );
        assert!(output.shapes.iter().any(|clipped| match &clipped.shape {
            egui::Shape::Text(text) => text.galley.job.text == app.status_message,
            _ => false,
        }));

        app.start_clone(
            &ctx,
            source_dir.path().to_string_lossy().into_owned(),
            temp_dir.path().join("successful-clone").to_string_lossy().into_owned(),
        );
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        while app.is_busy() && Instant::now() < deadline {
            app.process_pending_ops(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        assert!(!app.is_busy(), "retry should finish");
        assert!(app.git.is_open());
        assert!(!app.status_is_error);
        assert!(!app.show_clone_dialog);
    }

    #[test]
    fn late_clone_success_after_timeout_opens_repository() {
        let clone_dir = tempfile::tempdir().expect("clone directory");
        let clone_repo = git2::Repository::init(clone_dir.path()).expect("initialize cloned repository");
        let signature = git2::Signature::now("test", "test@example.com").expect("signature");
        let tree_oid = {
            let mut index = clone_repo.index().expect("clone index");
            index.write_tree().expect("write clone tree")
        };
        let tree = clone_repo.find_tree(tree_oid).expect("clone tree");
        clone_repo
            .commit(Some("HEAD"), &signature, &signature, "initial", &tree, &[])
            .expect("create clone commit");
        drop(tree);
        drop(clone_repo);
        let recent_dir = tempfile::tempdir().expect("recent repos directory");
        let mut app = App::new();
        app.recent_repos = RecentRepos::load_from(recent_dir.path().join("recent.json"));
        app.show_clone_dialog = true;
        app.show_error("Operation 'Cloning repository' timed out".into());
        let (tx, rx) = mpsc::channel();
        tx.send(OpResult::CloneSuccess(clone_dir.path().to_path_buf()))
            .expect("send late clone result");
        app.pending_ops.push(PendingOp {
            description: "Cloning repository".to_string(),
            receiver: rx,
            repo_generation: app.repo_generation,
            started_at: Instant::now(),
            progress: Arc::new(Mutex::new(String::new())),
            last_progress_update: Instant::now(),
            last_seen_progress: String::new(),
            timed_out: true,
            task_diff_identity: None,
            form_submission: None,

        });

        app.process_pending_ops(&egui::Context::default());

        assert!(app.git.is_open());
        assert_eq!(app.repo_path, clone_dir.path().to_string_lossy());
        assert_eq!(app.current_tab, Tab::Worktrees);
        assert!(!app.show_clone_dialog);
        assert!(!app.status_is_error);
    }

    #[test]
    fn test_status_color_by_type_known_dark() {
        assert_eq!(App::status_color_by_type('M', true), egui::Color32::from_rgb(80, 220, 80));
        assert_eq!(App::status_color_by_type('A', true), egui::Color32::from_rgb(80, 220, 80));
        assert_eq!(App::status_color_by_type('R', true), egui::Color32::from_rgb(80, 220, 80));
        assert_eq!(App::status_color_by_type('D', true), egui::Color32::from_rgb(240, 80, 80));
        assert_eq!(App::status_color_by_type('U', true), egui::Color32::from_rgb(220, 200, 50));
    }

    #[test]
    fn test_status_color_by_type_known_light() {
        assert_eq!(App::status_color_by_type('M', false), egui::Color32::from_rgb(0, 120, 0));
        assert_eq!(App::status_color_by_type('A', false), egui::Color32::from_rgb(0, 120, 0));
        assert_eq!(App::status_color_by_type('R', false), egui::Color32::from_rgb(0, 120, 0));
        assert_eq!(App::status_color_by_type('D', false), egui::Color32::from_rgb(180, 30, 30));
        assert_eq!(App::status_color_by_type('U', false), egui::Color32::from_rgb(180, 130, 0));
    }

    #[test]
    fn test_status_color_by_type_unknown() {
        assert_eq!(App::status_color_by_type('X', true), egui::Color32::GRAY);
        assert_eq!(App::status_color_by_type('X', false), egui::Color32::GRAY);
    }

    #[test]
    fn test_status_color_gray_untracked() {
        assert_eq!(App::status_color_by_type('?', true), egui::Color32::GRAY);
        assert_eq!(App::status_color_by_type('!', true), egui::Color32::GRAY);
        assert_eq!(App::status_color_by_type('?', false), egui::Color32::GRAY);
        assert_eq!(App::status_color_by_type('!', false), egui::Color32::GRAY);
    }

    #[test]
    fn test_adaptive_color_dark_vs_light_different() {
        // Green should be different in dark vs light mode
        assert_ne!(
            App::status_color_by_type('M', true),
            App::status_color_by_type('M', false)
        );
        // Yellow/conflict should be different
        assert_ne!(
            App::status_color_by_type('U', true),
            App::status_color_by_type('U', false)
        );
        // Red should be different
        assert_ne!(
            App::status_color_by_type('D', true),
            App::status_color_by_type('D', false)
        );
        // Gray should stay the same
        assert_eq!(
            App::status_color_by_type('?', true),
            App::status_color_by_type('?', false)
        );
        assert_eq!(
            App::status_color_by_type('!', true),
            App::status_color_by_type('!', false)
        );
    }

    #[test]
    fn test_format_elapsed_just_updated() {
        assert_eq!(App::format_elapsed(0), "Just updated");
        assert_eq!(App::format_elapsed(1), "Just updated");
        assert_eq!(App::format_elapsed(30), "Just updated");
        assert_eq!(App::format_elapsed(59), "Just updated");
    }

    #[test]
    fn test_format_elapsed_minutes() {
        assert_eq!(App::format_elapsed(60), "Updated 1m ago");
        assert_eq!(App::format_elapsed(120), "Updated 2m ago");
        assert_eq!(App::format_elapsed(3540), "Updated 59m ago");
    }

    #[test]
    fn test_format_elapsed_hours() {
        assert_eq!(App::format_elapsed(3600), "Updated 1h ago");
        assert_eq!(App::format_elapsed(7200), "Updated 2h ago");
        assert_eq!(App::format_elapsed(86400), "Updated 24h ago");
    }

    #[test]
    fn test_format_elapsed_boundaries() {
        // 59 seconds → "Just updated"
        assert_eq!(App::format_elapsed(59), "Just updated");
        // 60 seconds → 1m
        assert_eq!(App::format_elapsed(60), "Updated 1m ago");
        // 3599 seconds → 59m
        assert_eq!(App::format_elapsed(3599), "Updated 59m ago");
        // 3600 seconds → 1h
        assert_eq!(App::format_elapsed(3600), "Updated 1h ago");
    }

    #[test]
    fn test_tab_partial_eq() {
        assert_eq!(Tab::Status, Tab::Status);
        assert_eq!(Tab::Log, Tab::Log);
        assert_ne!(Tab::Status, Tab::Branches);
    }

    #[test]
    fn test_tab_clone() {
        assert_eq!(Tab::Worktrees.clone(), Tab::Worktrees);
    }

    // --- Font / encoding related tests ---

    #[test]
    fn test_font_definitions_default_has_font_data() {
        let fonts = egui::FontDefinitions::default();
        assert!(!fonts.font_data.is_empty(), "Default font definitions should contain font data");
        assert!(!fonts.families.is_empty(), "Default font definitions should have font families");
    }

    #[test]
    fn test_font_definitions_proportional_has_fallback() {
        let fonts = egui::FontDefinitions::default();
        let prop = fonts.families.get(&egui::FontFamily::Proportional);
        assert!(prop.is_some(), "Proportional font family should exist");
        let prop = prop.unwrap();
        assert!(!prop.is_empty(), "Proportional family should have at least one font");
    }

    #[test]
    fn test_font_definitions_monospace_family() {
        let fonts = egui::FontDefinitions::default();
        let mono = fonts.families.get(&egui::FontFamily::Monospace);
        assert!(mono.is_some(), "Monospace font family should exist");
        let mono = mono.unwrap();
        assert!(!mono.is_empty(), "Monospace family should have at least one font");
    }

    #[test]
    fn test_font_data_support_emoji_range() {
        let rocket_emoji = "🚀";
        assert_eq!(rocket_emoji.len(), 4, "Rocket emoji should be 4 bytes in UTF-8");
        assert!(rocket_emoji.chars().all(|c| c.is_ascii() || c as u32 > 127),
            "Emoji characters should be valid Unicode");
    }

    #[test]
    fn test_unicode_arrows_are_valid_utf8() {
        let up_arrow = '↑'; // U+2191
        let down_arrow = '↓'; // U+2193
        let play_icon = '▶'; // U+25B6
        assert_eq!(up_arrow as u32, 0x2191, "↑ should be U+2191");
        assert_eq!(down_arrow as u32, 0x2193, "↓ should be U+2193");
        assert_eq!(play_icon as u32, 0x25B6, "▶ should be U+25B6");
        let s = format!("{} {} {}", up_arrow, down_arrow, play_icon);
        assert_eq!(s.chars().count(), 5, "String should contain 5 chars (3 symbols + 2 spaces)");
    }

    #[test]
    fn test_emoji_chars_in_app_ui() {
        let emojis = ['📂', '🔀', '📋', '📦', '🌐', '▶', 'ℹ', '🔄', '🗑', '⏳', '📊'];
        for (i, &emoji) in emojis.iter().enumerate() {
            assert!(emoji as u32 > 127, "Emoji {} (index {}) should be a Unicode character", emoji, i);
        }
    }

    #[test]
    fn test_log_tab_uses_clipboard_emoji_not_alarm_clock() {
        let tabs = [
            (Tab::Status, "📊 Status"),
            (Tab::Branches, "🔀 Branches"),
            (Tab::Worktrees, "📂 Worktrees"),
            (Tab::Log, "📋 Log"),
            (Tab::Stash, "📦 Stash"),
            (Tab::Remotes, "🌐 Remotes"),
        ];
        let log_label = tabs.iter().find(|(t, _)| *t == Tab::Log).map(|(_, l)| *l).unwrap();
        assert!(
            !log_label.contains('\u{23F0}'),
            "Log tab must NOT use ⏰ (alarm clock) which renders as a box. Found: {}",
            log_label
        );
        assert!(
            log_label.contains("📋"),
            "Log tab should use 📋 (clipboard) emoji. Found: {}",
            log_label
        );
    }

    #[test]
    fn test_about_button_does_not_use_circled_i() {
        let bad_char = '\u{24D8}';
        assert_eq!(ABOUT_BUTTON_LABEL, "ℹ");
        assert!(
            !ABOUT_BUTTON_LABEL.contains(bad_char),
            "About button label '{}' must NOT use ⓘ which renders as a box",
            ABOUT_BUTTON_LABEL
        );

        let ctx = egui::Context::default();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    egui::vec2(200.0, 100.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let _ = ui.button(ABOUT_BUTTON_LABEL);
                });
            },
        );
        let text_shape = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.job.text == ABOUT_BUTTON_LABEL => {
                    Some(text)
                }
                _ => None,
            })
            .expect("About button should paint a text shape");
        let font_id = text_shape
            .galley
            .job
            .sections
            .first()
            .expect("About button text should have a font section")
            .format
            .font_id
            .clone();
        assert!(
            ctx.fonts(|fonts| fonts.has_glyph(&font_id, 'ℹ')),
            "About button font should provide an actual ℹ glyph"
        );
        assert!(text_shape
            .galley
            .rows
            .iter()
            .flat_map(|row| row.glyphs.iter())
            .any(|glyph| glyph.chr == 'ℹ'));
    }

    #[test]
    fn test_repo_name_extracts_last_path_component() {
        let cases = [
            ("C:\\Users\\me\\projects\\my-project", "my-project"),
            ("C:\\Users\\me\\projects\\my-project\\", "my-project"),
            ("/home/user/projects/my-repo", "my-repo"),
            ("/home/user/projects/my-repo/", "my-repo"),
            ("/a/b/c", "c"),
            ("just-a-name", "just-a-name"),
            ("", ""),
        ];
        for (path, expected) in &cases {
            let mut app = App::new();
            app.repo_path = path.to_string();
            assert_eq!(app.repo_name(), *expected, "repo_name() for path '{}'", path);
        }
    }

    #[test]
    fn test_repo_name_empty_when_no_repo_open() {
        let app = App::new();
        assert_eq!(app.repo_name(), "", "repo_name should be empty when no repo is open");
    }

    #[test]
    fn test_pending_op_progress_sharing() {
        use std::sync::{Arc, Mutex};
        let progress = Arc::new(Mutex::new(String::new()));

        {
            let p = progress.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(10));
                *p.lock().unwrap() = "Receiving objects: 45%".to_string();
            });
        }

        std::thread::sleep(std::time::Duration::from_millis(50));
        let current = progress.lock().unwrap().clone();
        assert_eq!(current, "Receiving objects: 45%");
    }

    #[test]
    fn test_pending_op_progress_empty_initially() {
        use std::sync::{Arc, Mutex};
        let progress = Arc::new(Mutex::new(String::new()));
        assert!(progress.lock().unwrap().is_empty());
    }

    #[test]
    fn test_current_operation_empty() {
        let app = App::new();
        assert_eq!(app.current_operation(), "");
    }

    #[test]
    fn test_is_busy_initially_false() {
        let app = App::new();
        assert!(!app.is_busy());
    }

    #[test]
    fn test_timed_out_operation_stays_busy_until_worker_finishes() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::Duration;

        let mut app = App::new();
        let ctx = egui::Context::default();
        let (tx, rx) = mpsc::channel();
        let worker_finished = Arc::new(AtomicBool::new(false));
        let worker_finished_clone = worker_finished.clone();

        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            tx.send(OpResult::Success("late mutation".to_string())).unwrap();
            worker_finished_clone.store(true, Ordering::SeqCst);
        });

        app.pending_ops.push(PendingOp {
            description: "Timed operation".to_string(),
            receiver: rx,
            repo_generation: app.repo_generation,
            started_at: Instant::now() - Duration::from_secs(61),
            progress: Arc::new(Mutex::new(String::new())),
            last_progress_update: Instant::now(),
            last_seen_progress: String::new(),
            timed_out: false,
            task_diff_identity: None,
            form_submission: None,

        });

        app.process_pending_ops(&ctx);

        assert!(app.is_busy(), "a timed-out worker must still block new operations");
        assert!(app.status_message.contains("timed out"));

        while !worker_finished.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
        app.process_pending_ops(&ctx);

        assert!(!app.is_busy(), "the operation can be released after its worker exits");
        assert!(
            app.status_message.contains("timed out"),
            "a late result must not replace the timeout status"
        );
    }

    #[test]
    fn test_queued_result_is_processed_before_timeout_classification() {
        use std::time::Duration;

        let mut app = App::new();
        let ctx = egui::Context::default();
        let (tx, rx) = mpsc::channel();
        tx.send(OpResult::Success("completed mutation".to_string()))
            .unwrap();

        app.pending_ops.push(PendingOp {
            description: "Completed operation".to_string(),
            receiver: rx,
            repo_generation: app.repo_generation,
            started_at: Instant::now() - Duration::from_secs(61),
            progress: Arc::new(Mutex::new(String::new())),
            last_progress_update: Instant::now(),
            last_seen_progress: String::new(),
            timed_out: false,
            task_diff_identity: None,
            form_submission: None,

        });

        app.process_pending_ops(&ctx);

        assert!(
            !app.is_busy(),
            "a queued result should finish the operation"
        );
        assert_eq!(app.status_message, "completed mutation");
        assert!(!app.status_is_error);
        assert!(app.last_operation_log.contains("✓ completed mutation"));
    }

    #[test]
    fn test_pending_op_contains_progress() {
        use std::sync::{Arc, Mutex};
        let op = PendingOp {
            description: "Fetch from origin".to_string(),
            receiver: mpsc::channel::<OpResult>().1,
            repo_generation: 0,
            started_at: Instant::now(),
            progress: Arc::new(Mutex::new("initial progress".to_string())),
            last_progress_update: Instant::now(),
            last_seen_progress: String::new(),
            timed_out: false,
            task_diff_identity: None,
            form_submission: None,

        };
        assert_eq!(*op.progress.lock().unwrap(), "initial progress");
    }

    #[test]
    fn test_format_elapsed_no_panic_on_large_values() {
        let result = App::format_elapsed(u64::MAX);
        assert!(!result.is_empty());
        assert!(result.contains("h ago"));
    }

    // --- Update dialog behavior tests ---

    fn run_app_frame(
        app: &mut App,
        ctx: &egui::Context,
        frame: &mut eframe::Frame,
        screen_rect: egui::Rect,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                ..Default::default()
            },
            |ctx| eframe::App::update(app, ctx, frame),
        )
    }

    #[test]
    fn test_update_dialog_dismissed_initially_false() {
        let app = App::new();
        assert!(!app.update_dialog_dismissed, "Dismiss flag should start as false");
    }

    #[test]
    fn test_trigger_update_check_resets_dismiss_flag() {
        let mut app = App::new();
        app.update_dialog_dismissed = true;
        app.trigger_update_check();
        assert!(!app.update_dialog_dismissed, "Triggering a new check should reset dismiss flag");
    }

    #[test]
    fn test_update_available_opens_dialog_when_not_dismissed() {
        let mut app = App::new();
        app.auto_check_done = true;
        *app.update_state.lock().unwrap() = UpdateState::UpdateAvailable {
            latest_version: "0.2.0".to_string(),
            download_url: String::new(),
            assets: Vec::new(),
        };

        let ctx = egui::Context::default();
        let screen_rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        let mut frame = eframe::Frame::_new_kittest();
        run_app_frame(&mut app, &ctx, &mut frame, screen_rect);

        assert!(
            app.show_update_dialog,
            "An available update should open the dialog"
        );
    }

    #[test]
    fn test_remind_later_dismisses_dialog_and_prevents_reopening() {
        let mut app = App::new();
        app.auto_check_done = true;
        *app.update_state.lock().unwrap() = UpdateState::UpdateAvailable {
            latest_version: "0.2.0".to_string(),
            download_url: String::new(),
            assets: Vec::new(),
        };

        let ctx = egui::Context::default();
        let screen_rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        let mut frame = eframe::Frame::_new_kittest();
        run_app_frame(&mut app, &ctx, &mut frame, screen_rect);
        let output = run_app_frame(&mut app, &ctx, &mut frame, screen_rect);
        let remind_later_pos = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.job.text == "Remind Later" => {
                    Some(text.visual_bounding_rect().center())
                }
                _ => None,
            })
            .expect("Update dialog should render a Remind Later button");
        let pointer_input = |pressed| egui::RawInput {
            screen_rect: Some(screen_rect),
            events: vec![
                egui::Event::PointerMoved(remind_later_pos),
                egui::Event::PointerButton {
                    pos: remind_later_pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            ..Default::default()
        };

        let _ = ctx.run(pointer_input(true), |ctx| {
            eframe::App::update(&mut app, ctx, &mut frame);
        });
        let _ = ctx.run(pointer_input(false), |ctx| {
            eframe::App::update(&mut app, ctx, &mut frame);
        });

        assert!(
            !app.show_update_dialog,
            "Remind Later should close the dialog"
        );
        assert!(
            app.update_dialog_dismissed,
            "Remind Later should mark the dialog dismissed"
        );

        run_app_frame(&mut app, &ctx, &mut frame, screen_rect);
        assert!(
            !app.show_update_dialog,
            "An available update should stay dismissed on the next frame"
        );
    }

    #[test]
    fn test_open_in_browser_dismisses_dialog_for_next_frame() {
        let mut app = App::new();
        let repo_dir = tempfile::tempdir().expect("create temporary repository directory");
        git2::Repository::init(repo_dir.path()).expect("initialize temporary repository");
        app.git.open(repo_dir.path()).expect("open temporary repository");
        assert!(app.git.is_open());
        app.auto_check_done = true;
        app.show_update_dialog = true;
        app.update_dialog_dismissed = false;
        *app.update_state.lock().unwrap() = UpdateState::UpdateAvailable {
            latest_version: "0.2.0".to_string(),
            download_url: String::new(),
            assets: Vec::new(),
        };

        let ctx = egui::Context::default();
        let screen_rect = egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(800.0, 600.0),
        );
        let mut frame = eframe::Frame::_new_kittest();
        // Give egui one frame to initialize the update window before locating its button.
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                ..Default::default()
            },
            |ctx| eframe::App::update(&mut app, ctx, &mut frame),
        );
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                ..Default::default()
            },
            |ctx| eframe::App::update(&mut app, ctx, &mut frame),
        );
        let browser_button_pos = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.job.text == "Open in Browser" => {
                    Some(text.visual_bounding_rect().center())
                }
                _ => None,
            })
            .expect("Update dialog should render an Open in Browser button");

        let pointer_input = |pressed| egui::RawInput {
            screen_rect: Some(screen_rect),
            events: vec![
                egui::Event::PointerMoved(browser_button_pos),
                egui::Event::PointerButton {
                    pos: browser_button_pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            ..Default::default()
        };

        let _ = ctx.run(pointer_input(true), |ctx| {
            eframe::App::update(&mut app, ctx, &mut frame);
        });
        let _ = ctx.run(pointer_input(false), |ctx| {
            eframe::App::update(&mut app, ctx, &mut frame);
        });

        assert!(!app.show_update_dialog, "Opening the browser should close the dialog");
        assert!(
            app.update_dialog_dismissed,
            "Opening the browser should prevent the dialog from reopening"
        );

        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                ..Default::default()
            },
            |ctx| eframe::App::update(&mut app, ctx, &mut frame),
        );
        assert!(
            !app.show_update_dialog,
            "The dialog should stay closed on the next frame after opening the browser"
        );
    }

    #[test]
    fn test_auto_update_error_dialog_shows_message_and_dismiss_action() {
        let mut app = App::new();
        app.auto_check_done = true;
        app.show_update_dialog = true;
        let failure_message = "Failed to download update: connection refused";
        *app.update_state.lock().unwrap() = UpdateState::Error(failure_message.to_string());

        let ctx = egui::Context::default();
        let screen_rect = egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(800.0, 600.0),
        );
        let mut frame = eframe::Frame::_new_kittest();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                ..Default::default()
            },
            |ctx| eframe::App::update(&mut app, ctx, &mut frame),
        );
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                ..Default::default()
            },
            |ctx| eframe::App::update(&mut app, ctx, &mut frame),
        );

        let rendered_text = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(app.show_update_dialog, "Error state should keep the dialog open");
        assert!(
            rendered_text.contains(&failure_message),
            "The dialog should display the download failure reason"
        );
        assert!(rendered_text.contains(&"Retry"), "The dialog should offer retry");
        assert!(rendered_text.contains(&"Dismiss"), "The dialog should offer dismissal");

        let dismiss_button_pos = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.job.text == "Dismiss" => {
                    Some(text.visual_bounding_rect().center())
                }
                _ => None,
            })
            .expect("Update error dialog should render a Dismiss button");
        let pointer_input = |pressed| egui::RawInput {
            screen_rect: Some(screen_rect),
            events: vec![
                egui::Event::PointerMoved(dismiss_button_pos),
                egui::Event::PointerButton {
                    pos: dismiss_button_pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            ..Default::default()
        };

        let _ = ctx.run(pointer_input(true), |ctx| {
            eframe::App::update(&mut app, ctx, &mut frame);
        });
        let _ = ctx.run(pointer_input(false), |ctx| {
            eframe::App::update(&mut app, ctx, &mut frame);
        });

        assert!(!app.show_update_dialog, "Dismiss should close the error dialog");
        assert!(app.update_dialog_dismissed, "Dismiss should prevent automatic reopening");
    }

    // --- Download tracking tests ---

    #[test]
    fn test_download_progress_field_defaults() {
        let app = App::new();
        assert_eq!(app.download_progress, 0.0, "Download progress should start at 0");
    }

    #[test]
    fn test_update_asset_download_path_rejects_paths_outside_download_dir() {
        let download_dir = Path::new("/home/user/Downloads");
        assert_eq!(
            update_asset_download_path(download_dir, "git-manager.zip").unwrap(),
            download_dir.join("git-manager.zip")
        );

        for file_name in [
            "../outside.zip",
            "/tmp/outside.zip",
            "nested/asset.zip",
            "..\\outside.zip",
            "nested\\asset.zip",
            "",
            ".",
            "..",
        ] {
            assert!(
                update_asset_download_path(download_dir, file_name).is_err(),
                "asset filename should be rejected: {file_name:?}"
            );
        }
    }

    #[test]
    fn test_stale_download_progress_cannot_overwrite_completed_download() {
        let state = Arc::new(Mutex::new(UpdateState::Downloaded {
            file_path: "/tmp/update.zip".to_string(),
        }));
        let generation = AtomicU64::new(1);

        assert!(!App::update_download_progress_if_active(
            &state,
            &generation,
            1,
            0.5,
            "update.zip",
        ));
        assert_eq!(
            *state.lock().unwrap(),
            UpdateState::Downloaded {
                file_path: "/tmp/update.zip".to_string(),
            }
        );
    }

    #[test]
    fn test_stale_update_check_result_cannot_overwrite_newer_request() {
        let state = Mutex::new(UpdateState::Idle);
        let generation = AtomicU64::new(0);
        let older_request = begin_update_request_if(
            &state,
            &generation,
            |_| true,
            UpdateState::Checking,
        )
        .expect("start older check");
        let newer_request = begin_update_request_if(
            &state,
            &generation,
            |_| true,
            UpdateState::Checking,
        )
        .expect("start newer check");
        let newer_result = UpdateState::UpdateAvailable {
            latest_version: "2.0.0".to_string(),
            download_url: "https://example.com/update.zip".to_string(),
            assets: Vec::new(),
        };

        assert!(!commit_update_state_if_current(
            &state,
            &generation,
            older_request,
            |current| matches!(current, UpdateState::Checking),
            UpdateState::UpToDate,
        ));
        assert_eq!(*state.lock().unwrap(), UpdateState::Checking);
        assert!(commit_update_state_if_current(
            &state,
            &generation,
            newer_request,
            |current| matches!(current, UpdateState::Checking),
            newer_result.clone(),
        ));
        assert!(!commit_update_state_if_current(
            &state,
            &generation,
            older_request,
            |current| matches!(current, UpdateState::Checking),
            UpdateState::UpToDate,
        ));
        assert_eq!(*state.lock().unwrap(), newer_result);
    }

    #[test]
    fn test_new_download_invalidates_pending_check_result() {
        let state = Mutex::new(UpdateState::Idle);
        let generation = AtomicU64::new(0);
        let check_request = begin_update_request_if(
            &state,
            &generation,
            |_| true,
            UpdateState::Checking,
        )
        .expect("start check");
        let download_request = begin_update_request_if(
            &state,
            &generation,
            |current| !matches!(current, UpdateState::Downloading { .. }),
            UpdateState::Downloading {
                progress: 0.0,
                file_name: "new.zip".to_string(),
            },
        )
        .expect("start download");

        assert!(!commit_update_state_if_current(
            &state,
            &generation,
            check_request,
            |current| matches!(current, UpdateState::Checking),
            UpdateState::UpToDate,
        ));
        assert_eq!(
            *state.lock().unwrap(),
            UpdateState::Downloading {
                progress: 0.0,
                file_name: "new.zip".to_string(),
            }
        );
        assert!(download_request > check_request);
    }

    #[test]
    fn test_stale_download_progress_and_result_cannot_overwrite_newer_download() {
        let state = Mutex::new(UpdateState::Idle);
        let generation = AtomicU64::new(0);
        let older_request = begin_update_request_if(
            &state,
            &generation,
            |current| !matches!(current, UpdateState::Downloading { .. }),
            UpdateState::Downloading {
                progress: 0.0,
                file_name: "old.zip".to_string(),
            },
        )
        .expect("start older download");

        assert!(begin_update_request_if(
            &state,
            &generation,
            |current| !matches!(current, UpdateState::Downloading { .. }),
            UpdateState::Downloading {
                progress: 0.0,
                file_name: "duplicate.zip".to_string(),
            },
        )
        .is_none());
        assert!(App::update_download_progress_if_active(
            &state,
            &generation,
            older_request,
            0.25,
            "old.zip",
        ));
        assert!(commit_update_state_if_current(
            &state,
            &generation,
            older_request,
            |current| matches!(current, UpdateState::Downloading { .. }),
            UpdateState::Downloaded {
                file_path: "/tmp/old.zip".to_string(),
            },
        ));

        let newer_request = begin_update_request_if(
            &state,
            &generation,
            |current| !matches!(current, UpdateState::Downloading { .. }),
            UpdateState::Downloading {
                progress: 0.0,
                file_name: "new.zip".to_string(),
            },
        )
        .expect("start newer download");
        assert!(!App::update_download_progress_if_active(
            &state,
            &generation,
            older_request,
            0.9,
            "old.zip",
        ));
        assert!(!commit_update_state_if_current(
            &state,
            &generation,
            older_request,
            |current| matches!(current, UpdateState::Downloading { .. }),
            UpdateState::Error("stale download failure".to_string()),
        ));
        assert_eq!(
            *state.lock().unwrap(),
            UpdateState::Downloading {
                progress: 0.0,
                file_name: "new.zip".to_string(),
            }
        );
        assert!(newer_request > older_request);
    }

}
