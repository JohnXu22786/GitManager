use crate::git_ops::{GitRepo, TaskDiffReviewState};
use crate::task_verification::source_fingerprint;
use crate::tasks::{repository_root, TaskDependency, TaskRecord};
use git2::{BranchType, Index, MergeOptions, Repository, Status, StatusOptions};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

#[derive(Clone, Debug, Default)]
pub enum TaskMergePreviewState {
    #[default]
    Idle,
    Running,
    ConflictFree {
        changed_files: Vec<String>,
        task_titles: Vec<String>,
        source_fingerprints: HashMap<String, String>,
        base_oid: String,
        preview_tree_oid: String,
    },
    Conflicted {
        changed_files: Vec<String>,
        task_titles: Vec<String>,
        task_title: String,
        task_position: usize,
        conflicts: Vec<MergeConflict>,
        source_fingerprints: HashMap<String, String>,
        base_oid: String,
    },
    Unavailable {
        reason: String,
        changed_files: Vec<String>,
        task_titles: Vec<String>,
    },
}

#[derive(Clone, Debug)]
pub struct MergeConflict {
    pub path: String,
    pub stages: Vec<String>,
}

pub struct TaskMergePreviewController {
    pub repository_path: String,
    pub base_ref: String,
    pub base_refs: Vec<String>,
    pub base_ref_error: Option<String>,
    pub selected_task_ids: HashSet<String>,
    state: TaskMergePreviewState,
    receiver: Option<Receiver<PreviewWorkerCompletion>>,
    workspace: Option<Arc<PreviewWorkspace>>,
    completed_identity: Option<PreviewIdentity>,
    completed_base_oid: Option<String>,
    base_checked_at: Option<Instant>,
    base_refs_checked_at: Option<Instant>,
}

struct PreviewWorkerCompletion {
    state: TaskMergePreviewState,
    workspace: Option<PreviewWorkspace>,
}

pub struct PreviewWorkspace {
    _temporary_directory: TempDir,
    pub path: std::path::PathBuf,
    pub object_repository_path: std::path::PathBuf,
    pub base_oid: String,
    pub tree_oid: String,
}

