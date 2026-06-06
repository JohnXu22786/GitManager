use chrono::DateTime;
use git2::{BranchType, DiffOptions, Repository, Status, WorktreeAddOptions, WorktreePruneOptions};
use std::cell::RefCell;
use std::path::{Path, PathBuf};

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
pub fn execute_operation(path: &Path, op: GitOperation) -> OpResult {
    let mut repo = GitRepo::new();
    match repo.open(path) {
        Ok(()) => op.dispatch(&repo),
        Err(e) => OpResult::Error(format!("Failed to open repo: {}", e)),
    }
}

impl GitOperation {
    fn dispatch(self, repo: &GitRepo) -> OpResult {
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
            GitOperation::Push { remote, branch, force } => match repo.push(&remote, &branch, force) {
                Ok(msg) => OpResult::Success(msg),
                Err(e) => OpResult::Error(e),
            },
            GitOperation::Pull { remote, branch, rebase } => match repo.pull(&remote, &branch, rebase) {
                Ok(msg) => OpResult::Success(msg),
                Err(e) => OpResult::Error(e),
            },
            GitOperation::Fetch(remote) => match repo.fetch(&remote) {
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

    fn repo(&self) -> GitResult<std::cell::Ref<Repository>> {
        let r = self.repo.borrow();
        if r.is_some() {
            Ok(std::cell::Ref::map(r, |o| o.as_ref().unwrap()))
        } else {
            Err("No repo open".into())
        }
    }

    fn repo_mut(&self) -> GitResult<std::cell::RefMut<Repository>> {
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
            Ok(head.shorthand().unwrap_or("HEAD").to_string())
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
                let name = branch.name().ok().flatten().unwrap_or("").to_string();
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
        if let Ok(wt) = repo.find_worktree(wname) {
            if force {
                let mut opts = WorktreePruneOptions::new();
                opts.valid(true);
                wt.prune(Some(&mut opts)).map_err(|e| format!("Prune: {}", e))?;
            } else {
                wt.prune(None).map_err(|e| format!("Remove: {}", e))?;
            }
        }
        if path.exists() { std::fs::remove_dir_all(path).map_err(|e| format!("Rm dir: {}", e))?; }
        Ok(())
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
                    author: c.author().name().unwrap_or("").to_string(),
                    time: DateTime::from_timestamp(c.time().seconds(), 0)
                        .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_default(),
                    message: c.message().unwrap_or("").to_string(),
                    summary: c.summary().unwrap_or("").to_string(),
                });
            }
        }
        Ok(commits)
    }

    pub fn push(&self, remote: &str, branch: &str, force: bool) -> GitResult<String> {
        let mut repo = self.repo_mut()?;
        let mut cb = git2::RemoteCallbacks::new();
        let mut fo = git2::PushOptions::new();
        fo.remote_callbacks(cb);
        let rs = if force { format!("+refs/heads/{}:refs/heads/{}", branch, branch) }
                 else { format!("refs/heads/{}:refs/heads/{}", branch, branch) };
        let mut rm = repo.find_remote(remote).map_err(|e| format!("Remote: {}", e))?;
        rm.push(&[&rs], Some(&mut fo)).map_err(|e| format!("Push: {}", e))?;
        Ok(format!("Pushed {}", branch))
    }

    pub fn fetch(&self, remote: &str) -> GitResult<String> {
        let mut repo = self.repo_mut()?;
        let mut cb = git2::RemoteCallbacks::new();
        let mut fo = git2::FetchOptions::new();
        fo.remote_callbacks(cb);
        let mut rm = repo.find_remote(remote).map_err(|e| format!("Remote: {}", e))?;
        rm.fetch(&["+refs/heads/*:refs/remotes/origin/*"], Some(&mut fo), None)
            .map_err(|e| format!("Fetch: {}", e))?;
        Ok(format!("Fetched from {}", remote))
    }

    pub fn pull(&self, remote: &str, branch: &str, rebase: bool) -> GitResult<String> {
        let mut repo = self.repo_mut()?;
        let mut cb = git2::RemoteCallbacks::new();
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
