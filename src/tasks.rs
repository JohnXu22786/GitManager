use chrono::{SecondsFormat, Utc};
use crate::task_verification::{VerificationCommand, VerificationResult, VerificationState};
use git2::Repository;
use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tempfile::NamedTempFile;

static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(0);
static REGISTRY_WRITE_LOCK: Mutex<()> = Mutex::new(());
const REGISTRY_VERSION: u32 = 1;
const LOCK_WAIT_LIMIT: Duration = Duration::from_secs(10);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TaskRecord {
    pub id: String,
    pub title: String,
    pub repository_path: String,
    pub worktree_path: String,
    pub branch: Option<String>,
    pub base_commit: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub provider_ref: Option<String>,
    #[serde(default)]
    pub session_ref: Option<String>,
    #[serde(default)]
    pub validation_ref: Option<String>,
    #[serde(default)]
    pub verification_command: Option<VerificationCommand>,
    #[serde(default)]
    pub verification_result: Option<VerificationResult>,
    #[serde(default)]
    pub reviewed_source_fingerprint: Option<String>,
    #[serde(default)]
    pub reviewed_at: Option<String>,
    #[serde(default)]
    pub pull_request_url: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<TaskDependency>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TaskDependency {
    pub task_id: String,
    pub repository_path: String,
    pub worktree_path: String,
}

pub enum DependencyReference<'a> {
    Resolved(&'a TaskRecord),
    Missing,
    RepositoryChanged,
}

pub fn resolve_dependency<'a>(
    owner: &TaskRecord,
    dependency: &TaskDependency,
    entries: &'a [TaskRecord],
) -> DependencyReference<'a> {
    let Some(task) = entries.iter().find(|task| task.id == dependency.task_id) else {
        return DependencyReference::Missing;
    };
    if owner.repository_path != dependency.repository_path
        || task.repository_path != dependency.repository_path
        || task.worktree_path != dependency.worktree_path
    {
        return DependencyReference::RepositoryChanged;
    }
    DependencyReference::Resolved(task)
}

impl TaskRecord {
    pub fn from_worktree(title: &str, selected_path: &Path) -> Result<Self, String> {
        let title = title.trim();
        if title.is_empty() {
            return Err("Enter a task title or goal".into());
        }

        let repository = Repository::open(selected_path).map_err(|error| {
            format!(
                "Could not open a Git worktree at {}: {error}",
                selected_path.display()
            )
        })?;
        let worktree = repository
            .workdir()
            .ok_or_else(|| "The selected Git repository has no worktree".to_string())?;
        let worktree = std::fs::canonicalize(worktree)
            .map_err(|error| format!("Could not resolve the worktree path: {error}"))?;
        let repository_path = repository_root(&repository, &worktree)?;
        let head = repository.head().ok();
        let branch = head
            .as_ref()
            .filter(|reference| reference.is_branch())
            .and_then(|reference| reference.shorthand().map(str::to_owned));
        let base_commit = head
            .and_then(|reference| reference.target())
            .map(|oid| oid.to_string());
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);

        Ok(Self {
            id: next_id(),
            title: title.to_string(),
            repository_path: repository_path.to_string_lossy().into_owned(),
            worktree_path: worktree.to_string_lossy().into_owned(),
            branch,
            base_commit,
            created_at: now.clone(),
            updated_at: now,
            provider_ref: None,
            session_ref: None,
            validation_ref: None,
            verification_command: None,
            verification_result: None,
            reviewed_source_fingerprint: None,
            reviewed_at: None,
            pull_request_url: None,
            dependencies: Vec::new(),
        })
    }
}

#[derive(Serialize, Deserialize)]
struct RegistryFile {
    version: u32,
    tasks: Vec<TaskRecord>,
}

pub struct TaskRegistry {
    entries: Vec<TaskRecord>,
    file_path: PathBuf,
    load_error: Option<String>,
}

impl TaskRegistry {
    pub fn load() -> Self {
        Self::load_from(config_path())
    }

    fn load_from(file_path: PathBuf) -> Self {
        let (entries, load_error) = match load_registry(&file_path) {
            Ok(entries) => (entries, None),
            Err(error) => (Vec::new(), Some(error.to_string())),
        };
        Self {
            entries,
            file_path,
            load_error,
        }
    }

