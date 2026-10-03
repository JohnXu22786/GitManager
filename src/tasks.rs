use chrono::{SecondsFormat, Utc};
use git2::Repository;
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
        self.update_registry(|entries| {
            let old_len = entries.len();
            entries.retain(|entry| entry.id != id);
            Ok(entries.len() != old_len)
        })
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

struct RegistryFileLock {
    _file: fs::File,
    #[cfg(windows)]
    _overlapped: windows_sys::Win32::System::IO::OVERLAPPED,
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

fn repository_root(repository: &Repository, worktree: &Path) -> Result<PathBuf, String> {
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
