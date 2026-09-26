use chrono::DateTime;
use git2::{BranchType, DiffOptions, Repository, Status, WorktreeAddOptions, WorktreePruneOptions};
use std::cell::RefCell;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub type GitResult<T> = Result<T, String>;

#[derive(Clone, Debug)]
pub struct WorktreeFileIdentity {
    metadata: std::fs::Metadata,
    contents: Vec<u8>,
    _lock_file: Option<Arc<std::fs::File>>,
}

/// Describes a Git operation to be executed in a background thread.
pub enum GitOperation {
    StageAll,
    UnstageAll,
    RestoreAll,
    StageFile(PathBuf),
    UnstageFile(PathBuf),
    RestoreFile(PathBuf),
    Commit { message: String, amend: bool },
    Uncommit,
    CreateBranch { name: String, base: Option<String> },
    DeleteBranch { name: String, force: bool },
    RenameBranch { old: String, new: String },
    CheckoutBranch(String),
    MergeBranch(String),
    RemoveWorktree {
        path: PathBuf,
        force: bool,
        expected_git_link: Option<WorktreeFileIdentity>,
        require_git_link_identity: bool,
    },
    PruneWorktrees,
    CreateWorktree { name: String, path: PathBuf, branch: Option<String>, new_branch: bool },
    StashAll(Option<String>),
    StashPop,
    StashApply(usize),
    StashDrop(usize),
    Push { remote: String, branch: String, force: bool },
    Pull { remote: String, branch: String, rebase: bool },
    Fetch(String),
    GetDiff { path: PathBuf, staged: bool },
    /// Search commits in the log, identified by the request that started it.
    LogSearch { filter: String, request_id: u64 },
    /// Refresh all cached data from the repository.
    RefreshAll,
}

/// Result of a Git operation executed in a background thread.
#[derive(Debug, Clone)]
pub enum OpResult {
    /// Operation succeeded with a message.
    Success(String),
    /// A repository was cloned successfully and can be opened by the app.
    CloneSuccess(PathBuf),
    /// Operation failed with an error message.
    Error(String),
    /// Diff content for a file.
    DiffContent {
        path: String,
        lines: Vec<DiffLine>,
    },
    /// Search results for commit log, tagged with the query and request that produced them.
    SearchResults {
        request_id: u64,
        filter: String,
        commits: Vec<CommitInfo>,
    },
    /// Refreshed data from the repository, with optional errors.
    RefreshData {
        status_entries: Vec<StatusEntry>,
        branches: Vec<BranchInfo>,
        worktrees: Vec<WorktreeInfo>,
        commits: Vec<CommitInfo>,
        stashes: Vec<StashEntry>,
        remote_list: Vec<RemoteInfo>,
        errors: Vec<String>,
    },
}

/// Execute a GitOperation in the current process (blocking).
/// This function opens the repository at `path` and runs the operation.
/// `progress` is a shared string that the operation can update in real-time for UI display.
pub fn execute_operation(path: &Path, op: GitOperation, progress: Arc<Mutex<String>>) -> OpResult {
    let mut repo = GitRepo::new();
    match repo.open(path) {
        Ok(()) => op.dispatch_with_progress(&repo, progress),
        Err(e) => OpResult::Error(format!("Failed to open repo: {}", e)),
    }
}

impl GitOperation {
    fn dispatch_with_progress(self, repo: &GitRepo, progress: Arc<Mutex<String>>) -> OpResult {
        match self {
            GitOperation::StageAll => Self::simple(repo.stage_all(), "Staged all"),
            GitOperation::UnstageAll => Self::simple(repo.unstage_all(), "Unstaged all"),
            GitOperation::RestoreAll => Self::simple(repo.restore_all(), "Restored all"),
            GitOperation::StageFile(p) => Self::simple(repo.stage_file(&p), format!("Staged {}", p.display())),
            GitOperation::UnstageFile(p) => Self::simple(repo.unstage_file(&p), format!("Unstaged {}", p.display())),
            GitOperation::RestoreFile(p) => Self::simple(repo.restore_file(&p), format!("Restored {}", p.display())),
            GitOperation::Commit { message, amend } => match repo.commit(&message, amend) {
                Ok(sha) => OpResult::Success(format!("Committed: {}", &sha[..sha.len().min(7)])),
                Err(e) => OpResult::Error(e),
            },
            GitOperation::Uncommit => match repo.uncommit() {
                Ok(sha) => OpResult::Success(format!("Uncommitted to {}", &sha[..sha.len().min(7)])),
                Err(e) => OpResult::Error(e),
            },
            GitOperation::CreateBranch { name, base } => {
                Self::simple(repo.create_branch(&name, base.as_deref()), format!("Created branch '{}'", name))
            }
            GitOperation::DeleteBranch { name, force } => {
                // When force is true, we first try regular delete, and if that fails
                // we delete the branch reference directly
                match repo.delete_branch(&name, false) {
                    Ok(()) => OpResult::Success(format!("Deleted '{}'", name)),
                    Err(first_err) => {
                        if force {
                            // Force delete: try to delete reference directly
                            match repo.delete_branch_ref(&name) {
                                Ok(()) => OpResult::Success(format!("Force deleted '{}'", name)),
                                Err(_) => OpResult::Error(format!("Failed to delete '{}': {}", name, first_err)),
                            }
                        } else {
                            OpResult::Error(first_err)
                        }
                    }
                }
            }
            GitOperation::RenameBranch { old, new } => {
                Self::simple(repo.rename_branch(&old, &new), format!("Renamed '{}' -> '{}'", old, new))
            }
            GitOperation::CheckoutBranch(name) => {
                Self::simple(repo.checkout_branch(&name), format!("Switched to '{}'", name))
            }
            GitOperation::MergeBranch(name) => match repo.merge_branch(&name) {
                Ok(msg) => OpResult::Success(msg),
                Err(e) => OpResult::Error(e),
            },
            GitOperation::RemoveWorktree {
                path,
                force,
                expected_git_link,
                require_git_link_identity,
            } => {
                Self::simple(
                    repo.remove_worktree_with_identity(
                        &path,
                        force,
                        expected_git_link,
                        require_git_link_identity,
                    ),
                    {
                    if force { format!("Force removed worktree at {:?}", path) }
                    else { format!("Removed worktree at {:?}", path) }
                    },
                )
            }
            GitOperation::PruneWorktrees => match repo.prune_worktrees() {
                Ok(count) => OpResult::Success(format!("Pruned {} stale worktree(s)", count)),
                Err(e) => OpResult::Error(e),
            },
            GitOperation::CreateWorktree { name, path, branch, new_branch } => {
                match repo.create_worktree(&name, &path, branch.as_deref(), new_branch) {
                    Ok(()) => OpResult::Success(format!("Created worktree '{}' at {:?}", name, path)),
                    Err(e) => OpResult::Error(e),
                }
            }
            GitOperation::StashAll(msg) => {
                Self::simple(repo.stash_all(msg.as_deref()), "Stashed changes")
            }
            GitOperation::StashPop => Self::simple(repo.stash_pop(), "Stash popped"),
            GitOperation::StashApply(index) => match repo.stash_apply_at(index) {
                Ok(()) => OpResult::Success(format!("Applied stash@{{{}}}", index)),
                Err(e) => OpResult::Error(e),
            },
            GitOperation::StashDrop(index) => {
                Self::simple(repo.stash_drop(index), format!("Dropped stash@{{{}}}", index))
            }
            GitOperation::Push { remote, branch, force } => match repo.push(&remote, &branch, force, progress) {
                Ok(msg) => OpResult::Success(msg),
                Err(e) => OpResult::Error(e),
            },
            GitOperation::Pull { remote, branch, rebase } => match repo.pull(&remote, &branch, rebase, progress) {
                Ok(msg) => OpResult::Success(msg),
                Err(e) => OpResult::Error(e),
            },
            GitOperation::Fetch(remote) => match repo.fetch(&remote, progress) {
                Ok(msg) => OpResult::Success(msg),
                Err(e) => OpResult::Error(e),
            },
            GitOperation::GetDiff { path, staged } => match repo.get_diff(&path, staged) {
                Ok(lines) => OpResult::DiffContent { path: path.to_string_lossy().into_owned(), lines },
                Err(e) => OpResult::Error(format!("Diff error: {}", e)),
            },
            GitOperation::LogSearch { filter, request_id } => {
                let commits = repo.log(100).unwrap_or_default();
                let filtered = filter_commits(commits, &filter);
                OpResult::SearchResults {
                    request_id,
                    filter,
                    commits: filtered,
                }
            }
            GitOperation::RefreshAll => {
                let mut errors: Vec<String> = Vec::new();
                let status_entries = repo.get_status().unwrap_or_else(|e| { errors.push(format!("Status: {}", e)); Vec::new() });
                let branches = repo.branches().unwrap_or_else(|e| { errors.push(format!("Branches: {}", e)); Vec::new() });
                let worktrees = repo.worktrees().unwrap_or_else(|e| { errors.push(format!("Worktrees: {}", e)); Vec::new() });
                let commits = repo.log(100).unwrap_or_else(|e| { errors.push(format!("Log: {}", e)); Vec::new() });
                let stashes = repo.stash_list().unwrap_or_else(|e| { errors.push(format!("Stash: {}", e)); Vec::new() });
                let remote_list = repo.remotes().unwrap_or_else(|e| { errors.push(format!("Remotes: {}", e)); Vec::new() });
                OpResult::RefreshData { status_entries, branches, worktrees, commits, stashes, remote_list, errors }
            }
        }
    }

    fn simple(result: GitResult<()>, msg: impl Into<String>) -> OpResult {
        match result {
            Ok(()) => OpResult::Success(msg.into()),
            Err(e) => OpResult::Error(e),
        }
    }
}

pub struct GitRepo {
    repo: RefCell<Option<Repository>>,
    path: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct BranchInfo {
    pub name: String,
    pub is_head: bool,
    pub is_remote: bool,
    pub upstream: Option<String>,
    pub ahead: i32,
    pub behind: i32,
    pub last_commit: Option<String>,
    #[allow(dead_code)]
    pub last_commit_time: Option<String>,
}

#[derive(Clone, Debug)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub sha: String,
    pub is_main: bool,
    pub git_link_identity: Option<WorktreeFileIdentity>,
}

#[derive(Clone, Debug)]
pub struct StatusEntry {
    pub path: PathBuf,
    pub status: char,
    pub staged: bool,
}

#[derive(Clone, Debug)]
pub struct CommitInfo {
    #[allow(dead_code)]
    pub sha: String,
    pub short_sha: String,
    pub author: String,
    pub time: String,
    pub message: String,
    pub summary: String,
}

/// Filter commits using the same fields exposed by the log search UI.
pub fn filter_commits(commits: Vec<CommitInfo>, filter: &str) -> Vec<CommitInfo> {
    if filter.is_empty() {
        return commits;
    }

    let filter = filter.to_lowercase();
    commits
        .into_iter()
        .filter(|commit| {
            commit.message.to_lowercase().contains(&filter)
                || commit.author.to_lowercase().contains(&filter)
                || commit.short_sha.contains(&filter)
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct StashEntry {
    pub index: usize,
    pub message: String,
    pub time: String,
}

#[derive(Clone, Debug)]
pub struct RemoteInfo {
    pub name: String,
    pub url: String,
}

#[derive(Clone, Debug)]
pub struct DiffLine {
    pub origin: char,
    pub content: String,
}

/// Extract a branch name with lossy UTF-8 handling.
/// Falls back to `from_utf8_lossy` if the name contains invalid UTF-8 bytes.
/// This ensures branch names with non-UTF-8 encodings (common on Windows with
/// non-English locales) still display rather than being silently dropped.
fn safe_branch_name(branch: &git2::Branch) -> String {
    // Try the standard UTF-8 name first (returns shorthand like "main")
    if let Ok(Some(name)) = branch.name() {
        return name.to_string();
    }
    // Fall back to raw shorthand bytes with lossy conversion
    // NOTE: use shorthand_bytes() not name_bytes():
    //   name_bytes() returns full ref name ("refs/heads/main")
    //   shorthand_bytes() returns short name ("main")
    let bytes = branch.get().shorthand_bytes();
    String::from_utf8_lossy(bytes).to_string()
}

/// Safely convert an optional `&str` to `String`, falling back to lossy UTF-8
/// conversion from raw bytes when the `&str` is `None` (which happens when
/// git2 encounters non-UTF-8 encoded data).
fn safe_str_lossy(text: Option<&str>, bytes: Option<&[u8]>) -> String {
    text.map(|s| s.to_string())
        .or_else(|| bytes.map(|b| String::from_utf8_lossy(b).to_string()))
        .unwrap_or_default()
}

/// Convenience wrapper for `safe_str_lossy` when bytes are infallible (&[u8]).
fn safe_str_lossy_infallible(text: Option<&str>, bytes: &[u8]) -> String {
    safe_str_lossy(text, Some(bytes))
}

fn path_from_git_bytes(bytes: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        PathBuf::from(OsString::from_vec(bytes.to_vec()))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
    }
}

fn repository_path_bytes(path: &Path) -> Option<Vec<u8>> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;

        Some(path.as_os_str().as_bytes().to_vec())
    }
    #[cfg(not(unix))]
    {
        Some(path.to_str()?.replace('\\', "/").into_bytes())
    }
}

fn index_path_overlaps_any(path: &[u8], changed_paths: &[Vec<u8>]) -> bool {
    fn overlaps(a: &[u8], b: &[u8]) -> bool {
        fn component_prefix(prefix: &[u8], path: &[u8]) -> bool {
            path == prefix
                || (path.starts_with(prefix) && path.get(prefix.len()) == Some(&b'/'))
        }
        component_prefix(a, b) || component_prefix(b, a)
    }

    changed_paths
        .iter()
        .any(|changed_path| overlaps(path, changed_path))
}

/// Compare two paths for equality, handling case-insensitivity and separator normalization on Windows.
fn paths_match(a: &Path, b: &Path) -> bool {
    #[cfg(windows)]
    {
        // Normalize both paths: lowercase, replace / with \
        fn normalize(p: &Path) -> String {
            p.to_string_lossy().to_lowercase().replace('/', "\\")
        }
        normalize(a) == normalize(b)
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}

fn path_entry_exists(path: &Path) -> std::io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

fn path_exists(path: &Path) -> GitResult<bool> {
    path_entry_exists(path)
        .map_err(|e| format!("Check path '{}': {}", path.display(), e))
}

fn collect_diff_paths(diff: &git2::Diff<'_>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for delta in diff.deltas() {
        for path in [delta.old_file().path(), delta.new_file().path()].into_iter().flatten() {
            if !paths.iter().any(|existing: &PathBuf| existing == path) {
                paths.push(path.to_path_buf());
            }
        }
    }
    paths
}

fn paths_overlap(a: &Path, b: &Path) -> bool {
    a == b || a.starts_with(b) || b.starts_with(a)
}

fn worktree_is_case_insensitive(repo: &Repository) -> bool {
    let Some(workdir) = repo.workdir() else {
        return false;
    };
    match (
        std::fs::canonicalize(workdir.join(".git")),
        std::fs::canonicalize(workdir.join(".GIT")),
    ) {
        (Ok(git_dir), Ok(upper_git_dir)) => git_dir == upper_git_dir,
        _ => false,
    }
}

fn paths_overlap_case_insensitively(a: &Path, b: &Path) -> bool {
    let a_components = a
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_lowercase())
        .collect::<Vec<_>>();
    let b_components = b
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_lowercase())
        .collect::<Vec<_>>();
    let shared_components = a_components.len().min(b_components.len());
    shared_components > 0
        && a_components[..shared_components]
            .iter()
            .zip(&b_components[..shared_components])
            .all(|(a, b)| a == b)
}

fn paths_overlap_for_checkout(a: &Path, b: &Path, case_insensitive: bool) -> bool {
    paths_overlap(a, b)
        || (case_insensitive && paths_overlap_case_insensitively(a, b))
}

fn checkout_source_tree(repo: &Repository) -> GitResult<git2::Tree<'_>> {
    match repo.head() {
        Ok(head) => {
            if head.target().is_some() {
                return head
                    .peel_to_tree()
                    .map_err(|e| format!("Current tree: {}", e));
            }
            if !head.is_branch() || head.symbolic_target_bytes().is_none() {
                return Err("Current HEAD has no commit".into());
            }
        }
        Err(error) => {
            if error.code() != git2::ErrorCode::UnbornBranch {
                return Err(format!("HEAD: {}", error));
            }
        }
    }

    let mut index = repo.index().map_err(|e| format!("Index: {}", e))?;
    let tree_id = index
        .write_tree()
        .map_err(|e| format!("Write pre-checkout tree: {}", e))?;
    repo.find_tree(tree_id)
        .map_err(|e| format!("Find pre-checkout tree: {}", e))
}

fn index_bytes_for_tree(
    tree: &git2::Tree<'_>,
    index_path: &Path,
    original_index: Option<&[u8]>,
    changed_paths: &[PathBuf],
) -> GitResult<Vec<u8>> {
    const INDEX_ENTRY_EXTENDED: u16 = 0x4000;
    const INDEX_ENTRY_VALID: u16 = 0x8000;
    const INDEX_ENTRY_SKIP_WORKTREE: u16 = 0x4000;

    let parent = index_path
        .parent()
        .ok_or_else(|| "Index path has no parent".to_string())?;
    let changed_path_bytes = changed_paths
        .iter()
        .filter_map(|path| repository_path_bytes(path))
        .collect::<Vec<_>>();
    if changed_path_bytes.is_empty() {
        if let Some(original_index) = original_index {
            return Ok(original_index.to_vec());
        }
    }
    let include_all_target_entries = original_index.is_none();

    let mut tree_index = git2::Index::new()
        .map_err(|e| format!("Create target tree index: {}", e))?;
    tree_index
        .read_tree(tree)
        .map_err(|e| format!("Read checkout tree into temporary index: {}", e))?;
    let target_entries = tree_index.iter().collect::<Vec<_>>();

    let temp_index_path = tempfile::NamedTempFile::new_in(parent)
        .map_err(|e| format!("Create temporary index file: {}", e))?
        .into_temp_path();
    std::fs::remove_file(&temp_index_path)
        .map_err(|e| format!("Prepare temporary index file: {}", e))?;
    if let Some(bytes) = original_index {
        std::fs::write(&temp_index_path, bytes)
            .map_err(|e| format!("Seed temporary checkout index: {}", e))?;
    }
    let mut index = git2::Index::open(&temp_index_path)
        .map_err(|e| format!("Open temporary index: {}", e))?;
    let original_entries = index
        .iter()
        .map(|entry| (entry.path.clone(), entry.flags, entry.flags_extended))
        .collect::<Vec<_>>();
    let existing_paths = index.iter().map(|entry| entry.path).collect::<Vec<_>>();
    for entry_path in existing_paths {
        if !index_path_overlaps_any(&entry_path, &changed_path_bytes) {
            continue;
        }
        let path = path_from_git_bytes(&entry_path);
        for stage in 0..=3 {
            if index.get_path(&path, stage).is_some() {
                index
                    .remove(&path, stage)
                    .map_err(|e| format!("Remove stale target index entry: {}", e))?;
            }
        }
    }
    for mut entry in target_entries {
        if !include_all_target_entries
            && !index_path_overlaps_any(&entry.path, &changed_path_bytes)
        {
            continue;
        }
        if let Some((_, flags, flags_extended)) = original_entries
            .iter()
            .find(|(path, _, _)| path == &entry.path)
        {
            let skip_worktree = flags_extended & INDEX_ENTRY_SKIP_WORKTREE;
            entry.flags = flags & INDEX_ENTRY_VALID;
            entry.flags_extended = skip_worktree;
            if skip_worktree != 0 {
                entry.flags |= INDEX_ENTRY_EXTENDED;
            }
        }
        index
            .add(&entry)
            .map_err(|e| format!("Add target index entry: {}", e))?;
    }
    index
        .write()
        .map_err(|e| format!("Write temporary checkout index: {}", e))?;
    let bytes = std::fs::read(&temp_index_path)
        .map_err(|e| format!("Read temporary checkout index: {}", e))?;
    drop(temp_index_path);
    Ok(bytes)
}

fn checkout_head_state(repo: &Repository) -> GitResult<(Vec<u8>, bool, Option<git2::Oid>)> {
    match repo.head_detached() {
        Ok(true) => {
            let head = repo.head().map_err(|e| format!("HEAD: {}", e))?;
            Ok((head.name_bytes().to_vec(), false, head.target()))
        }
        Ok(false) => {
            let mut contents = std::fs::read(repo.path().join("HEAD"))
                .map_err(|e| format!("Read symbolic HEAD: {}", e))?;
            while contents.last() == Some(&b'\n') {
                contents.pop();
            }
            let name = contents
                .strip_prefix(b"ref: ")
                .ok_or_else(|| "Symbolic HEAD has invalid contents".to_string())?;
            let target = match repo.head() {
                Ok(head) => head.target(),
                Err(error) if error.code() == git2::ErrorCode::UnbornBranch => None,
                Err(error) => return Err(format!("HEAD: {}", error)),
            };
            Ok((name.to_vec(), true, target))
        }
        Err(error) => Err(format!("HEAD state: {}", error)),
    }
}

fn restore_checkout_head(
    repo: &Repository,
    original_name: &[u8],
    original_symbolic: bool,
    original_target: Option<git2::Oid>,
) -> GitResult<()> {
    if original_symbolic || original_target.is_none() {
        repo.set_head_bytes(original_name)
            .map_err(|e| format!("Restore HEAD: {}", e))?;
    } else {
        repo.set_head_detached(original_target.expect("checked above"))
            .map_err(|e| format!("Restore detached HEAD: {}", e))?;
    }
    Ok(())
}

enum IndexRestore<'a> {
    Always(Option<&'a [u8]>),
    Preserve,
}

fn read_index_snapshot(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

struct IndexLockFile {
    path: PathBuf,
    file: Option<std::fs::File>,
    identity: std::fs::Metadata,
    owned: bool,
}

impl IndexLockFile {
    fn create(index_path: &Path) -> GitResult<Self> {
        let file_name = index_path
            .file_name()
            .ok_or_else(|| "Index path has no file name".to_string())?;
        let mut lock_name = file_name.to_os_string();
        lock_name.push(".lock");
        let path = index_path.with_file_name(lock_name);
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| format!("Acquire index lock: {}", error))?;
        let identity = file
            .metadata()
            .map_err(|error| format!("Inspect index lock: {}", error))?;
        Ok(Self {
            path,
            file: Some(file),
            identity,
            owned: true,
        })
    }

    fn file_mut(&mut self) -> &mut std::fs::File {
        self.file.as_mut().expect("index lock file is open")
    }

    fn verify_owned(&self) -> GitResult<()> {
        let current = std::fs::symlink_metadata(&self.path)
            .map_err(|error| format!("Verify index lock ownership: {}", error))?;
        if !same_file(&current, &self.identity) {
            return Err("Index lock was replaced; preserving current lock".into());
        }
        Ok(())
    }

    fn write_bytes(&mut self, bytes: &[u8]) -> GitResult<()> {
        self.verify_owned()?;
        self.file_mut()
            .write_all(bytes)
            .map_err(|error| format!("Write rollback index: {}", error))?;
        self.file_mut()
            .sync_all()
            .map_err(|error| format!("Flush rollback index: {}", error))?;
        Ok(())
    }

    fn install(&mut self, index_path: &Path) -> GitResult<()> {
        self.verify_owned()?;
        std::fs::rename(&self.path, index_path)
            .map_err(|error| format!("Replace index during rollback: {}", error))?;
        self.owned = false;
        Ok(())
    }
}

impl Drop for IndexLockFile {
    fn drop(&mut self) {
        if !self.owned {
            return;
        }
        if self.verify_owned().is_ok() {
            let _ = std::fs::remove_file(&self.path);
        }
        self.file.take();
    }
}

fn rollback_merge_checkout(
    repo: &Repository,
    old_tree: &git2::Tree<'_>,
    merge_tree: &git2::Tree<'_>,
    index_path: &Path,
    clean_paths: &[PathBuf],
    index_restore: IndexRestore<'_>,
) -> GitResult<()> {
    let mut errors = Vec::new();

    let mut diff_options = DiffOptions::new();
    diff_options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(true)
        .recurse_ignored_dirs(true);
    let paths_to_restore = match repo.diff_tree_to_workdir(Some(merge_tree), Some(&mut diff_options)) {
        Ok(diff) => {
            let dirty_after = collect_diff_paths(&diff);
            let case_insensitive = worktree_is_case_insensitive(repo);
            clean_paths
                .iter()
                .filter(|path| {
                    !dirty_after
                        .iter()
                        .any(|dirty| paths_overlap_for_checkout(path, dirty, case_insensitive))
                })
                .cloned()
                .collect()
        }
        Err(e) => {
            errors.push(format!("Inspect worktree for rollback: {}", e));
            clean_paths.to_vec()
        }
    };

    if !paths_to_restore.is_empty() {
        let mut checkout = git2::build::CheckoutBuilder::new();
        checkout
            .force()
            .update_index(false)
            .overwrite_ignored(false)
            .disable_pathspec_match(true);
        for path in &paths_to_restore {
            checkout.path(path);
        }
        if let Err(e) = repo.checkout_tree(old_tree.as_object(), Some(&mut checkout)) {
            errors.push(format!("Restore worktree: {}", e));
        }
    }

    let index_restore_result = match index_restore {
        IndexRestore::Always(original) => match original {
            Some(bytes) => std::fs::write(index_path, bytes),
            None => match std::fs::remove_file(index_path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                result => result,
            },
        },
        IndexRestore::Preserve => Ok(()),
    };
    if let Err(e) = index_restore_result {
        errors.push(format!("Restore index: {}", e));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Read the main worktree path from the common repository configuration.
///
/// A repository initialized with an external git directory can store this in
/// `core.worktree` (or the main worktree's `config.worktree` file). Relative
/// values are resolved from the common git directory, as Git does.
fn configured_main_worktree_path(repo: &Repository) -> Option<PathBuf> {
    let common_dir = repo.commondir();
    let config_paths = [common_dir.join("config.worktree"), common_dir.join("config")];

    for config_path in config_paths {
        let Ok(config) = git2::Config::open(&config_path) else {
            continue;
        };
        let Ok(path) = config.get_path("core.worktree") else {
            continue;
        };
        if path.as_os_str().is_empty() {
            continue;
        }

        let path = if path.is_absolute() {
            path
        } else {
            common_dir.join(path)
        };
        return Some(std::fs::canonicalize(&path).unwrap_or(path));
    }

    None
}

/// Resolve the git directory referenced by a worktree's `.git` entry.
fn worktree_git_dir(worktree_path: &Path) -> Option<PathBuf> {
    let git_entry = worktree_path.join(".git");
    if git_entry.is_dir() {
        return Some(git_entry);
    }

    let contents = std::fs::read(&git_entry).ok()?;
    let value = gitdir_link_value(&contents)?;
    #[cfg(unix)]
    let git_dir = {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(OsString::from_vec(value.to_vec()))
    };
    #[cfg(not(unix))]
    let git_dir = PathBuf::from(String::from_utf8(value.to_vec()).ok()?);

    if git_dir.is_absolute() {
        Some(git_dir)
    } else {
        git_entry.parent().map(|parent| parent.join(git_dir))
    }
}

/// Confirm that an existing path still contains the linked worktree identified
/// by its Git metadata, rather than merely matching the path recorded by Git.
fn registered_worktree_path_matches(
    repo: &Repository,
    worktree: &git2::Worktree,
    path: &Path,
) -> bool {
    registered_worktree_link_matches(repo, worktree, path, path)
}

fn registered_worktree_link_matches(
    repo: &Repository,
    worktree: &git2::Worktree,
    actual_path: &Path,
    registered_path: &Path,
) -> bool {
    if !paths_match(worktree.path(), registered_path) {
        return false;
    }

    let Some(name) = worktree.name() else {
        return false;
    };
    let Some(actual_git_dir) = worktree_git_dir_from_link(actual_path, registered_path) else {
        return false;
    };
    let Ok(actual_git_dir) = std::fs::canonicalize(actual_git_dir) else {
        return false;
    };
    let expected_git_dir = repo.commondir().join("worktrees").join(name);
    let Ok(expected_git_dir) = std::fs::canonicalize(expected_git_dir) else {
        return false;
    };

    paths_match(&actual_git_dir, &expected_git_dir)
}

fn worktree_git_link(worktree_path: &Path) -> Option<Vec<u8>> {
    let git_entry = worktree_path.join(".git");
    let metadata = std::fs::symlink_metadata(&git_entry).ok()?;
    let file_type = metadata.file_type();
    if !file_type.is_file() && !file_type.is_symlink() {
        return None;
    }
    if !std::fs::metadata(&git_entry).ok()?.is_file() {
        return None;
    }
    let contents = std::fs::read(git_entry).ok()?;
    if gitdir_link_value(&contents).is_some() {
        Some(contents)
    } else {
        None
    }
}

fn gitdir_link_value(contents: &[u8]) -> Option<&[u8]> {
    let end = contents
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map(|index| index + 1)?;
    let value = contents[..end].strip_prefix(b"gitdir:")?;
    let start = value.iter().position(|byte| !byte.is_ascii_whitespace())?;
    Some(&value[start..])
}

fn worktree_git_link_identity(worktree_path: &Path) -> Option<WorktreeFileIdentity> {
    let git_entry = worktree_path.join(".git");
    let metadata = std::fs::symlink_metadata(&git_entry).ok()?;
    let file_type = metadata.file_type();
    if !file_type.is_file() && !file_type.is_symlink() {
        return None;
    }
    let contents = worktree_git_link(worktree_path)?;
    Some(WorktreeFileIdentity {
        metadata,
        contents,
        _lock_file: None,
    })
}

fn same_file(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        a.dev() == b.dev() && a.ino() == b.ino()
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        matches!(
            (
                a.volume_serial_number(),
                a.file_index(),
                b.volume_serial_number(),
                b.file_index()
            ),
            (Some(a_volume), Some(a_index), Some(b_volume), Some(b_index))
                if a_volume == b_volume && a_index == b_index
        )
    }
    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

fn staged_worktree_link_matches(
    staged_path: &Path,
    expected: &WorktreeFileIdentity,
) -> bool {
    let Some(actual) = worktree_git_link_identity(staged_path) else {
        return false;
    };
    same_file(&actual.metadata, &expected.metadata) && actual.contents == expected.contents
}

fn path_identity_matches(path: &Path, expected: &std::fs::Metadata) -> bool {
    std::fs::symlink_metadata(path)
        .map(|actual| same_file(&actual, expected))
        .unwrap_or(false)
}

fn worktree_git_dir_from_link(worktree_path: &Path, relative_base: &Path) -> Option<PathBuf> {
    let contents = worktree_git_link(worktree_path)?;
    let value = gitdir_link_value(&contents)?;
    #[cfg(unix)]
    let git_dir = {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(OsString::from_vec(value.to_vec()))
    };
    #[cfg(not(unix))]
    let git_dir = PathBuf::from(String::from_utf8(value.to_vec()).ok()?);

    if git_dir.is_absolute() {
        Some(git_dir)
    } else {
        Some(relative_base.join(git_dir))
    }
}

fn worktree_staging_path(path: &Path) -> std::io::Result<PathBuf> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "worktree".into());

    for attempt in 0..100 {
        let candidate = parent.join(format!(
            ".{}.git-manager-removing-{}-{}",
            name,
            std::process::id(),
            attempt
        ));
        match std::fs::symlink_metadata(&candidate) {
            Ok(_) => continue,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(candidate),
            Err(e) => return Err(e),
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not find an unused worktree staging path",
    ))
}