    pub fn entries(&self) -> &[TaskRecord] {
        &self.entries
    }

    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    pub fn add(&mut self, entry: TaskRecord) -> io::Result<()> {
        self.update_registry(|entries| {
            if entries.iter().any(|stored| stored.id == entry.id) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("task ID {} already exists", entry.id),
                ));
            }
            entries.insert(0, entry);
            Ok(true)
        })
    }

    pub fn unlink(&mut self, id: &str) -> io::Result<()> {
        let _run_lock = self.acquire_verification_run_lock(id)?;
        self.update_registry(|entries| {
            let old_len = entries.len();
            entries.retain(|entry| entry.id != id);
            Ok(entries.len() != old_len)
        })
    }

    pub fn add_dependency(&mut self, task_id: &str, dependency_id: &str) -> io::Result<()> {
        self.update_registry(|entries| {
            let task = entries
                .iter()
                .find(|entry| entry.id == task_id)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "task no longer exists"))?;
            let dependency = entries
                .iter()
                .find(|entry| entry.id == dependency_id)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "dependency task no longer exists"))?;
            let dependency_task_id = dependency.id.clone();
            let dependency_repository_path = dependency.repository_path.clone();
            let dependency_worktree_path = dependency.worktree_path.clone();
            if task_id == dependency_id {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "a task cannot depend on itself",
                ));
            }
            if task.repository_path != dependency.repository_path {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "dependency tasks must belong to the same repository",
                ));
            }
            if task.dependencies.iter().any(|item| item.task_id == dependency_id) {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "this dependency is already linked",
                ));
            }
            if has_dependency_path(entries, dependency_id, task_id, &mut HashSet::new()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "this dependency would create a cycle",
                ));
            }

            let task = entries
                .iter_mut()
                .find(|entry| entry.id == task_id)
                .expect("task was checked above");
            task.dependencies.push(TaskDependency {
                task_id: dependency_task_id,
                repository_path: dependency_repository_path,
                worktree_path: dependency_worktree_path,
            });
            task.updated_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
            Ok(true)
        })
    }

    pub fn remove_dependency(&mut self, task_id: &str, dependency_id: &str) -> io::Result<()> {
        self.update_registry(|entries| {
            let task = entries
                .iter_mut()
                .find(|entry| entry.id == task_id)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "task no longer exists"))?;
            let old_len = task.dependencies.len();
            task.dependencies
                .retain(|dependency| dependency.task_id != dependency_id);
            if task.dependencies.len() != old_len {
                task.updated_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
                Ok(true)
            } else {
                Ok(false)
            }
        })
    }

    pub fn check_provider_start(&mut self, task: &TaskRecord) -> io::Result<()> {
        self.update_registry(|entries| {
            let entry = entries.iter().find(|entry| entry.id == task.id).ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "task no longer exists")
            })?;
            if entry.provider_ref.as_deref() != task.provider_ref.as_deref()
                || entry.session_ref.as_deref() != task.session_ref.as_deref()
            {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "task provider changed before the launch could start",
                ));
            }
            Ok(false)
        })
    }

    pub fn record_provider_start(
        &mut self,
        id: &str,
        expected_provider_ref: Option<&str>,
        expected_session_ref: Option<&str>,
        provider_ref: &str,
        session_ref: Option<&str>,
    ) -> io::Result<()> {
        self.update_registry(|entries| {
            let entry = entries
                .iter_mut()
                .find(|entry| entry.id == id)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "task no longer exists"))?;
            if entry.provider_ref.as_deref() != expected_provider_ref
                || entry.session_ref.as_deref() != expected_session_ref
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "task provider changed before the launch could be recorded",
                ));
            }
            if entry.provider_ref.as_deref() == Some(provider_ref)
                && entry.session_ref.as_deref() == session_ref
            {
                return Ok(false);
            }
            entry.provider_ref = Some(provider_ref.to_string());
            entry.session_ref = session_ref.map(|session_ref| session_ref.to_string());
            entry.updated_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
            Ok(true)
        })
    }

    pub fn save_verification_command(
        &mut self,
        id: &str,
        command: Option<VerificationCommand>,
    ) -> io::Result<()> {
        self.update_registry(|entries| {
            let entry = entries
                .iter_mut()
                .find(|entry| entry.id == id)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "task no longer exists"))?;
            if entry.verification_command == command {
                return Ok(false);
            }
            entry.verification_command = command;
            entry.updated_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
            Ok(true)
        })
    }

    pub fn mark_reviewed(&mut self, id: &str, source_fingerprint: &str) -> io::Result<()> {
        self.update_registry(|entries| {
            let entry = entries
                .iter_mut()
                .find(|entry| entry.id == id)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "task no longer exists"))?;
            let reviewed_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
            if entry.reviewed_source_fingerprint.as_deref() == Some(source_fingerprint)
                && entry.reviewed_at.is_some()
            {
                return Ok(false);
            }
            entry.reviewed_source_fingerprint = Some(source_fingerprint.to_string());
            entry.reviewed_at = Some(reviewed_at);
            entry.updated_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
            Ok(true)
        })
    }

    pub fn mark_review_pending(&mut self, id: &str) -> io::Result<()> {
        self.update_registry(|entries| {
            let entry = entries
                .iter_mut()
                .find(|entry| entry.id == id)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "task no longer exists"))?;
            if entry.reviewed_source_fingerprint.is_none() && entry.reviewed_at.is_none() {
                return Ok(false);
            }
            entry.reviewed_source_fingerprint = None;
            entry.reviewed_at = None;
            entry.updated_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
            Ok(true)
        })
    }

    pub fn record_pull_request(&mut self, id: &str, url: &str) -> io::Result<()> {
        self.update_registry(|entries| {
            let entry = entries
                .iter_mut()
                .find(|entry| entry.id == id)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "task no longer exists"))?;
            if entry.pull_request_url.as_deref() == Some(url) {
                return Ok(false);
            }
            entry.pull_request_url = Some(url.to_string());
            entry.updated_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
            Ok(true)
        })
    }

    pub fn start_verification(
        &mut self,
        id: &str,
        command: &VerificationCommand,
        result: VerificationResult,
    ) -> io::Result<TaskVerificationRunLock> {
        let run_lock = self.acquire_verification_run_lock(id)?;
        self.update_registry(|entries| {
            let entry = entries
                .iter_mut()
                .find(|entry| entry.id == id)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "task no longer exists"))?;
            if entry.verification_command.as_ref() != Some(command) {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "task verification command changed before the run could start",
                ));
            }
            if result.state != VerificationState::Running {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "a task verification must start in the running state",
                ));
            }
            entry.verification_result = Some(result);
            entry.updated_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
            Ok(true)
        })?;
        Ok(run_lock)
    }

    fn acquire_verification_run_lock(&self, id: &str) -> io::Result<TaskVerificationRunLock> {
        let parent = self
            .file_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        let key = digest(&SHA256, id.as_bytes());
        let suffix = key
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let lock_path = self
            .file_path
            .with_extension(format!("verification-{suffix}.lock"));
        TaskVerificationRunLock::try_acquire(&lock_path)
    }

    pub fn record_verification_fingerprint(
        &mut self,
        id: &str,
        run_id: &str,
        fingerprint: &str,
    ) -> io::Result<()> {
        self.update_registry(|entries| {
            let entry = entries
                .iter_mut()
                .find(|entry| entry.id == id)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "task no longer exists"))?;
            let result = entry.verification_result.as_mut().filter(|result| {
                result.run_id == run_id && result.state == VerificationState::Running
            }).ok_or_else(|| {
                io::Error::new(io::ErrorKind::WouldBlock, "task verification run is no longer current")
            })?;
            if result.source_fingerprint.as_deref() == Some(fingerprint) {
                return Ok(false);
            }
            result.source_fingerprint = Some(fingerprint.to_string());
            Ok(true)
        })
    }

    pub fn record_verification_result(
        &mut self,
        id: &str,
        run_id: &str,
        result: VerificationResult,
    ) -> io::Result<()> {
        self.update_registry(|entries| {
            let entry = entries
                .iter_mut()
                .find(|entry| entry.id == id)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "task no longer exists"))?;
            let current = entry.verification_result.as_ref().filter(|current| {
                current.run_id == run_id && current.state == VerificationState::Running
            }).ok_or_else(|| {
                io::Error::new(io::ErrorKind::WouldBlock, "task verification run is no longer current")
            })?;
            let mut result = result;
            if result.source_fingerprint.is_none() {
                result.source_fingerprint = current.source_fingerprint.clone();
            }
            entry.verification_result = Some(result);
            entry.updated_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
            Ok(true)
        })
    }

    pub fn reload(&mut self) {
        match load_registry(&self.file_path) {
            Ok(entries) => {
                self.entries = entries;
                self.load_error = None;
            }
            Err(error) => {
                self.entries.clear();
                self.load_error = Some(error.to_string());
            }
        }
    }

    fn update_registry(
        &mut self,
        change: impl FnOnce(&mut Vec<TaskRecord>) -> io::Result<bool>,
    ) -> io::Result<()> {
        if let Some(error) = &self.load_error {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("cannot update the task registry because it could not be loaded: {error}"),
            ));
        }

        let parent = self
            .file_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        let _process_lock = REGISTRY_WRITE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _file_lock = RegistryFileLock::acquire(&self.file_path)?;
        let mut entries = load_registry(&self.file_path)?;
        let changed = change(&mut entries)?;
        if changed {
            save_registry(&self.file_path, &entries)?;
        }
        self.entries = entries;
        Ok(())
    }
}

