use chrono::DateTime;
use git2::{BranchType, DiffOptions, Repository, Status, WorktreeAddOptions, WorktreePruneOptions};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub type GitResult<T> = Result<T, String>;

/// Describes a Git operation to be executed in a background thread.
pub enum GitOperation {
    StageAll,
    UnstageAll,
    RestoreAll,
    StageFile(String),
    UnstageFile(String),
    RestoreFile(String),
    Commit { message: String, amend: bool },
    Uncommit,
    CreateBranch { name: String, base: Option<String> },
    DeleteBranch { name: String, force: bool },
    RenameBranch { old: String, new: String },
    CheckoutBranch(String),
    MergeBranch(String),
    RemoveWorktree { path: PathBuf, force: bool },
    CreateWorktree { name: String, path: PathBuf, branch: Option<String>, new_branch: bool },
    StashAll(Option<String>),
    StashPop,
    StashApply(usize),
    StashDrop(usize),
    Push { remote: String, branch: String, force: bool },
    Pull { remote: String, branch: String, rebase: bool },
    Fetch(String),
    GetDiff { path: String, staged: bool },
    /// Search commits in the log.
    LogSearch(String),
    /// Refresh all cached data from the repository.
    RefreshAll,
}

/// Result of a Git operation executed in a background thread.
#[derive(Debug, Clone)]
pub enum OpResult {
    /// Operation succeeded with a message.
    Success(String),
    /// Operation failed with an error message.
    Error(String),
    /// Diff content for a file.
    DiffContent {
        path: String,
        lines: Vec<DiffLine>,
    },
    /// Search results for commit log.
    SearchResults(Vec<CommitInfo>),
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
            GitOperation::StageFile(p) => Self::simple(repo.stage_file(&p), format!("Staged {}", p)),
            GitOperation::UnstageFile(p) => Self::simple(repo.unstage_file(&p), format!("Unstaged {}", p)),
            GitOperation::RestoreFile(p) => Self::simple(repo.restore_file(&p), format!("Restored {}", p)),
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
            GitOperation::RemoveWorktree { path, force } => {
                Self::simple(repo.remove_worktree(&path, force), {
                    if force { format!("Force removed worktree at {:?}", path) }
                    else { format!("Removed worktree at {:?}", path) }
                })
            }
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
                Ok(lines) => OpResult::DiffContent { path, lines },
                Err(e) => OpResult::Error(format!("Diff error: {}", e)),
            },
            GitOperation::LogSearch(filter) => {
                let commits = repo.log(100).unwrap_or_default();
                let filtered: Vec<CommitInfo> = if filter.is_empty() {
                    commits
                } else {
                    let f = filter.to_lowercase();
                    commits.into_iter().filter(|c| {
                        c.message.to_lowercase().contains(&f)
                            || c.author.to_lowercase().contains(&f)
                            || c.short_sha.contains(&f)
                    }).collect()
                };
                OpResult::SearchResults(filtered)
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
}

#[derive(Clone, Debug)]
pub struct StatusEntry {
    pub path: String,
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
    if !path.exists() {
        return Ok(());
    }

    let mut last_err = None;

    for _ in 0..2 {
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
            if path.exists() {
                let _ = std::process::Command::new("cmd.exe")
                    .args(["/c", "rmdir", "/s", "/q", &path.to_string_lossy()])
                    .output();
            }
        }

        // Check if it's gone after the fallback
        if !path.exists() {
            return Ok(());
        }
        // No sleep — the delete commands are already synchronous and may take
        // significant time for large worktrees.
    }

    // Return the last error if path still exists
    Err(last_err.unwrap_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to remove {:?}", path))
    }))
}

/// Check if a path exists and contains any files (not just the directory entry itself).
/// Returns true only if the directory actually has content.
fn dir_has_content(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    // read_dir succeeds and has at least one entry → has content
    if let Ok(mut entries) = std::fs::read_dir(path) {
        entries.next().is_some()
    } else {
        // Can't even read the directory, treat as "has content" to be safe
        true
    }
}