impl PreviewWorkspace {
    /// Create an independent checkout of the exact combined preview tree for one command.
    /// Each configured command starts from a clean materialization even if an earlier command
    /// changed files in its own working directory.
    pub fn materialize_for_command(
        &self,
        cancel_requested: &AtomicBool,
    ) -> Result<Self, String> {
        let temporary_directory = tempfile::Builder::new()
            .prefix("git-manager-integrated-command-")
            .tempdir_in(self._temporary_directory.path())
            .map_err(|error| {
                format!("Could not create an isolated command preview directory: {error}")
            })?;
        let tree_oid = git2::Oid::from_str(&self.tree_oid)
            .map_err(|error| format!("The combined preview tree ID is invalid: {error}"))?;
        let base_oid = git2::Oid::from_str(&self.base_oid)
            .map_err(|error| format!("The combined preview base ID is invalid: {error}"))?;
        materialize_preview_tree(
            temporary_directory,
            &self.object_repository_path,
            base_oid,
            tree_oid,
            Some(cancel_requested),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PreviewIdentity {
    repository_path: String,
    base_ref: String,
    tasks: Vec<TaskIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TaskIdentity {
    id: String,
    title: String,
    repository_path: String,
    worktree_path: String,
    base_commit: Option<String>,
    dependencies: Vec<TaskDependency>,
}

impl From<&TaskRecord> for TaskIdentity {
    fn from(task: &TaskRecord) -> Self {
        Self {
            id: task.id.clone(),
            title: task.title.clone(),
            repository_path: task.repository_path.clone(),
            worktree_path: task.worktree_path.clone(),
            base_commit: task.base_commit.clone(),
            dependencies: task.dependencies.clone(),
        }
    }
}

impl Default for TaskMergePreviewController {
    fn default() -> Self {
        Self {
            repository_path: String::new(),
            base_ref: String::new(),
            base_refs: Vec::new(),
            base_ref_error: None,
            selected_task_ids: HashSet::new(),
            state: TaskMergePreviewState::Idle,
            receiver: None,
            workspace: None,
            completed_identity: None,
            completed_base_oid: None,
            base_checked_at: None,
            base_refs_checked_at: None,
        }
    }
}

impl TaskMergePreviewController {
    pub fn set_repository(&mut self, repository_path: String) {
        if self.repository_path == repository_path {
            return;
        }
        self.repository_path = repository_path;
        self.selected_task_ids.clear();
        self.state = TaskMergePreviewState::Idle;
        self.workspace = None;
        self.completed_identity = None;
        self.completed_base_oid = None;
        self.base_checked_at = None;
        self.base_refs_checked_at = None;
        self.base_ref_error = None;
        match local_base_refs(Path::new(&self.repository_path)) {
            Ok(refs) => {
                self.base_ref = default_base_ref(&refs).unwrap_or_default();
                self.base_refs = refs;
            }
            Err(error) => {
                self.base_ref.clear();
                self.base_refs.clear();
                self.base_ref_error = Some(error);
            }
        }
        self.base_refs_checked_at = Some(Instant::now());
    }

    pub fn refresh_base_refs_if_due(&mut self) {
        if self.repository_path.is_empty()
            || self.receiver.is_some()
            || self
                .base_refs_checked_at
                .is_some_and(|checked_at| checked_at.elapsed() < Duration::from_secs(5))
        {
            return;
        }
        self.base_refs_checked_at = Some(Instant::now());
        match local_base_refs(Path::new(&self.repository_path)) {
            Ok(refs) => {
                self.base_ref_error = None;
                if self.base_ref.is_empty() {
                    self.base_ref = default_base_ref(&refs).unwrap_or_default();
                } else if !refs.contains(&self.base_ref) {
                    let removed_base_ref = self.base_ref.clone();
                    self.base_ref = default_base_ref(&refs).unwrap_or_default();
                    self.base_refs = refs;
                    self.invalidate();
                    self.show_unavailable(format!(
                        "Selected base branch {removed_base_ref} is no longer available. Choose a current base branch and run the preview again."
                    ));
                    return;
                }
                self.base_refs = refs;
            }
            Err(error) => self.base_ref_error = Some(error),
        }
    }

    pub fn invalidate(&mut self) {
        if self.receiver.is_none() {
            self.state = TaskMergePreviewState::Idle;
            self.workspace = None;
            self.completed_identity = None;
            self.completed_base_oid = None;
            self.base_checked_at = None;
        }
    }

    pub fn state(&self) -> &TaskMergePreviewState {
        &self.state
    }

    pub fn workspace(&self) -> Option<Arc<PreviewWorkspace>> {
        self.workspace.clone()
    }

    pub fn is_running(&self) -> bool {
        self.receiver.is_some()
    }

    pub fn show_unavailable(&mut self, reason: impl Into<String>) {
        if self.receiver.is_none() {
            self.state = TaskMergePreviewState::Unavailable {
                reason: reason.into(),
                changed_files: Vec::new(),
                task_titles: Vec::new(),
            };
            self.workspace = None;
            self.completed_identity = None;
            self.completed_base_oid = None;
            self.base_checked_at = None;
        }
    }

    pub fn check_base_freshness(&mut self) -> Option<String> {
        let expected_base = self.completed_base_oid.as_ref()?;
        if self
            .base_checked_at
            .is_some_and(|checked_at| checked_at.elapsed() < Duration::from_secs(5))
        {
            return None;
        }
        self.base_checked_at = Some(Instant::now());
        let repository = match Repository::open(&self.repository_path) {
            Ok(repository) => repository,
            Err(error) => {
                return Some(format!("Could not recheck the selected preview base: {error}"))
            }
        };
        let current_base = repository
            .find_branch(&self.base_ref, BranchType::Local)
            .and_then(|branch| branch.get().peel_to_commit())
            .map(|commit| commit.id().to_string());
        match current_base {
            Ok(current) if &current == expected_base => None,
            Ok(_) => Some("The selected base branch moved after this preview. Run it again against the current base.".into()),
            Err(error) => Some(format!(
                "The selected base branch is no longer available: {error}"
            )),
        }
    }

    pub fn selection_matches(&self, repository_path: &str, base_ref: &str, tasks: &[TaskRecord]) -> bool {
        self.completed_identity.as_ref().is_some_and(|identity| {
            identity.repository_path == repository_path
                && identity.base_ref == base_ref
                && identity.tasks == tasks.iter().map(TaskIdentity::from).collect::<Vec<_>>()
        })
    }

    pub fn start(
        &mut self,
        ctx: &eframe::egui::Context,
        tasks: Vec<TaskRecord>,
    ) {
        if self.receiver.is_some() || tasks.is_empty() {
            return;
        }
        let repository_path = self.repository_path.clone();
        let base_ref = self.base_ref.clone();
        let task_titles = tasks.iter().map(|task| task.title.clone()).collect::<Vec<_>>();
        self.completed_identity = Some(PreviewIdentity {
            repository_path: repository_path.clone(),
            base_ref: base_ref.clone(),
            tasks: tasks.iter().map(TaskIdentity::from).collect(),
        });
        let repaint = ctx.clone();
        let (sender, receiver) = mpsc::channel();
        self.state = TaskMergePreviewState::Running;
        self.workspace = None;
        self.completed_base_oid = None;
        self.base_checked_at = None;
        match thread::Builder::new()
            .name("task-merge-preview".into())
            .spawn(move || {
                let mut workspace = None;
                let state = run_preview(&repository_path, &base_ref, &tasks, &mut workspace);
                let _ = sender.send(PreviewWorkerCompletion { state, workspace });
                repaint.request_repaint();
            })
        {
            Ok(_) => self.receiver = Some(receiver),
            Err(error) => {
                self.state = TaskMergePreviewState::Unavailable {
                    reason: format!("Could not start the isolated preview worker: {error}"),
                    changed_files: Vec::new(),
                    task_titles,
                };
                self.completed_identity = None;
            }
        }
    }

    pub fn poll(&mut self) {
        let Some(received) = self.receiver.as_ref().map(|receiver| receiver.try_recv()) else {
            return;
        };
        match received {
            Ok(completion) => {
                self.completed_base_oid = match &completion.state {
                    TaskMergePreviewState::ConflictFree { base_oid, .. }
                    | TaskMergePreviewState::Conflicted { base_oid, .. } => Some(base_oid.clone()),
                    _ => None,
                };
                self.workspace = completion.workspace.map(Arc::new);
                self.base_checked_at = self
                    .completed_base_oid
                    .as_ref()
                    .map(|_| Instant::now());
                self.state = completion.state;
                self.receiver = None;
            }
            Err(TryRecvError::Disconnected) => {
                self.state = TaskMergePreviewState::Unavailable {
                    reason: "The isolated preview worker stopped before returning a complete result."
                        .into(),
                    changed_files: Vec::new(),
                    task_titles: Vec::new(),
                };
                self.workspace = None;
                self.completed_identity = None;
                self.completed_base_oid = None;
                self.base_checked_at = None;
                self.receiver = None;
            }
            Err(TryRecvError::Empty) => {}
        }
    }
}

pub fn local_base_refs(repository_path: &Path) -> Result<Vec<String>, String> {
    let repository = Repository::open(repository_path)
        .map_err(|error| format!("Could not open the selected repository: {error}"))?;
    let mut refs = Vec::new();
    for branch in repository
        .branches(Some(BranchType::Local))
        .map_err(|error| format!("Could not list local base branches: {error}"))?
    {
        let (branch, _) = branch.map_err(|error| format!("Could not read a local branch: {error}"))?;
        if branch.get().target().is_none() {
            continue;
        }
        if let Some(name) = branch
            .name()
            .map_err(|error| format!("Could not read a local branch name: {error}"))?
        {
            refs.push(name.to_owned());
        }
    }
    refs.sort();
    refs.dedup();
    if refs.is_empty() {
        return Err("The selected repository has no local branch that can be used as a preview base.".into());
    }
    Ok(refs)
}

fn default_base_ref(refs: &[String]) -> Option<String> {
    refs.iter()
        .find(|name| name.as_str() == "main")
        .or_else(|| refs.iter().find(|name| name.as_str() == "master"))
        .or_else(|| refs.first())
        .cloned()
}

struct CapturedTask {
    task: TaskRecord,
    base_oid: git2::Oid,
    head_oid: git2::Oid,
    base_tree_oid: git2::Oid,
    head_tree_oid: git2::Oid,
    source_fingerprint: String,
}

fn run_preview(
    repository_path: &str,
    base_ref: &str,
    tasks: &[TaskRecord],
    workspace_out: &mut Option<PreviewWorkspace>,
) -> TaskMergePreviewState {
    let task_titles = tasks.iter().map(|task| task.title.clone()).collect::<Vec<_>>();
    let unavailable = |reason: String, changed_files: Vec<String>| {
        TaskMergePreviewState::Unavailable {
            reason,
            changed_files,
            task_titles: task_titles.clone(),
        }
    };
    if tasks.is_empty() {
        return unavailable("Select at least one task to preview.".into(), Vec::new());
    }
    let task_positions = tasks
        .iter()
        .enumerate()
        .map(|(position, task)| (task.id.as_str(), position))
        .collect::<HashMap<_, _>>();
    for (position, task) in tasks.iter().enumerate() {
        for dependency in &task.dependencies {
            match task_positions.get(dependency.task_id.as_str()) {
                None => {
                    return unavailable(
                        format!(
                            "{} depends on prerequisite task {}; select that task too.",
                            task.title, dependency.task_id
                        ),
                        Vec::new(),
                    )
                }
                Some(dependency_position) if *dependency_position >= position => {
                    return unavailable(
                        format!(
                            "The selected order does not place prerequisite {} before {}.",
                            dependency.task_id, task.title
                        ),
                        Vec::new(),
                    )
                }
                Some(_) => {}
            }
        }
    }

    let repository_path = match fs::canonicalize(repository_path) {
        Ok(path) => path,
        Err(error) => {
            return unavailable(
                format!("Could not resolve the selected repository path: {error}"),
                Vec::new(),
            )
        }
    };
    let source_repository = match Repository::open(&repository_path) {
        Ok(repository) => repository,
        Err(error) => {
            return unavailable(
                format!("Could not open the selected repository: {error}"),
                Vec::new(),
            )
        }
    };
    if let Err(reason) = ensure_external_merge_drivers_supported(&source_repository) {
        return unavailable(reason, Vec::new());
    }
    let base_commit = match source_repository
        .find_branch(base_ref, BranchType::Local)
        .and_then(|branch| branch.get().peel_to_commit())
    {
        Ok(commit) => commit,
        Err(error) => {
            return unavailable(
                format!("The selected base branch {base_ref} is unavailable: {error}"),
                Vec::new(),
            )
        }
    };
    let base_oid = base_commit.id();
    let base_tree = match base_commit.tree() {
        Ok(tree) => tree,
        Err(error) => {
            return unavailable(
                format!("Could not read the selected base tree: {error}"),
                Vec::new(),
            )
        }
    };

    let mut captured = Vec::new();
    let mut changed_files = BTreeSet::new();
    for task in tasks {
        let saved_repository_path = match fs::canonicalize(&task.repository_path) {
            Ok(path) => path,
            Err(error) => {
                return unavailable(
                    format!("{} saved repository is unavailable: {error}", task.title),
                    Vec::new(),
                )
            }
        };
        if saved_repository_path != repository_path {
            return unavailable(
                format!("{} is no longer linked to the selected repository.", task.title),
                Vec::new(),
            );
        }
        let worktree_path = match fs::canonicalize(&task.worktree_path) {
            Ok(path) => path,
            Err(error) => {
                return unavailable(
                    format!("{} worktree is unavailable: {error}", task.title),
                    Vec::new(),
                )
            }
        };
        let task_repository = match Repository::open(&worktree_path) {
            Ok(repository) => repository,
            Err(error) => {
                return unavailable(
                    format!("Could not open {} worktree: {error}", task.title),
                    Vec::new(),
                )
            }
        };
        if let Err(reason) = ensure_external_merge_drivers_supported(&task_repository) {
            return unavailable(
                format!("{} cannot be previewed: {reason}", task.title),
                Vec::new(),
            );
        }
        let Some(workdir) = task_repository.workdir() else {
            return unavailable(
                format!("{} no longer points to a Git worktree.", task.title),
                Vec::new(),
            );
        };
        let workdir = match fs::canonicalize(workdir) {
            Ok(path) => path,
            Err(error) => {
                return unavailable(
                    format!("Could not resolve {} worktree: {error}", task.title),
                    Vec::new(),
                )
            }
        };
        if workdir != worktree_path {
            return unavailable(
                format!("{} path no longer points to its saved worktree.", task.title),
                Vec::new(),
            );
        }
        let actual_repository_path = match repository_root(&task_repository, &workdir) {
            Ok(path) => path,
            Err(error) => return unavailable(format!("{}: {error}", task.title), Vec::new()),
        };
        let actual_repository_path = match fs::canonicalize(actual_repository_path) {
            Ok(path) => path,
            Err(error) => {
                return unavailable(
                    format!("Could not resolve the repository for {}: {error}", task.title),
                    Vec::new(),
                )
            }
        };
        if actual_repository_path != repository_path {
            return unavailable(
                format!("{} worktree is no longer linked to the selected repository.", task.title),
                Vec::new(),
            );
        }

        if let Err(reason) = ensure_source_is_clean(&task_repository) {
            return unavailable(format!("{} cannot be previewed: {reason}", task.title), Vec::new());
        }

        let mut git_repo = GitRepo::new();
        if let Err(error) = git_repo.open(&worktree_path) {
            return unavailable(
                format!("Could not inspect {} task changes: {error}", task.title),
                Vec::new(),
            );
        }
        let review = match git_repo.task_diff_review(
            task.base_commit.as_deref(),
            Path::new(&task.repository_path),
        ) {
            TaskDiffReviewState::Ready(review) => review,
            TaskDiffReviewState::Unavailable(reason) | TaskDiffReviewState::Error(reason) => {
                return unavailable(
                    format!("{} changes cannot be captured completely: {reason}", task.title),
                    Vec::new(),
                )
            }
            TaskDiffReviewState::Loading => {
                return unavailable(
                    format!("{} task source inspection did not finish.", task.title),
                    Vec::new(),
                )
            }
        };
        if let Err(reason) = ensure_source_is_clean(&task_repository) {
            return unavailable(format!("{} cannot be previewed: {reason}", task.title), Vec::new());
        }
        let final_fingerprint = match source_fingerprint(&worktree_path, &repository_path) {
            Ok(fingerprint) => fingerprint,
            Err(error) => {
                return unavailable(
                    format!("Could not verify {} source stability: {error}", task.title),
                    Vec::new(),
                )
            }
        };
        if final_fingerprint != review.source_fingerprint {
            return unavailable(
                format!("{} source changed while its preview snapshot was captured.", task.title),
                Vec::new(),
            );
        }
        let base_oid = match git2::Oid::from_str(&review.base_commit) {
            Ok(oid) => oid,
            Err(error) => {
                return unavailable(
                    format!("{} saved base is invalid: {error}", task.title),
                    Vec::new(),
                )
            }
        };
        let head_oid = match git2::Oid::from_str(&review.head_commit) {
            Ok(oid) => oid,
            Err(error) => {
                return unavailable(
                    format!("{} current task HEAD is invalid: {error}", task.title),
                    Vec::new(),
                )
            }
        };
        let base_tree_oid = match task_repository
            .find_commit(base_oid)
            .and_then(|commit| commit.tree())
        {
            Ok(tree) => tree.id(),
            Err(error) => {
                return unavailable(
                    format!("Could not read {} saved base tree: {error}", task.title),
                    Vec::new(),
                )
            }
        };
        let head_tree_oid = match task_repository
            .find_commit(head_oid)
            .and_then(|commit| commit.tree())
        {
            Ok(tree) => tree.id(),
            Err(error) => {
                return unavailable(
                    format!("Could not read {} current task tree: {error}", task.title),
                    Vec::new(),
                )
            }
        };
        changed_files.extend(review.files.into_iter().map(|file| file.path));
        captured.push(CapturedTask {
            task: task.clone(),
            base_oid,
            head_oid,
            base_tree_oid,
            head_tree_oid,
            source_fingerprint: review.source_fingerprint,
        });
    }

    let changed_files = changed_files.into_iter().collect::<Vec<_>>();
    let titles = captured
        .iter()
        .map(|captured| captured.task.title.clone())
        .collect::<Vec<_>>();
    let temp_root = match tempfile::Builder::new().prefix("git-manager-merge-preview-").tempdir() {
        Ok(directory) => directory,
        Err(error) => {
            return TaskMergePreviewState::Unavailable {
                reason: format!("Could not create a temporary preview area: {error}"),
                changed_files,
                task_titles: titles,
            }
        }
    };
    let source_object_dir = match fs::canonicalize(source_repository.commondir().join("objects")) {
        Ok(path) => path,
        Err(error) => {
            return TaskMergePreviewState::Unavailable {
                reason: format!("Could not resolve the repository object store: {error}"),
                changed_files,
                task_titles: titles,
            }
        }
    };
    let Some(source_objects) = source_object_dir.to_str().map(str::to_owned) else {
        return TaskMergePreviewState::Unavailable {
            reason: "The repository object path cannot be represented safely in the temporary preview area.".into(),
            changed_files,
            task_titles: titles,
        };
    };
    if source_objects.contains('\n') || source_objects.contains('\r') {
        return TaskMergePreviewState::Unavailable {
            reason: "The repository object path contains a line break and cannot be represented safely in the temporary preview area.".into(),
            changed_files,
            task_titles: titles,
        };
    }
    let preview_git_dir = temp_root.path().join("objects.git");
    if let Err(error) = Repository::init_bare(&preview_git_dir) {
        return TaskMergePreviewState::Unavailable {
            reason: format!("Could not initialize the isolated preview object store: {error}"),
            changed_files,
            task_titles: titles,
        };
    }
    let alternates_path = preview_git_dir.join("objects").join("info").join("alternates");
    if let Err(error) = fs::write(&alternates_path, format!("{source_objects}\n")) {
        return TaskMergePreviewState::Unavailable {
            reason: format!("Could not connect the temporary object store to repository objects: {error}"),
            changed_files,
            task_titles: titles,
        };
    }
    let preview_repository = match Repository::open_bare(&preview_git_dir) {
        Ok(repository) => repository,
        Err(error) => {
            return TaskMergePreviewState::Unavailable {
                reason: format!("Could not open the isolated preview object store: {error}"),
                changed_files,
                task_titles: titles,
            }
        }
    };

    let mut integrated_tree_oid = base_tree.id();
    for (position, snapshot) in captured.iter().enumerate() {
        let ancestor = match preview_repository.find_tree(snapshot.base_tree_oid) {
            Ok(tree) => tree,
            Err(error) => {
                return TaskMergePreviewState::Unavailable {
                    reason: format!("Could not load {} saved base tree: {error}", snapshot.task.title),
                    changed_files,
                    task_titles: titles,
                }
            }
        };
        let ours = match preview_repository.find_tree(integrated_tree_oid) {
            Ok(tree) => tree,
            Err(error) => {
                return TaskMergePreviewState::Unavailable {
                    reason: format!("Could not load the accumulated preview tree: {error}"),
                    changed_files,
                    task_titles: titles,
                }
            }
        };
        let theirs = match preview_repository.find_tree(snapshot.head_tree_oid) {
            Ok(tree) => tree,
            Err(error) => {
                return TaskMergePreviewState::Unavailable {
                    reason: format!("Could not load {} task tree: {error}", snapshot.task.title),
                    changed_files,
                    task_titles: titles,
                }
            }
        };
        let mut merge_options = MergeOptions::new();
        let mut merged = match preview_repository.merge_trees(
            &ancestor,
            &ours,
            &theirs,
            Some(&mut merge_options),
        ) {
            Ok(index) => index,
            Err(error) => {
                return TaskMergePreviewState::Unavailable {
                    reason: format!("Could not merge {} into the preview: {error}", snapshot.task.title),
                    changed_files,
                    task_titles: titles,
                }
            }
        };
        let conflicts = match merge_conflicts(&merged) {
            Ok(conflicts) => conflicts,
            Err(reason) => {
                return TaskMergePreviewState::Unavailable {
                    reason,
                    changed_files,
                    task_titles: titles,
                }
            }
        };
        if !conflicts.is_empty() {
            if let Err(reason) = verify_captured_sources(&repository_path, base_ref, base_oid, &captured) {
                return TaskMergePreviewState::Unavailable {
                    reason,
                    changed_files,
                    task_titles: titles,
                };
            }
            return TaskMergePreviewState::Conflicted {
                changed_files,
                task_titles: titles,
                task_title: snapshot.task.title.clone(),
                task_position: position + 1,
                conflicts,
                source_fingerprints: captured
                    .iter()
                    .map(|snapshot| (snapshot.task.id.clone(), snapshot.source_fingerprint.clone()))
                    .collect(),
                base_oid: base_oid.to_string(),
            };
        }
        integrated_tree_oid = match merged.write_tree_to(&preview_repository) {
            Ok(tree_oid) => tree_oid,
            Err(error) => {
                return TaskMergePreviewState::Unavailable {
                    reason: format!("Could not save the temporary integration tree: {error}"),
                    changed_files,
                    task_titles: titles,
                }
            }
        };
    }

    if let Err(reason) = verify_captured_sources(&repository_path, base_ref, base_oid, &captured) {
        return TaskMergePreviewState::Unavailable {
            reason,
            changed_files,
            task_titles: titles,
        };
    }
    let preview_tree_oid = integrated_tree_oid.to_string();
    let workspace =
        match materialize_preview_tree(
            temp_root,
            &preview_git_dir,
            base_oid,
            integrated_tree_oid,
            None,
        ) {
            Ok(workspace) => workspace,
            Err(reason) => {
                return TaskMergePreviewState::Unavailable {
                    reason,
                    changed_files,
                    task_titles: titles,
                }
            }
        };
    *workspace_out = Some(workspace);
    TaskMergePreviewState::ConflictFree {
        changed_files,
        task_titles: titles,
        source_fingerprints: captured
            .iter()
            .map(|snapshot| (snapshot.task.id.clone(), snapshot.source_fingerprint.clone()))
            .collect(),
        base_oid: base_oid.to_string(),
        preview_tree_oid,
    }
}

fn materialize_preview_tree(
    temporary_directory: TempDir,
    object_repository_path: &Path,
    base_oid: git2::Oid,
    tree_oid: git2::Oid,
    cancel_requested: Option<&AtomicBool>,
) -> Result<PreviewWorkspace, String> {
    if cancel_requested.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
        return Err("Combined preview materialization was cancelled.".into());
    }
    let path = temporary_directory.path().join("combined");
    fs::create_dir(&path)
        .map_err(|error| format!("Could not create the isolated combined preview tree: {error}"))?;
    let repository = Repository::init(&path)
        .map_err(|error| format!("Could not initialize the isolated combined preview tree: {error}"))?;
    let repository_git_directory = repository.path().to_path_buf();
    let object_repository_path = fs::canonicalize(object_repository_path)
        .map_err(|error| format!("Could not resolve the temporary preview object store: {error}"))?;
    let object_directory = fs::canonicalize(object_repository_path.join("objects"))
        .map_err(|error| format!("Could not resolve the temporary preview objects: {error}"))?;
    let object_directory = object_directory.to_str().ok_or_else(|| {
        "The temporary preview object path cannot be represented safely.".to_string()
    })?;
    if object_directory.contains('\n') || object_directory.contains('\r') {
        return Err(
            "The temporary preview object path contains a line break and cannot be represented safely."
                .into(),
        );
    }
    drop(repository);
    let alternates_path = repository_git_directory
        .join("objects")
        .join("info")
        .join("alternates");
    let alternates_directory = alternates_path
        .parent()
        .ok_or_else(|| "The isolated preview object link has no parent directory.".to_string())?;
    fs::create_dir_all(alternates_directory).map_err(|error| {
        format!("Could not prepare the isolated preview object link: {error}")
    })?;
    fs::write(&alternates_path, format!("{object_directory}\n"))
        .map_err(|error| format!("Could not link the combined tree to its temporary objects: {error}"))?;
    let repository = Repository::open(&path)
        .map_err(|error| format!("Could not open the isolated combined preview tree: {error}"))?;
    let tree = repository
        .find_tree(tree_oid)
        .map_err(|error| format!("Could not read the combined preview tree: {error}"))?;
    let mut checkout = git2::build::CheckoutBuilder::new();
    checkout.force().remove_untracked(true);
    if let Some(cancel_requested) = cancel_requested {
        checkout
            .notify_on(git2::build::CheckoutNotificationType::all())
            .notify(|_, _, _, _, _| !cancel_requested.load(Ordering::Relaxed));
    }
    if let Err(error) = repository.checkout_tree(tree.as_object(), Some(&mut checkout)) {
        if cancel_requested.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
            return Err("Combined preview materialization was cancelled.".into());
        }
        return Err(format!("Could not materialize the combined preview tree: {error}"));
    }
    if cancel_requested.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
        return Err("Combined preview materialization was cancelled.".into());
    }
    let base_commit = repository
        .find_commit(base_oid)
        .map_err(|error| format!("Could not load the combined preview base commit: {error}"))?;
    let signature = git2::Signature::now("GitManager Preview", "preview@invalid")
        .map_err(|error| format!("Could not create isolated preview Git metadata: {error}"))?;
    let preview_commit = repository
        .commit(
            None,
            &signature,
            &signature,
            "Combined task preview",
            &tree,
            &[&base_commit],
        )
        .map_err(|error| format!("Could not create isolated preview Git metadata: {error}"))?;
    repository
        .set_head_detached(preview_commit)
        .map_err(|error| format!("Could not set the combined preview HEAD: {error}"))?;
    let path = fs::canonicalize(&path)
        .map_err(|error| format!("Could not resolve the combined preview tree: {error}"))?;
    Ok(PreviewWorkspace {
        _temporary_directory: temporary_directory,
        path,
        object_repository_path,
        base_oid: base_oid.to_string(),
        tree_oid: tree_oid.to_string(),
    })
}

fn ensure_source_is_clean(repository: &Repository) -> Result<(), String> {
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(true)
        .recurse_ignored_dirs(true)
        .exclude_submodules(false);
    let statuses = repository
        .statuses(Some(&mut options))
        .map_err(|error| format!("Could not read worktree status: {error}"))?;
    if let Some(entry) = statuses.iter().next() {
        let path = std::str::from_utf8(entry.path_bytes())
            .map_err(|_| "a changed path contains non-UTF-8 bytes".to_string())?;
        let status = entry.status();
        if status.contains(Status::IGNORED) {
            return Err(format!("ignored path {path} is outside the committed task source"));
        }
        if status.contains(Status::WT_NEW) || status.contains(Status::INDEX_NEW) {
            return Err(format!("untracked path {path} is outside the committed task source"));
        }
        return Err(format!("worktree or index has uncommitted changes at {path}"));
    }
    let index = repository
        .index()
        .map_err(|error| format!("Could not read the task index: {error}"))?;
    if let Some(entry) = index.iter().find(|entry| entry.mode == 0o160000) {
        let path = std::str::from_utf8(&entry.path)
            .map_err(|_| "a nested repository path contains non-UTF-8 bytes".to_string())?;
        return Err(format!("nested repository {path} cannot be represented as task files"));
    }
    Ok(())
}

fn ensure_external_merge_drivers_supported(repository: &Repository) -> Result<(), String> {
    let config = repository
        .config()
        .map_err(|error| format!("Could not read Git merge-driver configuration: {error}"))?;
    let mut entries = config
        .entries(None)
        .map_err(|error| format!("Could not inspect Git merge-driver configuration: {error}"))?;
    while let Some(entry) = entries.next() {
        let entry = entry
            .map_err(|error| format!("Could not inspect Git merge-driver configuration: {error}"))?;
        let Some(name) = entry.name() else {
            continue;
        };
        let normalized_name = name.to_ascii_lowercase();
        if normalized_name.starts_with("merge.") && normalized_name.ends_with(".driver") {
            return Err(format!(
                "The selected repository configures an external merge driver ({name}), which an isolated preview cannot run."
            ));
        }
    }
    Ok(())
}

fn merge_conflicts(index: &Index) -> Result<Vec<MergeConflict>, String> {
    let conflicts = index
        .conflicts()
        .map_err(|error| format!("Could not read merge conflict details: {error}"))?;
    let mut paths: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for conflict in conflicts {
        let conflict = conflict.map_err(|error| format!("Could not read a merge conflict: {error}"))?;
        for (stage, entry) in [
            ("base", conflict.ancestor),
            ("preview", conflict.our),
            ("task", conflict.their),
        ] {
            if let Some(entry) = entry {
                let path = std::str::from_utf8(&entry.path).map_err(|_| {
                    "A conflicted path contains non-UTF-8 bytes and cannot be shown exactly.".to_string()
                })?;
                paths.entry(path.to_owned()).or_default().insert(stage.into());
            }
        }
    }
    Ok(paths
        .into_iter()
        .map(|(path, stages)| MergeConflict {
            path,
            stages: stages.into_iter().collect(),
        })
        .collect())
}

fn verify_captured_sources(
    repository_path: &Path,
    base_ref: &str,
    expected_base_oid: git2::Oid,
    captured: &[CapturedTask],
) -> Result<(), String> {
    let repository = Repository::open(repository_path)
        .map_err(|error| format!("Could not recheck the selected repository: {error}"))?;
    let current_base_oid = repository
        .find_branch(base_ref, BranchType::Local)
        .and_then(|branch| branch.get().peel_to_commit())
        .map(|commit| commit.id())
        .map_err(|error| format!("Could not recheck selected base {base_ref}: {error}"))?;
    if current_base_oid != expected_base_oid {
        return Err("The selected base branch moved during the preview; retry against its current state.".into());
    }
    for snapshot in captured {
        let worktree_path = Path::new(&snapshot.task.worktree_path);
        let fingerprint = source_fingerprint(worktree_path, repository_path)
            .map_err(|error| format!("Could not recheck {} source: {error}", snapshot.task.title))?;
        if fingerprint != snapshot.source_fingerprint {
            return Err(format!("{} source changed during the preview; retry with the current task state.", snapshot.task.title));
        }
        let task_repository = Repository::open(worktree_path)
            .map_err(|error| format!("Could not recheck {} worktree: {error}", snapshot.task.title))?;
        ensure_source_is_clean(&task_repository).map_err(|reason| {
            format!("{} is no longer clean: {reason}", snapshot.task.title)
        })?;
        let current_head = task_repository
            .head()
            .and_then(|head| head.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|error| format!("Could not recheck {} HEAD: {error}", snapshot.task.title))?;
        if current_head != snapshot.head_oid {
            return Err(format!("{} HEAD moved during the preview; retry with the current task state.", snapshot.task.title));
        }
        if task_repository.find_commit(snapshot.base_oid).is_err() {
            return Err(format!("{} saved base disappeared during the preview.", snapshot.task.title));
        }
    }
    Ok(())
}