fn has_dependency_path(
    entries: &[TaskRecord],
    from_id: &str,
    target_id: &str,
    visited: &mut HashSet<String>,
) -> bool {
    if from_id == target_id {
        return true;
    }
    if !visited.insert(from_id.to_string()) {
        return false;
    }
    let Some(task) = entries.iter().find(|entry| entry.id == from_id) else {
        return false;
    };
    task.dependencies.iter().any(|dependency| {
        match resolve_dependency(task, dependency, entries) {
            DependencyReference::Resolved(next) => {
                has_dependency_path(entries, &next.id, target_id, visited)
            }
            DependencyReference::Missing | DependencyReference::RepositoryChanged => false,
        }
    })
}

struct RegistryFileLock {
    _file: fs::File,
    #[cfg(windows)]
    _overlapped: windows_sys::Win32::System::IO::OVERLAPPED,
}

pub struct TaskVerificationRunLock {
    _file: fs::File,
    #[cfg(windows)]
    _overlapped: windows_sys::Win32::System::IO::OVERLAPPED,
}

impl TaskVerificationRunLock {
    fn try_acquire(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;
        #[cfg(windows)]
        let mut overlapped = unsafe { std::mem::zeroed() };

        #[cfg(windows)]
        let locked = try_lock_file(&file, &mut overlapped)?;
        #[cfg(not(windows))]
        let locked = try_lock_file(&file)?;
        if !locked {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "task verification is already running in another Git Manager instance",
            ));
        }

        Ok(Self {
            _file: file,
            #[cfg(windows)]
            _overlapped: overlapped,
        })
    }
}