impl GitRepo {
    pub fn new() -> Self {
        GitRepo { repo: RefCell::new(None), path: None }
    }

    pub fn open(&mut self, path: &Path) -> GitResult<()> {
        let r = Repository::open(path).map_err(|e| format!("Open repo: {}", e))?;
        let p = r.path().parent().unwrap().to_path_buf();
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
        let obj = repo.revparse_single(name).map_err(|e| format!("Find '{}': {}", name, e))?;
        repo.checkout_tree(&obj, None)
            .map_err(|e| format!("Checkout: {}", e))?;
        let rf = if name.starts_with("refs/") { name.to_string() } else { format!("refs/heads/{}", name) };
        repo.set_head(&rf).map_err(|e| format!("Set HEAD: {}", e))?;
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
        let their = repo.revparse_single(branch_name)
            .map_err(|e| format!("Find '{}': {}", branch_name, e))?
            .peel_to_commit().map_err(|_| "Not a commit".to_string())?;
        let head = repo.head().map_err(|e| format!("HEAD: {}", e))?
            .peel_to_commit().map_err(|_| "No commit".to_string())?;

        let base = repo.merge_base(head.id(), their.id())
            .ok()
            .and_then(|oid| repo.find_commit(oid).ok())
            .and_then(|c| c.tree().ok());

        let ours = head.tree().map_err(|e| format!("Tree: {}", e))?;
        let theirs = their.tree().map_err(|e| format!("Tree: {}", e))?;

        let mut idx = if let Some(ancestor) = base.as_ref() {
            repo.merge_trees(ancestor, &ours, &theirs, None::<&git2::MergeOptions>)
                .map_err(|e| format!("Merge: {}", e))?
        } else {
            repo.merge_trees(&ours, &ours, &theirs, None::<&git2::MergeOptions>)
                .map_err(|e| format!("Merge: {}", e))?
        };

        if idx.has_conflicts() { return Err("Merge conflicts".into()); }

        let sig = repo.signature().map_err(|e| format!("Sig: {}", e))?;
        let toid = idx.write_tree_to(&*repo).map_err(|e| format!("Write tree: {}", e))?;
        let t = repo.find_tree(toid).map_err(|e| format!("Find tree: {}", e))?;

        let msg = format!("Merge branch '{}'", branch_name);
        repo.commit(Some("HEAD"), &sig, &sig, &msg, &t, &[&head, &their])
            .map_err(|e| format!("Commit: {}", e))?;

        let mut co = git2::build::CheckoutBuilder::new();
        co.force();
        repo.checkout_tree(t.as_object(), Some(&mut co))
            .map_err(|e| format!("Checkout after merge: {}", e))?;

        Ok(msg)
    }