fn worktree_is_clean(path: &Path) -> GitResult<bool> {
    let worktree_repo = Repository::open(path).map_err(|e| format!("Check worktree status: {}", e))?;
    let mut status_opts = git2::StatusOptions::new();
    status_opts
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(true)
        .recurse_ignored_dirs(true);
    let statuses = worktree_repo
        .statuses(Some(&mut status_opts))
        .map_err(|e| format!("Check worktree status: {}", e))?;
    Ok(statuses.is_empty())
}

fn worktree_lock_path(repo: &Repository, worktree: &git2::Worktree) -> GitResult<PathBuf> {
    let name = worktree
        .name()
        .ok_or_else(|| "Check worktree lock: worktree name is unavailable".to_string())?;
    Ok(repo.commondir().join("worktrees").join(name).join("locked"))
}

fn worktree_lock_identity(
    repo: &Repository,
    worktree: &git2::Worktree,
) -> GitResult<Option<WorktreeFileIdentity>> {
    let lock_path = worktree_lock_path(repo, worktree)?;
    let entry_metadata = match std::fs::symlink_metadata(&lock_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Check worktree lock: {}", error)),
    };
    if !entry_metadata.file_type().is_file() {
        return Err("Check worktree lock: lock entry is not a regular file".into());
    }
    let mut file = std::fs::File::open(&lock_path)
        .map_err(|error| format!("Check worktree lock: {}", error))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("Check worktree lock: {}", error))?;
    if !metadata.file_type().is_file() || !same_file(&entry_metadata, &metadata) {
        return Err("Check worktree lock: lock entry changed during inspection".into());
    }
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)
        .map_err(|error| format!("Check worktree lock: {}", error))?;
    let current_metadata = std::fs::symlink_metadata(&lock_path)
        .map_err(|error| format!("Check worktree lock: {}", error))?;
    if !same_file(&current_metadata, &metadata) {
        return Err("Check worktree lock: lock entry changed during inspection".into());
    }
    Ok(Some(WorktreeFileIdentity {
        metadata,
        contents,
        _lock_file: Some(Arc::new(file)),
    }))
}

const WORKTREE_LOCK_REASON: &str = "GitManager is removing this worktree";

fn acquire_worktree_lock_with<F>(
    repo: &Repository,
    worktree: &git2::Worktree,
    after_write: F,
) -> GitResult<WorktreeFileIdentity>
where
    F: FnOnce(&Path),
{
    let lock_path = worktree_lock_path(repo, worktree)?;
    let mut lock_options = std::fs::OpenOptions::new();
    lock_options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        lock_options.mode(0o644);
    }
    let mut lock_file = lock_options
        .open(&lock_path)
        .map_err(|error| format!("Lock worktree: {}", error))?;
    let metadata = lock_file.metadata().map_err(|error| {
        format!(
            "Lock worktree: {}; lock entry retained because ownership could not be verified",
            error
        )
    })?;
    let contents = WORKTREE_LOCK_REASON.as_bytes().to_vec();
    lock_file
        .write_all(&contents)
        .and_then(|()| lock_file.flush())
        .map_err(|error| {
            format!(
                "Lock worktree: {}; incomplete lock entry retained",
                error
            )
        })?;

    let identity = WorktreeFileIdentity {
        metadata,
        contents,
        _lock_file: Some(Arc::new(lock_file)),
    };
    after_write(&lock_path);
    let still_owned = worktree_lock_identity_matches(repo, worktree, &identity);
    if !still_owned {
        return Err(
            "Lock worktree: lock entry changed before ownership could be verified; lock retained"
                .into(),
        );
    }
    Ok(identity)
}

fn worktree_lock_status(repo: &Repository, worktree: &git2::Worktree) -> GitResult<git2::WorktreeLockStatus> {
    let Some(identity) = worktree_lock_identity(repo, worktree)? else {
        return Ok(git2::WorktreeLockStatus::Unlocked);
    };
    let reason = if identity.contents.is_empty() {
        None
    } else {
        Some(
            String::from_utf8(identity.contents)
                .map_err(|_| "Check worktree lock: lock reason is not valid UTF-8".to_string())?,
        )
    };
    Ok(git2::WorktreeLockStatus::Locked(reason))
}

fn acquire_worktree_lock(repo: &Repository, worktree: &git2::Worktree) -> GitResult<WorktreeFileIdentity> {
    acquire_worktree_lock_with(repo, worktree, |_| {})
}

fn worktree_lock_identity_matches(
    repo: &Repository,
    worktree: &git2::Worktree,
    expected: &WorktreeFileIdentity,
) -> bool {
    let Ok(Some(actual)) = worktree_lock_identity(repo, worktree) else {
        return false;
    };
    same_file(&actual.metadata, &expected.metadata) && actual.contents == expected.contents
}

fn unlock_worktree_if_owned(
    repo: &Repository,
    worktree: &git2::Worktree,
    expected: &WorktreeFileIdentity,
) -> GitResult<()> {
    if !worktree_lock_identity_matches(repo, worktree, expected) {
        return Err("Worktree lock changed during removal".into());
    }
    worktree.unlock().map_err(|error| format!("Unlock worktree: {}", error))
}

fn remove_worktree_directory(
    repo: &Repository,
    worktree: &git2::Worktree,
    path: &Path,
    force: bool,
    expected_git_link: Option<&WorktreeFileIdentity>,
    require_git_link_identity: bool,
) -> std::io::Result<Option<WorktreeFileIdentity>> {
    if require_git_link_identity && expected_git_link.is_none() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "worktree identity was unavailable when the path was listed",
        ));
    }
    if let Some(expected_git_link) = expected_git_link {
        if !staged_worktree_link_matches(path, expected_git_link) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "directory identity changed since it was listed",
            ));
        }
    }
    if !registered_worktree_path_matches(repo, worktree, path) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "directory identity changed before removal",
        ));
    }
    let expected_git_link = worktree_git_link_identity(path).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            "worktree metadata link is unavailable",
        )
    })?;
    if !registered_worktree_path_matches(repo, worktree, path) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "directory identity changed before removal",
        ));
    }
    let lock_identity = if force {
        None
    } else {
        Some(
            acquire_worktree_lock(repo, worktree)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?,
        )
    };

    let result = (|| {
        if lock_identity.is_some() {
            let clean = worktree_is_clean(path)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
            if !clean {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Cannot remove worktree with uncommitted changes. Use Force Remove to delete it.",
                ));
            }
        }

        let staging_path = worktree_staging_path(path)?;
        if let Err(rename_error) = std::fs::rename(path, &staging_path) {
            if force {
                let path_metadata = std::fs::symlink_metadata(path)?;
                if let Err(force_error) = force_remove_worktree_directory_fallback(
                    path,
                    &path_metadata,
                    &expected_git_link,
                ) {
                    return Err(std::io::Error::new(
                        force_error.kind(),
                        format!(
                            "{}; force cleanup fallback failed: {}",
                            rename_error, force_error
                        ),
                    ));
                }
                return Ok(());
            }
            return Err(rename_error);
        }

        if !staged_worktree_link_matches(&staging_path, &expected_git_link) {
            if let Err(restore_error) = std::fs::rename(&staging_path, path) {
                return Err(std::io::Error::new(
                    restore_error.kind(),
                    format!(
                        "directory identity changed before removal; restore failed: {}",
                        restore_error
                    ),
                ));
            }
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "directory identity changed before removal",
            ));
        }

        let is_safe = || staged_worktree_link_matches(&staging_path, &expected_git_link);
        let result = if force {
            force_remove_dir_checked(&staging_path, is_safe)
        } else {
            remove_dir_checked(&staging_path, is_safe)
        };

        if let Err(error) = result {
            if let Err(restore_error) = std::fs::rename(&staging_path, path) {
                return Err(std::io::Error::new(
                    restore_error.kind(),
                    format!("{}; restore failed: {}", error, restore_error),
                ));
            }
            return Err(error);
        }

        Ok(())
    })();

    if let Err(error) = result {
        if let Some(ref lock_identity) = lock_identity {
            if let Err(unlock_error) = unlock_worktree_if_owned(repo, worktree, lock_identity) {
                return Err(std::io::Error::new(
                    error.kind(),
                    format!("{}; unlock failed: {}", error, unlock_error),
                ));
            }
        }
        return Err(error);
    }

    Ok(lock_identity)
}

/// Find a worktree whose `.git` entry points to the shared git directory.
///
/// This covers `--separate-git-dir` repositories, where the common git
/// directory no longer identifies the main worktree by its parent. Git does
/// not keep a reverse link for this layout, so inspect the current/common
/// directory neighborhoods as a filesystem fallback.
fn find_main_worktree_path(repo: &Repository) -> Option<PathBuf> {
    let common_dir = std::fs::canonicalize(repo.commondir()).ok()?;
    let current_worktree = repo.workdir();
    let mut roots = Vec::new();

    for start in [
        common_dir.parent(),
        current_worktree.and_then(Path::parent),
    ] {
        let mut path = start.map(Path::to_path_buf);
        while let Some(current) = path {
            if !roots.iter().any(|root| root == &current) {
                roots.push(current.clone());
            }
            path = current.parent().map(Path::to_path_buf);
        }
    }

    for root in roots {
        let mut candidates = vec![root.clone()];
        if let Ok(entries) = std::fs::read_dir(&root) {
            candidates.extend(entries.flatten().map(|entry| entry.path()));
        }

        for candidate in candidates {
            if !candidate.is_dir() {
                continue;
            }
            let Some(git_dir) = worktree_git_dir(&candidate) else {
                continue;
            };
            let Ok(git_dir) = std::fs::canonicalize(git_dir) else {
                continue;
            };
            if paths_match(&git_dir, &common_dir) {
                return Some(candidate);
            }
        }
    }

    None
}

/// Determine the main worktree path without confusing an external git
/// directory or linked-worktree metadata directory for the worktree itself.
fn main_worktree_path(repo: &Repository) -> GitResult<PathBuf> {
    if !repo.is_worktree() {
        if let Some(path) = repo.workdir() {
            return Ok(path.to_path_buf());
        }
        if repo.is_bare() {
            return Ok(repo.commondir().to_path_buf());
        }
    }

    if let Some(path) = configured_main_worktree_path(repo) {
        return Ok(path);
    }
    if let Some(path) = find_main_worktree_path(repo) {
        return Ok(path);
    }

    Err("Unable to determine the main worktree path".into())
}

fn remove_dir_checked<F>(path: &Path, is_safe: F) -> std::io::Result<()>
where
    F: Fn() -> bool,
{
    if !path_entry_exists(path)? {
        return Ok(());
    }
    if !is_safe() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "directory identity changed before removal",
        ));
    }
    std::fs::remove_dir_all(path)
}

#[cfg(windows)]
fn force_remove_dir_windows(path: &Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;

    // Delayed expansion occurs after cmd.exe parses command metacharacters, so
    // the path remains a literal operand even when it contains &, |, %, or !.
    const PATH_ENV: &str = "GIT_MANAGER_FORCE_REMOVE_PATH";
    let mut command = std::process::Command::new("cmd.exe");
    command.args(["/d", "/v:on"]).env(PATH_ENV, path);
    command.raw_arg(format!(" /c rmdir /s /q \"!{}!\"", PATH_ENV));
    let status = command.output()?.status;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("cmd.exe rmdir failed with status {}", status),
        ))
    }
}

/// Forcefully remove a directory, with OS-level fallback.
///
/// Used ONLY by Force Remove — regular Remove uses a single gentle attempt.
///
/// Strategy:
/// 1. Try `std::fs::remove_dir_all` (standard recursive delete, blocks until done)
/// 2. On Windows, if that fails, try `cmd.exe /c rmdir /s /q` (bypasses many file locks)
/// 3. If both fail, try one more full round — no sleep between attempts because
///    the delete commands themselves are synchronous and take as long as needed
/// 4. If the path no longer exists at the end, consider it success
fn force_remove_dir(path: &Path) -> std::io::Result<()> {
    force_remove_dir_checked(path, || true)
}

/// Forcefully remove a directory only while a caller-provided identity check
/// continues to confirm that it is safe to remove.
fn force_remove_dir_checked<F>(path: &Path, is_safe: F) -> std::io::Result<()>
where
    F: Fn() -> bool,
{
    if !path_entry_exists(path)? {
        return Ok(());
    }

    let mut last_err = None;

    for _ in 0..2 {
        if !path_entry_exists(path)? {
            return Ok(());
        }
        if !is_safe() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "directory identity changed before removal",
            ));
        }

        // Try standard remove_dir_all (synchronous — blocks until files are deleted)
        match std::fs::remove_dir_all(path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = Some(e);
            }
        }

        // On Windows, try OS-level force delete as fallback.
        // rmdir /s /q is more aggressive and can bypass locks that Rust's std cannot.
        // This command is also synchronous — it runs until the directory is gone or fails.
        #[cfg(windows)]
        {
            if path_entry_exists(path)? {
                if !is_safe() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "directory identity changed before removal",
                    ));
                }
                let _ = force_remove_dir_windows(path);
            }
        }

        // Check if it's gone after the fallback
        if !path_entry_exists(path)? {
            return Ok(());
        }
        // No sleep — the delete commands are already synchronous and may take
        // significant time for large worktrees. The next iteration rechecks
        // the directory identity before trying again.
    }

    // Return the last error if path still exists
    Err(last_err.unwrap_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to remove {:?}", path))
    }))
}

fn force_remove_worktree_directory_fallback(
    path: &Path,
    expected_path: &std::fs::Metadata,
    expected_git_link: &WorktreeFileIdentity,
) -> std::io::Result<()> {
    force_remove_dir_checked(
        path,
        worktree_fallback_identity_guard(path, expected_path, expected_git_link),
    )
}

fn worktree_fallback_identity_guard<'a>(
    path: &'a Path,
    expected_path: &'a std::fs::Metadata,
    expected_git_link: &'a WorktreeFileIdentity,
) -> impl Fn() -> bool + 'a {
    let git_link_was_verified = std::cell::Cell::new(false);
    move || {
        if !path_identity_matches(path, expected_path) {
            return false;
        }

        match path_entry_exists(&path.join(".git")) {
            Ok(false) => git_link_was_verified.get(),
            Ok(true) if staged_worktree_link_matches(path, expected_git_link) => {
                git_link_was_verified.set(true);
                true
            }
            _ => false,
        }
    }
}