impl RegistryFileLock {
    fn acquire(registry_path: &Path) -> io::Result<Self> {
        let mut lock_name = registry_path.as_os_str().to_os_string();
        lock_name.push(".lock");
        let path = PathBuf::from(lock_name);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;
        let deadline = Instant::now() + LOCK_WAIT_LIMIT;
        #[cfg(windows)]
        let mut overlapped = unsafe { std::mem::zeroed() };

        loop {
            #[cfg(windows)]
            let locked = try_lock_file(&file, &mut overlapped)?;
            #[cfg(not(windows))]
            let locked = try_lock_file(&file)?;
            if locked {
                return Ok(Self {
                    _file: file,
                    #[cfg(windows)]
                    _overlapped: overlapped,
                });
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "timed out waiting for another Git Manager instance to update tasks",
                ));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

#[cfg(unix)]
fn try_lock_file(file: &fs::File) -> io::Result<bool> {
    use std::os::fd::AsRawFd;

    const LOCK_EXCLUSIVE: std::os::raw::c_int = 2;
    const LOCK_NONBLOCKING: std::os::raw::c_int = 4;
    extern "C" {
        fn flock(fd: std::os::raw::c_int, operation: std::os::raw::c_int) -> std::os::raw::c_int;
    }

    let result = unsafe { flock(file.as_raw_fd(), LOCK_EXCLUSIVE | LOCK_NONBLOCKING) };
    if result == 0 {
        Ok(true)
    } else {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::WouldBlock {
            Ok(false)
        } else {
            Err(error)
        }
    }
}

#[cfg(windows)]
fn try_lock_file(
    file: &fs::File,
    overlapped: &mut windows_sys::Win32::System::IO::OVERLAPPED,
) -> io::Result<bool> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_LOCK_VIOLATION};
    use windows_sys::Win32::Storage::FileSystem::{
        LockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
    };