    pub fn worktrees(&self) -> GitResult<Vec<WorktreeInfo>> {
        let repo = self.repo()?;
        let mp = repo.path().parent().unwrap().to_path_buf();
        let mut list = Vec::new();
        list.push(WorktreeInfo {
            path: mp, branch: Some(self.current_branch().unwrap_or_default()),
            sha: repo.head().ok().and_then(|h| h.target().map(|o| o.to_string())).unwrap_or_default(),
            is_main: true,
        });

        let names = repo.worktrees().map_err(|e| format!("Worktrees: {}", e))?;
        for name in names.iter().flatten() {
            if let Ok(wt) = repo.find_worktree(name) {
                let wp = wt.path().to_path_buf();
                if let Ok(r) = Repository::open(&wp) {
                    let branch = r.head().ok().and_then(|h| {
                        if h.is_branch() { h.shorthand().map(String::from) } else { None }
                    });
                    let sha = r.head().ok().and_then(|h| h.target().map(|o| o.to_string())).unwrap_or_default();
                    list.push(WorktreeInfo { path: wp, branch, sha, is_main: false });
                } else {
                    list.push(WorktreeInfo { path: wp, branch: None, sha: String::new(), is_main: false });
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
        let wt = repo.worktree(name, path, Some(&opts))
            .map_err(|e| format!("Create worktree: {}", e))?;

        if new_branch {
            if let Ok(wr) = Repository::open(wt.path()) { let _ = wr.set_head(&branch_ref); }
        }
        Ok(())
    }

    pub fn remove_worktree(&self, path: &Path, force: bool) -> GitResult<()> {
        let repo = self.repo()?;
        let wname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let mut errors = Vec::new();

        // Try to find worktree by name (fast path)
        let mut found_wt = repo.find_worktree(wname).ok();
        
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

        // Attempt pruning with manual fallback for metadata cleanup
        if let Some(ref wt) = found_wt {
            let prune_result = if force {
                let mut opts = WorktreePruneOptions::new();
                opts.valid(true);       // Prune even if the worktree is valid
                opts.locked(true);      // Prune even if locked
                opts.working_tree(true); // Recursively remove the working tree directory
                wt.prune(Some(&mut opts))
            } else {
                wt.prune(None)
            };

            if let Err(e) = prune_result {
                errors.push(if force { format!("Prune: {}", e) } else { format!("Remove: {}", e) });
                // Fallback: clean up git metadata manually since prune refused
                if let Some(name) = wt.name() {
                    let wt_gitdir = repo.path().join("worktrees").join(name);
                    if wt_gitdir.exists() {
                        if let Err(e) = std::fs::remove_dir_all(&wt_gitdir) {
                            errors.push(format!("Remove git metadata: {}", e));
                        }
                    }
                }
            }
        }

        // --- Directory cleanup: different strategy for Remove vs Force Remove ---
        if force {
            // Force Remove: aggressive — retries + OS-level fallback
            if path.exists() {
                if let Err(e) = force_remove_dir(path) {
                    errors.push(format!("Rm dir: {}", e));
                }
            }
        } else {
            // Regular Remove: gentle — one attempt, no force fallback
            if path.exists() {
                if let Err(e) = std::fs::remove_dir_all(path) {
                    errors.push(format!("Rm dir: {}", e));
                }
            }
            // If directory still has content after gentle attempt, tell the user
            if dir_has_content(path) {
                errors.push("Directory still contains files. Use Force Remove to delete it.".into());
            }
        }

        // If directory no longer has content, consider it a success
        // (primary user concern is getting rid of the files on disk)
        if !dir_has_content(path) {
            return Ok(());
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
            let p = e.path().unwrap_or("").to_string();
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

    pub fn stage_file(&self, path: &str) -> GitResult<()> {
        let repo = self.repo()?;
        let mut idx = repo.index().map_err(|e| format!("Index: {}", e))?;
        idx.add_path(Path::new(path)).map_err(|e| format!("Stage: {}", e))?;
        idx.write().map_err(|e| format!("Write: {}", e))?;
        Ok(())
    }

    pub fn unstage_file(&self, path: &str) -> GitResult<()> {
        let repo = self.repo()?;
        let mut idx = repo.index().map_err(|e| format!("Index: {}", e))?;
        idx.remove_path(Path::new(path)).map_err(|e| format!("Unstage: {}", e))?;
        idx.write().map_err(|e| format!("Write: {}", e))?;
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
        let mut idx = repo.index().map_err(|e| format!("Index: {}", e))?;
        idx.remove_all(["*"].iter(), None).map_err(|e| format!("Unstage all: {}", e))?;
        idx.write().map_err(|e| format!("Write: {}", e))?;
        Ok(())
    }

    pub fn restore_file(&self, path: &str) -> GitResult<()> {
        let repo = self.repo()?;
        let t = repo.head().map_err(|e| format!("HEAD: {}", e))?
            .peel_to_tree().map_err(|e| format!("Tree: {}", e))?;
        let mut cb = git2::build::CheckoutBuilder::new();
        cb.force().path(Path::new(path));
        repo.checkout_tree(t.as_object(), Some(&mut cb))
            .map_err(|e| format!("Restore: {}", e))?;
        Ok(())
    }

    pub fn restore_all(&self) -> GitResult<()> {
        let repo = self.repo()?;
        let hc = repo.head().map_err(|e| format!("HEAD: {}", e))?
            .peel_to_commit().map_err(|e| format!("Peel: {}", e))?;
        repo.checkout_tree(hc.as_object(), None).map_err(|e| format!("Checkout: {}", e))?;
        Ok(())
    }

    pub fn get_diff(&self, path: &str, staged: bool) -> GitResult<Vec<DiffLine>> {
        let repo = self.repo()?;
        let mut lines = Vec::new();
        let tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
        let mut dopts = DiffOptions::new();
        dopts.pathspec(path);

        let diff = if staged {
            let idx = repo.index().map_err(|e| format!("Index: {}", e))?;
            repo.diff_tree_to_index(tree.as_ref(), Some(&idx), Some(&mut dopts))
                .map_err(|e| format!("Diff: {}", e))?
        } else {
            repo.diff_tree_to_workdir(tree.as_ref(), Some(&mut dopts))
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
        let parent_id = hc.parent(0).ok().map(|p| p.id());
        drop(hc);
        drop(repo);

        if let Some(pid) = parent_id {
            let repo = self.repo()?;
            let parent = repo.find_commit(pid).map_err(|e| format!("Find parent: {}", e))?;
            let tree = parent.tree().map_err(|e| format!("Tree: {}", e))?;
            let mut cb = git2::build::CheckoutBuilder::new();
            cb.force();
            repo.checkout_tree(tree.as_object(), Some(&mut cb))
                .map_err(|e| format!("Checkout: {}", e))?;
            repo.set_head(pid.to_string().as_str())
                .map_err(|e| format!("Set HEAD: {}", e))?;
            Ok(pid.to_string())
        } else {
            Err("No parent commit".into())
        }
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
        let mut fo = git2::PushOptions::new();
        fo.remote_callbacks(cb);
        let rs = if force { format!("+refs/heads/{}:refs/heads/{}", branch, branch) }
                 else { format!("refs/heads/{}:refs/heads/{}", branch, branch) };
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
            let mut reb = repo.rebase(Some(&ac), None, None, Some(&mut ropts))
                .map_err(|e| format!("Rebase: {}", e))?;
            while let Some(op) = reb.next() {
                let _ = op.map_err(|e| format!("Op: {}", e))?;
                let sg = repo.signature().map_err(|e| format!("Sig: {}", e))?;
                reb.commit(None, &sg, None).map_err(|e| format!("Rebase commit: {}", e))?;
            }
            reb.finish(None).map_err(|e| format!("Finish: {}", e))?;
            Ok("Rebase complete".into())
        } else {
            let hc = repo.head().map_err(|e| format!("HEAD: {}", e))?
                .peel_to_commit().map_err(|e| format!("Peel: {}", e))?;

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
            repo.commit(Some("HEAD"), &sg, &sg,
                &format!("Merge '{}'", remote), &t, &[&hc, &fc])
                .map_err(|e| format!("Merge commit: {}", e))?;

            let mut co = git2::build::CheckoutBuilder::new();
            co.force();
            repo.checkout_tree(t.as_object(), Some(&mut co))
                .map_err(|e| format!("Checkout after merge: {}", e))?;

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
    fn test_normal_remove_valid_with_fallback_succeeds() {
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

        // Normal remove should succeed now with our improved implementation
        // (it falls back to manual cleanup when prune refuses)
        let git = open_git_repo(main_dir.path());
        let result = git.remove_worktree(&wt_path, false);
        assert!(result.is_ok(), "Normal remove should succeed with fallback: {:?}", result);
        assert!(!wt_path.exists(), "Worktree dir should be gone");
        assert!(!wt_gitdir.exists(), "Git worktree metadata should be cleaned up");
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

    #[test]
    fn test_force_remove_dir_nonexistent() {
        let path = std::path::Path::new("C:\\this_path_should_not_exist_xyz_12345");
        // Should succeed even if path doesn't exist
        let result = force_remove_dir(path);
        assert!(result.is_ok(), "Removing nonexistent path should be ok: {:?}", result);
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