/// Check if a path exists and contains any files (not just the directory entry itself).
/// Returns true only if the directory actually has content.
fn dir_has_content(path: &Path) -> bool {
    match std::fs::read_dir(path) {
        Ok(mut entries) => entries.next().is_some(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => {
            // Can't read the directory, treat it as containing content to be safe.
            true
        }
    }
}

fn push_refspec(head_is_branch: bool, branch: &str, force: bool) -> String {
    let source = if head_is_branch {
        format!("refs/heads/{}", branch)
    } else {
        "HEAD".to_string()
    };
    let force_prefix = if force { "+" } else { "" };
    format!("{}{}:refs/heads/{}", force_prefix, source, branch)
}

impl GitRepo {
    pub fn new() -> Self {
        GitRepo { repo: RefCell::new(None), path: None }
    }

    pub fn open(&mut self, path: &Path) -> GitResult<()> {
        let r = Repository::open(path).map_err(|e| format!("Open repo: {}", e))?;
        let p = r.workdir().map(Path::to_path_buf).unwrap_or_else(|| path.to_path_buf());
        *self.repo.borrow_mut() = Some(r);
        self.path = Some(p);
        Ok(())
    }

    pub fn is_open(&self) -> bool { self.repo.borrow().is_some() }
    pub fn path(&self) -> Option<&Path> { self.path.as_deref() }

    fn repo(&self) -> GitResult<std::cell::Ref<'_, Repository>> {
        let r = self.repo.borrow();
        if r.is_some() {
            Ok(std::cell::Ref::map(r, |o| o.as_ref().unwrap()))
        } else {
            Err("No repo open".into())
        }
    }

    fn repo_mut(&self) -> GitResult<std::cell::RefMut<'_, Repository>> {
        let r = self.repo.borrow_mut();
        if r.is_some() {
            Ok(std::cell::RefMut::map(r, |o| o.as_mut().unwrap()))
        } else {
            Err("No repo open".into())
        }
    }

    pub fn current_branch(&self) -> GitResult<String> {
        let repo = self.repo()?;
        let head = repo.head().map_err(|e| format!("Get HEAD: {}", e))?;
        if head.is_branch() {
            match head.shorthand() {
                Some(s) => Ok(s.to_string()),
                None => {
                    let bytes = head.shorthand_bytes();
                    if bytes.is_empty() {
                        Ok("HEAD".to_string())
                    } else {
                        Ok(String::from_utf8_lossy(bytes).to_string())
                    }
                }
            }
        } else {
            let oid = head.target().map(|o| o.to_string()).unwrap_or_default();
            Ok(format!("detached at {}", &oid[..oid.len().min(7)]))
        }
    }

    pub fn head_is_detached(&self) -> GitResult<bool> {
        let repo = self.repo()?;
        repo.head_detached().map_err(|e| format!("HEAD state: {}", e))
    }

    pub fn branches(&self) -> GitResult<Vec<BranchInfo>> {
        let repo = self.repo()?;
        let head_branch = repo.head().ok().and_then(|h| {
            if h.is_branch() { h.shorthand().map(String::from) } else { None }
        });

        let mut branches = Vec::new();
        for bt in &[BranchType::Local, BranchType::Remote] {
            let iter = repo.branches(Some(*bt)).map_err(|e| format!("List branches: {}", e))?;
            for b in iter {
                let (branch, _) = b.map_err(|e| format!("Branch: {}", e))?;
                let name = safe_branch_name(&branch);
                let is_remote = *bt == BranchType::Remote;
                let is_head = !is_remote && Some(name.as_str()) == head_branch.as_deref();

                let (upstream, ahead, behind) = if !is_remote {
                    branch.upstream().ok().and_then(|u| {
                        let uname = u.name().ok().flatten().map(String::from);
                        let (t, ut) = (branch.get().target(), u.get().target());
                        if let (Some(t), Some(ut)) = (t, ut) {
                            let (a, b) = repo.graph_ahead_behind(t, ut).ok()?;
                            Some((uname.unwrap_or_default(), a as i32, b as i32))
                        } else {
                            Some((uname.unwrap_or_default(), 0, 0))
                        }
                    }).unwrap_or((String::new(), 0, 0))
                } else { (String::new(), 0, 0) };

                let (lc, lt) = branch.get().peel_to_commit().ok().map(|c| {
                    let m = c.message().unwrap_or("").lines().next().unwrap_or("").to_string();
                    let t = DateTime::from_timestamp(c.time().seconds(), 0)
                        .map(|d| d.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default();
                    (m, t)
                }).unwrap_or((String::new(), String::new()));

                branches.push(BranchInfo {
                    name, is_head, is_remote,
                    upstream: if upstream.is_empty() { None } else { Some(upstream) },
                    ahead, behind, last_commit: Some(lc), last_commit_time: Some(lt),
                });
            }
        }
        Ok(branches)
    }

    pub fn create_branch(&self, name: &str, base_branch: Option<&str>) -> GitResult<()> {
        let repo = self.repo()?;
        let target_commit = if let Some(base) = base_branch {
            repo.revparse_single(base).map_err(|e| format!("Base '{}': {}", base, e))?
                .peel_to_commit().map_err(|e| format!("Peel: {}", e))?
        } else {
            repo.head().map_err(|e| format!("HEAD: {}", e))?
                .peel_to_commit().map_err(|e| format!("Peel: {}", e))?
        };
        repo.branch(name, &target_commit, false)
            .map_err(|e| format!("Create branch: {}", e))?;
        Ok(())
    }

    pub fn checkout_branch(&self, name: &str) -> GitResult<()> {
        let repo = self.repo()?;
        let (obj, reference) = repo.revparse_ext(name).map_err(|e| format!("Find '{}': {}", name, e))?;
        let resolved_ref = reference.as_ref()
            .and_then(|r| r.name())
            .map(str::to_owned);

        let (original_head_name, original_head_symbolic, original_head_target) =
            checkout_head_state(&repo)?;
        let old_tree = checkout_source_tree(&repo)?;
        let new_tree = obj
            .peel_to_tree()
            .map_err(|e| format!("Checkout tree: {}", e))?;
        let index_path = repo
            .index()
            .map_err(|e| format!("Index: {}", e))?
            .path()
            .map(Path::to_path_buf)
            .ok_or_else(|| "Index path unavailable".to_string())?;
        let original_index = read_index_snapshot(&index_path)
            .map_err(|e| format!("Read index: {}", e))?;

        let changed_diff = repo
            .diff_tree_to_tree(Some(&old_tree), Some(&new_tree), None)
            .map_err(|e| format!("Inspect checkout changes: {}", e))?;
        let changed_paths = collect_diff_paths(&changed_diff);
        drop(changed_diff);
        if changed_paths.is_empty()
            && repo
                .index()
                .map_err(|e| format!("Index: {}", e))?
                .has_conflicts()
        {
            return Err("Checkout: index has unresolved conflicts".into());
        }
        let target_index = index_bytes_for_tree(
            &new_tree,
            &index_path,
            original_index.as_deref(),
            &changed_paths,
        )?;

        let mut workdir_diff_options = DiffOptions::new();
        workdir_diff_options
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_ignored(true)
            .recurse_ignored_dirs(true);
        let workdir_diff = repo
            .diff_tree_to_workdir(Some(&old_tree), Some(&mut workdir_diff_options))
            .map_err(|e| format!("Inspect worktree changes: {}", e))?;
        let dirty_workdir_paths = collect_diff_paths(&workdir_diff);
        drop(workdir_diff);
        let case_insensitive = worktree_is_case_insensitive(&repo);
        let clean_paths = changed_paths
            .iter()
            .filter(|path| {
                !dirty_workdir_paths
                    .iter()
                    .any(|dirty| paths_overlap_for_checkout(path, dirty, case_insensitive))
            })
            .cloned()
            .collect::<Vec<_>>();

        let mut index_lock = IndexLockFile::create(&index_path)?;
        let current_index = read_index_snapshot(&index_path)
            .map_err(|e| format!("Recheck index before checkout: {}", e))?;
        if current_index != original_index {
            return Err("Index changed before checkout; preserving current index".into());
        }
        index_lock.write_bytes(&target_index)?;

        if !changed_paths.is_empty() {
            let mut checkout = git2::build::CheckoutBuilder::new();
            checkout.update_index(false).disable_pathspec_match(true);
            for path in &changed_paths {
                checkout.path(path);
            }
            if let Err(e) = repo.checkout_tree(&obj, Some(&mut checkout)) {
                let checkout_error = format!("Checkout: {}", e);
                return match rollback_merge_checkout(
                    &repo,
                    &old_tree,
                    &new_tree,
                    &index_path,
                    &clean_paths,
                    IndexRestore::Preserve,
                ) {
                    Ok(()) => Err(checkout_error),
                    Err(rollback_error) => Err(format!("{}; {}", checkout_error, rollback_error)),
                };
            }
        }
        let rf = resolved_ref.unwrap_or_else(|| {
            if name.starts_with("refs/") { name.to_string() } else { format!("refs/heads/{}", name) }
        });
        if let Err(e) = repo.set_head(&rf) {
            let head_error = format!("Set HEAD: {}", e);
            return match rollback_merge_checkout(
                &repo,
                &old_tree,
                &new_tree,
                &index_path,
                &clean_paths,
                IndexRestore::Preserve,
            ) {
                Ok(()) => Err(head_error),
                Err(rollback_error) => Err(format!("{}; {}", head_error, rollback_error)),
            };
        }

        if let Err(e) = index_lock.install(&index_path) {
            let replace_error = format!("Install checkout index: {}", e);
            let head_restore = restore_checkout_head(
                &repo,
                &original_head_name,
                original_head_symbolic,
                original_head_target,
            );
            let worktree_restore = rollback_merge_checkout(
                &repo,
                &old_tree,
                &new_tree,
                &index_path,
                &clean_paths,
                IndexRestore::Preserve,
            );
            let mut errors = vec![replace_error];
            if let Err(error) = head_restore {
                errors.push(error);
            }
            if let Err(error) = worktree_restore {
                errors.push(error);
            }
            return Err(errors.join("; "));
        }
        Ok(())
    }

    pub fn rename_branch(&self, old: &str, new: &str) -> GitResult<()> {
        let repo = self.repo()?;
        let mut b = repo.find_branch(old, BranchType::Local)
            .map_err(|e| format!("Find '{}': {}", old, e))?;
        b.rename(new, false).map_err(|e| format!("Rename: {}", e))?;
        Ok(())
    }

    pub fn delete_branch(&self, name: &str, remote: bool) -> GitResult<()> {
        let repo = self.repo()?;
        let bt = if remote { BranchType::Remote } else { BranchType::Local };
        let mut b = repo.find_branch(name, bt)
            .map_err(|e| format!("Find '{}': {}", name, e))?;
        b.delete().map_err(|e| format!("Delete: {}", e))?;
        Ok(())
    }

    /// Force delete a branch by removing its reference directly.
    /// Used when regular delete fails (e.g., unmerged changes).
    pub fn delete_branch_ref(&self, name: &str) -> GitResult<()> {
        let repo = self.repo()?;
        let ref_name = if name.starts_with("refs/") {
            name.to_string()
        } else {
            format!("refs/heads/{}", name)
        };
        repo.find_reference(&ref_name)
            .map_err(|e| format!("Find ref '{}': {}", name, e))?
            .delete()
            .map_err(|e| format!("Delete ref '{}': {}", name, e))?;
        Ok(())
    }

    pub fn merge_branch(&self, branch_name: &str) -> GitResult<String> {
        let repo = self.repo()?;
        let head_ref = repo.head().map_err(|e| format!("HEAD: {}", e))?;
        let original_head_name = head_ref.name_bytes().to_vec();
        let original_head_symbolic_target = head_ref.symbolic_target_bytes().map(|target| target.to_vec());
        let mut update_ref = head_ref.resolve().map_err(|e| format!("Resolve HEAD: {}", e))?;
        let update_ref_name = update_ref.name_bytes().to_vec();
        let head = head_ref.peel_to_commit().map_err(|_| "No commit".to_string())?;

        let their = repo.revparse_single(branch_name)
            .map_err(|e| format!("Find '{}': {}", branch_name, e))?
            .peel_to_commit().map_err(|_| "Not a commit".to_string())?;

        let original_index = repo.index().map_err(|e| format!("Index: {}", e))?;
        if original_index.has_conflicts() {
            return Err("Index has conflicts".into());
        }

        let already_integrated = head.id() == their.id()
            || repo.graph_descendant_of(head.id(), their.id())
                .map_err(|e| format!("Check ancestry: {}", e))?;
        if already_integrated {
            return Ok("Already up to date".into());
        }

        let fast_forward = repo.graph_descendant_of(their.id(), head.id())
            .map_err(|e| format!("Check ancestry: {}", e))?;
        let ours = head.tree().map_err(|e| format!("Tree: {}", e))?;
        let t = if fast_forward {
            their.tree().map_err(|e| format!("Tree: {}", e))?
        } else {
            let base = repo.merge_base(head.id(), their.id())
                .ok()
                .and_then(|oid| repo.find_commit(oid).ok())
                .and_then(|c| c.tree().ok());
            let theirs = their.tree().map_err(|e| format!("Tree: {}", e))?;
            let mut idx = if let Some(ancestor) = base.as_ref() {
                repo.merge_trees(ancestor, &ours, &theirs, None::<&git2::MergeOptions>)
                    .map_err(|e| format!("Merge: {}", e))?
            } else {
                repo.merge_trees(&ours, &ours, &theirs, None::<&git2::MergeOptions>)
                    .map_err(|e| format!("Merge: {}", e))?
            };

            if idx.has_conflicts() { return Err("Merge conflicts".into()); }

            let toid = idx.write_tree_to(&*repo).map_err(|e| format!("Write tree: {}", e))?;
            repo.find_tree(toid).map_err(|e| format!("Find tree: {}", e))?
        };
        let index_path = original_index
            .path()
            .map(Path::to_path_buf)
            .ok_or_else(|| "Index path unavailable".to_string())?;
        let original_index_bytes = std::fs::read(&index_path)
            .map_err(|e| format!("Read index: {}", e))?;

        let staged_diff = repo
            .diff_tree_to_index(Some(&ours), Some(&original_index), None)
            .map_err(|e| format!("Inspect staged changes: {}", e))?;
        let staged_paths = collect_diff_paths(&staged_diff);
        drop(staged_diff);
        let staged_entries = staged_paths
            .iter()
            .map(|path| (path.clone(), original_index.get_path(path, 0)))
            .collect::<Vec<_>>();
        drop(original_index);

        let msg = format!("Merge branch '{}'", branch_name);
        let (commit_oid, ref_message, success_message) = if fast_forward {
            (
                their.id(),
                format!("Fast-forward merge branch '{}'", branch_name),
                format!("Fast-forwarded to branch '{}'", branch_name),
            )
        } else {
            let sig = repo.signature().map_err(|e| format!("Sig: {}", e))?;
            let commit_oid = repo
                .commit(None, &sig, &sig, &msg, &t, &[&head, &their])
                .map_err(|e| format!("Create merge commit: {}", e))?;
            (commit_oid, msg.clone(), msg)
        };

        let merge_diff = repo
            .diff_tree_to_tree(Some(&ours), Some(&t), None)
            .map_err(|e| format!("Inspect merge changes: {}", e))?;
        let changed_paths = collect_diff_paths(&merge_diff);
        drop(merge_diff);

        let mut workdir_diff_options = DiffOptions::new();
        workdir_diff_options
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_ignored(true)
            .recurse_ignored_dirs(true);
        let workdir_diff = repo
            .diff_tree_to_workdir(Some(&ours), Some(&mut workdir_diff_options))
            .map_err(|e| format!("Inspect worktree changes: {}", e))?;
        let dirty_workdir_paths = collect_diff_paths(&workdir_diff);
        drop(workdir_diff);
        let clean_paths = changed_paths
            .iter()
            .filter(|path| !dirty_workdir_paths.iter().any(|dirty| paths_overlap(path, dirty)))
            .cloned()
            .collect::<Vec<_>>();

        // Checkout while HEAD still points to the pre-merge tree. Safe checkout
        // then updates clean merge paths without overwriting unrelated changes.
        // Continue when dirty paths are reported as conflicts so unrelated
        // local changes do not prevent the merge. Never overwrite ignored files.
        let mut co = git2::build::CheckoutBuilder::new();
        co.allow_conflicts(true).overwrite_ignored(false).update_index(false);
        if let Err(e) = repo.checkout_tree(t.as_object(), Some(&mut co)) {
            let checkout_error = format!("Checkout before merge commit: {}", e);
            return match rollback_merge_checkout(
                &repo,
                &ours,
                &t,
                &index_path,
                &clean_paths,
                IndexRestore::Always(Some(&original_index_bytes)),
            ) {
                Ok(()) => Err(checkout_error),
                Err(rollback_error) => Err(format!("{}; {}", checkout_error, rollback_error)),
            };
        }

        let sync_index_result = (|| -> GitResult<()> {
            let mut index = repo.index().map_err(|e| format!("Index: {}", e))?;
            // The commit tree is the new baseline. Reapply any staged paths
            // from before the merge so local staged work remains staged.
            index.read_tree(&t).map_err(|e| format!("Read merge tree into index: {}", e))?;
            for (path, entry) in &staged_entries {
                if let Some(entry) = entry {
                    index.add(entry).map_err(|e| format!("Preserve staged '{}': {}", path.display(), e))?;
                } else if index.get_path(path, 0).is_some() {
                    index.remove(path, 0).map_err(|e| format!("Preserve deletion '{}': {}", path.display(), e))?;
                }
            }
            index.write().map_err(|e| format!("Write merge index: {}", e))?;
            Ok(())
        })();
        if let Err(e) = sync_index_result {
            let sync_error = format!("Synchronize merge index: {}", e);
            return match rollback_merge_checkout(
                &repo,
                &ours,
                &t,
                &index_path,
                &clean_paths,
                IndexRestore::Always(Some(&original_index_bytes)),
            ) {
                Ok(()) => Err(sync_error),
                Err(rollback_error) => Err(format!("{}; {}", sync_error, rollback_error)),
            };
        }

        let current_head = repo.head().map_err(|e| format!("Re-read HEAD: {}", e));
        let ref_update_result = match current_head {
            Ok(current_head) => {
                let current_resolved = current_head.resolve().map_err(|e| format!("Resolve current HEAD: {}", e));
                match current_resolved {
                    Ok(current_resolved)
                        if current_head.name_bytes() == original_head_name.as_slice()
                            && current_head.symbolic_target_bytes().map(|target| target.to_vec())
                                == original_head_symbolic_target
                            && current_resolved.name_bytes() == update_ref_name.as_slice()
                            && current_resolved.target() == Some(head.id()) =>
                    {
                        update_ref
                            .set_target(commit_oid, &ref_message)
                            .map(|_| ())
                            .map_err(|e| format!("Update HEAD: {}", e))
                    }
                    Ok(_) => Err("HEAD changed while merging".into()),
                    Err(e) => Err(e),
                }
            }
            Err(e) => Err(e),
        };
        if let Err(e) = ref_update_result {
            let ref_error = format!("{}", e);
            return match rollback_merge_checkout(
                &repo,
                &ours,
                &t,
                &index_path,
                &clean_paths,
                IndexRestore::Always(Some(&original_index_bytes)),
            ) {
                Ok(()) => Err(ref_error),
                Err(rollback_error) => Err(format!("{}; {}", ref_error, rollback_error)),
            };
        }

        Ok(success_message)
    }

    pub fn worktrees(&self) -> GitResult<Vec<WorktreeInfo>> {
        let repo = self.repo()?;
        let mp = main_worktree_path(&repo)?;
        let mut list = Vec::new();
        list.push(WorktreeInfo {
            path: mp, branch: Some(self.current_branch().unwrap_or_default()),
            sha: repo.head().ok().and_then(|h| h.target().map(|o| o.to_string())).unwrap_or_default(),
            is_main: true,
            git_link_identity: None,
        });

        let names = repo.worktrees().map_err(|e| format!("Worktrees: {}", e))?;
        for name in names.iter().flatten() {
            if let Ok(wt) = repo.find_worktree(name) {
                let wp = wt.path().to_path_buf();
                let git_link_identity = worktree_git_link_identity(&wp);
                if let Ok(r) = Repository::open(&wp) {
                    let branch = r.head().ok().and_then(|h| {
                        if h.is_branch() { h.shorthand().map(String::from) } else { None }
                    });
                    let sha = r.head().ok().and_then(|h| h.target().map(|o| o.to_string())).unwrap_or_default();
                    list.push(WorktreeInfo {
                        path: wp,
                        branch,
                        sha,
                        is_main: false,
                        git_link_identity,
                    });
                } else {
                    list.push(WorktreeInfo {
                        path: wp,
                        branch: None,
                        sha: String::new(),
                        is_main: false,
                        git_link_identity,
                    });
                }
            }
        }
        Ok(list)
    }

    pub fn create_worktree(&self, name: &str, path: &Path, branch: Option<&str>, new_branch: bool) -> GitResult<()> {
        let repo = self.repo()?;

        let branch_ref = if let Some(b) = branch {
            if new_branch {
                let bc = repo.head().map_err(|e| format!("HEAD: {}", e))?
                    .peel_to_commit().map_err(|_| "No commit".to_string())?;
                repo.branch(name, &bc, false).map_err(|e| format!("Create branch: {}", e))?;
                format!("refs/heads/{}", name)
            } else if b.starts_with("refs/") { b.to_string() }
            else { format!("refs/heads/{}", b) }
        } else { return Err("Branch required".into()); };

        let reference = repo.find_reference(&branch_ref).ok();
        let mut opts = WorktreeAddOptions::new();
        if let Some(ref r) = reference {
            opts.reference(Some(r));
        }
        let wt = match repo.worktree(name, path, Some(&opts)) {
            Ok(wt) => wt,
            Err(e) => {
                if new_branch {
                    if let Ok(mut created_branch) = repo.find_reference(&branch_ref) {
                        let _ = created_branch.delete();
                    }
                }
                return Err(format!("Create worktree: {}", e));
            }
        };

        if new_branch {
            if let Ok(wr) = Repository::open(wt.path()) { let _ = wr.set_head(&branch_ref); }
        }
        Ok(())
    }

    /// Remove only stale worktree metadata, matching `git worktree prune`.
    ///
    /// The default libgit2 prune options preserve valid and locked worktrees
    /// and never delete working-tree files.
    pub fn prune_worktrees(&self) -> GitResult<usize> {
        let repo = self.repo()?;
        let names = repo.worktrees().map_err(|e| format!("Worktrees: {}", e))?;
        let mut pruned = 0;

        for name in names.iter().flatten() {
            let wt = repo
                .find_worktree(name)
                .map_err(|e| format!("Find worktree '{}': {}", name, e))?;
            if wt
                .is_prunable(None)
                .map_err(|e| format!("Check worktree '{}': {}", name, e))?
            {
                wt.prune(None)
                    .map_err(|e| format!("Prune worktree '{}': {}", name, e))?;
                pruned += 1;
            }
        }

        Ok(pruned)
    }

    pub fn remove_worktree(&self, path: &Path, force: bool) -> GitResult<()> {
        self.remove_worktree_with_identity(path, force, None, false)
    }

    fn remove_worktree_with_identity(
        &self,
        path: &Path,
        force: bool,
        expected_git_link: Option<WorktreeFileIdentity>,
        require_git_link_identity: bool,
    ) -> GitResult<()> {
        let repo = self.repo()?;
        let wname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let mut errors = Vec::new();

        // Try to find worktree by name (fast path), but verify its path first.
        // A different worktree may have the same basename as the requested path.
        let mut found_wt = repo
            .find_worktree(wname)
            .ok()
            .filter(|wt| paths_match(wt.path(), path));
        
        // Fallback: if name-based lookup fails, iterate through all worktrees
        if found_wt.is_none() {
            if let Ok(names) = repo.worktrees() {
                for name in names.iter().flatten() {
                    if let Ok(wt) = repo.find_worktree(name) {
                        if paths_match(wt.path(), path) {
                            found_wt = Some(wt);
                            break;
                        }
                    }
                }
            }
        }

        if require_git_link_identity && expected_git_link.is_none() && path_exists(path)? {
            return Err("Cannot remove path because its worktree identity was unavailable when it was listed.".into());
        }
        if let Some(ref expected_git_link) = expected_git_link {
            if path_exists(path)? && !staged_worktree_link_matches(path, expected_git_link) {
                return Err("Cannot remove path because its worktree identity changed since it was listed.".into());
            }
        }

        // Never delete an existing path unless Git confirms that it is a
        // registered linked worktree. This protects the main repository,
        // unrelated directories, and paths replaced after a stale selection.
        if let Some(ref wt) = found_wt {
            if path_exists(path)? && !registered_worktree_path_matches(&repo, wt, path) {
                return Err("Cannot remove path because it is no longer the registered worktree.".into());
            }
        } else if path_exists(path)? {
            return Err("Cannot remove path that is not a registered worktree.".into());
        }

        // Regular removal must not discard uncommitted work. Check the
        // worktree before pruning metadata or deleting its directory.
        if !force && path_exists(path)? {
            if !worktree_is_clean(path)? {
                return Err("Cannot remove worktree with uncommitted changes. Use Force Remove to delete it.".into());
            }
        }

        if !force {
            if let Some(ref wt) = found_wt {
                match worktree_lock_status(&repo, wt)? {
                    git2::WorktreeLockStatus::Unlocked => {}
                    git2::WorktreeLockStatus::Locked(reason) => {
                        let reason = reason
                            .map(|reason| format!(": {}", reason))
                            .unwrap_or_default();
                        return Err(format!("Cannot remove locked worktree{}.", reason));
                    }
                }
            }
        }

        // --- Directory cleanup: different strategy for Remove vs Force Remove ---
        let (mut directory_removed, mut lock_identity) = if path_exists(path)? {
            let path_is_registered = found_wt
                .as_ref()
                .map(|wt| registered_worktree_path_matches(&repo, wt, path))
                .unwrap_or(false);
            if !path_is_registered {
                return Err("Cannot remove path because it is no longer a registered worktree.".into());
            }

            let worktree = found_wt
                .as_ref()
                .expect("registered worktree validated above");
            match remove_worktree_directory(
                &repo,
                worktree,
                path,
                force,
                expected_git_link.as_ref(),
                require_git_link_identity,
            ) {
                Ok(lock_identity) => (true, lock_identity),
                Err(e) => {
                    errors.push(format!("Rm dir: {}", e));
                    if !force && dir_has_content(path) {
                        errors.push("Directory still contains files. Use Force Remove to delete it.".into());
                    }
                    (false, None)
                }
            }
        } else {
            (true, None)
        };

        if directory_removed && require_git_link_identity && expected_git_link.is_none() && path_exists(path)? {
            errors.push("Cannot remove path because its worktree identity was unavailable when it was listed.".into());
            directory_removed = false;
        }

        if directory_removed && !force && lock_identity.is_none() {
            if let Some(ref wt) = found_wt {
                match acquire_worktree_lock(&repo, wt) {
                    Ok(identity) => lock_identity = Some(identity),
                    Err(error) => {
                        errors.push(error);
                        directory_removed = false;
                    }
                }
            }
        }

        // Prune only Git's worktree metadata. The directory was removed above
        // only after validating its .git link, so libgit2 must not recursively
        // delete the working tree itself from stale metadata.
        if directory_removed {
            if let Some(ref wt) = found_wt {
                let lock_owned = lock_identity
                    .as_ref()
                    .map(|identity| worktree_lock_identity_matches(&repo, wt, identity))
                    .unwrap_or(true);
                let prune_result = if !lock_owned {
                    errors.push("Worktree lock changed during removal.".into());
                    None
                } else if lock_identity.is_some() {
                    let mut opts = WorktreePruneOptions::new();
                    opts.valid(true).locked(true);
                    Some(wt.prune(Some(&mut opts)))
                } else if force {
                    let mut opts = WorktreePruneOptions::new();
                    opts.valid(true); // Prune even if the worktree is valid
                    opts.locked(true); // Prune even if locked
                    Some(wt.prune(Some(&mut opts)))
                } else {
                    Some(wt.prune(None))
                };

                if let Some(Err(e)) = prune_result {
                    if force || lock_identity.is_some() {
                        // Force removal, or a normal removal holding its own
                        // temporary lock, may clean up metadata after prune
                        // refuses. A pre-existing lock never reaches here.
                        let mut fallback_error = None;
                        let lock_owned = lock_identity
                            .as_ref()
                            .map(|identity| worktree_lock_identity_matches(&repo, wt, identity))
                            .unwrap_or(true);
                        if !lock_owned {
                            fallback_error = Some("Worktree lock changed during removal".into());
                        } else if let Some(name) = wt.name() {
                            let wt_gitdir = repo.commondir().join("worktrees").join(name);
                            match path_entry_exists(&wt_gitdir) {
                                Ok(true) => {
                                    if let Err(error) = std::fs::remove_dir_all(&wt_gitdir) {
                                        fallback_error = Some(format!("Remove git metadata: {}", error));
                                    }
                                }
                                Ok(false) => {}
                                Err(error) => fallback_error = Some(format!("Check git metadata: {}", error)),
                            }
                        } else {
                            fallback_error = Some("worktree name is unavailable".into());
                        }

                        if let Some(fallback_error) = fallback_error {
                            errors.push(format!("Prune: {}; {}", e, fallback_error));
                            if let Some(ref identity) = lock_identity {
                                if worktree_lock_identity_matches(&repo, wt, identity) {
                                    if let Err(unlock_error) = unlock_worktree_if_owned(&repo, wt, identity) {
                                        errors.push(format!("Unlock worktree: {}", unlock_error));
                                    }
                                }
                            }
                        }
                    } else {
                        // A normal removal must respect Git's lock refusal;
                        // do not delete metadata behind libgit2's back.
                        errors.push(format!("Remove: {}", e));
                    }
                }
            }
        }

        // If directory no longer has content, consider it a success
        // (primary user concern is getting rid of the files on disk)
        if directory_removed && errors.is_empty() && !dir_has_content(path) {
            return Ok(());
        }
        if errors.is_empty() {
            errors.push("Directory still exists after removal attempt.".into());
        }

        // Everything we tried failed — report all errors
        Err(errors.join("; "))
    }

    pub fn get_status(&self) -> GitResult<Vec<StatusEntry>> {
        let repo = self.repo()?;
        let mut entries = Vec::new();
        let ss = repo.statuses(Some(
            git2::StatusOptions::new().include_untracked(true).recurse_untracked_dirs(true)
                .show(git2::StatusShow::IndexAndWorkdir),
        )).map_err(|e| format!("Status: {}", e))?;

        for e in ss.iter() {
            let p = path_from_git_bytes(e.path_bytes());
            let f = e.status();
            let staged = f.intersects(Status::INDEX_NEW | Status::INDEX_MODIFIED | Status::INDEX_DELETED
                | Status::INDEX_RENAMED | Status::INDEX_TYPECHANGE);
            let unstaged = f.intersects(Status::WT_NEW | Status::WT_MODIFIED | Status::WT_DELETED
                | Status::WT_RENAMED | Status::WT_TYPECHANGE | Status::CONFLICTED);

            let s = if f.intersects(Status::CONFLICTED) { 'U' }
                else if f.intersects(Status::INDEX_NEW) { 'A' }
                else if f.intersects(Status::INDEX_DELETED) { 'D' }
                else if f.intersects(Status::WT_NEW) { '?' }
                else if f.intersects(Status::WT_DELETED) { 'D' }
                else { 'M' };

            if staged && !unstaged { entries.push(StatusEntry { path: p, status: s, staged: true }); }
            else if !staged && unstaged { entries.push(StatusEntry { path: p, status: s, staged: false }); }
            else {
                entries.push(StatusEntry { path: p.clone(), status: s, staged: true });
                entries.push(StatusEntry { path: p, status: s, staged: false });
            }
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(entries)
    }

    pub fn stage_file<P: AsRef<Path>>(&self, path: P) -> GitResult<()> {
        let repo = self.repo()?;
        let mut idx = repo.index().map_err(|e| format!("Index: {}", e))?;
        idx.add_path(path.as_ref()).map_err(|e| format!("Stage: {}", e))?;
        idx.write().map_err(|e| format!("Write: {}", e))?;
        Ok(())
    }

    pub fn unstage_file<P: AsRef<Path>>(&self, path: P) -> GitResult<()> {
        let path = path.as_ref();
        let path_bytes = repository_path_bytes(path)
            .ok_or_else(|| "Unstage: path must be valid UTF-8".to_string())?;
        if path_bytes.is_empty() {
            return Err("Unstage: path must not be empty".into());
        }
        if path_bytes.contains(&0) {
            return Err("Unstage: path must not contain NUL bytes".into());
        }
        let has_invalid_component = path_bytes
            .split(|byte| *byte == b'/')
            .any(|component| component.is_empty() || component == b"." || component == b"..");
        if has_invalid_component {
            return Err("Unstage: path must be repository-relative".into());
        }
        if path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err("Unstage: path must be repository-relative".into());
        }

        let repo = self.repo()?;
        let ignore_case = repo
            .config()
            .ok()
            .and_then(|config| config.get_bool("core.ignorecase").ok())
            .unwrap_or(false);
        let normalized_path = path_bytes;
        let mut directory_prefix = normalized_path.clone();
        directory_prefix.push(b'/');
        let matches_path = |entry_path: &[u8]| {
            if ignore_case {
                entry_path.eq_ignore_ascii_case(&normalized_path)
                    || entry_path
                        .get(..directory_prefix.len())
                        .map(|prefix| prefix.eq_ignore_ascii_case(&directory_prefix))
                        .unwrap_or(false)
            } else {
                entry_path == normalized_path.as_slice()
                    || entry_path.starts_with(&directory_prefix)
            }
        };
        let matching_index_paths = |idx: &git2::Index| {
            let mut paths: Vec<_> = idx
                .iter()
                .filter_map(|entry| {
                    matches_path(&entry.path).then(|| path_from_git_bytes(&entry.path))
                })
                .collect();
            paths.sort_unstable();
            paths.dedup();
            paths
        };

        match repo.head() {
            Ok(head) => {
                let head = head.peel_to_commit().map_err(|e| format!("Peel: {}", e))?;
                let head_tree = head.tree().map_err(|e| format!("Tree: {}", e))?;
                let mut head_index = git2::Index::new().map_err(|e| format!("Index: {}", e))?;
                head_index.read_tree(&head_tree).map_err(|e| format!("Read HEAD index: {}", e))?;
                let head_entries: Vec<_> = head_index
                    .iter()
                    .filter(|entry| matches_path(&entry.path))
                    .collect();

                let mut idx = repo.index().map_err(|e| format!("Index: {}", e))?;
                for index_path in matching_index_paths(&idx) {
                    idx.remove_path(&index_path).map_err(|e| format!("Unstage: {}", e))?;
                }
                for entry in head_entries {
                    idx.add(&entry).map_err(|e| format!("Unstage: {}", e))?;
                }
                idx.write().map_err(|e| format!("Write: {}", e))?;
            }
            Err(e) if e.code() == git2::ErrorCode::UnbornBranch => {
                let mut idx = repo.index().map_err(|e| format!("Index: {}", e))?;
                for index_path in matching_index_paths(&idx) {
                    idx.remove_path(&index_path).map_err(|e| format!("Unstage: {}", e))?;
                }
                idx.write().map_err(|e| format!("Write: {}", e))?;
            }
            Err(e) => return Err(format!("HEAD: {}", e)),
        }
        Ok(())
    }

    pub fn stage_all(&self) -> GitResult<()> {
        let repo = self.repo()?;
        let mut idx = repo.index().map_err(|e| format!("Index: {}", e))?;
        idx.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .map_err(|e| format!("Stage all: {}", e))?;
        idx.write().map_err(|e| format!("Write: {}", e))?;
        Ok(())
    }

    pub fn unstage_all(&self) -> GitResult<()> {
        let repo = self.repo()?;
        let target = match repo.head() {
            Ok(head) => Some(
                head.peel_to_commit()
                    .map_err(|e| format!("HEAD commit: {}", e))?,
            ),
            Err(e) if e.code() == git2::ErrorCode::UnbornBranch => None,
            Err(e) => return Err(format!("HEAD: {}", e)),
        };
        repo.reset_default(target.as_ref().map(|commit| commit.as_object()), ["*"])
            .map_err(|e| format!("Unstage all: {}", e))
    }

    pub fn restore_file<P: AsRef<Path>>(&self, path: P) -> GitResult<()> {
        let repo = self.repo()?;
        let mut cb = git2::build::CheckoutBuilder::new();
        cb.force()
            .disable_pathspec_match(true)
            .path(path.as_ref());
        repo.checkout_index(None, Some(&mut cb))
            .map_err(|e| format!("Restore: {}", e))?;
        Ok(())
    }

    pub fn restore_all(&self) -> GitResult<()> {
        let repo = self.repo()?;
        let hc = repo.head().map_err(|e| format!("HEAD: {}", e))?
            .peel_to_commit().map_err(|e| format!("Peel: {}", e))?;
        let mut cb = git2::build::CheckoutBuilder::new();
        cb.force();
        repo.checkout_tree(hc.as_object(), Some(&mut cb))
            .map_err(|e| format!("Checkout: {}", e))?;
        Ok(())
    }

    pub fn get_diff<P: AsRef<Path>>(&self, path: P, staged: bool) -> GitResult<Vec<DiffLine>> {
        let path = path.as_ref();
        let repo = self.repo()?;
        let mut lines = Vec::new();
        let tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
        let mut dopts = DiffOptions::new();
        dopts.disable_pathspec_match(true).pathspec(path);
        if !staged {
            dopts
                .include_untracked(true)
                .recurse_untracked_dirs(true)
                .show_untracked_content(true);
        }
        let idx = repo.index().map_err(|e| format!("Index: {}", e))?;

        let diff = if staged {
            repo.diff_tree_to_index(tree.as_ref(), Some(&idx), Some(&mut dopts))
                .map_err(|e| format!("Diff: {}", e))?
        } else {
            repo.diff_index_to_workdir(Some(&idx), Some(&mut dopts))
                .map_err(|e| format!("Diff: {}", e))?
        };

        diff.foreach(
            &mut |_, _| true, None, None,
            Some(&mut |_, _, line| {
                lines.push(DiffLine {
                    origin: line.origin(),
                    content: String::from_utf8_lossy(line.content()).to_string(),
                });
                true
            }),
        ).map_err(|e| format!("Diff foreach: {}", e))?;

        Ok(lines)
    }

    pub fn commit(&self, message: &str, amend: bool) -> GitResult<String> {
        let repo = self.repo_mut()?;
        let sig = repo.signature().map_err(|e| format!("Sig: {}", e))?;

        if amend {
            let hc = repo.head().map_err(|e| format!("HEAD: {}", e))?
                .peel_to_commit().map_err(|e| format!("Peel: {}", e))?;
            let toid = repo.index().and_then(|mut i| i.write_tree())
                .map_err(|e| format!("Write tree: {}", e))?;
            let t = repo.find_tree(toid).map_err(|e| format!("Find tree: {}", e))?;
            let parents: Vec<git2::Commit> = (0..hc.parent_count()).filter_map(|i| hc.parent(i).ok()).collect();
            let pref: Vec<&git2::Commit> = parents.iter().collect();
            return repo.commit(Some("HEAD"), &sig, &sig, message, &t, &pref)
                .map(|o| o.to_string()).map_err(|e| format!("Amend: {}", e));
        }

        let toid = repo.index().map_err(|e| format!("Index: {}", e))
            .and_then(|mut i| i.write_tree().map_err(|e| format!("Write tree: {}", e)))?;
        let t = repo.find_tree(toid).map_err(|e| format!("Find tree: {}", e))?;
        let pc = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        let parents: Vec<&git2::Commit> = pc.iter().collect();

        repo.commit(Some("HEAD"), &sig, &sig, message, &t, &parents)
            .map(|o| o.to_string()).map_err(|e| format!("Commit: {}", e))
    }

    pub fn uncommit(&self) -> GitResult<String> {
        let repo = self.repo()?;
        let hc = repo.head().map_err(|e| format!("HEAD: {}", e))?
            .peel_to_commit().map_err(|e| format!("Peel: {}", e))?;
        let parent = hc.parent(0).map_err(|_| "No parent commit".to_string())?;
        let parent_id = parent.id();

        // A soft reset moves the current branch ref while leaving both the
        // index and working tree untouched, preserving all local changes.
        repo.reset(parent.as_object(), git2::ResetType::Soft, None)
            .map_err(|e| format!("Reset: {}", e))?;

        Ok(parent_id.to_string())
    }

    pub fn stash_all(&self, message: Option<&str>) -> GitResult<()> {
        let mut repo = self.repo_mut()?;
        let sig = repo.signature().map_err(|e| format!("Sig: {}", e))?;
        let msg = message.unwrap_or("");
        repo.stash_save(&sig, msg, Some(git2::StashFlags::INCLUDE_UNTRACKED))
            .map(|_| ()).map_err(|e| format!("Stash: {}", e))
    }

    pub fn stash_pop(&self) -> GitResult<()> {
        let mut repo = self.repo_mut()?;
        repo.stash_pop(0, None).map_err(|e| format!("Pop: {}", e))
    }

    #[allow(dead_code)]
    pub fn stash_apply(&self) -> GitResult<()> {
        let mut repo = self.repo_mut()?;
        repo.stash_apply(0, None).map_err(|e| format!("Apply: {}", e))
    }

    pub fn stash_apply_at(&self, index: usize) -> GitResult<()> {
        let mut repo = self.repo_mut()?;
        repo.stash_apply(index, None).map_err(|e| format!("Apply stash@{}: {}", index, e))
    }

    pub fn stash_drop(&self, index: usize) -> GitResult<()> {
        let mut repo = self.repo_mut()?;
        repo.stash_drop(index).map_err(|e| format!("Drop: {}", e))
    }

    pub fn stash_list(&self) -> GitResult<Vec<StashEntry>> {
        let repo = self.repo()?;
        let mut stash_data = Vec::new();
        let repo_path = repo.path().to_path_buf();
        drop(repo);

        let mut repo2 = Repository::open(&repo_path).map_err(|e| format!("Open: {}", e))?;
        repo2.stash_foreach(|idx, name, oid| {
            stash_data.push((idx, name.to_string(), *oid));
            true
        }).map_err(|e| format!("Stash list: {}", e))?;
        drop(repo2);

        let repo3 = Repository::open(&repo_path).map_err(|e| format!("Open: {}", e))?;
        let mut entries = Vec::new();
        for (idx, name, oid) in stash_data {
            let time = repo3.find_commit(oid).ok().map(|c| {
                DateTime::from_timestamp(c.time().seconds(), 0)
                    .map(|d| d.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default()
            }).unwrap_or_default();
            entries.push(StashEntry { index: idx, message: name, time });
        }
        Ok(entries)
    }

    pub fn log(&self, max_count: usize) -> GitResult<Vec<CommitInfo>> {
        let repo = self.repo()?;
        let mut rw = repo.revwalk().map_err(|e| format!("Revwalk: {}", e))?;
        rw.push_head().map_err(|e| format!("Push HEAD: {}", e))?;
        rw.set_sorting(git2::Sort::TIME).ok();

        let mut commits = Vec::new();
        for oid in rw.take(max_count) {
            let oid = oid.map_err(|e| format!("Oid: {}", e))?;
            if let Ok(c) = repo.find_commit(oid) {
                commits.push(CommitInfo {
                    sha: oid.to_string(),
                    short_sha: oid.to_string().get(..7).unwrap_or("").to_string(),
                    author: safe_str_lossy_infallible(c.author().name(), c.author().name_bytes()),
                    time: DateTime::from_timestamp(c.time().seconds(), 0)
                        .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_default(),
                    message: safe_str_lossy_infallible(c.message(), c.message_bytes()),
                    summary: safe_str_lossy(c.summary(), c.summary_bytes()),
                });
            }
        }
        Ok(commits)
    }

    pub fn push(&self, remote: &str, branch: &str, force: bool, progress: Arc<Mutex<String>>) -> GitResult<String> {
        let repo = self.repo_mut()?;
        let prog = progress.clone();
        let mut cb = git2::RemoteCallbacks::new();
        cb.sideband_progress(move |data| {
            if let Ok(mut p) = prog.lock() {
                *p = String::from_utf8_lossy(data).to_string();
            }
            true
        });
        cb.push_update_reference(|refname, status| {
            if let Some(status) = status {
                return Err(git2::Error::from_str(&format!(
                    "Push {} rejected: {}",
                    refname, status
                )));
            }
            Ok(())
        });
        let mut fo = git2::PushOptions::new();
        fo.remote_callbacks(cb);
        let head_is_branch = repo.head().map_err(|e| format!("HEAD: {}", e))?.is_branch();
        let rs = push_refspec(head_is_branch, branch, force);
        let mut rm = repo.find_remote(remote).map_err(|e| format!("Remote: {}", e))?;
        rm.push(&[&rs], Some(&mut fo)).map_err(|e| format!("Push: {}", e))?;
        Ok(format!("Pushed {}", branch))
    }

        pub fn fetch(&self, remote: &str, progress: Arc<Mutex<String>>) -> GitResult<String> {
        let repo = self.repo_mut()?;
        let prog = progress.clone();
        let mut cb = git2::RemoteCallbacks::new();
        cb.sideband_progress(move |data| {
            if let Ok(mut p) = prog.lock() {
                *p = String::from_utf8_lossy(data).to_string();
            }
            true
        });
        let mut fo = git2::FetchOptions::new();
        fo.remote_callbacks(cb);
        let spec = format!("+refs/heads/*:refs/remotes/{}/*", remote);
        let mut rm = repo.find_remote(remote).map_err(|e| format!("Remote: {}", e))?;
        rm.fetch(&[&spec], Some(&mut fo), None)
            .map_err(|e| format!("Fetch: {}", e))?;
        Ok(format!("Fetched from {}", remote))
    }

    pub fn pull(&self, remote: &str, branch: &str, rebase: bool, progress: Arc<Mutex<String>>) -> GitResult<String> {
        let repo = self.repo_mut()?;
        let prog = progress.clone();
        let mut cb = git2::RemoteCallbacks::new();
        cb.sideband_progress(move |data| {
            if let Ok(mut p) = prog.lock() {
                *p = String::from_utf8_lossy(data).to_string();
            }
            true
        });
        let mut fo = git2::FetchOptions::new();
        fo.remote_callbacks(cb);
        let rs = format!("+refs/heads/{}:refs/remotes/{}/{}", branch, remote, branch);
        let mut rm = repo.find_remote(remote).map_err(|e| format!("Remote: {}", e))?;
        rm.fetch(&[&rs], Some(&mut fo), None).map_err(|e| format!("Fetch: {}", e))?;

        let rbref = format!("refs/remotes/{}/{}", remote, branch);
        let fc = repo.find_reference(&rbref)
            .map_err(|e| format!("Ref '{}': {}", rbref, e))?
            .peel_to_commit().map_err(|e| format!("Peel: {}", e))?;

        if rebase {
            let ac = repo.find_annotated_commit(fc.id())
                .map_err(|e| format!("Annotated: {}", e))?;
            let mut ropts = git2::RebaseOptions::new();
            ropts.checkout_options(git2::build::CheckoutBuilder::new());
            let mut reb = repo.rebase(None, Some(&ac), None, Some(&mut ropts))
                .map_err(|e| format!("Rebase: {}", e))?;
            let result = (|| -> GitResult<()> {
                while let Some(op) = reb.next() {
                    let _ = op.map_err(|e| format!("Op: {}", e))?;
                    let sg = repo.signature().map_err(|e| format!("Sig: {}", e))?;
                    reb.commit(None, &sg, None).map_err(|e| format!("Rebase commit: {}", e))?;
                }
                reb.finish(None).map_err(|e| format!("Finish: {}", e))?;
                Ok(())
            })();
            match result {
                Ok(()) => Ok("Rebase complete".into()),
                Err(e) => {
                    let _ = reb.abort();
                    Err(e)
                }
            }
        } else {
            let hc = repo.head().map_err(|e| format!("HEAD: {}", e))?
                .peel_to_commit().map_err(|e| format!("Peel: {}", e))?;

            if repo.index().map_err(|e| format!("Index: {}", e))?.has_conflicts() {
                return Err("Pulling is not possible because you have unmerged files".into());
            }

            if hc.id() == fc.id()
                || repo.graph_descendant_of(hc.id(), fc.id())
                    .map_err(|e| format!("Check ancestry: {}", e))?
            {
                return Ok("Already up to date".into());
            }

            if repo.graph_descendant_of(fc.id(), hc.id())
                .map_err(|e| format!("Check ancestry: {}", e))?
            {
                let mut co = git2::build::CheckoutBuilder::new();
                repo.checkout_tree(fc.as_object(), Some(&mut co))
                    .map_err(|e| format!("Checkout fast-forward: {}", e))?;

                let mut update_ref = repo
                    .head()
                    .map_err(|e| format!("HEAD: {}", e))?
                    .resolve()
                    .map_err(|e| format!("Resolve HEAD: {}", e))?;
                update_ref
                    .set_target(fc.id(), "Fast-forward pull")
                    .map_err(|e| format!("Fast-forward HEAD: {}", e))?;

                return Ok("Fast-forward complete".into());
            }

            let base = repo.merge_base(hc.id(), fc.id())
                .ok()
                .and_then(|oid| repo.find_commit(oid).ok())
                .and_then(|c| c.tree().ok());

            let ours = hc.tree().map_err(|e| format!("Tree: {}", e))?;
            let theirs = fc.tree().map_err(|e| format!("Tree: {}", e))?;

            let mut idx = if let Some(ancestor) = base.as_ref() {
                repo.merge_trees(ancestor, &ours, &theirs, None::<&git2::MergeOptions>)
                    .map_err(|e| format!("Merge: {}", e))?
            } else {
                repo.merge_trees(&ours, &ours, &theirs, None::<&git2::MergeOptions>)
                    .map_err(|e| format!("Merge: {}", e))?
            };
            if idx.has_conflicts() { return Err("Merge conflicts".into()); }

            let sg = repo.signature().map_err(|e| format!("Sig: {}", e))?;
            let toid = idx.write_tree_to(&*repo).map_err(|e| format!("Write tree: {}", e))?;
            let t = repo.find_tree(toid).map_err(|e| format!("Find tree: {}", e))?;

            let mut co = git2::build::CheckoutBuilder::new();
            repo.checkout_tree(t.as_object(), Some(&mut co))
                .map_err(|e| format!("Checkout before merge: {}", e))?;

            repo.commit(Some("HEAD"), &sg, &sg,
                &format!("Merge '{}'", remote), &t, &[&hc, &fc])
                .map_err(|e| format!("Merge commit: {}", e))?;

            Ok("Merge complete".into())
        }
    }

    pub fn remotes(&self) -> GitResult<Vec<RemoteInfo>> {
        let repo = self.repo()?;
        let mut list = Vec::new();
        for name in repo.remotes().map_err(|e| format!("Remotes: {}", e))?.iter().flatten() {
            if let Ok(rm) = repo.find_remote(name) {
                list.push(RemoteInfo {
                    name: name.to_string(),
                    url: rm.url().unwrap_or("").to_string(),
                });
            }
        }
        Ok(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::Repository;

    /// Helper to create a temporary git repo with an initial commit
    fn create_repo_with_commit(dir: &Path) -> Repository {
        let repo = Repository::init(dir).expect("init repo");
        let sig = repo.signature().expect("signature");
        let tree_oid = {
            let mut idx = repo.index().expect("index");
            idx.write_tree().expect("write tree")
        };
        let tree = repo.find_tree(tree_oid).expect("find tree");
        repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
            .expect("initial commit");
        drop(tree);
        repo
    }

    /// Helper: create a GitRepo (our wrapper) from a temp directory path
    fn open_git_repo(repo_dir: &Path) -> GitRepo {
        let mut git = GitRepo::new();
        git.open(repo_dir).expect("open repo");
        git
    }

    fn local_remote_url(path: &Path) -> String {
        url::Url::from_file_path(path)
            .expect("local remote path")
            .to_string()
    }

    #[test]
    fn test_acquired_worktree_lock_rejects_replaced_lock_entry() {
        let main_dir = tempfile::tempdir().expect("main temp dir");
        let wt_root = tempfile::tempdir().expect("worktree temp dir");
        let wt_path = wt_root.path().join("lock-race-wt");
        let repo = create_repo_with_commit(main_dir.path());
        let commit = repo.head().expect("HEAD").peel_to_commit().expect("commit");
        let branch = repo.branch("lock-race-wt", &commit, false).expect("branch");
        let reference = repo
            .find_reference("refs/heads/lock-race-wt")
            .expect("reference");
        let mut opts = git2::WorktreeAddOptions::new();
        opts.reference(Some(&reference));
        repo.worktree("lock-race-wt", &wt_path, Some(&opts))
            .expect("create worktree");
        let worktree = repo.find_worktree("lock-race-wt").expect("find worktree");
        let lock_path = worktree_lock_path(&repo, &worktree).expect("worktree lock path");
        let replacement_reason = WORKTREE_LOCK_REASON;
        let result = acquire_worktree_lock_with(&repo, &worktree, |lock_path| {
            std::fs::remove_file(lock_path).expect("replace acquired lock");
            std::fs::write(lock_path, replacement_reason).expect("write replacement lock");
        });

        assert!(
            result.is_err(),
            "a replaced lock must not be adopted as this operation's lock"
        );
        assert_eq!(
            std::fs::read_to_string(&lock_path).expect("read replacement lock"),
            replacement_reason,
            "rejection must leave the replacement lock untouched"
        );

        drop(reference);
        drop(branch);
        drop(commit);
    }

    #[cfg(unix)]
    #[test]
    fn test_index_lock_drop_preserves_replaced_lock_entry() {
        let dir = tempfile::tempdir().expect("temp dir");
        let index_path = dir.path().join("index");
        let lock_path = dir.path().join("index.lock");
        let lock = IndexLockFile::create(&index_path).expect("create index lock");
        std::fs::remove_file(&lock_path).expect("replace index lock");
        std::fs::write(&lock_path, b"replacement lock").expect("write replacement lock");
        drop(lock);
        assert_eq!(
            std::fs::read(&lock_path).expect("read replacement lock"),
            b"replacement lock"
        );
    }

    fn commit_file(repo: &Repository, path: &str, contents: &str, message: &str) -> git2::Oid {
        std::fs::write(repo.workdir().expect("workdir").join(path), contents)
            .expect("write file");
        let tree_oid = {
            let mut index = repo.index().expect("index");
            index.add_path(Path::new(path)).expect("stage file");
            let tree_oid = index.write_tree().expect("write tree");
            index.write().expect("write index");
            tree_oid
        };
        let tree = repo.find_tree(tree_oid).expect("find tree");
        let signature = repo.signature().expect("signature");
        let parent = repo
            .head()
            .ok()
            .and_then(|head| head.target())
            .map(|oid| repo.find_commit(oid).expect("parent commit"));
        let parents = parent.as_ref().map(|commit| vec![commit]).unwrap_or_default();
        let oid = repo
            .commit(Some("HEAD"), &signature, &signature, message, &tree, &parents)
            .expect("commit file");
        drop(tree);
        drop(parent);
        oid
    }

    fn commit_file_on_branch(
        repo: &Repository,
        branch: &str,
        parent: &git2::Commit<'_>,
        path: &str,
        contents: &str,
        message: &str,
    ) -> git2::Oid {
        let blob_oid = repo.blob(contents.as_bytes()).expect("write branch blob");
        let parent_tree = parent.tree().expect("branch parent tree");
        let tree_oid = {
            let mut builder = repo.treebuilder(Some(&parent_tree)).expect("create tree builder");
            builder
                .insert(path, blob_oid, 0o100644)
                .expect("add branch file");
            builder.write().expect("write branch tree")
        };
        let tree = repo.find_tree(tree_oid).expect("find branch tree");
        let signature = repo.signature().expect("signature");
        repo.commit(
            Some(&format!("refs/heads/{}", branch)),
            &signature,
            &signature,
            message,
            &tree,
            &[parent],
        )
        .expect("commit branch file")
    }

    fn setup_rebase_repositories(
        initial_file: Option<(&str, &str)>,
        local_file: (&str, &str),
        remote_file: (&str, &str),
    ) -> (tempfile::TempDir, tempfile::TempDir, String, git2::Oid, git2::Oid) {
        let local_dir = tempfile::tempdir().expect("local temp dir");
        let local_repo = create_repo_with_commit(local_dir.path());
        if let Some((path, contents)) = initial_file {
            commit_file(&local_repo, path, contents, "base");
        }
        let branch = local_repo
            .head()
            .expect("HEAD")
            .shorthand()
            .expect("branch name")
            .to_string();

        let remote_dir = tempfile::tempdir().expect("remote temp dir");
        let remote_repo = Repository::init_bare(remote_dir.path()).expect("init bare repo");
        remote_repo
            .reference_symbolic(
                "HEAD",
                &format!("refs/heads/{}", branch),
                true,
                "set remote HEAD",
            )
            .expect("set remote HEAD");
        let remote_url = format!("file://{}", remote_dir.path().to_string_lossy());
        let mut origin = local_repo.remote("origin", &remote_url).expect("add origin");
        let refspec = format!(
            "refs/heads/{0}:refs/heads/{0}",
            branch
        );
        origin.push(&[refspec.as_str()], None).expect("push base");
        drop(origin);
        drop(remote_repo);

        let local_commit = commit_file(&local_repo, local_file.0, local_file.1, "local");
        drop(local_repo);

        let remote_work_dir = tempfile::tempdir().expect("remote work temp dir");
        let remote_work_path = remote_work_dir.path().join("clone");
        let remote_work_repo =
            Repository::clone(&remote_url, &remote_work_path).expect("clone remote repo");
        let remote_commit = commit_file(&remote_work_repo, remote_file.0, remote_file.1, "remote");
        let mut remote_origin = remote_work_repo.find_remote("origin").expect("find origin");
        remote_origin
            .push(&[refspec.as_str()], None)
            .expect("push remote commit");

        (local_dir, remote_dir, branch, local_commit, remote_commit)
    }

    fn setup_pull_repository() -> (tempfile::TempDir, tempfile::TempDir, String, git2::Oid) {
        let local_dir = tempfile::tempdir().expect("local temp dir");
        let local_repo = create_repo_with_commit(local_dir.path());
        let branch = local_repo
            .head()
            .expect("HEAD")
            .shorthand()
            .expect("branch name")
            .to_string();
        let base_commit = local_repo
            .head()
            .expect("HEAD")
            .target()
            .expect("base commit");

        let remote_dir = tempfile::tempdir().expect("remote temp dir");
        let remote_repo = Repository::init_bare(remote_dir.path()).expect("init bare repo");
        remote_repo
            .reference_symbolic(
                "HEAD",
                &format!("refs/heads/{}", branch),
                true,
                "set remote HEAD",
            )
            .expect("set remote HEAD");
        let remote_url = local_remote_url(remote_dir.path());
        let mut origin = local_repo.remote("origin", &remote_url).expect("add origin");
        let refspec = format!("refs/heads/{0}:refs/heads/{0}", branch);
        origin.push(&[refspec.as_str()], None).expect("push base");

        drop(origin);
        drop(remote_repo);
        drop(local_repo);

        (local_dir, remote_dir, branch, base_commit)
    }

    fn setup_push_repository() -> (tempfile::TempDir, tempfile::TempDir, String, git2::Oid) {
        let local_dir = tempfile::tempdir().expect("local temp dir");
        let local_repo = create_repo_with_commit(local_dir.path());
        let branch = local_repo
            .head()
            .expect("HEAD")
            .shorthand()
            .expect("branch name")
            .to_string();
        let initial_oid = local_repo.head().expect("HEAD").target().expect("initial commit");

        let remote_dir = tempfile::tempdir().expect("remote temp dir");
        Repository::init_bare(remote_dir.path()).expect("init bare remote");
        let remote_url = local_remote_url(remote_dir.path());
        local_repo.remote("origin", &remote_url).expect("add origin");
        drop(local_repo);

        (local_dir, remote_dir, branch, initial_oid)
    }

    fn create_divergent_remote_commit(remote_dir: &Path, branch: &str) -> git2::Oid {
        let remote_repo = Repository::open_bare(remote_dir).expect("open bare remote");
        remote_repo
            .reference_symbolic(
                "HEAD",
                &format!("refs/heads/{}", branch),
                true,
                "set remote HEAD",
            )
            .expect("set remote HEAD");
        drop(remote_repo);

        let remote_clone_dir = tempfile::tempdir().expect("remote clone dir");
        let remote_url = local_remote_url(remote_dir);
        let remote_clone = Repository::clone(&remote_url, remote_clone_dir.path())
            .expect("clone remote branch");
        let remote_oid = commit_file(&remote_clone, "remote.txt", "remote\n", "remote change");
        let mut remote_origin = remote_clone.find_remote("origin").expect("find origin");
        let refspec = format!(
            "refs/heads/{0}:refs/heads/{0}",
            branch
        );
        remote_origin
            .push(&[refspec.as_str()], None)
            .expect("push divergent remote commit");
        remote_oid
    }

    #[test]
    fn push_uses_selected_branch_when_head_is_attached() {
        let (local_dir, remote_dir, _branch, initial_oid) = setup_push_repository();
        let destination = "release/v1";
        let repo = Repository::open(local_dir.path()).expect("reopen local repo");
        let selected_oid = {
            let initial_commit = repo.find_commit(initial_oid).expect("initial commit");
            repo.branch(destination, &initial_commit, false)
                .expect("create selected branch");
            let tree = initial_commit.tree().expect("initial tree");
            let signature = repo.signature().expect("signature");
            repo.commit(
                Some(&format!("refs/heads/{}", destination)),
                &signature,
                &signature,
                "selected branch commit",
                &tree,
                &[&initial_commit],
            )
            .expect("commit selected branch")
        };
        drop(repo);

        let git = open_git_repo(local_dir.path());
        let progress = Arc::new(Mutex::new(String::new()));

        git.push("origin", destination, false, progress.clone())
            .expect("push attached HEAD");
        {
            let remote_repo = Repository::open_bare(remote_dir.path()).expect("open bare remote");
            let remote_ref = remote_repo
                .find_reference(&format!("refs/heads/{}", destination))
                .expect("pushed branch");
            assert_eq!(remote_ref.target(), Some(selected_oid));
        }

        let divergent_oid = create_divergent_remote_commit(remote_dir.path(), destination);
        assert!(
            git.push("origin", destination, false, progress.clone()).is_err(),
            "non-force attached push should reject a non-fast-forward update"
        );
        {
            let remote_repo = Repository::open_bare(remote_dir.path()).expect("open bare remote");
            let remote_ref = remote_repo
                .find_reference(&format!("refs/heads/{}", destination))
                .expect("divergent branch");
            assert_eq!(remote_ref.target(), Some(divergent_oid));
        }
        git.push("origin", destination, true, progress)
            .expect("force push attached HEAD");

        let remote_repo = Repository::open_bare(remote_dir.path()).expect("open bare remote");
        let remote_ref = remote_repo
            .find_reference(&format!("refs/heads/{}", destination))
            .expect("pushed branch");
        assert_eq!(remote_ref.target(), Some(selected_oid));
        assert_ne!(selected_oid, initial_oid);
    }

    #[test]
    fn detached_push_uses_head_and_force_overwrites_remote_branch() {
        let (local_dir, remote_dir, _branch, initial_oid) = setup_push_repository();
        let destination = "release/v1";
        let repo = Repository::open(local_dir.path()).expect("reopen local repo");
        repo.set_head_detached(initial_oid).expect("detach HEAD");
        drop(repo);

        let git = open_git_repo(local_dir.path());
        let progress = Arc::new(Mutex::new(String::new()));
        git.push("origin", destination, false, progress.clone())
            .expect("push detached HEAD");
        {
            let remote_repo = Repository::open_bare(remote_dir.path()).expect("open bare remote");
            let remote_ref = remote_repo
                .find_reference(&format!("refs/heads/{}", destination))
                .expect("pushed destination branch");
            assert_eq!(remote_ref.target(), Some(initial_oid));
        }

        let divergent_oid = create_divergent_remote_commit(remote_dir.path(), destination);

        assert!(
            git.push("origin", destination, false, progress.clone()).is_err(),
            "non-force detached push should reject a non-fast-forward update"
        );
        {
            let remote_repo = Repository::open_bare(remote_dir.path()).expect("open bare remote");
            let remote_ref = remote_repo
                .find_reference(&format!("refs/heads/{}", destination))
                .expect("divergent branch");
            assert_eq!(remote_ref.target(), Some(divergent_oid));
        }
        git.push("origin", destination, true, progress)
            .expect("force push detached HEAD");

        let remote_repo = Repository::open_bare(remote_dir.path()).expect("reopen bare remote");
        let remote_ref = remote_repo
            .find_reference(&format!("refs/heads/{}", destination))
            .expect("pushed destination branch");
        assert_eq!(remote_ref.target(), Some(initial_oid));
    }

    #[test]
    fn test_pull_rebase_places_local_commit_on_top_of_remote_commit() {
        let (local_dir, _remote_dir, branch, local_commit, remote_commit) = setup_rebase_repositories(
            None,
            ("local.txt", "local\n"),
            ("remote.txt", "remote\n"),
        );
        let git = open_git_repo(local_dir.path());
        let status = git.get_status().expect("status");
        assert!(status.is_empty(), "local setup must be clean: {:?}", status);
        let progress = Arc::new(Mutex::new(String::new()));

        git.pull("origin", &branch, true, progress)
            .expect("rebase pull");

        let repo = Repository::open(local_dir.path()).expect("reopen local repo");
        let head = repo.head().expect("HEAD").peel_to_commit().expect("HEAD commit");
        assert_ne!(head.id(), local_commit, "rebase should create a new local commit");
        assert_eq!(head.parent_id(0).expect("rebased parent"), remote_commit);
        assert_eq!(
            std::fs::read_to_string(local_dir.path().join("local.txt")).expect("read local file"),
            "local\n"
        );
        assert_eq!(
            std::fs::read_to_string(local_dir.path().join("remote.txt")).expect("read remote file"),
            "remote\n"
        );
    }

    #[test]
    fn test_pull_rebase_aborts_after_conflict() {
        let (local_dir, _remote_dir, branch, local_commit, _remote_commit) = setup_rebase_repositories(
            Some(("conflict.txt", "base\n")),
            ("conflict.txt", "local\n"),
            ("conflict.txt", "remote\n"),
        );
        let git = open_git_repo(local_dir.path());
        let progress = Arc::new(Mutex::new(String::new()));

        let result = git.pull("origin", &branch, true, progress);
        assert!(result.is_err(), "conflicting rebase pull should fail");
        drop(git);

        let repo = Repository::open(local_dir.path()).expect("reopen local repo");
        assert!(
            repo.open_rebase(None).is_err(),
            "failed rebase pull must not leave an in-progress rebase"
        );
        assert_eq!(
            repo.head().expect("HEAD").target(),
            Some(local_commit),
            "abort should restore the original HEAD"
        );
        assert_eq!(
            std::fs::read_to_string(local_dir.path().join("conflict.txt"))
                .expect("read conflict file"),
            "local\n",
            "abort should restore the pre-rebase working tree"
        );
        assert!(
            !repo.index().expect("index").has_conflicts(),
            "abort should clear rebase conflicts"
        );
    }

    #[test]
    fn test_pull_non_rebase_does_not_create_commit_when_already_up_to_date() {
        let (local_dir, _remote_dir, branch, base_commit) = setup_pull_repository();
        let git = open_git_repo(local_dir.path());

        git.pull("origin", &branch, false, Arc::new(Mutex::new(String::new())))
            .expect("no-op pull");

        let repo = Repository::open(local_dir.path()).expect("reopen local repo");
        let head = repo.head().expect("HEAD").peel_to_commit().expect("HEAD commit");
        assert_eq!(head.id(), base_commit, "no-op pull must not create a commit");
        assert_eq!(head.parent_count(), 0, "no-op pull must preserve the existing history");
    }

    #[test]
    fn test_pull_non_rebase_fast_forwards_without_merge_commit() {
        let (local_dir, remote_dir, branch, base_commit) = setup_pull_repository();
        let remote_clone_dir = tempfile::tempdir().expect("remote clone dir");
        let remote_clone = Repository::clone(
            &local_remote_url(remote_dir.path()),
            remote_clone_dir.path(),
        )
        .expect("clone remote");
        let remote_commit = commit_file(&remote_clone, "remote.txt", "remote\n", "remote change");
        let mut remote_origin = remote_clone.find_remote("origin").expect("find origin");
        let refspec = format!("refs/heads/{0}:refs/heads/{0}", branch);
        remote_origin
            .push(&[refspec.as_str()], None)
            .expect("push remote commit");
        drop(remote_origin);
        drop(remote_clone);

        let git = open_git_repo(local_dir.path());
        git.pull("origin", &branch, false, Arc::new(Mutex::new(String::new())))
            .expect("fast-forward pull");

        let repo = Repository::open(local_dir.path()).expect("reopen local repo");
        let head = repo.head().expect("HEAD").peel_to_commit().expect("HEAD commit");
        assert_eq!(head.id(), remote_commit, "fast-forward pull must advance to the remote tip");
        assert_eq!(head.parent_id(0).expect("fast-forward parent"), base_commit);
        assert_eq!(head.parent_count(), 1, "fast-forward pull must not create a merge commit");
        assert_eq!(
            std::fs::read_to_string(local_dir.path().join("remote.txt")).expect("read remote file"),
            "remote\n",
        );
    }

    #[test]
    fn test_pull_non_rebase_rejects_unmerged_index_before_ancestry_fast_path() {
        let (local_dir, _remote_dir, branch, base_commit) = setup_pull_repository();
        let local_repo = Repository::open(local_dir.path()).expect("open local repo");
        let local_commit = commit_file(&local_repo, "conflict.txt", "local\n", "local change");

        let conflict_index = {
            let base_tree = local_repo
                .find_commit(base_commit)
                .expect("find base commit")
                .tree()
                .expect("find base tree");
            let ours_tree = local_repo
                .find_commit(local_commit)
                .expect("find local commit")
                .tree()
                .expect("find local tree");
            let remote_blob = local_repo.blob(b"remote\n").expect("write remote blob");
            let remote_tree_oid = {
                let mut builder = local_repo
                    .treebuilder(Some(&base_tree))
                    .expect("create remote tree builder");
                builder
                    .insert("conflict.txt", remote_blob, 0o100644)
                    .expect("add remote conflict");
                builder.write().expect("write remote tree")
            };
            let remote_tree = local_repo
                .find_tree(remote_tree_oid)
                .expect("find remote tree");
            local_repo
                .merge_trees(
                    &base_tree,
                    &ours_tree,
                    &remote_tree,
                    None::<&git2::MergeOptions>,
                )
                .expect("create conflict index")
        };
        assert!(conflict_index.has_conflicts(), "test setup must create index conflicts");

        {
            let mut index = local_repo.index().expect("open repository index");
            for entry in conflict_index.iter() {
                index.add(&entry).expect("write conflict entry");
            }
            index.write().expect("persist conflicted index");
        }
        drop(conflict_index);
        drop(local_repo);

        let git = open_git_repo(local_dir.path());
        let result = git.pull("origin", &branch, false, Arc::new(Mutex::new(String::new())));
        let error = result.expect_err("pull must reject an unresolved index");
        assert!(
            error.contains("unmerged files"),
            "pull should explain the unresolved index: {}",
            error
        );

        let repo = Repository::open(local_dir.path()).expect("reopen local repo");
        assert_eq!(
            repo.head().expect("HEAD").target(),
            Some(local_commit),
            "rejecting the pull must preserve HEAD"
        );
        assert!(
            repo.index().expect("index").has_conflicts(),
            "rejecting the pull must preserve unresolved index entries"
        );
    }

    #[test]
    fn test_checkout_remote_branch_uses_remote_tracking_ref() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        let remote_oid = repo.head().expect("HEAD").target().expect("remote target");
        repo.reference(
            "refs/remotes/origin/feature",
            remote_oid,
            true,
            "create remote tracking ref",
        )
        .expect("create remote tracking ref");
        drop(repo);

        let git = open_git_repo(dir.path());
        git.checkout_branch("origin/feature")
            .expect("checkout remote branch");

        let checked_out = Repository::open(dir.path()).expect("reopen repo");
        let head = checked_out.head().expect("HEAD");
        assert!(checked_out.head_detached().expect("HEAD state"));
        assert_eq!(head.target(), Some(remote_oid));
    }

    #[test]
    fn test_checkout_branch_preserves_staged_and_sparse_index_state() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        commit_file(&repo, "tracked.txt", "tracked contents\n", "add tracked file");
        let old_commit = repo.head().expect("old HEAD").peel_to_commit().expect("old commit");
        repo.branch("feature", &old_commit, false).expect("create feature branch");
        let feature_oid = commit_file_on_branch(
            &repo,
            "feature",
            &old_commit,
            "target.txt",
            "target branch contents\n",
            "add target file",
        );
        repo.reference(
            "refs/remotes/origin/feature",
            feature_oid,
            true,
            "create remote tracking ref",
        )
        .expect("create remote tracking ref");
        std::fs::write(dir.path().join("staged.txt"), "staged contents\n")
            .expect("write staged file");
        let mut index = repo.index().expect("index");
        index.add_path(Path::new("staged.txt")).expect("stage unrelated file");
        let mut tracked_entry = index
            .get_path(Path::new("tracked.txt"), 0)
            .expect("tracked index entry");
        tracked_entry.flags |= 0x4000;
        tracked_entry.flags_extended |= 0x4000;
        index.add(&tracked_entry).expect("mark skip-worktree");
        index.set_version(4).expect("set index version");
        index.write().expect("write index metadata");
        drop(old_commit);
        drop(repo);

        let git = open_git_repo(dir.path());
        git.checkout_branch("origin/feature")
            .expect("checkout remote branch");

        let checked_out = Repository::open(dir.path()).expect("reopen repository");
        assert_eq!(checked_out.head().expect("HEAD").target(), Some(feature_oid));
        let index = checked_out.index().expect("index");
        assert_eq!(index.version(), 4);
        assert!(index.get_path(Path::new("target.txt"), 0).is_some());
        assert!(index.get_path(Path::new("staged.txt"), 0).is_some());
        assert_ne!(
            index.get_path(Path::new("tracked.txt"), 0)
                .expect("tracked index entry")
                .flags_extended & 0x4000,
            0,
            "skip-worktree state must survive checkout"
        );
    }

    #[test]
    fn test_checkout_branch_treats_glob_filename_as_literal() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        commit_file(&repo, "pattern[1].txt", "old literal contents\n", "add bracketed file");
        commit_file(&repo, "pattern1.txt", "neighbor contents\n", "add glob neighbor");
        let old_commit = repo.head().expect("old HEAD").peel_to_commit().expect("old commit");
        repo.branch("feature", &old_commit, false).expect("create feature branch");
        let feature_oid = commit_file_on_branch(
            &repo,
            "feature",
            &old_commit,
            "pattern[1].txt",
            "new literal contents\n",
            "update bracketed file",
        );
        drop(old_commit);
        drop(repo);
        std::fs::write(dir.path().join("pattern1.txt"), "local neighbor edit\n")
            .expect("make glob neighbor dirty");

        let git = open_git_repo(dir.path());
        git.checkout_branch("feature")
            .expect("checkout literal bracketed path");

        assert_eq!(
            std::fs::read_to_string(dir.path().join("pattern[1].txt"))
                .expect("read checked out literal file"),
            "new literal contents\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("pattern1.txt"))
                .expect("read dirty neighbor"),
            "local neighbor edit\n",
            "literal path checkout must not match a glob neighbor"
        );
        let checked_out = Repository::open(dir.path()).expect("reopen repository");
        assert_eq!(checked_out.head().expect("HEAD").target(), Some(feature_oid));
    }

    #[test]
    fn test_checkout_rollback_treats_glob_filename_as_literal() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        commit_file(&repo, "pattern[1].txt", "old literal contents\n", "add bracketed file");
        commit_file(&repo, "pattern1.txt", "neighbor contents\n", "add glob neighbor");
        let old_commit = repo.head().expect("old HEAD").peel_to_commit().expect("old commit");
        let old_head_oid = old_commit.id();
        let old_head_name = repo
            .head()
            .expect("old HEAD")
            .name()
            .expect("old branch name")
            .to_owned();
        repo.branch("feature", &old_commit, false).expect("create feature branch");
        commit_file_on_branch(
            &repo,
            "feature",
            &old_commit,
            "pattern[1].txt",
            "new literal contents\n",
            "update bracketed file",
        );
        drop(old_commit);
        drop(repo);
        std::fs::write(dir.path().join("pattern1.txt"), "local neighbor edit\n")
            .expect("make glob neighbor dirty");
        let head_lock_path = dir.path().join(".git").join("HEAD.lock");
        std::fs::write(&head_lock_path, "block HEAD update").expect("create HEAD lock");

        let git = open_git_repo(dir.path());
        let result = git.checkout_branch("feature");
        std::fs::remove_file(&head_lock_path).expect("remove HEAD lock");

        let error = result.expect_err("locked HEAD must fail checkout");
        assert!(error.contains("Set HEAD:"), "unexpected checkout error: {}", error);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("pattern[1].txt"))
                .expect("read restored literal file"),
            "old literal contents\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("pattern1.txt"))
                .expect("read dirty neighbor"),
            "local neighbor edit\n",
            "rollback must not treat the literal path as a glob"
        );
        let checked_out = Repository::open(dir.path()).expect("reopen repository");
        assert_eq!(checked_out.head().expect("HEAD").name(), Some(old_head_name.as_str()));
        assert_eq!(checked_out.head().expect("HEAD").target(), Some(old_head_oid));
    }

    #[test]
    fn test_checkout_branch_restores_worktree_when_head_update_fails() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        let old_commit_oid = commit_file(&repo, "tracked.txt", "old contents\n", "add tracked file");
        let old_commit = repo.find_commit(old_commit_oid).expect("find old commit");
        let old_head_name = repo
            .head()
            .expect("old HEAD")
            .name()
            .expect("old HEAD name")
            .to_string();
        let (head_state_name, head_is_symbolic, head_target) =
            checkout_head_state(&repo).expect("snapshot attached HEAD");
        assert!(head_is_symbolic);
        assert!(head_state_name.starts_with(b"refs/heads/"));
        assert_eq!(head_target, Some(old_commit_oid));
        let signature = repo.signature().expect("signature");
        repo.branch("feature", &old_commit, false).expect("create feature branch");

        let old_tree = old_commit.tree().expect("old tree");
        let feature_blob = repo.blob(b"feature contents\n").expect("write feature blob");
        let feature_tree_oid = {
            let mut builder = repo.treebuilder(Some(&old_tree)).expect("create feature tree");
            builder
                .insert("feature.txt", feature_blob, 0o100644)
                .expect("update feature file");
            builder.write().expect("write feature tree")
        };
        let feature_tree = repo.find_tree(feature_tree_oid).expect("find feature tree");
        repo.commit(
            Some("refs/heads/feature"),
            &signature,
            &signature,
            "update feature file",
            &feature_tree,
            &[&old_commit],
        )
        .expect("commit feature file");

        let index_path = repo.index().expect("index").path().expect("index path").to_path_buf();
        let original_index = std::fs::read(&index_path).expect("read original index");
        drop(feature_tree);
        drop(old_tree);
        drop(old_commit);
        drop(repo);

        let head_lock_path = dir.path().join(".git").join("HEAD.lock");
        std::fs::write(&head_lock_path, "block HEAD update").expect("create HEAD lock");

        let git = open_git_repo(dir.path());
        let result = git.checkout_branch("feature");
        std::fs::remove_file(&head_lock_path).expect("remove HEAD lock");

        let error = result.expect_err("locked HEAD must fail checkout");
        assert!(error.contains("Set HEAD:"), "unexpected checkout error: {}", error);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("tracked.txt")).expect("read restored file"),
            "old contents\n",
            "failed HEAD update must restore the original worktree"
        );

        let final_repo = Repository::open(dir.path()).expect("reopen repo");
        let final_head = final_repo.head().expect("restored HEAD");
        assert_eq!(final_head.name(), Some(old_head_name.as_str()));
        assert_eq!(final_head.target(), Some(old_commit_oid));
        assert_eq!(
            std::fs::read(&index_path).expect("read restored index"),
            original_index,
            "failed HEAD update must restore the original index"
        );
    }

    #[test]
    fn test_checkout_branch_from_unborn_head() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = Repository::init(dir.path()).expect("initialize repository");
        let signature = git2::Signature::now("Test User", "test@example.com").expect("signature");
        let blob = repo.blob(b"feature contents\n").expect("write feature blob");
        let tree_oid = {
            let mut builder = repo.treebuilder(None).expect("create feature tree");
            builder
                .insert("tracked.txt", blob, 0o100644)
                .expect("add feature file");
            builder.write().expect("write feature tree")
        };
        let tree = repo.find_tree(tree_oid).expect("find feature tree");
        let feature_oid = repo
            .commit(
                Some("refs/heads/feature"),
                &signature,
                &signature,
                "create feature branch",
                &tree,
                &[],
            )
            .expect("create feature commit");
        drop(tree);
        drop(repo);

        let git = open_git_repo(dir.path());
        git.checkout_branch("feature").expect("checkout from unborn HEAD");

        assert_eq!(
            std::fs::read_to_string(dir.path().join("tracked.txt")).expect("read checked out file"),
            "feature contents\n"
        );
        let checked_out = Repository::open(dir.path()).expect("reopen repository");
        assert_eq!(checked_out.head().expect("HEAD").target(), Some(feature_oid));
        assert_eq!(checked_out.head().expect("HEAD").name(), Some("refs/heads/feature"));
        assert!(
            checked_out
                .index()
                .expect("installed index")
                .get_path(Path::new("tracked.txt"), 0)
                .is_some(),
            "installed index must contain the checked out tree"
        );
    }

    #[test]
    fn test_target_checkout_index_clears_conflict_stages_for_changed_path() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        commit_file(&repo, "tracked.txt", "target contents\n", "add tracked file");
        let tree = repo.head().expect("HEAD").peel_to_tree().expect("target tree");
        let mut index = repo.index().expect("index");
        for stage in 1..=3 {
            let mut entry = index
                .get_path(Path::new("tracked.txt"), 0)
                .expect("stage-zero source entry");
            entry.flags = (entry.flags & !0x3000) | ((stage as u16) << 12);
            index.add(&entry).expect("add conflict stage");
        }
        assert!(index.has_conflicts());
        index.write().expect("write conflicted index");
        let index_path = index.path().expect("index path").to_path_buf();
        let original_index = std::fs::read(&index_path).expect("read conflicted index");

        let target_index = index_bytes_for_tree(
            &tree,
            &index_path,
            Some(&original_index),
            &[PathBuf::from("tracked.txt")],
        )
        .expect("build target checkout index");
        std::fs::write(&index_path, target_index).expect("install generated test index");

        let checked_out = Repository::open(dir.path()).expect("reopen repository");
        let checked_out_index = checked_out.index().expect("reopen target index");
        assert!(!checked_out_index.has_conflicts());
        assert!(checked_out_index.get_path(Path::new("tracked.txt"), 0).is_some());
        for stage in 1..=3 {
            assert!(checked_out_index.get_path(Path::new("tracked.txt"), stage).is_none());
        }
    }

    #[test]
    fn test_checkout_branch_rebuilds_full_index_when_missing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        commit_file(&repo, "unchanged.txt", "same contents\n", "add unchanged file");
        commit_file(&repo, "also-unchanged.txt", "other contents\n", "add second unchanged file");
        let old_commit_oid = repo.head().expect("old HEAD").target().expect("old target");
        let old_commit = repo.find_commit(old_commit_oid).expect("old commit");
        let signature = repo.signature().expect("signature");
        repo.branch("feature", &old_commit, false).expect("create feature branch");
        let added_blob = repo.blob(b"new target file\n").expect("write target blob");
        let old_tree = old_commit.tree().expect("old tree");
        let feature_tree_oid = {
            let mut builder = repo.treebuilder(Some(&old_tree)).expect("create feature tree");
            builder
                .insert("added.txt", added_blob, 0o100644)
                .expect("add target file");
            builder.write().expect("write feature tree")
        };
        let feature_tree = repo.find_tree(feature_tree_oid).expect("find feature tree");
        let feature_oid = repo
            .commit(
                Some("refs/heads/feature"),
                &signature,
                &signature,
                "add target file",
                &feature_tree,
                &[&old_commit],
            )
            .expect("commit feature file");
        let index_path = repo.index().expect("index").path().expect("index path").to_path_buf();
        std::fs::remove_file(&index_path).expect("remove original index");
        drop(feature_tree);
        drop(old_tree);
        drop(old_commit);
        drop(repo);

        let git = open_git_repo(dir.path());
        git.checkout_branch("feature")
            .expect("checkout without an on-disk index");

        let checked_out = Repository::open(dir.path()).expect("reopen repository");
        assert_eq!(checked_out.head().expect("HEAD").target(), Some(feature_oid));
        let index = checked_out.index().expect("rebuilt index");
        assert!(index.get_path(Path::new("unchanged.txt"), 0).is_some());
        assert!(index.get_path(Path::new("also-unchanged.txt"), 0).is_some());
        assert!(index.get_path(Path::new("added.txt"), 0).is_some());
    }

    #[test]
    fn test_checkout_branch_rebuilds_index_for_identical_tree_when_missing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        commit_file(&repo, "tracked.txt", "tracked contents\n", "add tracked file");
        let target_oid = repo.head().expect("HEAD").target().expect("target commit");
        let target_commit = repo.find_commit(target_oid).expect("target commit");
        repo.branch("feature", &target_commit, false).expect("create feature branch");
        let index_path = repo.index().expect("index").path().expect("index path").to_path_buf();
        std::fs::remove_file(&index_path).expect("remove original index");
        drop(target_commit);
        drop(repo);

        let git = open_git_repo(dir.path());
        git.checkout_branch("feature")
            .expect("checkout identical tree without an on-disk index");

        let checked_out = Repository::open(dir.path()).expect("reopen repository");
        assert_eq!(checked_out.head().expect("HEAD").target(), Some(target_oid));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("tracked.txt")).expect("read tracked file"),
            "tracked contents\n"
        );
        assert!(
            checked_out
                .index()
                .expect("rebuilt index")
                .get_path(Path::new("tracked.txt"), 0)
                .is_some()
        );
    }

    #[test]
    fn test_checkout_same_tree_rejects_unresolved_index() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        commit_file(&repo, "tracked.txt", "tracked contents\n", "add tracked file");
        let old_head = repo.head().expect("HEAD");
        let old_head_oid = old_head.target().expect("old target");
        let old_head_name = old_head.name().expect("old branch name").to_owned();
        let old_commit = repo.find_commit(old_head_oid).expect("old commit");
        repo.branch("feature", &old_commit, false).expect("create feature branch");
        let mut index = repo.index().expect("index");
        let mut conflict_entry = index
            .get_path(Path::new("tracked.txt"), 0)
            .expect("stage-zero entry");
        index
            .remove(Path::new("tracked.txt"), 0)
            .expect("remove stage-zero entry");
        for stage in 1..=3 {
            conflict_entry.flags = (conflict_entry.flags & !0x3000) | ((stage as u16) << 12);
            index.add(&conflict_entry).expect("add conflict stage");
        }
        assert!(index.has_conflicts());
        index.write().expect("write conflicted index");
        let index_path = index.path().expect("index path").to_path_buf();
        let original_index = std::fs::read(&index_path).expect("read conflicted index");
        drop(old_head);
        drop(old_commit);
        drop(repo);

        let git = open_git_repo(dir.path());
        let error = git
            .checkout_branch("feature")
            .expect_err("same-tree checkout must reject unresolved index");

        assert!(error.contains("unresolved conflicts"), "unexpected error: {}", error);
        let checked_out = Repository::open(dir.path()).expect("reopen repository");
        assert_eq!(checked_out.head().expect("HEAD").name(), Some(old_head_name.as_str()));
        assert_eq!(checked_out.head().expect("HEAD").target(), Some(old_head_oid));
        assert!(checked_out.index().expect("index").has_conflicts());
        assert_eq!(
            std::fs::read(index_path).expect("read preserved index"),
            original_index
        );
    }

    #[test]
    fn test_checkout_branch_restores_absent_index_when_head_update_fails() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        let old_head = repo.head().expect("old HEAD").target().expect("old target");
        let old_commit = repo.find_commit(old_head).expect("old commit");
        let old_tree = old_commit.tree().expect("old tree");
        let signature = repo.signature().expect("signature");
        let feature_blob = repo.blob(b"feature contents\n").expect("write feature blob");
        let feature_tree_oid = {
            let mut builder = repo.treebuilder(Some(&old_tree)).expect("create feature tree");
            builder
                .insert("tracked.txt", feature_blob, 0o100644)
                .expect("update feature file");
            builder.write().expect("write feature tree")
        };
        let feature_tree = repo.find_tree(feature_tree_oid).expect("find feature tree");
        repo.commit(
            Some("refs/heads/feature"),
            &signature,
            &signature,
            "update feature file",
            &feature_tree,
            &[&old_commit],
        )
        .expect("commit feature file");
        let index_path = repo.index().expect("index").path().expect("index path").to_path_buf();
        assert!(!index_path.exists(), "empty initial repository should have no index");
        drop(feature_tree);
        drop(old_tree);
        drop(old_commit);
        drop(repo);

        let head_lock_path = dir.path().join(".git").join("HEAD.lock");
        std::fs::write(&head_lock_path, "block HEAD update").expect("create HEAD lock");
        let git = open_git_repo(dir.path());
        let result = git.checkout_branch("feature");
        std::fs::remove_file(&head_lock_path).expect("remove HEAD lock");

        let error = result.expect_err("locked HEAD must fail checkout");
        assert!(error.contains("Set HEAD:"), "unexpected checkout error: {}", error);
        assert!(!index_path.exists(), "failed checkout must preserve an absent index");
        assert!(!dir.path().join("feature.txt").exists());
        let final_repo = Repository::open(dir.path()).expect("reopen repository");
        assert_eq!(final_repo.head().expect("HEAD").target(), Some(old_head));
    }

    #[test]
    fn test_pull_does_not_overwrite_dirty_worktree() {
        let remote_dir = tempfile::tempdir().expect("remote temp dir");
        let remote_repo = create_repo_with_commit(remote_dir.path());
        let branch = remote_repo
            .head()
            .expect("remote HEAD")
            .shorthand()
            .expect("remote branch")
            .to_string();
        let tracked_path = remote_dir.path().join("tracked.txt");

        std::fs::write(&tracked_path, "base\n").expect("write base file");
        let parent = remote_repo
            .head()
            .expect("remote HEAD")
            .peel_to_commit()
            .expect("remote parent");
        let signature = remote_repo.signature().expect("signature");
        let tree_oid = {
            let mut index = remote_repo.index().expect("index");
            index.add_path(Path::new("tracked.txt")).expect("stage base file");
            index.write_tree().expect("write base tree")
        };
        let tree = remote_repo.find_tree(tree_oid).expect("find base tree");
        remote_repo
            .commit(Some("HEAD"), &signature, &signature, "add tracked file", &tree, &[&parent])
            .expect("commit base file");
        drop(tree);
        drop(parent);
        let initial_id = remote_repo.head().expect("remote HEAD").target().expect("initial id");
        drop(remote_repo);

        let local_dir = tempfile::tempdir().expect("local temp dir");
        let local_repo = Repository::clone(
            remote_dir.path().to_str().expect("remote path"),
            local_dir.path(),
        )
        .expect("clone remote");
        drop(local_repo);

        let remote_repo = Repository::open(remote_dir.path()).expect("reopen remote");
        std::fs::write(&tracked_path, "remote\n").expect("write remote change");
        let parent = remote_repo
            .head()
            .expect("remote HEAD")
            .peel_to_commit()
            .expect("remote parent");
        let signature = remote_repo.signature().expect("signature");
        let tree_oid = {
            let mut index = remote_repo.index().expect("index");
            index.add_path(Path::new("tracked.txt")).expect("stage remote change");
            index.write_tree().expect("write remote tree")
        };
        let tree = remote_repo.find_tree(tree_oid).expect("find remote tree");
        remote_repo
            .commit(Some("HEAD"), &signature, &signature, "remote change", &tree, &[&parent])
            .expect("commit remote change");
        drop(tree);
        drop(parent);
        drop(remote_repo);

        let local_tracked_path = local_dir.path().join("tracked.txt");
        std::fs::write(&local_tracked_path, "local\n").expect("write local change");

        let git = open_git_repo(local_dir.path());
        let result = git.pull("origin", &branch, false, Arc::new(Mutex::new(String::new())));

        assert!(result.is_err(), "pull should reject an overwrite of local changes: {:?}", result);
        assert_eq!(
            std::fs::read_to_string(&local_tracked_path).expect("read local file"),
            "local\n",
            "pull must preserve the dirty working-tree content"
        );

        let local_repo = Repository::open(local_dir.path()).expect("reopen local");
        assert_eq!(
            local_repo.head().expect("local HEAD").target(),
            Some(initial_id),
            "a rejected pull must not advance HEAD"
        );
    }

    #[test]
    fn test_unstaged_diff_compares_index_to_worktree() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        let tracked_path = dir.path().join("tracked.txt");

        std::fs::write(&tracked_path, "line 1\nline 2\nline 3\n").expect("write tracked file");
        let parent = repo.head().expect("HEAD").peel_to_commit().expect("parent commit");
        let signature = repo.signature().expect("signature");
        let tree_oid = {
            let mut index = repo.index().expect("index");
            index.add_path(Path::new("tracked.txt")).expect("stage tracked file");
            index.write_tree().expect("write tree")
        };
        let tree = repo.find_tree(tree_oid).expect("find tree");
        repo.commit(Some("HEAD"), &signature, &signature, "add tracked file", &tree, &[&parent])
            .expect("commit tracked file");
        drop(tree);
        drop(parent);
        drop(repo);

        let git = open_git_repo(dir.path());
        std::fs::write(&tracked_path, "staged line 1\nline 2\nline 3\n")
            .expect("write staged version");
        git.stage_file("tracked.txt").expect("stage tracked change");
        std::fs::write(&tracked_path, "staged line 1\nline 2\nunstaged line 3\n")
            .expect("write unstaged version");

        let diff = git.get_diff("tracked.txt", false).expect("get unstaged diff");

        assert_eq!(
            diff.iter().map(|line| (line.origin, line.content.as_str())).collect::<Vec<_>>(),
            vec![
                (' ', "staged line 1\n"),
                (' ', "line 2\n"),
                ('-', "line 3\n"),
                ('+', "unstaged line 3\n"),
            ],
            "unstaged diff must compare the index with the worktree"
        );
    }

    #[test]
    fn test_untracked_diff_includes_file_content() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        let untracked_path = dir.path().join("new.txt");
        std::fs::write(&untracked_path, "first line\nsecond line\n").expect("write untracked file");
        drop(repo);

        let git = open_git_repo(dir.path());
        let diff = git.get_diff("new.txt", false).expect("get untracked diff");

        assert_eq!(
            diff.iter().map(|line| (line.origin, line.content.as_str())).collect::<Vec<_>>(),
            vec![('+', "first line\n"), ('+', "second line\n")],
            "untracked diff must expose the file content as added lines"
        );
    }

    #[test]
    fn test_uncommit_preserves_changes_and_current_branch_head() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        let branch_name = repo
            .head()
            .expect("HEAD")
            .shorthand()
            .expect("branch name")
            .to_string();

        let tracked_path = dir.path().join("tracked.txt");
        std::fs::write(&tracked_path, "base\n").expect("write base file");
        let parent_id = repo.head().expect("HEAD").target().expect("parent id");
        let signature = repo.signature().expect("signature");
        let tree_oid = {
            let mut index = repo.index().expect("index");
            index.add_path(Path::new("tracked.txt")).expect("stage base file");
            index.write_tree().expect("write tree")
        };
        let tree = repo.find_tree(tree_oid).expect("find tree");
        let parent = repo.find_commit(parent_id).expect("parent commit");
        let feature_id = repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "feature commit",
                &tree,
                &[&parent],
            )
            .expect("create feature commit");
        drop(parent);
        drop(tree);
        drop(repo);

        let git = open_git_repo(dir.path());
        std::fs::write(&tracked_path, "feature\nstaged local change\n")
            .expect("write staged change");
        git.stage_file("tracked.txt").expect("stage local change");
        std::fs::write(&tracked_path, "feature\nstaged local change\nunstaged local change\n")
            .expect("write unstaged change");

        let result = git.uncommit().expect("uncommit");
        assert_eq!(result, parent_id.to_string(), "uncommit should return the parent id");
        assert_eq!(
            std::fs::read_to_string(&tracked_path).expect("read working tree"),
            "feature\nstaged local change\nunstaged local change\n",
            "uncommit must preserve both staged and unstaged working-tree changes"
        );

        let repo = Repository::open(dir.path()).expect("reopen repo");
        let head = repo.head().expect("HEAD");
        assert!(head.is_branch(), "uncommit must keep HEAD attached to the current branch");
        assert_eq!(head.shorthand(), Some(branch_name.as_str()));
        assert_eq!(head.target(), Some(parent_id), "HEAD should move to the previous commit");
        assert_eq!(
            repo.find_branch(&branch_name, BranchType::Local)
                .expect("current branch")
                .get()
                .target(),
            Some(parent_id),
            "the current branch ref should move to the previous commit"
        );

        let index = repo.index().expect("index");
        assert!(
            index.get_path(Path::new("tracked.txt"), 0).is_some(),
            "the removed commit's changes should remain staged"
        );
        assert_ne!(feature_id, parent_id, "the feature commit should have a distinct parent");
    }

    #[test]
    fn test_merge_branch_already_integrated_is_noop() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        let initial = repo.head().expect("HEAD").peel_to_commit().expect("commit");
        repo.branch("feature", &initial, false).expect("create feature branch");
        let current_oid = commit_file(&repo, "main.txt", "main\n", "main change");
        drop(initial);
        drop(repo);

        let git = open_git_repo(dir.path());
        assert_eq!(git.merge_branch("feature").expect("merge integrated branch"), "Already up to date");

        let merged_repo = Repository::open(dir.path()).expect("reopen repo");
        let head = merged_repo.head().expect("HEAD").peel_to_commit().expect("HEAD commit");
        assert_eq!(head.id(), current_oid, "an integrated branch must not move HEAD");
        assert_eq!(head.parent_count(), 1, "an integrated branch must not create a merge commit");
    }

    #[test]
    fn test_merge_branch_fast_forwards_without_merge_commit() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        let initial = repo.head().expect("HEAD").peel_to_commit().expect("commit");
        let current_branch = repo.head().expect("HEAD").shorthand().expect("branch").to_string();
        repo.branch("feature", &initial, false).expect("create feature branch");
        let feature_oid = commit_file_on_branch(
            &repo,
            "feature",
            &initial,
            "feature.txt",
            "feature\n",
            "feature change",
        );
        repo.index()
            .expect("repository index")
            .write()
            .expect("write repository index");
        drop(initial);
        drop(repo);

        let git = open_git_repo(dir.path());
        assert_eq!(
            git.merge_branch("feature").expect("fast-forward feature branch"),
            "Fast-forwarded to branch 'feature'"
        );

        let merged_repo = Repository::open(dir.path()).expect("reopen repo");
        let head = merged_repo.head().expect("HEAD").peel_to_commit().expect("HEAD commit");
        assert_eq!(head.id(), feature_oid, "fast-forward should advance to the feature tip");
        assert_eq!(head.parent_count(), 1, "fast-forward must not create a merge commit");
        assert_eq!(
            merged_repo.head().expect("HEAD").shorthand(),
            Some(current_branch.as_str()),
            "fast-forward must leave HEAD attached to the current branch"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("feature.txt")).expect("read feature file"),
            "feature\n"
        );
    }

    #[test]
    fn test_merge_branch_divergent_history_creates_two_parent_commit() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        let initial = repo.head().expect("HEAD").peel_to_commit().expect("commit");
        repo.branch("feature", &initial, false).expect("create feature branch");
        let main_oid = commit_file(&repo, "main.txt", "main\n", "main change");
        let feature_oid = commit_file_on_branch(
            &repo,
            "feature",
            &initial,
            "feature.txt",
            "feature\n",
            "feature change",
        );
        drop(initial);
        drop(repo);

        let git = open_git_repo(dir.path());
        assert_eq!(
            git.merge_branch("feature").expect("merge divergent branch"),
            "Merge branch 'feature'"
        );

        let merged_repo = Repository::open(dir.path()).expect("reopen repo");
        let head = merged_repo.head().expect("HEAD").peel_to_commit().expect("HEAD commit");
        assert_eq!(head.parent_count(), 2, "divergent histories should create a merge commit");
        assert_eq!(head.parent_id(0).expect("first parent"), main_oid);
        assert_eq!(head.parent_id(1).expect("second parent"), feature_oid);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("main.txt")).expect("read main file"),
            "main\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("feature.txt")).expect("read feature file"),
            "feature\n"
        );
    }

    #[test]
    fn test_merge_branch_preserves_unrelated_dirty_worktree_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        let signature = repo.signature().expect("signature");
        let initial_commit = repo.head().expect("head").peel_to_commit().expect("commit");
        let current_branch = repo.head().expect("head").shorthand().expect("branch").to_string();

        std::fs::write(dir.path().join("merged.txt"), "base").expect("write merge file");
        std::fs::write(dir.path().join("local.txt"), "base-local").expect("write local file");
        let main_tree_oid = {
            let mut index = repo.index().expect("index");
            index.add_path(Path::new("merged.txt")).expect("stage merge file");
            index.add_path(Path::new("local.txt")).expect("stage local file");
            index.write().expect("write main index");
            index.write_tree().expect("write main tree")
        };
        let main_tree = repo.find_tree(main_tree_oid).expect("find main tree");
        let main_commit_oid = repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "add merge files",
                &main_tree,
                &[&initial_commit],
            )
            .expect("commit merge files");
        drop(main_tree);
        drop(initial_commit);

        let main_commit = repo.find_commit(main_commit_oid).expect("find main commit");
        repo.branch("feature", &main_commit, false).expect("create feature branch");
        let feature_blob = repo.blob(b"feature").expect("write feature blob");
        let main_tree = main_commit.tree().expect("get main tree");
        let feature_tree_oid = {
            let mut builder = repo.treebuilder(Some(&main_tree)).expect("create tree builder");
            builder
                .insert("merged.txt", feature_blob, 0o100644)
                .expect("update merge file");
            builder.write().expect("write feature tree")
        };
        let feature_tree = repo.find_tree(feature_tree_oid).expect("find feature tree");
        repo.commit(
            Some("refs/heads/feature"),
            &signature,
            &signature,
            "update merge file",
            &feature_tree,
            &[&main_commit],
        )
        .expect("commit feature change");
        drop(feature_tree);
        drop(main_tree);
        drop(main_commit);
        drop(repo);

        let local_path = dir.path().join("local.txt");
        std::fs::write(&local_path, "local change").expect("modify unrelated local file");

        let git = open_git_repo(dir.path());
        git.stage_file("local.txt").expect("stage unrelated local file");
        git.merge_branch("feature").expect("merge feature branch");

        assert_eq!(
            std::fs::read_to_string(&local_path).expect("read local file"),
            "local change",
            "merging must preserve unrelated dirty working-tree content"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("merged.txt")).expect("read merged file"),
            "feature",
            "the merged file must still be checked out"
        );
        assert_eq!(
            git.current_branch().expect("current branch"),
            current_branch,
            "merge must leave HEAD on the current branch"
        );

        let final_repo = Repository::open(dir.path()).expect("reopen merged repo");
        let final_index = final_repo.index().expect("read merged index");
        let local_entry = final_index
            .get_path(Path::new("local.txt"), 0)
            .expect("preserved staged file in index");
        assert_eq!(
            final_repo
                .find_blob(local_entry.id)
                .expect("find staged local blob")
                .content(),
            b"local change",
            "merging must preserve the staged unrelated index entry"
        );
    }

    #[test]
    fn test_merge_branch_preserves_ignored_worktree_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        let signature = repo.signature().expect("signature");
        let initial_commit = repo.head().expect("head").peel_to_commit().expect("commit");

        std::fs::write(dir.path().join(".gitignore"), "ignored.txt\n")
            .expect("write ignore file");
        let base_tree_oid = {
            let mut index = repo.index().expect("index");
            index.add_path(Path::new(".gitignore")).expect("stage ignore file");
            index.write().expect("write index");
            index.write_tree().expect("write base tree")
        };
        let base_tree = repo.find_tree(base_tree_oid).expect("find base tree");
        let base_commit_oid = repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "add ignore rule",
                &base_tree,
                &[&initial_commit],
            )
            .expect("commit ignore rule");
        drop(base_tree);
        drop(initial_commit);

        let base_commit = repo.find_commit(base_commit_oid).expect("find base commit");
        repo.branch("feature", &base_commit, false).expect("create feature branch");
        let feature_blob = repo.blob(b"feature").expect("write feature blob");
        let base_tree = base_commit.tree().expect("get base tree");
        let feature_tree_oid = {
            let mut builder = repo.treebuilder(Some(&base_tree)).expect("create tree builder");
            builder
                .insert("ignored.txt", feature_blob, 0o100644)
                .expect("add ignored path to feature");
            builder.write().expect("write feature tree")
        };
        let feature_tree = repo.find_tree(feature_tree_oid).expect("find feature tree");
        repo.commit(
            Some("refs/heads/feature"),
            &signature,
            &signature,
            "add feature file",
            &feature_tree,
            &[&base_commit],
        )
        .expect("commit feature file");
        drop(feature_tree);
        drop(base_tree);
        drop(base_commit);
        drop(repo);

        let ignored_path = dir.path().join("ignored.txt");
        std::fs::write(&ignored_path, "keep this local file").expect("write ignored file");

        let git = open_git_repo(dir.path());
        git.merge_branch("feature").expect("merge feature branch");

        assert_eq!(
            std::fs::read_to_string(&ignored_path).expect("read ignored file"),
            "keep this local file",
            "merging must not overwrite an ignored local file"
        );

        let final_repo = Repository::open(dir.path()).expect("reopen merged repo");
        let final_index = final_repo.index().expect("read merged index");
        let ignored_entry = final_index
            .get_path(Path::new("ignored.txt"), 0)
            .expect("merged ignored path in index");
        assert_eq!(
            final_repo
                .find_blob(ignored_entry.id)
                .expect("find merged ignored blob")
                .content(),
            b"feature",
            "the index must match the merged tree for a preserved ignored file"
        );
    }

    #[test]
    fn test_merge_branch_rolls_back_checkout_when_head_update_fails() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        let signature = repo.signature().expect("signature");
        let initial_commit = repo.head().expect("head").peel_to_commit().expect("commit");

        std::fs::write(dir.path().join("merged.txt"), "base").expect("write merge file");
        let base_tree_oid = {
            let mut index = repo.index().expect("index");
            index.add_path(Path::new("merged.txt")).expect("stage merge file");
            index.write().expect("write index");
            index.write_tree().expect("write base tree")
        };
        let base_tree = repo.find_tree(base_tree_oid).expect("find base tree");
        let base_commit_oid = repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "add merge file",
                &base_tree,
                &[&initial_commit],
            )
            .expect("commit merge file");
        drop(base_tree);
        drop(initial_commit);

        let base_commit = repo.find_commit(base_commit_oid).expect("find base commit");
        repo.branch("feature", &base_commit, false).expect("create feature branch");
        let feature_blob = repo.blob(b"feature").expect("write feature blob");
        let base_tree = base_commit.tree().expect("get base tree");
        let feature_tree_oid = {
            let mut builder = repo.treebuilder(Some(&base_tree)).expect("create tree builder");
            builder
                .insert("merged.txt", feature_blob, 0o100644)
                .expect("update merge file");
            builder.write().expect("write feature tree")
        };
        let feature_tree = repo.find_tree(feature_tree_oid).expect("find feature tree");
        repo.commit(
            Some("refs/heads/feature"),
            &signature,
            &signature,
            "update merge file",
            &feature_tree,
            &[&base_commit],
        )
        .expect("commit feature change");
        let original_head = repo
            .head()
            .expect("head")
            .resolve()
            .expect("resolve head");
        let original_head_oid = original_head.target().expect("head target");
        let original_head_name = original_head.name().expect("head name").to_string();
        drop(feature_tree);
        drop(base_tree);
        drop(base_commit);
        drop(original_head);
        drop(repo);

        let ref_lock = dir
            .path()
            .join(".git")
            .join(format!("{}.lock", original_head_name));
        std::fs::write(&ref_lock, "lock").expect("lock head reference");

        let git = open_git_repo(dir.path());
        let result = git.merge_branch("feature");
        std::fs::remove_file(&ref_lock).expect("remove head reference lock");

        assert!(result.is_err(), "a locked HEAD reference must fail the merge");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("merged.txt")).expect("read merge file"),
            "base",
            "a failed ref update must restore the pre-merge working tree"
        );

        let final_repo = Repository::open(dir.path()).expect("reopen failed merge repo");
        assert_eq!(
            final_repo
                .head()
                .expect("head")
                .resolve()
                .expect("resolve head")
                .target()
                .expect("head target"),
            original_head_oid,
            "a failed ref update must leave HEAD unchanged"
        );
        let final_index = final_repo.index().expect("read restored index");
        let merged_entry = final_index
            .get_path(Path::new("merged.txt"), 0)
            .expect("merged file in restored index");
        assert_eq!(
            final_repo
                .find_blob(merged_entry.id)
                .expect("find restored merge blob")
                .content(),
            b"base",
            "a failed ref update must restore the pre-merge index"
        );
    }

    #[test]
    fn test_open_linked_worktree_preserves_path_for_background_reopen() {
        let main_dir = tempfile::tempdir().expect("temp dir");
        let wt_root = tempfile::tempdir().expect("temp dir");
        let wt_path = wt_root.path().join("linked-wt");

        let repo = create_repo_with_commit(main_dir.path());
        let head = repo.head().expect("head");
        let commit = head.peel_to_commit().expect("commit");
        let wt_name = "linked-wt";
        let _branch = repo.branch(wt_name, &commit, false).expect("branch");
        let reference = repo.find_reference(&format!("refs/heads/{}", wt_name)).expect("reference");
        let mut opts = git2::WorktreeAddOptions::new();
        opts.reference(Some(&reference));
        repo.worktree(wt_name, &wt_path, Some(&opts)).expect("create worktree");

        std::fs::write(wt_path.join("staged.txt"), "change").expect("write worktree file");

        let git = open_git_repo(&wt_path);
        let reopened_path = git.path().expect("opened repo path").to_path_buf();
        assert_eq!(reopened_path, wt_path, "reopened operations must use the linked worktree");

        let progress = Arc::new(Mutex::new(String::new()));
        let result = execute_operation(&reopened_path, GitOperation::StageAll, progress);
        assert!(matches!(result, OpResult::Success(_)), "staging failed: {:?}", result);

        let linked_repo = Repository::open(&wt_path).expect("reopen linked worktree");
        let index = linked_repo.index().expect("linked worktree index");
        assert!(
            index.get_path(Path::new("staged.txt"), 0).is_some(),
            "background operation should update the linked worktree index"
        );
    }

    #[test]
    fn test_unstage_file_restores_head_entry_for_tracked_change() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        let path = Path::new("tracked.txt");

        std::fs::write(dir.path().join(path), "HEAD version\n").expect("write tracked file");
        let sig = repo.signature().expect("signature");
        let tree_oid = {
            let mut index = repo.index().expect("index");
            index.add_path(path).expect("add tracked file");
            index.write_tree().expect("write tree")
        };
        let tree = repo.find_tree(tree_oid).expect("find tree");
        let parent = repo.head().expect("HEAD").peel_to_commit().expect("parent commit");
        repo.commit(Some("HEAD"), &sig, &sig, "add tracked file", &tree, &[&parent])
            .expect("commit tracked file");
        drop(tree);

        std::fs::write(dir.path().join(path), "working tree version\n").expect("modify tracked file");
        let git = open_git_repo(dir.path());
        git.stage_file(path.to_str().expect("UTF-8 path")).expect("stage file");
        git.unstage_file(path.to_str().expect("UTF-8 path")).expect("unstage file");

        let statuses = git.get_status().expect("get status");
        assert_eq!(statuses.len(), 1, "tracked change should remain a single unstaged entry");
        assert_eq!(statuses[0].path, Path::new("tracked.txt"));
        assert_eq!(statuses[0].status, 'M');
        assert!(!statuses[0].staged, "tracked change should no longer be staged");

        let reopened = Repository::open(dir.path()).expect("reopen repo");
        let head_tree = reopened.head().expect("HEAD").peel_to_tree().expect("HEAD tree");
        let head_entry = head_tree.get_path(path).expect("HEAD entry");
        let index = reopened.index().expect("index");
        let index_entry = index.get_path(path, 0).expect("index entry");

        assert_eq!(index_entry.id, head_entry.id(), "unstaging must restore the HEAD index entry");
        assert_eq!(
            std::fs::read_to_string(dir.path().join(path)).expect("read working tree file"),
            "working tree version\n",
            "unstaging must preserve the working tree change"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_status_preserves_non_utf8_path_for_file_operations() {
        use std::ffi::OsString;
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let dir = tempfile::tempdir().expect("temp dir");
        create_repo_with_commit(dir.path());
        let path_bytes = b"invalid-\xff.txt";
        let path = PathBuf::from(OsString::from_vec(path_bytes.to_vec()));
        std::fs::write(dir.path().join(&path), "untracked\n").expect("write non-UTF-8 file");

        let git = open_git_repo(dir.path());
        let statuses = git.get_status().expect("get status");
        assert_eq!(statuses.len(), 1, "the non-UTF-8 file should be reported once");
        assert_eq!(statuses[0].path.as_os_str().as_bytes(), path_bytes);
        assert!(!statuses[0].path.as_os_str().is_empty());

        git.stage_file(&statuses[0].path)
            .expect("stage the path returned by status");
        let reopened = Repository::open(dir.path()).expect("reopen repo");
        assert!(
            reopened
                .index()
                .expect("index")
                .get_path(&path, 0)
                .is_some(),
            "staging must use the original non-UTF-8 path bytes"
        );
    }

    #[test]
    fn test_worktrees_from_linked_worktree_identifies_main_path() {
        let main_dir = tempfile::tempdir().expect("temp dir");
        let wt_root = tempfile::tempdir().expect("temp dir");
        let wt_path = wt_root.path().join("linked-wt");

        let repo = create_repo_with_commit(main_dir.path());
        let head = repo.head().expect("head");
        let commit = head.peel_to_commit().expect("commit");
        let wt_name = "linked-wt";
        let _branch = repo.branch(wt_name, &commit, false).expect("branch");
        let reference = repo
            .find_reference(&format!("refs/heads/{}", wt_name))
            .expect("reference");
        let mut opts = git2::WorktreeAddOptions::new();
        opts.reference(Some(&reference));
        repo.worktree(wt_name, &wt_path, Some(&opts))
            .expect("create worktree");

        let git = open_git_repo(&wt_path);
        let worktrees = git.worktrees().expect("list worktrees");
        let main = worktrees
            .iter()
            .find(|worktree| worktree.is_main)
            .expect("main worktree");

        assert_eq!(main.path, main_dir.path());
    }

    #[test]
    fn test_worktrees_from_linked_worktree_with_separate_git_dir_identifies_main_path() {
        let root = tempfile::tempdir().expect("temp dir");
        let main_dir = root.path().join("main");
        let git_dir = root.path().join("repo.git");
        let wt_path = root.path().join("linked-wt");
        std::fs::create_dir(&main_dir).expect("create main worktree");

        let repo = create_repo_with_commit(&main_dir);
        drop(repo);
        std::fs::rename(main_dir.join(".git"), &git_dir)
            .expect("move git directory outside worktree");
        std::fs::write(
            main_dir.join(".git"),
            format!("gitdir: {}\n", git_dir.display()),
        )
        .expect("write separate git dir link");

        let repo = Repository::open(&main_dir).expect("open separate git dir repository");
        let head = repo.head().expect("head");
        let commit = head.peel_to_commit().expect("commit");
        let wt_name = "linked-wt";
        let _branch = repo.branch(wt_name, &commit, false).expect("branch");
        let reference = repo
            .find_reference(&format!("refs/heads/{}", wt_name))
            .expect("reference");
        let mut opts = git2::WorktreeAddOptions::new();
        opts.reference(Some(&reference));
        repo.worktree(wt_name, &wt_path, Some(&opts))
            .expect("create worktree");

        let git = open_git_repo(&wt_path);
        let worktrees = git.worktrees().expect("list worktrees");
        let main = worktrees
            .iter()
            .find(|worktree| worktree.is_main)
            .expect("main worktree");

        assert_eq!(main.path, main_dir);
    }

    #[test]
    fn test_failed_new_worktree_creation_removes_created_branch() {
        let main_dir = tempfile::tempdir().expect("temp dir");
        let wt_root = tempfile::tempdir().expect("temp dir");
        let wt_path = wt_root.path().join("existing");
        std::fs::create_dir(&wt_path).expect("create existing worktree path");
        std::fs::write(wt_path.join("blocker"), "not an empty worktree").expect("write blocker");

        create_repo_with_commit(main_dir.path());
        let git = open_git_repo(main_dir.path());

        let result = git.create_worktree("orphaned-branch", &wt_path, Some("main"), true);

        assert!(result.is_err(), "worktree creation should fail for a non-empty path");
        let repo = Repository::open(main_dir.path()).expect("reopen repo");
        assert!(
            repo.find_branch("orphaned-branch", BranchType::Local).is_err(),
            "failed worktree creation must not leave its newly created branch"
        );
    }

    #[test]
    fn test_unstage_file_treats_path_as_literal() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());

        let literal_path = Path::new("file[ab].txt");
        let other_path = Path::new("filea.txt");
        std::fs::write(dir.path().join(literal_path), "literal HEAD\n").expect("write literal file");
        std::fs::write(dir.path().join(other_path), "other HEAD\n").expect("write other file");
        let sig = repo.signature().expect("signature");
        let tree_oid = {
            let mut index = repo.index().expect("index");
            index.add_path(literal_path).expect("add literal file");
            index.add_path(other_path).expect("add other file");
            index.write_tree().expect("write tree")
        };
        let tree = repo.find_tree(tree_oid).expect("find tree");
        let parent = repo.head().expect("HEAD").peel_to_commit().expect("parent commit");
        repo.commit(Some("HEAD"), &sig, &sig, "add literal files", &tree, &[&parent])
            .expect("commit literal files");
        drop(tree);

        std::fs::write(dir.path().join(literal_path), "literal changed\n").expect("modify literal file");
        std::fs::write(dir.path().join(other_path), "other changed\n").expect("modify other file");

        let git = open_git_repo(dir.path());
        git.stage_file(literal_path.to_str().expect("UTF-8 path")).expect("stage literal file");
        git.stage_file(other_path.to_str().expect("UTF-8 path")).expect("stage other file");
        git.unstage_file(literal_path.to_str().expect("UTF-8 path")).expect("unstage literal file");

        let reopened = Repository::open(dir.path()).expect("reopen repo");
        let index = reopened.index().expect("index");
        let head_tree = reopened.head().expect("HEAD").peel_to_tree().expect("HEAD tree");
        let head_entry = head_tree.get_path(literal_path).expect("HEAD literal entry");
        let literal_entry = index.get_path(literal_path, 0).expect("literal index entry");
        assert!(
            literal_entry.id == head_entry.id(),
            "the requested literal path should be restored from HEAD"
        );
        assert!(
            index.get_path(other_path, 0).is_some(),
            "a path matched by the literal path text must remain staged"
        );

        let statuses = git.get_status().expect("get status");
        assert!(
            statuses.iter().any(|entry| entry.path == literal_path && !entry.staged),
            "the literal path should remain as an unstaged working-tree change"
        );
        assert!(
            statuses.iter().any(|entry| entry.path == other_path && entry.staged),
            "the path matched by the literal text should remain staged"
        );
    }

    #[test]
    fn test_unstage_file_rejects_invalid_paths_without_changing_index() {
        let dir = tempfile::tempdir().expect("temp dir");
        create_repo_with_commit(dir.path());

        let staged_path = Path::new("staged.txt");
        std::fs::write(dir.path().join(staged_path), "staged\n").expect("write staged file");

        let git = open_git_repo(dir.path());
        git.stage_file(staged_path.to_str().expect("UTF-8 path")).expect("stage file");
        for invalid_path in [
            "",
            "../staged.txt",
            "/absolute/staged.txt",
            "dir/../staged.txt",
            "dir/./staged.txt",
            "dir//staged.txt",
            "dir///staged.txt",
            "bad\0path",
        ] {
            let error = git.unstage_file(invalid_path).expect_err("invalid path should be rejected");
            assert!(error.starts_with("Unstage: path"));
        }

        let reopened = Repository::open(dir.path()).expect("reopen repo");
        let index = reopened.index().expect("index");
        assert!(
            index.get_path(staged_path, 0).is_some(),
            "rejecting an empty path must not change the index"
        );
    }

    #[test]
    fn test_log_search_result_preserves_request_id() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        drop(repo);

        let result = execute_operation(
            dir.path(),
            GitOperation::LogSearch {
                filter: "initial".to_string(),
                request_id: 42,
            },
            Arc::new(Mutex::new(String::new())),
        );

        match result {
            OpResult::SearchResults { request_id, filter, commits } => {
                assert_eq!(request_id, 42);
                assert_eq!(filter, "initial");
                assert_eq!(commits.len(), 1);
            }
            other => panic!("expected search results, got {:?}", other),
        }
    }

    #[test]
    fn test_unstage_all_preserves_index_entries() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());

        std::fs::write(dir.path().join("tracked.txt"), "committed").expect("write tracked file");
        std::fs::write(dir.path().join("unchanged.txt"), "unchanged").expect("write unchanged file");
        {
            let mut index = repo.index().expect("index");
            index.add_path(Path::new("tracked.txt")).expect("stage tracked file");
            index.add_path(Path::new("unchanged.txt")).expect("stage unchanged file");
            index.write().expect("write index");
        }

        let parent = repo.head().expect("head").peel_to_commit().expect("parent commit");
        let signature = repo.signature().expect("signature");
        let tree_oid = {
            let mut index = repo.index().expect("index");
            index.write_tree().expect("write tree")
        };
        let tree = repo.find_tree(tree_oid).expect("find tree");
        repo.commit(Some("HEAD"), &signature, &signature, "add tracked files", &tree, &[&parent])
            .expect("commit tracked files");
        drop(tree);
        drop(parent);
        drop(repo);

        let git = open_git_repo(dir.path());
        std::fs::write(dir.path().join("tracked.txt"), "working tree change")
            .expect("modify tracked file");
        git.stage_file("tracked.txt").expect("stage tracked change");
        git.unstage_all().expect("unstage all");

        let repo = Repository::open(dir.path()).expect("reopen repo");
        let index = repo.index().expect("index");
        assert_eq!(index.len(), 2, "unstage all must keep tracked index entries");
        assert!(
            index.get_path(Path::new("tracked.txt"), 0).is_some(),
            "modified tracked file must remain in the index"
        );
        assert!(
            index.get_path(Path::new("unchanged.txt"), 0).is_some(),
            "unchanged tracked file must remain in the index"
        );
        let statuses = git.get_status().expect("get status");
        assert!(
            statuses.iter().any(|entry| entry.path == Path::new("tracked.txt") && !entry.staged),
            "tracked change must be unstaged; statuses: {:?}",
            statuses
        );
        assert!(
            statuses.iter().all(|entry| entry.path != Path::new("tracked.txt") || !entry.staged),
            "tracked change must not remain staged"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("tracked.txt")).expect("read working tree"),
            "working tree change",
            "unstage all must not modify the working tree"
        );
    }

    #[test]
    fn test_unstage_file_restores_file_directory_replacement() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        let path = Path::new("dir");

        std::fs::write(dir.path().join(path), "HEAD file\n").expect("write HEAD file");
        let sig = repo.signature().expect("signature");
        let tree_oid = {
            let mut index = repo.index().expect("index");
            index.add_path(path).expect("add HEAD file");
            index.write_tree().expect("write tree")
        };
        let tree = repo.find_tree(tree_oid).expect("find tree");
        let parent = repo.head().expect("HEAD").peel_to_commit().expect("parent commit");
        repo.commit(Some("HEAD"), &sig, &sig, "add directory replacement file", &tree, &[&parent])
            .expect("commit HEAD file");
        drop(tree);

        std::fs::remove_file(dir.path().join(path)).expect("remove HEAD file");
        std::fs::create_dir(dir.path().join(path)).expect("create replacement directory");
        let child_path = path.join("child.txt");
        std::fs::write(dir.path().join(&child_path), "staged child\n").expect("write staged child");

        let replacement_repo = Repository::open(dir.path()).expect("reopen repo");
        let mut index = replacement_repo.index().expect("index");
        index.remove_path(path).expect("remove HEAD file from index");
        index.add_path(&child_path).expect("stage replacement child");
        index.write().expect("write replacement index");

        let git = open_git_repo(dir.path());
        git.unstage_file(path.to_str().expect("UTF-8 path")).expect("unstage replacement");

        let reopened = Repository::open(dir.path()).expect("reopen repo");
        let index = reopened.index().expect("index");
        assert!(index.get_path(path, 0).is_some(), "HEAD file should be restored");
        assert!(index.get_path(&child_path, 0).is_none(), "replacement child should be removed");
    }

    #[test]
    fn test_unstage_file_restores_directory_file_replacement() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        let path = Path::new("dir");
        let child_path = path.join("child.txt");

        std::fs::create_dir(dir.path().join(path)).expect("create HEAD directory");
        std::fs::write(dir.path().join(&child_path), "HEAD child\n").expect("write HEAD child");
        let sig = repo.signature().expect("signature");
        let tree_oid = {
            let mut index = repo.index().expect("index");
            index.add_path(&child_path).expect("add HEAD child");
            index.write_tree().expect("write tree")
        };
        let tree = repo.find_tree(tree_oid).expect("find tree");
        let parent = repo.head().expect("HEAD").peel_to_commit().expect("parent commit");
        repo.commit(Some("HEAD"), &sig, &sig, "add directory replacement directory", &tree, &[&parent])
            .expect("commit HEAD directory");
        drop(tree);

        std::fs::remove_file(dir.path().join(&child_path)).expect("remove HEAD child");
        std::fs::remove_dir(dir.path().join(path)).expect("remove HEAD directory");
        std::fs::write(dir.path().join(path), "staged file\n").expect("write staged file");

        let replacement_repo = Repository::open(dir.path()).expect("reopen repo");
        let mut index = replacement_repo.index().expect("index");
        index.remove_path(&child_path).expect("remove HEAD child from index");
        index.add_path(path).expect("stage replacement file");
        index.write().expect("write replacement index");

        let git = open_git_repo(dir.path());
        git.unstage_file(path.to_str().expect("UTF-8 path")).expect("unstage replacement");

        let reopened = Repository::open(dir.path()).expect("reopen repo");
        let index = reopened.index().expect("index");
        assert!(index.get_path(path, 0).is_none(), "replacement file should be removed");
        assert!(index.get_path(&child_path, 0).is_some(), "HEAD child should be restored");
    }

    #[test]
    fn test_unstage_file_removes_directory_descendants_on_unborn_branch() {
        let dir = tempfile::tempdir().expect("temp dir");
        Repository::init(dir.path()).expect("init repo");
        let child_path = Path::new("dir/child.txt");
        std::fs::create_dir(dir.path().join("dir")).expect("create directory");
        std::fs::write(dir.path().join(child_path), "staged child\n").expect("write child");

        let git = open_git_repo(dir.path());
        git.stage_file(child_path.to_str().expect("UTF-8 path")).expect("stage child");
        git.unstage_file("dir").expect("unstage directory");

        let reopened = Repository::open(dir.path()).expect("reopen repo");
        let index = reopened.index().expect("index");
        assert!(index.get_path(child_path, 0).is_none(), "directory child should be removed");
    }

    #[test]
    fn test_unstage_all_on_unborn_head_clears_index() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = Repository::init(dir.path()).expect("init repo");
        std::fs::write(dir.path().join("new.txt"), "new file").expect("write new file");
        {
            let mut index = repo.index().expect("index");
            index.add_path(Path::new("new.txt")).expect("stage new file");
            index.write().expect("write index");
        }
        drop(repo);

        let git = open_git_repo(dir.path());
        git.unstage_all().expect("unstage all");

        let repo = Repository::open(dir.path()).expect("reopen repo");
        let index = repo.index().expect("index");
        assert!(index.is_empty(), "unstage all must clear staged entries without a HEAD");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("new.txt")).expect("read working tree"),
            "new file",
            "unstage all must not modify the working tree"
        );
    }

    #[test]
    fn test_restore_all_discards_dirty_worktree_content() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        let tracked_path = dir.path().join("tracked.txt");

        std::fs::write(&tracked_path, "committed").expect("write tracked file");
        let parent = repo.head().expect("head").peel_to_commit().expect("parent commit");
        let signature = repo.signature().expect("signature");
        let tree_oid = {
            let mut index = repo.index().expect("index");
            index.add_path(Path::new("tracked.txt")).expect("stage tracked file");
            index.write_tree().expect("write tree")
        };
        let tree = repo.find_tree(tree_oid).expect("find tree");
        repo.commit(Some("HEAD"), &signature, &signature, "add tracked file", &tree, &[&parent])
            .expect("commit tracked file");
        drop(tree);
        drop(parent);
        drop(repo);

        std::fs::write(&tracked_path, "dirty").expect("modify tracked file");

        let git = open_git_repo(dir.path());
        git.restore_all().expect("restore all");

        assert_eq!(
            std::fs::read_to_string(&tracked_path).expect("read restored file"),
            "committed",
            "restore all must discard dirty working-tree content"
        );
    }

    #[test]
    fn test_restore_file_preserves_staged_changes() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());

        std::fs::write(dir.path().join("tracked.txt"), "HEAD").expect("write tracked file");
        let parent = repo.head().expect("HEAD").peel_to_commit().expect("parent commit");
        let signature = repo.signature().expect("signature");
        let tree_oid = {
            let mut index = repo.index().expect("index");
            index.add_path(Path::new("tracked.txt")).expect("stage tracked file");
            index.write_tree().expect("write tree")
        };
        let tree = repo.find_tree(tree_oid).expect("find tree");
        repo.commit(Some("HEAD"), &signature, &signature, "add tracked file", &tree, &[&parent])
            .expect("commit tracked file");
        drop(tree);
        drop(parent);
        drop(repo);

        let git = open_git_repo(dir.path());
        std::fs::write(dir.path().join("tracked.txt"), "staged")
            .expect("write staged version");
        git.stage_file("tracked.txt").expect("stage tracked change");
        std::fs::write(dir.path().join("tracked.txt"), "unstaged")
            .expect("write unstaged version");

        git.restore_file("tracked.txt").expect("restore file");

        assert_eq!(
            std::fs::read_to_string(dir.path().join("tracked.txt")).expect("read working tree"),
            "staged",
            "discarding unstaged changes must restore the index version"
        );
        let statuses = git.get_status().expect("get status");
        assert!(
            statuses.iter().any(|entry| entry.path == Path::new("tracked.txt") && entry.staged),
            "the staged change must remain staged; statuses: {:?}",
            statuses
        );
        assert!(
            statuses.iter().all(|entry| entry.path != Path::new("tracked.txt") || entry.staged),
            "restoring unstaged changes must leave no unstaged portion; statuses: {:?}",
            statuses
        );
    }

    #[test]
    fn test_restore_file_treats_path_as_literal() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        let literal_path = Path::new("file[ab].txt");
        let other_path = Path::new("filea.txt");

        std::fs::write(dir.path().join(literal_path), "literal HEAD\n").expect("write literal file");
        std::fs::write(dir.path().join(other_path), "other HEAD\n").expect("write other file");
        let signature = repo.signature().expect("signature");
        let tree_oid = {
            let mut index = repo.index().expect("index");
            index.add_path(literal_path).expect("stage literal file");
            index.add_path(other_path).expect("stage other file");
            index.write_tree().expect("write tree")
        };
        let tree = repo.find_tree(tree_oid).expect("find tree");
        let parent = repo.head().expect("HEAD").peel_to_commit().expect("parent commit");
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "add literal files",
            &tree,
            &[&parent],
        )
        .expect("commit literal files");
        drop(tree);
        drop(parent);
        drop(repo);

        let git = open_git_repo(dir.path());
        git.stage_file(literal_path.to_str().expect("UTF-8 path"))
            .expect("stage literal HEAD");
        git.stage_file(other_path.to_str().expect("UTF-8 path"))
            .expect("stage other HEAD");
        std::fs::write(dir.path().join(literal_path), "literal dirty\n").expect("modify literal file");
        std::fs::write(dir.path().join(other_path), "other dirty\n").expect("modify other file");

        git.restore_file(literal_path.to_str().expect("UTF-8 path"))
            .expect("restore literal file");

        assert_eq!(
            std::fs::read_to_string(dir.path().join(literal_path)).expect("read literal file"),
            "literal HEAD\n",
            "restore must target the requested literal path"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join(other_path)).expect("read other file"),
            "other dirty\n",
            "restore must not affect a path matched by the literal text as a glob"
        );
    }

    #[test]
    fn test_diff_treats_path_as_literal() {
        for staged in [false, true] {
            let dir = tempfile::tempdir().expect("temp dir");
            let repo = create_repo_with_commit(dir.path());
            let literal_path = Path::new("file[ab].txt");
            let other_path = Path::new("filea.txt");

            std::fs::write(dir.path().join(literal_path), "literal HEAD\n").expect("write literal file");
            std::fs::write(dir.path().join(other_path), "other HEAD\n").expect("write other file");
            let signature = repo.signature().expect("signature");
            let tree_oid = {
                let mut index = repo.index().expect("index");
                index.add_path(literal_path).expect("stage literal file");
                index.add_path(other_path).expect("stage other file");
                index.write_tree().expect("write tree")
            };
            let tree = repo.find_tree(tree_oid).expect("find tree");
            let parent = repo.head().expect("HEAD").peel_to_commit().expect("parent commit");
            repo.commit(
                Some("HEAD"),
                &signature,
                &signature,
                "add literal files",
                &tree,
                &[&parent],
            )
            .expect("commit literal files");
            drop(tree);
            drop(parent);
            drop(repo);

            let git = open_git_repo(dir.path());
            git.stage_file(literal_path.to_str().expect("UTF-8 path"))
                .expect("stage literal HEAD");
            git.stage_file(other_path.to_str().expect("UTF-8 path"))
                .expect("stage other HEAD");
            std::fs::write(dir.path().join(literal_path), "literal changed\n")
                .expect("modify literal file");
            std::fs::write(dir.path().join(other_path), "other changed\n")
                .expect("modify other file");

            if staged {
                git.stage_file(literal_path.to_str().expect("UTF-8 path"))
                    .expect("stage literal file");
                git.stage_file(other_path.to_str().expect("UTF-8 path"))
                    .expect("stage other file");
            }

            let diff = git
                .get_diff(literal_path.to_str().expect("UTF-8 path"), staged)
                .expect("get literal diff");

            assert!(
                diff.iter()
                    .any(|line| line.origin == '+' && line.content == "literal changed\n"),
                "literal diff should include the selected file (staged: {staged}): {:?}",
                diff.iter().map(|line| (&line.origin, &line.content)).collect::<Vec<_>>()
            );
            assert!(
                diff.iter().all(|line| line.content != "other changed\n"),
                "literal diff should not include the glob-matched file (staged: {staged}): {:?}",
                diff.iter().map(|line| (&line.origin, &line.content)).collect::<Vec<_>>()
            );
        }

        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        let literal_path = Path::new("untracked[ab].txt");
        let other_path = Path::new("untrackeda.txt");
        std::fs::write(dir.path().join(literal_path), "literal untracked\n")
            .expect("write literal untracked file");
        std::fs::write(dir.path().join(other_path), "other untracked\n")
            .expect("write other untracked file");
        drop(repo);

        let git = open_git_repo(dir.path());
        let diff = git
            .get_diff(literal_path.to_str().expect("UTF-8 path"), false)
            .expect("get untracked literal diff");

        assert!(
            diff.iter()
                .any(|line| line.origin == '+' && line.content == "literal untracked\n"),
            "untracked literal diff should include the selected file: {:?}",
            diff.iter().map(|line| (&line.origin, &line.content)).collect::<Vec<_>>()
        );
        assert!(
            diff.iter().all(|line| line.content != "other untracked\n"),
            "untracked literal diff should not include the glob-matched file: {:?}",
            diff.iter().map(|line| (&line.origin, &line.content)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_force_remove_rejects_unregistered_main_worktree() {
        let main_dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(main_dir.path());
        commit_file(&repo, "important.txt", "keep this repository", "add important file");
        drop(repo);

        let git = open_git_repo(main_dir.path());
        let result = git.remove_worktree(main_dir.path(), true);

        assert!(result.is_err(), "Force remove must reject an unregistered path");
        assert!(main_dir.path().exists(), "The main repository directory must be preserved");
        assert!(
            main_dir.path().join("important.txt").exists(),
            "Files in the main repository must be preserved"
        );
    }

    #[test]
    fn test_normal_remove_rejects_clean_unrelated_repository() {
        let main_dir = tempfile::tempdir().expect("main temp dir");
        create_repo_with_commit(main_dir.path());

        let unrelated_dir = tempfile::tempdir().expect("unrelated temp dir");
        let unrelated_repo = create_repo_with_commit(unrelated_dir.path());
        commit_file(
            &unrelated_repo,
            "important.txt",
            "keep this unrelated repository",
            "add important file",
        );
        drop(unrelated_repo);

        let git = open_git_repo(main_dir.path());
        let result = git.remove_worktree(unrelated_dir.path(), false);

        assert!(result.is_err(), "Normal remove must reject an unregistered path");
        assert!(
            unrelated_dir.path().exists(),
            "The unrelated repository directory must be preserved"
        );
        assert!(
            unrelated_dir.path().join("important.txt").exists(),
            "Files in the unrelated repository must be preserved"
        );
    }

    #[test]
    fn test_remove_worktree_rejects_replaced_worktree_path() {
        for force in [false, true] {
            let main_dir = tempfile::tempdir().expect("main temp dir");
            let wt_root = tempfile::tempdir().expect("worktree temp dir");
            let wt_path = wt_root.path().join("replaced-wt");

            let repo = create_repo_with_commit(main_dir.path());
            let wt_name = "replaced-wt";
            let commit = repo.head().expect("head").peel_to_commit().expect("commit");
            let branch = repo.branch(wt_name, &commit, false).expect("branch");
            let reference = repo
                .find_reference(&format!("refs/heads/{}", wt_name))
                .expect("reference");
            let mut opts = git2::WorktreeAddOptions::new();
            opts.reference(Some(&reference));
            repo.worktree(wt_name, &wt_path, Some(&opts)).expect("create worktree");
            drop(reference);
            drop(branch);
            drop(commit);

            std::fs::remove_dir_all(&wt_path).expect("remove original worktree directory");
            let replacement = create_repo_with_commit(&wt_path);
            commit_file(
                &replacement,
                "important.txt",
                "keep this replacement repository",
                "add important file",
            );
            drop(replacement);
            drop(repo);

            let git = open_git_repo(main_dir.path());
            let result = git.remove_worktree(&wt_path, force);

            assert!(
                result.is_err(),
                "Removal must reject a path whose contents no longer identify the registered worktree"
            );
            assert!(wt_path.exists(), "The replacement repository must be preserved");
            assert!(
                wt_path.join("important.txt").exists(),
                "Files in the replacement repository must be preserved"
            );
        }
    }

    #[test]
    fn test_remove_worktree_rejects_copied_git_link_after_selection() {
        let main_dir = tempfile::tempdir().expect("main temp dir");
        let wt_root = tempfile::tempdir().expect("worktree temp dir");
        let wt_path = wt_root.path().join("copied-link-wt");

        let repo = create_repo_with_commit(main_dir.path());
        let wt_name = "copied-link-wt";
        let commit = repo.head().expect("head").peel_to_commit().expect("commit");
        let branch = repo.branch(wt_name, &commit, false).expect("branch");
        let reference = repo
            .find_reference(&format!("refs/heads/{}", wt_name))
            .expect("reference");
        let mut opts = git2::WorktreeAddOptions::new();
        opts.reference(Some(&reference));
        repo.worktree(wt_name, &wt_path, Some(&opts)).expect("create worktree");
        let wt_gitdir = repo.commondir().join("worktrees").join(wt_name);
        drop(reference);
        drop(branch);
        drop(commit);
        drop(repo);

        let git = open_git_repo(main_dir.path());
        let expected_identity = git
            .worktrees()
            .expect("list worktrees")
            .into_iter()
            .find(|worktree| worktree.path == wt_path)
            .and_then(|worktree| worktree.git_link_identity)
            .expect("capture git link identity");
        let git_link_contents = std::fs::read(wt_path.join(".git")).expect("read git link");

        std::fs::remove_dir_all(&wt_path).expect("remove original worktree directory");
        std::fs::create_dir(&wt_path).expect("create replacement directory");
        std::fs::write(wt_path.join(".git"), git_link_contents).expect("copy git link");
        std::fs::write(wt_path.join("important.txt"), "keep copied-link replacement")
            .expect("write replacement file");
        let result = git.remove_worktree_with_identity(&wt_path, true, Some(expected_identity), true);

        assert!(result.is_err(), "Removal must reject a copied .git link after selection");
        assert!(wt_path.exists(), "The copied-link replacement must be preserved");
        assert!(wt_path.join("important.txt").exists(), "Replacement files must be preserved");
        assert!(wt_gitdir.exists(), "Registered worktree metadata must be preserved");

        let unavailable_snapshot_result =
            git.remove_worktree_with_identity(&wt_path, true, None, true);
        assert!(
            unavailable_snapshot_result.is_err(),
            "Removal must reject an existing path when the listed identity was unavailable"
        );
        assert!(wt_path.exists(), "The replacement must remain after an unavailable snapshot");
    }

    #[test]
    fn test_staged_worktree_link_must_resolve_to_registered_metadata() {
        let main_dir = tempfile::tempdir().expect("main temp dir");
        let wt_root = tempfile::tempdir().expect("worktree temp dir");
        let wt_path = wt_root.path().join("registered-wt");
        let staged_path = wt_root.path().join("staged-wt");

        let repo = create_repo_with_commit(main_dir.path());
        let wt_name = "registered-wt";
        let commit = repo.head().expect("head").peel_to_commit().expect("commit");
        let branch = repo.branch(wt_name, &commit, false).expect("branch");
        let reference = repo
            .find_reference(&format!("refs/heads/{}", wt_name))
            .expect("reference");
        let mut opts = git2::WorktreeAddOptions::new();
        opts.reference(Some(&reference));
        repo.worktree(wt_name, &wt_path, Some(&opts)).expect("create worktree");
        let worktree = repo.find_worktree(wt_name).expect("find worktree");
        drop(reference);
        drop(branch);
        drop(commit);

        std::fs::create_dir(&staged_path).expect("create staged path");
        std::fs::copy(wt_path.join(".git"), staged_path.join(".git")).expect("copy registered link");
        assert!(registered_worktree_link_matches(&repo, &worktree, &staged_path, &wt_path));
        let git_link_identity = worktree_git_link_identity(&wt_path).expect("registered git link identity");
        assert!(
            !staged_worktree_link_matches(&staged_path, &git_link_identity),
            "A copied .git file must not be accepted as the staged worktree identity"
        );

        std::fs::write(staged_path.join(".git"), b"unrelated header\ngitdir: /tmp/invalid\n")
            .expect("write malformed git link");
        assert!(!registered_worktree_link_matches(&repo, &worktree, &staged_path, &wt_path));

        let main_name = main_dir
            .path()
            .file_name()
            .expect("main directory name")
            .to_string_lossy();
        std::fs::write(
            staged_path.join(".git"),
            format!("gitdir: ../../{}/.git/worktrees/{}\n", main_name, wt_name),
        )
        .expect("write relative registered link");
        assert!(registered_worktree_link_matches(&repo, &worktree, &staged_path, &wt_path));

        std::fs::write(staged_path.join(".git"), b"gitdir: registered\ntrailing garbage")
            .expect("write trailing git link data");
        assert!(!registered_worktree_link_matches(&repo, &worktree, &staged_path, &wt_path));

        let unrelated_dir = tempfile::tempdir().expect("unrelated temp dir");
        let unrelated_repo = create_repo_with_commit(unrelated_dir.path());
        let unrelated_gitdir = unrelated_repo.path().to_path_buf();
        drop(unrelated_repo);
        std::fs::write(
            staged_path.join(".git"),
            format!("gitdir: {}\n", unrelated_gitdir.display()),
        )
        .expect("write unrelated link");
        assert!(!registered_worktree_link_matches(&repo, &worktree, &staged_path, &wt_path));
    }

    #[test]
    fn test_force_fallback_identity_survives_git_link_deletion() {
        let root = tempfile::tempdir().expect("temp dir");
        let path = root.path().join("worktree");
        std::fs::create_dir(&path).expect("create worktree directory");
        std::fs::write(path.join(".git"), "gitdir: placeholder\n").expect("write git link");
        let path_metadata = std::fs::symlink_metadata(&path).expect("capture directory identity");

        std::fs::remove_file(path.join(".git")).expect("remove git link during cleanup");

        assert!(
            path_identity_matches(&path, &path_metadata),
            "Directory identity must remain valid after recursive cleanup removes .git"
        );
    }

    #[test]
    fn test_force_fallback_rejects_replaced_git_link() {
        let root = tempfile::tempdir().expect("temp dir");
        let path = root.path().join("worktree");
        std::fs::create_dir(&path).expect("create worktree directory");
        std::fs::write(path.join(".git"), b"gitdir: expected\n").expect("write git link");
        std::fs::write(path.join("important.txt"), b"keep this file").expect("write worktree file");

        let expected_path = std::fs::symlink_metadata(&path).expect("capture directory identity");
        let expected_git_link =
            worktree_git_link_identity(&path).expect("capture git link identity");
        let is_safe = worktree_fallback_identity_guard(&path, &expected_path, &expected_git_link);
        assert!(is_safe(), "Original directory and git link should match");

        let replacement_git = path.join(".git-replacement");
        std::fs::write(&replacement_git, b"gitdir: replacement\n")
            .expect("write replacement git link");
        std::fs::remove_file(path.join(".git")).expect("remove original git link");
        std::fs::rename(&replacement_git, path.join(".git")).expect("install replacement git link");

        let result = force_remove_dir_checked(&path, is_safe);

        assert!(result.is_err(), "Fallback must reject a replaced .git link");
        assert!(
            path_identity_matches(&path, &expected_path),
            "Outer directory identity should be unchanged"
        );
        assert_eq!(
            std::fs::read(path.join("important.txt")).expect("read preserved worktree file"),
            b"keep this file",
            "Replacement worktree data must be preserved"
        );
        assert_eq!(
            std::fs::read(path.join(".git")).expect("read replacement git link"),
            b"gitdir: replacement\n"
        );
    }

    #[test]
    fn test_normal_remove_worktree_with_relative_git_link() {
        let main_dir = tempfile::tempdir().expect("main temp dir");
        let wt_root = tempfile::tempdir().expect("worktree temp dir");
        let wt_path = wt_root.path().join("relative-wt");

        let repo = create_repo_with_commit(main_dir.path());
        let wt_name = "relative-wt";
        let commit = repo.head().expect("head").peel_to_commit().expect("commit");
        let branch = repo.branch(wt_name, &commit, false).expect("branch");
        let reference = repo
            .find_reference(&format!("refs/heads/{}", wt_name))
            .expect("reference");
        let mut opts = git2::WorktreeAddOptions::new();
        opts.reference(Some(&reference));
        repo.worktree(wt_name, &wt_path, Some(&opts)).expect("create worktree");
        let wt_gitdir = repo.commondir().join("worktrees").join(wt_name);
        let main_name = main_dir
            .path()
            .file_name()
            .expect("main directory name")
            .to_string_lossy();
        std::fs::write(
            wt_path.join(".git"),
            format!("gitdir: ../../{}/.git/worktrees/{}\n", main_name, wt_name),
        )
        .expect("write relative registered link");
        drop(reference);
        drop(branch);
        drop(commit);
        drop(repo);

        let git = open_git_repo(main_dir.path());
        git.remove_worktree(&wt_path, false)
            .expect("remove relative-link worktree");

        assert!(!wt_path.exists(), "Relative-link worktree directory should be removed");
        assert!(!wt_gitdir.exists(), "Relative-link worktree metadata should be removed");
    }

    #[cfg(unix)]
    #[test]
    fn test_force_remove_worktree_with_symlinked_git_file() {
        use std::os::unix::fs::symlink;

        let main_dir = tempfile::tempdir().expect("main temp dir");
        let wt_root = tempfile::tempdir().expect("worktree temp dir");
        let wt_path = wt_root.path().join("symlinked-git-wt");

        let repo = create_repo_with_commit(main_dir.path());
        let wt_name = "symlinked-git-wt";
        let commit = repo.head().expect("head").peel_to_commit().expect("commit");
        let branch = repo.branch(wt_name, &commit, false).expect("branch");
        let reference = repo
            .find_reference(&format!("refs/heads/{}", wt_name))
            .expect("reference");
        let mut opts = git2::WorktreeAddOptions::new();
        opts.reference(Some(&reference));
        repo.worktree(wt_name, &wt_path, Some(&opts)).expect("create worktree");
        let wt_gitdir = repo.commondir().join("worktrees").join(wt_name);
        std::fs::rename(wt_path.join(".git"), wt_path.join(".git-file"))
            .expect("move git file");
        symlink(".git-file", wt_path.join(".git")).expect("link git file");
        drop(reference);
        drop(branch);
        drop(commit);
        drop(repo);

        let git = open_git_repo(main_dir.path());
        git.remove_worktree(&wt_path, true)
            .expect("remove symlinked-git worktree");

        assert!(!wt_path.exists(), "Symlinked-git worktree directory should be removed");
        assert!(!wt_gitdir.exists(), "Symlinked-git worktree metadata should be removed");
    }

    #[cfg(unix)]
    #[test]
    fn test_worktree_git_link_preserves_non_utf8_path_bytes() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join(OsString::from_vec(b"g\xffit".to_vec()));
        std::fs::create_dir(&target).expect("create non-UTF-8 target");
        let contents = b"gitdir: g\xffit\n".to_vec();
        std::fs::write(dir.path().join(".git"), &contents).expect("write gitdir file");

        assert_eq!(worktree_git_link(dir.path()), Some(contents));
        assert_eq!(worktree_git_dir(dir.path()), Some(target.clone()));
        assert_eq!(worktree_git_dir_from_link(dir.path(), dir.path()), Some(target));
    }

    #[cfg(unix)]
    #[test]
    fn test_remove_worktree_rejects_dangling_symlink_path() {
        use std::os::unix::fs::symlink;

        let main_dir = tempfile::tempdir().expect("main temp dir");
        let wt_root = tempfile::tempdir().expect("worktree temp dir");
        let wt_path = wt_root.path().join("dangling-wt");
        let missing_target = wt_root.path().join("missing-target");

        let repo = create_repo_with_commit(main_dir.path());
        let wt_name = "dangling-wt";
        let commit = repo.head().expect("head").peel_to_commit().expect("commit");
        let branch = repo.branch(wt_name, &commit, false).expect("branch");
        let reference = repo
            .find_reference(&format!("refs/heads/{}", wt_name))
            .expect("reference");
        let mut opts = git2::WorktreeAddOptions::new();
        opts.reference(Some(&reference));
        repo.worktree(wt_name, &wt_path, Some(&opts)).expect("create worktree");
        let wt_gitdir = repo.commondir().join("worktrees").join(wt_name);
        drop(reference);
        drop(branch);
        drop(commit);
        std::fs::remove_dir_all(&wt_path).expect("remove original worktree directory");
        symlink(&missing_target, &wt_path).expect("create dangling symlink");
        drop(repo);

        let git = open_git_repo(main_dir.path());
        let result = git.remove_worktree(&wt_path, true);

        assert!(result.is_err(), "Removal must reject a dangling worktree path");
        assert!(
            std::fs::symlink_metadata(&wt_path).is_ok(),
            "The dangling symlink must be preserved"
        );
        assert!(wt_gitdir.exists(), "Worktree metadata must be preserved");
    }

    #[test]
    fn test_force_remove_valid_worktree() {
        let main_dir = tempfile::tempdir().expect("temp dir");
        let wt_root = tempfile::tempdir().expect("temp dir");
        let wt_path = wt_root.path().join("test-wt");

        // Create main repo with commit
        let repo = create_repo_with_commit(main_dir.path());

        // Create worktree
        let wt_name = "test-wt";
        let _sig = repo.signature().expect("sig");
        let head = repo.head().expect("head");
        let commit = head.peel_to_commit().expect("commit");
        let _branch = repo.branch(wt_name, &commit, false).expect("branch");
        let reference = repo.find_reference(&format!("refs/heads/{}", wt_name)).ok();
        let mut opts = git2::WorktreeAddOptions::new();
        if let Some(ref r) = reference {
            opts.reference(Some(r));
        }
        repo.worktree(wt_name, &wt_path, Some(&opts)).expect("create worktree");

        // Now remove it with force
        let git = open_git_repo(main_dir.path());
        let result = git.remove_worktree(&wt_path, true);
        assert!(result.is_ok(), "Force remove should succeed: {:?}", result);

        // Verify worktree directory is gone
        assert!(!wt_path.exists(), "Worktree dir should be removed");

        // Verify git metadata is removed
        let wt_gitdir = repo.path().join("worktrees").join(wt_name);
        assert!(!wt_gitdir.exists(), "Git worktree metadata should be removed");

        // Verify worktree is no longer listed
        let after_wts = git.worktrees().unwrap();
        assert_eq!(after_wts.len(), 1, "Only main worktree should remain");
        assert!(after_wts[0].is_main, "Remaining worktree should be main");
    }

    #[test]
    fn test_prune_preserves_valid_clean_and_dirty_worktrees() {
        let main_dir = tempfile::tempdir().expect("temp dir");
        let wt_root = tempfile::tempdir().expect("temp dir");
        let clean_path = wt_root.path().join("clean-wt");
        let dirty_path = wt_root.path().join("dirty-wt");
        let stale_path = wt_root.path().join("stale-wt");

        let repo = create_repo_with_commit(main_dir.path());
        let commit = repo.head().expect("head").peel_to_commit().expect("commit");

        for (name, path) in [
            ("clean-wt", &clean_path),
            ("dirty-wt", &dirty_path),
            ("stale-wt", &stale_path),
        ] {
            let branch = repo.branch(name, &commit, false).expect("branch");
            let reference = repo
                .find_reference(&format!("refs/heads/{}", name))
                .expect("reference");
            let mut opts = git2::WorktreeAddOptions::new();
            opts.reference(Some(&reference));
            repo.worktree(name, path, Some(&opts)).expect("create worktree");
            drop(reference);
            drop(branch);
        }

        let dirty_file = dirty_path.join("important.txt");
        std::fs::write(&dirty_file, "keep this change").expect("write dirty file");
        let stale_gitdir = repo.path().join("worktrees").join("stale-wt");
        std::fs::remove_dir_all(&stale_path).expect("remove stale worktree directory");
        drop(commit);
        drop(repo);

        let git = open_git_repo(main_dir.path());
        let pruned = git.prune_worktrees().expect("prune stale worktrees");

        assert_eq!(pruned, 1, "only the stale worktree should be pruned");
        assert!(clean_path.exists(), "valid clean worktree must be preserved");
        assert!(dirty_path.exists(), "valid dirty worktree must be preserved");
        assert_eq!(
            std::fs::read_to_string(&dirty_file).expect("read preserved dirty file"),
            "keep this change"
        );
        assert!(!stale_path.exists(), "stale worktree directory should remain absent");
        assert!(!stale_gitdir.exists(), "stale worktree metadata should be pruned");

        let remaining = git.worktrees().expect("list worktrees");
        assert_eq!(remaining.len(), 3, "main and both valid worktrees should remain");
        assert!(remaining.iter().any(|wt| wt.path == clean_path));
        assert!(remaining.iter().any(|wt| wt.path == dirty_path));
    }

    #[test]
    fn test_normal_remove_valid_succeeds() {
        let main_dir = tempfile::tempdir().expect("temp dir");
        let wt_root = tempfile::tempdir().expect("temp dir");
        let wt_path = wt_root.path().join("test-wt-normal");

        let repo = create_repo_with_commit(main_dir.path());

        let wt_name = "test-wt-normal";
        let _branch = repo.branch(wt_name, &repo.head().unwrap().peel_to_commit().unwrap(), false).unwrap();
        let reference = repo.find_reference(&format!("refs/heads/{}", wt_name)).ok();
        let mut opts = git2::WorktreeAddOptions::new();
        if let Some(ref r) = reference {
            opts.reference(Some(r));
        }
        repo.worktree(wt_name, &wt_path, Some(&opts)).expect("create worktree");

        let wt_gitdir = repo.path().join("worktrees").join(wt_name);

        // Normal remove should delete the directory before pruning its metadata.
        let git = open_git_repo(main_dir.path());
        let result = git.remove_worktree(&wt_path, false);
        assert!(result.is_ok(), "Normal remove should succeed with fallback: {:?}", result);
        assert!(!wt_path.exists(), "Worktree dir should be gone");
        assert!(!wt_gitdir.exists(), "Git worktree metadata should be cleaned up");
    }

    #[test]
    fn test_normal_remove_from_linked_worktree_cleans_shared_metadata() {
        let main_dir = tempfile::tempdir().expect("temp dir");
        let wt_root = tempfile::tempdir().expect("temp dir");
        let wt_path = wt_root.path().join("test-wt-linked-open");

        let repo = create_repo_with_commit(main_dir.path());
        let wt_name = "test-wt-linked-open";
        let _branch = repo
            .branch(wt_name, &repo.head().unwrap().peel_to_commit().unwrap(), false)
            .unwrap();
        let reference = repo.find_reference(&format!("refs/heads/{}", wt_name)).unwrap();
        let mut opts = git2::WorktreeAddOptions::new();
        opts.reference(Some(&reference));
        repo.worktree(wt_name, &wt_path, Some(&opts)).expect("create worktree");

        let wt_gitdir = repo.commondir().join("worktrees").join(wt_name);
        let git = open_git_repo(&wt_path);
        let result = git.remove_worktree(&wt_path, false);

        assert!(result.is_ok(), "Normal remove should succeed: {:?}", result);
        assert!(!wt_path.exists(), "Worktree dir should be gone");
        assert!(!wt_gitdir.exists(), "Linked worktree metadata should be cleaned up");
    }

    #[test]
    fn test_force_remove_missing_worktree_dir() {
        let main_dir = tempfile::tempdir().expect("temp dir");
        let wt_root = tempfile::tempdir().expect("temp dir");
        let wt_path = wt_root.path().join("test-wt-missing");

        let repo = create_repo_with_commit(main_dir.path());

        let wt_name = "test-wt-missing";
        let _branch = repo.branch(wt_name, &repo.head().unwrap().peel_to_commit().unwrap(), false).unwrap();
        let reference = repo.find_reference(&format!("refs/heads/{}", wt_name)).ok();
        let mut opts = git2::WorktreeAddOptions::new();
        if let Some(ref r) = reference {
            opts.reference(Some(r));
        }
        repo.worktree(wt_name, &wt_path, Some(&opts)).expect("create worktree");

        // Manually remove the worktree directory first
        std::fs::remove_dir_all(&wt_path).expect("remove wt dir");

        // Force remove should still clean up git metadata
        let git = open_git_repo(main_dir.path());
        let result = git.remove_worktree(&wt_path, true);
        assert!(result.is_ok(), "Force remove with missing dir should succeed: {:?}", result);

        // Verify git metadata is gone
        let wt_gitdir = repo.path().join("worktrees").join(wt_name);
        assert!(!wt_gitdir.exists(), "Git worktree metadata should be removed");
    }

    #[test]
    fn test_force_remove_already_gone() {
        let main_dir = tempfile::tempdir().expect("temp dir");
        let wt_root = tempfile::tempdir().expect("temp dir");
        let wt_path = wt_root.path().join("test-wt-gone");

        let repo = create_repo_with_commit(main_dir.path());

        let wt_name = "test-wt-gone";
        let _branch = repo.branch(wt_name, &repo.head().unwrap().peel_to_commit().unwrap(), false).unwrap();
        let reference = repo.find_reference(&format!("refs/heads/{}", wt_name)).ok();
        let mut opts = git2::WorktreeAddOptions::new();
        if let Some(ref r) = reference {
            opts.reference(Some(r));
        }
        repo.worktree(wt_name, &wt_path, Some(&opts)).expect("create worktree");

        // Remove both directory and git metadata manually
        std::fs::remove_dir_all(&wt_path).ok();
        let wt_gitdir = repo.path().join("worktrees").join(wt_name);
        std::fs::remove_dir_all(&wt_gitdir).ok();

        // Force remove on already-removed worktree should be a no-op success
        let git = open_git_repo(main_dir.path());
        let result = git.remove_worktree(&wt_path, true);
        assert!(result.is_ok(), "Remove already-gone worktree should succeed: {:?}", result);
    }

    #[test]
    fn test_remove_nonexistent_worktree() {
        let main_dir = tempfile::tempdir().expect("temp dir");
        create_repo_with_commit(main_dir.path());

        // Try to remove a worktree that was never created
        let nonexistent_path = main_dir.path().join("nonexistent-wt");
        let git = open_git_repo(main_dir.path());
        let result = git.remove_worktree(&nonexistent_path, true);
        assert!(result.is_ok(), "Remove nonexistent worktree should be ok: {:?}", result);
    }

    #[test]
    fn test_force_remove_worktree_wt_name_mismatch() {
        // Test: worktree with a different name than directory name
        let main_dir = tempfile::tempdir().expect("temp dir");
        let wt_root = tempfile::tempdir().expect("temp dir");
        let custom_dir_name = "my-custom-dir";
        let wt_path = wt_root.path().join(custom_dir_name);

        let repo = create_repo_with_commit(main_dir.path());

        // Create worktree with name "test-name" but at path ending in "my-custom-dir"
        let wt_name = "test-name";
        let _branch = repo.branch(wt_name, &repo.head().unwrap().peel_to_commit().unwrap(), false).unwrap();
        let reference = repo.find_reference(&format!("refs/heads/{}", wt_name)).ok();
        let mut opts = git2::WorktreeAddOptions::new();
        if let Some(ref r) = reference {
            opts.reference(Some(r));
        }
        repo.worktree(wt_name, &wt_path, Some(&opts)).expect("create worktree");

        // The path's file_name is "my-custom-dir", but the worktree name is "test-name"
        // Our implementation should still find it via the path-based fallback
        let git = open_git_repo(main_dir.path());
        let result = git.remove_worktree(&wt_path, true);
        assert!(result.is_ok(), "Force remove with name mismatch should succeed: {:?}", result);

        assert!(!wt_path.exists(), "Worktree dir should be removed");
        let wt_gitdir = repo.path().join("worktrees").join(wt_name);
        assert!(!wt_gitdir.exists(), "Git worktree metadata should be removed");
    }

    #[test]
    fn test_remove_worktree_with_basename_collision_preserves_other_metadata() {
        let main_dir = tempfile::tempdir().expect("temp dir");
        let first_root = tempfile::tempdir().expect("temp dir");
        let target_root = tempfile::tempdir().expect("temp dir");
        let first_path = first_root.path().join("collision-wt");
        let target_path = target_root.path().join("collision-wt");

        let repo = create_repo_with_commit(main_dir.path());
        let commit = repo.head().expect("head").peel_to_commit().expect("commit");

        let first_name = "collision-wt";
        let _first_branch = repo.branch(first_name, &commit, false).expect("first branch");
        let first_reference = repo
            .find_reference(&format!("refs/heads/{}", first_name))
            .expect("first reference");
        let mut first_opts = git2::WorktreeAddOptions::new();
        first_opts.reference(Some(&first_reference));
        repo.worktree(first_name, &first_path, Some(&first_opts))
            .expect("create first worktree");

        let target_name = "target-wt";
        let _target_branch = repo.branch(target_name, &commit, false).expect("target branch");
        let target_reference = repo
            .find_reference(&format!("refs/heads/{}", target_name))
            .expect("target reference");
        let mut target_opts = git2::WorktreeAddOptions::new();
        target_opts.reference(Some(&target_reference));
        repo.worktree(target_name, &target_path, Some(&target_opts))
            .expect("create target worktree");

        let first_gitdir = repo.path().join("worktrees").join(first_name);
        let target_gitdir = repo.path().join("worktrees").join(target_name);
        let git = open_git_repo(main_dir.path());
        git.remove_worktree(&target_path, true)
            .expect("remove target worktree");

        assert!(!target_path.exists(), "Target worktree directory should be removed");
        assert!(first_path.exists(), "Other worktree directory must be preserved");
        assert!(first_gitdir.exists(), "Other worktree metadata must be preserved");
        assert!(!target_gitdir.exists(), "Target worktree metadata should be removed");
    }

    // --- Encoding / UTF-8 tests ---

    #[test]
    fn test_safe_branch_name_utf8() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = create_repo_with_commit(dir.path());
        // Get the actual default branch name (may be 'master' or 'main' depending on git config)
        let head = repo.head().expect("HEAD");
        let expected_name = head.shorthand().expect("branch name").to_string();
        let branch = repo.find_branch(&expected_name, BranchType::Local).expect("find branch");
        let name = safe_branch_name(&branch);
        assert_eq!(name, expected_name, "Regular UTF-8 branch name '{}' should be preserved", expected_name);
    }

    #[test]
    fn test_safe_str_lossy_valid_utf8() {
        let text = "Hello, 世界!";
        let result = safe_str_lossy(Some(text), Some(text.as_bytes()));
        assert_eq!(result, text, "Valid UTF-8 text should be preserved");
    }

    #[test]
    fn test_safe_str_lossy_fallback_to_bytes() {
        let bytes: &[u8] = &[0x48, 0x65, 0x6c, 0x6c, 0x6f];
        // When text is None but bytes are valid UTF-8, should still work
        let result = safe_str_lossy(None, Some(bytes));
        assert_eq!(result, "Hello", "Should fall back to bytes when text is None");
    }

    #[test]
    fn test_safe_str_lossy_non_utf8_bytes() {
        let invalid_bytes: &[u8] = &[0x48, 0x65, 0xFF, 0xFE, 0x6c]; // invalid UTF-8
        let result = safe_str_lossy(None, Some(invalid_bytes));
        // Should use replacement characters for invalid bytes
        assert!(result.starts_with("He"), "Should preserve valid prefix");
        assert!(result.ends_with("l"), "Should preserve valid suffix");
    }

    #[test]
    fn test_safe_str_lossy_both_none() {
        let result = safe_str_lossy(None, None);
        assert_eq!(result, "", "Should return empty string when both are None");
    }

    // --- force_remove_dir tests ---

    #[test]
    fn test_force_remove_dir_normal() {
        let dir = tempfile::tempdir().expect("temp dir");
        let sub = dir.path().join("subdir");
        std::fs::create_dir_all(&sub).expect("create subdir");
        let file = sub.join("test.txt");
        std::fs::write(&file, "hello").expect("write file");

        assert!(dir.path().exists());
        force_remove_dir(dir.path()).expect("force_remove_dir should succeed");
        assert!(!dir.path().exists(), "Directory should be deleted");
    }

    #[cfg(windows)]
    #[test]
    fn test_force_remove_dir_fallback_handles_shell_metacharacters() {
        let root = tempfile::tempdir().expect("temp dir");
        let path = root
            .path()
            .join("worktree&echo|preserved%literal!name^");
        std::fs::create_dir_all(&path).expect("create directory");
        std::fs::write(path.join("file.txt"), "content").expect("write file");

        force_remove_dir_windows(&path).expect("Windows fallback should remove the directory");
        assert!(!path.exists(), "Directory with shell metacharacters should be deleted");
    }

    #[test]
    fn test_force_remove_dir_nonexistent() {
        let path = std::path::Path::new("C:\\this_path_should_not_exist_xyz_12345");
        // Should succeed even if path doesn't exist
        let result = force_remove_dir(path);
        assert!(result.is_ok(), "Removing nonexistent path should be ok: {:?}", result);
    }

    #[test]
    fn test_normal_remove_locked_clean_worktree_preserves_directory() {
        let main_dir = tempfile::tempdir().expect("main temp dir");
        let wt_root = tempfile::tempdir().expect("worktree temp dir");
        let wt_path = wt_root.path().join("locked-clean-wt");

        let repo = create_repo_with_commit(main_dir.path());
        commit_file(&repo, "important.txt", "keep this locked worktree", "add important file");
        let wt_name = "locked-clean-wt";
        let commit = repo.head().expect("head").peel_to_commit().expect("commit");
        let branch = repo.branch(wt_name, &commit, false).expect("branch");
        let reference = repo
            .find_reference(&format!("refs/heads/{}", wt_name))
            .expect("reference");
        let mut opts = git2::WorktreeAddOptions::new();
        opts.reference(Some(&reference));
        repo.worktree(wt_name, &wt_path, Some(&opts)).expect("create worktree");
        let wt_gitdir = repo.commondir().join("worktrees").join(wt_name);
        repo.find_worktree(wt_name)
            .expect("find worktree")
            .lock(Some("preserve this worktree"))
            .expect("lock worktree");
        let lock_path = wt_gitdir.join("locked");
        assert!(lock_path.exists(), "The setup must create a Git worktree lock");
        drop(reference);
        drop(branch);
        drop(commit);
        drop(repo);

        let git = open_git_repo(main_dir.path());
        let result = git.remove_worktree(&wt_path, false);

        assert!(result.is_err(), "Normal remove must reject a locked worktree");
        assert!(wt_path.exists(), "Locked worktree directory must be preserved");
        assert_eq!(
            std::fs::read_to_string(wt_path.join("important.txt")).expect("read preserved file"),
            "keep this locked worktree"
        );
        assert!(wt_gitdir.exists(), "Locked worktree metadata must be preserved");
        assert!(lock_path.exists(), "The pre-existing worktree lock must be preserved");
    }

    #[cfg(unix)]
    #[test]
    fn test_normal_remove_malformed_locked_worktree_preserves_directory() {
        let main_dir = tempfile::tempdir().expect("main temp dir");
        let wt_root = tempfile::tempdir().expect("worktree temp dir");
        let wt_path = wt_root.path().join("malformed-locked-wt");

        let repo = create_repo_with_commit(main_dir.path());
        commit_file(&repo, "important.txt", "keep malformed lock", "add important file");
        let wt_name = "malformed-locked-wt";
        let commit = repo.head().expect("head").peel_to_commit().expect("commit");
        let branch = repo.branch(wt_name, &commit, false).expect("branch");
        let reference = repo
            .find_reference(&format!("refs/heads/{}", wt_name))
            .expect("reference");
        let mut opts = git2::WorktreeAddOptions::new();
        opts.reference(Some(&reference));
        repo.worktree(wt_name, &wt_path, Some(&opts)).expect("create worktree");
        let wt_gitdir = repo.commondir().join("worktrees").join(wt_name);
        std::fs::write(wt_gitdir.join("locked"), [0xff]).expect("write malformed lock");
        drop(reference);
        drop(branch);
        drop(commit);
        drop(repo);

        let git = open_git_repo(main_dir.path());
        let result = git.remove_worktree(&wt_path, false);

        assert!(result.is_err(), "Malformed lock metadata must reject normal removal");
        assert!(wt_path.exists(), "Malformed-locked worktree directory must be preserved");
        assert!(wt_gitdir.exists(), "Malformed-locked worktree metadata must be preserved");
    }

    #[test]
    fn test_remove_worktree_force_false_deletes_directory() {
        let main_dir = tempfile::tempdir().expect("temp dir");
        let wt_root = tempfile::tempdir().expect("temp dir");
        let wt_path = wt_root.path().join("test-wt-normal-del");

        let repo = create_repo_with_commit(main_dir.path());

        let wt_name = "test-wt-normal-del";
        let _branch = repo.branch(wt_name, &repo.head().unwrap().peel_to_commit().unwrap(), false).unwrap();
        let reference = repo.find_reference(&format!("refs/heads/{}", wt_name)).ok();
        let mut opts = git2::WorktreeAddOptions::new();
        if let Some(ref r) = reference {
            opts.reference(Some(r));
        }
        repo.worktree(wt_name, &wt_path, Some(&opts)).expect("create worktree");

        // Verify directory exists
        assert!(wt_path.exists(), "Worktree directory should exist before remove");

        let git = open_git_repo(main_dir.path());
        let result = git.remove_worktree(&wt_path, false);
        assert!(result.is_ok(), "Normal remove should succeed: {:?}", result);

        // Directory should be gone
        assert!(!wt_path.exists(), "Worktree dir should be deleted after remove");
    }

    #[test]
    fn test_normal_remove_dirty_worktree_preserves_directory() {
        let main_dir = tempfile::tempdir().expect("temp dir");
        let wt_root = tempfile::tempdir().expect("temp dir");
        let wt_path = wt_root.path().join("test-wt-dirty");

        let repo = create_repo_with_commit(main_dir.path());

        let wt_name = "test-wt-dirty";
        let _branch = repo.branch(wt_name, &repo.head().unwrap().peel_to_commit().unwrap(), false).unwrap();
        let reference = repo.find_reference(&format!("refs/heads/{}", wt_name)).ok();
        let mut opts = git2::WorktreeAddOptions::new();
        if let Some(ref r) = reference {
            opts.reference(Some(r));
        }
        repo.worktree(wt_name, &wt_path, Some(&opts)).expect("create worktree");

        let uncommitted_file = wt_path.join("important.txt");
        std::fs::write(&uncommitted_file, "keep this change").expect("write uncommitted file");
        let wt_gitdir = repo.path().join("worktrees").join(wt_name);

        let git = open_git_repo(main_dir.path());
        let result = git.remove_worktree(&wt_path, false);
        assert!(result.is_err(), "Normal remove must refuse a dirty worktree");
        assert!(wt_path.exists(), "Dirty worktree directory must be preserved");
        assert_eq!(
            std::fs::read_to_string(&uncommitted_file).expect("read preserved file"),
            "keep this change"
        );
        assert!(wt_gitdir.exists(), "Git worktree metadata must be preserved");
    }

    #[test]
    fn test_normal_remove_ignored_worktree_file_preserves_directory() {
        let main_dir = tempfile::tempdir().expect("temp dir");
        let wt_root = tempfile::tempdir().expect("temp dir");
        let wt_path = wt_root.path().join("test-wt-ignored");

        let repo = create_repo_with_commit(main_dir.path());
        let exclude_path = main_dir.path().join(".git").join("info").join("exclude");
        std::fs::write(&exclude_path, "target/\n").expect("ignore target directory");

        let wt_name = "test-wt-ignored";
        let _branch = repo
            .branch(
                wt_name,
                &repo.head().unwrap().peel_to_commit().unwrap(),
                false,
            )
            .unwrap();
        let reference = repo.find_reference(&format!("refs/heads/{}", wt_name)).ok();
        let mut opts = git2::WorktreeAddOptions::new();
        if let Some(ref r) = reference {
            opts.reference(Some(r));
        }
        repo.worktree(wt_name, &wt_path, Some(&opts)).expect("create worktree");

        let ignored_file = wt_path.join("target").join("important.txt");
        std::fs::create_dir_all(ignored_file.parent().unwrap()).expect("create ignored directory");
        std::fs::write(&ignored_file, "keep this ignored change").expect("write ignored file");
        let wt_gitdir = repo.path().join("worktrees").join(wt_name);

        let git = open_git_repo(main_dir.path());
        let result = git.remove_worktree(&wt_path, false);
        assert!(result.is_err(), "Normal remove must refuse an ignored dirty file");
        assert!(wt_path.exists(), "Worktree directory with ignored files must be preserved");
        assert_eq!(
            std::fs::read_to_string(&ignored_file).expect("read preserved ignored file"),
            "keep this ignored change"
        );
        assert!(wt_gitdir.exists(), "Git worktree metadata must be preserved");
    }

    #[test]
    fn test_remove_worktree_force_true_deletes_directory() {
        let main_dir = tempfile::tempdir().expect("temp dir");
        let wt_root = tempfile::tempdir().expect("temp dir");
        let wt_path = wt_root.path().join("test-wt-force-del");

        let repo = create_repo_with_commit(main_dir.path());

        let wt_name = "test-wt-force-del";
        let _branch = repo.branch(wt_name, &repo.head().unwrap().peel_to_commit().unwrap(), false).unwrap();
        let reference = repo.find_reference(&format!("refs/heads/{}", wt_name)).ok();
        let mut opts = git2::WorktreeAddOptions::new();
        if let Some(ref r) = reference {
            opts.reference(Some(r));
        }
        repo.worktree(wt_name, &wt_path, Some(&opts)).expect("create worktree");

        assert!(wt_path.exists(), "Worktree directory should exist before remove");

        let git = open_git_repo(main_dir.path());
        let result = git.remove_worktree(&wt_path, true);
        assert!(result.is_ok(), "Force remove should succeed: {:?}", result);

        // Directory should be gone
        assert!(!wt_path.exists(), "Worktree dir should be deleted after force remove");
    }

    #[test]
    fn test_safe_str_lossy_prefers_text_over_bytes() {
        let text = "preferred";
        let bytes: &[u8] = b"not_used";
        let result = safe_str_lossy(Some(text), Some(bytes));
        assert_eq!(result, "preferred", "Should prefer &str over bytes when both available");
    }

    #[test]
    fn test_safe_str_lossy_unicode_text() {
        let result = safe_str_lossy(Some("↑1 ↓0 — 分支 历史"), Some("↑1 ↓0 — 分支 历史".as_bytes()));
        assert_eq!(result, "↑1 ↓0 — 分支 历史", "Unicode characters should be preserved");
    }

    #[test]
    fn test_safe_str_lossy_infallible_valid() {
        let text = "Hello";
        let result = safe_str_lossy_infallible(Some(text), text.as_bytes());
        assert_eq!(result, "Hello", "Infallible wrapper should work with valid text");
    }

    #[test]
    fn test_safe_str_lossy_infallible_fallback() {
        let bytes: &[u8] = &[0x57, 0x6f, 0x72, 0x6c, 0x64];
        let result = safe_str_lossy_infallible(None, bytes);
        assert_eq!(result, "World", "Infallible wrapper should fall back to bytes");
    }
}