    let locked = unsafe {
        LockFileEx(
            file.as_raw_handle() as _,
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            1,
            0,
            overlapped,
        )
    };
    if locked != 0 {
        Ok(true)
    } else {
        let error = unsafe { GetLastError() };
        if error == ERROR_LOCK_VIOLATION {
            Ok(false)
        } else {
            Err(io::Error::from_raw_os_error(error as i32))
        }
    }
}

#[cfg(not(any(unix, windows)))]
fn try_lock_file(_file: &fs::File) -> io::Result<bool> {
    Ok(true)
}

fn next_id() -> String {
    let sequence = NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed);
    let nanos = Utc::now().timestamp_nanos_opt().unwrap_or_default();
    format!("task-{nanos:x}-{:x}-{sequence:x}", std::process::id())
}

pub(crate) fn repository_root(repository: &Repository, worktree: &Path) -> Result<PathBuf, String> {
    let git_dir = repository.path();
    let common_dir_file = git_dir.join("commondir");
    let common_dir = match std::fs::read_to_string(common_dir_file) {
        Ok(relative) => {
            let relative = PathBuf::from(relative.trim());
            if relative.is_absolute() {
                relative
            } else {
                git_dir.join(relative)
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(worktree.to_path_buf()),
        Err(error) => return Err(format!("Could not read Git common directory: {error}")),
    };
    let common_dir = std::fs::canonicalize(&common_dir)
        .map_err(|error| format!("Could not resolve the Git common directory: {error}"))?;

    let common_repository = Repository::open(&common_dir)
        .map_err(|error| format!("Could not inspect the shared Git repository: {error}"))?;
    if let Some(worktree) = common_repository.workdir() {
        return std::fs::canonicalize(worktree)
            .map_err(|error| format!("Could not resolve the shared repository worktree: {error}"));
    }
    if common_repository.is_bare() {
        return Ok(common_dir);
    }

    // For a standard repository the common Git directory is `<repo>/.git`;
    // linked worktrees point their `commondir` file back to that same path.
    common_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "Could not determine the repository path".to_string())
}

fn load_registry(path: &Path) -> io::Result<Vec<TaskRecord>> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let registry: RegistryFile = serde_json::from_str(&content)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if registry.version != REGISTRY_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported task registry version {}", registry.version),
        ));
    }
    let mut ids = HashSet::new();
    for entry in &registry.tasks {
        if entry.id.trim().is_empty()
            || entry.title.trim().is_empty()
            || entry.repository_path.trim().is_empty()
            || entry.worktree_path.trim().is_empty()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "task registry contains a record with a missing ID, title, or path",
            ));
        }
        if !ids.insert(&entry.id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("task registry contains duplicate ID {}", entry.id),
            ));
        }
    }
    Ok(registry.tasks)
}

fn save_registry(path: &Path, entries: &[TaskRecord]) -> io::Result<()> {
    let content = serde_json::to_vec_pretty(&RegistryFile {
        version: REGISTRY_VERSION,
        tasks: entries.to_vec(),
    })
    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;

    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(&content)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn config_path() -> PathBuf {
    #[cfg(windows)]
    {
        let config_dir = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                std::env::var_os("USERPROFILE")
                    .map(PathBuf::from)
                    .map(|path| path.join("AppData").join("Roaming"))
                    .filter(|path| path.is_absolute())
            })
            .unwrap_or_else(std::env::temp_dir);
        config_dir.join("GitManager").join("tasks.json")
    }

    #[cfg(not(windows))]
    {
        let config_dir = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|path| path.join(".config"))
                    .filter(|path| path.is_absolute())
            })
            .unwrap_or_else(std::env::temp_dir);
        config_dir.join("GitManager").join("tasks.json")
    }
}
