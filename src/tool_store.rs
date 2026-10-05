use crate::tool_project::{ProjectSnapshot, ProjectValidationError, PROJECT_FORMAT_VERSION};
use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
#[cfg(all(unix, any(target_os = "linux", target_os = "macos")))]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempfile::NamedTempFile;

const CURRENT_FILE: &str = "CURRENT";
const SNAPSHOTS_DIR: &str = "snapshots";
const LOCK_FILE: &str = ".write.lock";
const MAX_POINTER_BYTES: usize = 16 * 1024;
const MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;
const LOCK_WAIT_LIMIT: Duration = Duration::from_secs(5);

static PROCESS_WRITE_LOCK: Mutex<()> = Mutex::new(());
#[cfg(all(unix, any(target_os = "linux", target_os = "macos")))]
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadOnlyReason {
    UnsupportedFormat { found: u64, supported: u32 },
    UnsupportedRequiredFeature(String),
    MalformedPointer(String),
    MalformedSnapshot(String),
    ChecksumMismatch,
    UnsafePath,
    IncompleteProject,
    UnsupportedPlatform,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadOnlyProject {
    pub reason: ReadOnlyReason,
    pub raw_pointer_bytes: Option<Vec<u8>>,
    pub raw_snapshot_bytes: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreLoad {
    Writable(ProjectSnapshot),
    ReadOnly(ReadOnlyProject),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitDisposition {
    Committed,
    AlreadyApplied,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitResult {
    pub disposition: CommitDisposition,
    pub snapshot: ProjectSnapshot,
}

#[derive(Debug)]
pub enum StoreError {
    Io(io::Error),
    ReadOnly(ReadOnlyProject),
    GenerationConflict {
        expected: u64,
        actual: u64,
    },
    OperationIdentityConflict {
        operation_id: String,
    },
    InvalidSnapshot(ProjectValidationError),
    InvalidArgument(String),
    AlreadyExists,
    UnsafePath(String),
    LockBusy,
    UnsupportedPlatform,
    ObjectCollision(String),
    #[cfg(test)]
    InjectedInterruption(StoreFaultPoint),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "project store I/O failed: {error}"),
            Self::ReadOnly(project) => {
                write!(f, "project store is read-only: {:?}", project.reason)
            }
            Self::GenerationConflict { expected, actual } => write!(
                f,
                "project changed: expected generation {expected}, found {actual}"
            ),
            Self::OperationIdentityConflict { operation_id } => write!(
                f,
                "operation ID {operation_id} was already used for a different request"
            ),
            Self::InvalidSnapshot(error) => write!(f, "invalid project snapshot: {error}"),
            Self::InvalidArgument(message) => {
                write!(f, "invalid project-store argument: {message}")
            }
            Self::AlreadyExists => write!(f, "project location already exists"),
            Self::UnsafePath(message) => write!(f, "unsafe project-store path: {message}"),
            Self::LockBusy => write!(
                f,
                "another project-store writer did not release its lock in time"
            ),
            Self::UnsupportedPlatform => write!(
                f,
                "this platform cannot provide the required project-store locking guarantees"
            ),
            Self::ObjectCollision(path) => write!(
                f,
                "immutable snapshot path already contains different bytes: {path}"
            ),
            #[cfg(test)]
            Self::InjectedInterruption(point) => write!(f, "injected interruption at {point:?}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<io::Error> for StoreError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ProjectValidationError> for StoreError {
    fn from(value: ProjectValidationError) -> Self {
        Self::InvalidSnapshot(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct CurrentPointer {
    format_version: u32,
    project_id: String,
    generation: u64,
    snapshot_file: String,
    snapshot_sha256: String,
    required_features: Vec<String>,
    #[serde(flatten)]
    extensions: BTreeMap<String, Value>,
}

#[derive(Clone, Debug)]
struct LoadedCurrent {
    snapshot: ProjectSnapshot,
    pointer: CurrentPointer,
}

#[derive(Clone, Debug)]
pub struct ProjectStore {
    root: PathBuf,
    parent_directory: Arc<File>,
    root_directory: Arc<File>,
    _parent_path_guards: Arc<Vec<File>>,
    root_name: OsString,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreFaultPoint {
    AfterSnapshotTempWrite,
    AfterSnapshotSync,
    AfterSnapshotPublish,
    AfterPointerSync,
    AfterPointerSwitch,
}

impl ProjectStore {
    pub fn create(
        path: impl AsRef<Path>,
        initial: &ProjectSnapshot,
        operation_id: &str,
    ) -> Result<Self, StoreError> {
        let root = normalized_root(path.as_ref())?;
        let parent = root.parent().ok_or_else(|| {
            StoreError::InvalidArgument("project directory must have an existing parent".to_owned())
        })?;
        let (directory, guards) =
            open_store_directory_chain(parent).map_err(map_unsafe_directory_error)?;
        let parent_directory = Arc::new(directory);
        let parent_path_guards = Arc::new(guards);
        let root_name = root
            .file_name()
            .ok_or_else(|| StoreError::UnsafePath("project path has no directory name".to_owned()))?
            .to_os_string();
        Self::create_in_pinned_parent(
            root,
            parent_directory,
            parent_path_guards,
            root_name,
            initial,
            operation_id,
        )
    }

    fn create_in_pinned_parent(
        root: PathBuf,
        parent_directory: Arc<File>,
        parent_path_guards: Arc<Vec<File>>,
        root_name: OsString,
        initial: &ProjectSnapshot,
        operation_id: &str,
    ) -> Result<Self, StoreError> {
        let parent = root.parent().ok_or_else(|| {
            StoreError::InvalidArgument("project directory must have an existing parent".to_owned())
        })?;
        validate_operation_id(operation_id)?;
        if initial.generation != 0 || !initial.operation_receipts.is_empty() {
            return Err(StoreError::InvalidArgument(
                "new projects must begin at generation zero without prior operation receipts"
                    .to_owned(),
            ));
        }
        initial.validate()?;
        if initial
            .rule_bindings
            .iter()
            .any(|binding| !scope_matches_confirmation(&binding.scope, initial))
            || initial.decisions.iter().any(|decision| {
                decision
                    .scope
                    .as_ref()
                    .is_some_and(|scope| !scope_matches_confirmation(scope, initial))
            })
        {
            return Err(StoreError::InvalidArgument(
                "initial project scopes do not match the records at confirmation".to_owned(),
            ));
        }
        crate::tool_runtime::validate_native_evidence(initial).map_err(|error| {
            StoreError::InvalidArgument(format!(
                "native evidence does not match its local scenario run: {error}"
            ))
        })?;
        let request_fingerprint = request_fingerprint(initial)?;

        match open_child_directory_name(&parent_directory, parent, &root_name) {
            Ok(root_directory) => {
                let existing = Self {
                    root: root.clone(),
                    parent_directory: Arc::clone(&parent_directory),
                    root_directory: Arc::new(root_directory),
                    _parent_path_guards: Arc::clone(&parent_path_guards),
                    root_name: root_name.clone(),
                };
                match existing.load()? {
                    StoreLoad::Writable(snapshot)
                        if snapshot.operation_receipts.get(operation_id)
                            == Some(&request_fingerprint) =>
                    {
                        return Ok(existing);
                    }
                    StoreLoad::Writable(_) => return Err(StoreError::AlreadyExists),
                    StoreLoad::ReadOnly(project)
                        if project.reason == ReadOnlyReason::IncompleteProject =>
                    {
                        return Err(StoreError::AlreadyExists)
                    }
                    StoreLoad::ReadOnly(project) => return Err(StoreError::ReadOnly(project)),
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) if is_unsafe_path_error(&error) => {
                return Err(StoreError::UnsafePath(error.to_string()))
            }
            Err(error) => return Err(StoreError::Io(error)),
        }

        let staging_parent = directory_path_from_handle(&parent_directory, parent)
            .map_err(map_unsafe_directory_error)?;
        let staging = tempfile::Builder::new()
            .prefix(".tool-project-create-")
            .tempdir_in(staging_parent)?;
        let staging_root = staging.path().to_path_buf();
        let snapshots = staging_root.join(SNAPSHOTS_DIR);
        fs::create_dir(&snapshots)?;
        let mut committed = initial.clone();
        committed
            .operation_receipts
            .insert(operation_id.to_owned(), request_fingerprint);
        committed.validate()?;
        let snapshot_bytes = serde_json::to_vec(&committed)
            .map_err(|error| StoreError::InvalidArgument(error.to_string()))?;
        ensure_snapshot_size(&snapshot_bytes)?;
        let snapshot_file = snapshot_file_name(committed.generation, operation_id);
        write_immutable_file(&snapshots.join(&snapshot_file), &snapshot_bytes)?;
        sync_directory(&snapshots)?;
        let pointer = CurrentPointer {
            format_version: PROJECT_FORMAT_VERSION,
            project_id: committed.project_id.clone(),
            generation: committed.generation,
            snapshot_file,
            snapshot_sha256: sha256_hex(&snapshot_bytes),
            required_features: Vec::new(),
            extensions: BTreeMap::new(),
        };
        write_replacement_file(&staging_root.join(CURRENT_FILE), &pointer_bytes(&pointer)?)?;
        sync_directory(&staging_root)?;
        let staging_name = staging.path().file_name().ok_or_else(|| {
            StoreError::InvalidArgument("staging directory has no basename".to_owned())
        })?;
        rename_noreplace_at_names(&parent_directory, parent, staging_name, &root_name)?;
        let _ = staging.keep();
        sync_directory_handle(&parent_directory)?;
        let root_directory = open_child_directory_name(&parent_directory, parent, &root_name)
            .map_err(map_unsafe_directory_error)?;
        Ok(Self {
            root,
            parent_directory,
            root_directory: Arc::new(root_directory),
            _parent_path_guards: parent_path_guards,
            root_name,
        })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let root = normalized_root(path.as_ref())?;
        let parent = root.parent().ok_or_else(|| {
            StoreError::InvalidArgument("project directory must have an existing parent".to_owned())
        })?;
        let (directory, guards) =
            open_store_directory_chain(parent).map_err(map_unsafe_directory_error)?;
        let parent_directory = Arc::new(directory);
        let parent_path_guards = Arc::new(guards);
        let root_name = root
            .file_name()
            .ok_or_else(|| StoreError::UnsafePath("project path has no directory name".to_owned()))?
            .to_os_string();
        let root_directory = open_child_directory_name(&parent_directory, parent, &root_name)
            .map_err(map_unsafe_directory_error)?;
        Ok(Self {
            root,
            parent_directory,
            root_directory: Arc::new(root_directory),
            _parent_path_guards: parent_path_guards,
            root_name,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn load(&self) -> Result<StoreLoad, StoreError> {
        match self.load_current()? {
            Ok(loaded) => Ok(StoreLoad::Writable(loaded.snapshot)),
            Err(read_only) => Ok(StoreLoad::ReadOnly(read_only)),
        }
    }

    pub fn commit(
        &self,
        expected_generation: u64,
        operation_id: &str,
        candidate: &ProjectSnapshot,
    ) -> Result<CommitResult, StoreError> {
        self.commit_inner(expected_generation, operation_id, candidate, None)
    }

    #[cfg(test)]
    pub fn commit_with_fault(
        &self,
        expected_generation: u64,
        operation_id: &str,
        candidate: &ProjectSnapshot,
        fault: StoreFaultPoint,
    ) -> Result<CommitResult, StoreError> {
        self.commit_inner(expected_generation, operation_id, candidate, Some(fault))
    }

    fn commit_inner(
        &self,
        expected_generation: u64,
        operation_id: &str,
        candidate: &ProjectSnapshot,
        #[cfg(test)] fault: Option<StoreFaultPoint>,
        #[cfg(not(test))] _fault: Option<()>,
    ) -> Result<CommitResult, StoreError> {
        validate_operation_id(operation_id)?;
        let fingerprint = request_fingerprint(candidate)?;
        if let Err(read_only) = self.load_current()? {
            return Err(StoreError::ReadOnly(read_only));
        }
        let _process_guard = PROCESS_WRITE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _file_guard = StoreFileLock::acquire(self)?;
        let loaded = match self.load_current_from(_file_guard.directory())? {
            Ok(loaded) => loaded,
            Err(read_only) => return Err(StoreError::ReadOnly(read_only)),
        };
        if candidate.generation != expected_generation {
            return Err(StoreError::GenerationConflict {
                expected: expected_generation,
                actual: candidate.generation,
            });
        }
        if let Some(previous_fingerprint) = loaded.snapshot.operation_receipts.get(operation_id) {
            if previous_fingerprint == &fingerprint {
                return Ok(CommitResult {
                    disposition: CommitDisposition::AlreadyApplied,
                    snapshot: loaded.snapshot,
                });
            }
            return Err(StoreError::OperationIdentityConflict {
                operation_id: operation_id.to_owned(),
            });
        }
        if expected_generation != loaded.snapshot.generation {
            return Err(StoreError::GenerationConflict {
                expected: expected_generation,
                actual: loaded.snapshot.generation,
            });
        }
        if candidate.project_id != loaded.snapshot.project_id {
            return Err(StoreError::InvalidArgument(
                "a commit cannot change the project ID".to_owned(),
            ));
        }
        validate_append_only(&loaded.snapshot, candidate)?;
        candidate.validate()?;
        crate::tool_runtime::validate_native_evidence(candidate).map_err(|error| {
            StoreError::InvalidArgument(format!(
                "native evidence does not match its local scenario run: {error}"
            ))
        })?;
        let mut committed = candidate.clone();
        committed.generation = expected_generation
            .checked_add(1)
            .ok_or_else(|| StoreError::InvalidArgument("project generation overflow".to_owned()))?;
        committed.operation_receipts = loaded.snapshot.operation_receipts.clone();
        committed
            .operation_receipts
            .insert(operation_id.to_owned(), fingerprint);
        committed.validate()?;

        let snapshots = self.root.join(SNAPSHOTS_DIR);
        let snapshots_directory =
            open_child_directory(_file_guard.directory(), &self.root, SNAPSHOTS_DIR)
                .map_err(map_unsafe_directory_error)?;
        let snapshot_bytes = serde_json::to_vec(&committed)
            .map_err(|error| StoreError::InvalidArgument(error.to_string()))?;
        ensure_snapshot_size(&snapshot_bytes)?;
        let snapshot_file = snapshot_file_name(committed.generation, operation_id);
        let snapshot_path = snapshots.join(&snapshot_file);
        let mut snapshot_temp =
            AnchoredTempFile::new(&snapshots_directory, &snapshots, "snapshot")?;
        snapshot_temp.write_all(&snapshot_bytes)?;
        checkpoint(
            #[cfg(test)]
            fault,
            #[cfg(test)]
            StoreFaultPoint::AfterSnapshotTempWrite,
        )?;
        snapshot_temp.as_file().sync_all()?;
        checkpoint(
            #[cfg(test)]
            fault,
            #[cfg(test)]
            StoreFaultPoint::AfterSnapshotSync,
        )?;
        match snapshot_temp.persist_noclobber(&snapshot_file, &snapshot_path) {
            Ok(()) => {}
            Err(_)
                if read_file_from_directory(
                    &snapshots_directory,
                    &snapshots,
                    &snapshot_file,
                    MAX_SNAPSHOT_BYTES,
                )
                .is_ok() =>
            {
                let existing = read_file_from_directory(
                    &snapshots_directory,
                    &snapshots,
                    &snapshot_file,
                    MAX_SNAPSHOT_BYTES,
                )?;
                if existing != snapshot_bytes {
                    return Err(StoreError::ObjectCollision(
                        snapshot_path.display().to_string(),
                    ));
                }
            }
            Err(error) => return Err(StoreError::Io(error)),
        }
        sync_directory_handle(&snapshots_directory)?;
        checkpoint(
            #[cfg(test)]
            fault,
            #[cfg(test)]
            StoreFaultPoint::AfterSnapshotPublish,
        )?;

        let mut pointer = loaded.pointer;
        pointer.generation = committed.generation;
        pointer.project_id = committed.project_id.clone();
        pointer.snapshot_file = snapshot_file;
        pointer.snapshot_sha256 = sha256_hex(&snapshot_bytes);
        let pointer_path = self.root.join(CURRENT_FILE);
        let mut pointer_temp =
            AnchoredTempFile::new(_file_guard.directory(), &self.root, "current")?;
        pointer_temp.write_all(&pointer_bytes(&pointer)?)?;
        pointer_temp.as_file().sync_all()?;
        checkpoint(
            #[cfg(test)]
            fault,
            #[cfg(test)]
            StoreFaultPoint::AfterPointerSync,
        )?;
        pointer_temp.persist_replace(CURRENT_FILE, &pointer_path)?;
        sync_directory_handle(_file_guard.directory())?;
        checkpoint(
            #[cfg(test)]
            fault,
            #[cfg(test)]
            StoreFaultPoint::AfterPointerSwitch,
        )?;
        Ok(CommitResult {
            disposition: CommitDisposition::Committed,
            snapshot: committed,
        })
    }

    fn load_current(&self) -> Result<Result<LoadedCurrent, ReadOnlyProject>, StoreError> {
        let root_directory = match self.open_project_directory() {
            Ok(directory) => directory,
            Err(error) if is_unsafe_path_error(&error) => {
                return Ok(Err(read_only(ReadOnlyReason::UnsafePath, None, None)))
            }
            Err(error) if error.kind() == io::ErrorKind::Unsupported => {
                return Ok(Err(read_only(
                    ReadOnlyReason::UnsupportedPlatform,
                    None,
                    None,
                )))
            }
            Err(error) => return Err(StoreError::Io(error)),
        };
        self.load_current_from(&root_directory)
    }

    fn open_project_directory(&self) -> io::Result<File> {
        let parent_path = self
            .root
            .parent()
            .expect("normalized project root has a parent");
        let directory =
            open_child_directory_name(&self.parent_directory, parent_path, &self.root_name)?;
        if !same_directory_identity(&directory, &self.root_directory)? {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "project directory identity changed after it was opened",
            ));
        }
        Ok(directory)
    }

    fn load_current_from(
        &self,
        root_directory: &File,
    ) -> Result<Result<LoadedCurrent, ReadOnlyProject>, StoreError> {
        let pointer_bytes = match read_file_from_directory(
            root_directory,
            &self.root,
            CURRENT_FILE,
            MAX_POINTER_BYTES,
        ) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Err(read_only(
                    ReadOnlyReason::IncompleteProject,
                    None,
                    None,
                )));
            }
            Err(error) if is_unsafe_path_error(&error) => {
                return Ok(Err(read_only(ReadOnlyReason::UnsafePath, None, None)));
            }
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                return Ok(Err(read_only(
                    ReadOnlyReason::MalformedPointer(error.to_string()),
                    None,
                    None,
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::Unsupported => {
                return Ok(Err(read_only(
                    ReadOnlyReason::UnsupportedPlatform,
                    None,
                    None,
                )));
            }
            Err(error) => return Err(StoreError::Io(error)),
        };
        let pointer_value: Value = match serde_json::from_slice(&pointer_bytes) {
            Ok(value) => value,
            Err(error) => {
                return Ok(Err(read_only(
                    ReadOnlyReason::MalformedPointer(error.to_string()),
                    Some(pointer_bytes),
                    None,
                )))
            }
        };
        let found_version = pointer_value
            .get("format_version")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                StoreError::ReadOnly(read_only(
                    ReadOnlyReason::MalformedPointer(
                        "format_version is missing or not an unsigned integer".to_owned(),
                    ),
                    Some(pointer_bytes.clone()),
                    None,
                ))
            });
        let found_version = match found_version {
            Ok(version) => version,
            Err(StoreError::ReadOnly(project)) => return Ok(Err(project)),
            Err(other) => return Err(other),
        };
        if found_version != u64::from(PROJECT_FORMAT_VERSION) {
            return Ok(Err(read_only(
                ReadOnlyReason::UnsupportedFormat {
                    found: found_version,
                    supported: PROJECT_FORMAT_VERSION,
                },
                Some(pointer_bytes),
                None,
            )));
        }
        if let Some(feature) = unsupported_feature(&pointer_value) {
            return Ok(Err(read_only(
                ReadOnlyReason::UnsupportedRequiredFeature(feature),
                Some(pointer_bytes),
                None,
            )));
        }
        let pointer: CurrentPointer = match serde_json::from_value(pointer_value) {
            Ok(pointer) => pointer,
            Err(error) => {
                return Ok(Err(read_only(
                    ReadOnlyReason::MalformedPointer(error.to_string()),
                    Some(pointer_bytes),
                    None,
                )))
            }
        };
        if !valid_basename(&pointer.snapshot_file) || !valid_hex_digest(&pointer.snapshot_sha256) {
            return Ok(Err(read_only(
                ReadOnlyReason::MalformedPointer(
                    "snapshot filename or checksum is invalid".to_owned(),
                ),
                Some(pointer_bytes),
                None,
            )));
        }
        let snapshots = self.root.join(SNAPSHOTS_DIR);
        let snapshots_directory =
            match open_child_directory(root_directory, &self.root, SNAPSHOTS_DIR) {
                Ok(directory) => directory,
                Err(error)
                    if is_unsafe_path_error(&error) || error.kind() == io::ErrorKind::NotFound =>
                {
                    return Ok(Err(read_only(
                        ReadOnlyReason::UnsafePath,
                        Some(pointer_bytes),
                        None,
                    )))
                }
                Err(error) if error.kind() == io::ErrorKind::Unsupported => {
                    return Ok(Err(read_only(
                        ReadOnlyReason::UnsupportedPlatform,
                        Some(pointer_bytes),
                        None,
                    )))
                }
                Err(error) => return Err(StoreError::Io(error)),
            };
        let snapshot_bytes = match read_file_from_directory(
            &snapshots_directory,
            &snapshots,
            &pointer.snapshot_file,
            MAX_SNAPSHOT_BYTES,
        ) {
            Ok(bytes) => bytes,
            Err(error) if is_unsafe_path_error(&error) => {
                return Ok(Err(read_only(
                    ReadOnlyReason::UnsafePath,
                    Some(pointer_bytes),
                    None,
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Err(read_only(
                    ReadOnlyReason::MalformedSnapshot("current snapshot is missing".to_owned()),
                    Some(pointer_bytes),
                    None,
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                return Ok(Err(read_only(
                    ReadOnlyReason::MalformedSnapshot(error.to_string()),
                    Some(pointer_bytes),
                    None,
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::Unsupported => {
                return Ok(Err(read_only(
                    ReadOnlyReason::UnsupportedPlatform,
                    Some(pointer_bytes),
                    None,
                )));
            }
            Err(error) => return Err(StoreError::Io(error)),
        };
        if sha256_hex(&snapshot_bytes) != pointer.snapshot_sha256 {
            return Ok(Err(read_only(
                ReadOnlyReason::ChecksumMismatch,
                Some(pointer_bytes),
                Some(snapshot_bytes),
            )));
        }
        let snapshot_value: Value = match serde_json::from_slice(&snapshot_bytes) {
            Ok(value) => value,
            Err(error) => {
                return Ok(Err(read_only(
                    ReadOnlyReason::MalformedSnapshot(error.to_string()),
                    Some(pointer_bytes),
                    Some(snapshot_bytes),
                )))
            }
        };
        let found_version = snapshot_value
            .get("format_version")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if found_version != u64::from(PROJECT_FORMAT_VERSION) {
            return Ok(Err(read_only(
                ReadOnlyReason::UnsupportedFormat {
                    found: found_version,
                    supported: PROJECT_FORMAT_VERSION,
                },
                Some(pointer_bytes),
                Some(snapshot_bytes),
            )));
        }
        if let Some(feature) = unsupported_feature(&snapshot_value) {
            return Ok(Err(read_only(
                ReadOnlyReason::UnsupportedRequiredFeature(feature),
                Some(pointer_bytes),
                Some(snapshot_bytes),
            )));
        }
        let snapshot: ProjectSnapshot = match serde_json::from_value(snapshot_value) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return Ok(Err(read_only(
                    ReadOnlyReason::MalformedSnapshot(error.to_string()),
                    Some(pointer_bytes),
                    Some(snapshot_bytes),
                )))
            }
        };
        if snapshot.validate().is_err()
            || snapshot.project_id != pointer.project_id
            || snapshot.generation != pointer.generation
        {
            return Ok(Err(read_only(
                ReadOnlyReason::MalformedSnapshot(
                    "snapshot failed integrity validation or does not match CURRENT".to_owned(),
                ),
                Some(pointer_bytes),
                Some(snapshot_bytes),
            )));
        }
        if crate::tool_runtime::validate_native_evidence(&snapshot).is_err() {
            return Ok(Err(read_only(
                ReadOnlyReason::MalformedSnapshot(
                    "stored native evidence does not match its local scenario run".to_owned(),
                ),
                Some(pointer_bytes),
                Some(snapshot_bytes),
            )));
        }
        Ok(Ok(LoadedCurrent { snapshot, pointer }))
    }
}

fn validate_append_only(
    current: &ProjectSnapshot,
    candidate: &ProjectSnapshot,
) -> Result<(), StoreError> {
    if current.project_id != candidate.project_id
        || current.created_at != candidate.created_at
        || current.event_sequence > candidate.event_sequence
        || !candidate.event_history.starts_with(&current.event_history)
        || !extensions_preserved(&current.extensions, &candidate.extensions)
    {
        return Err(StoreError::InvalidArgument(
            "a commit must preserve project identity, event history and optional extensions"
                .to_owned(),
        ));
    }
    for old_spec in &current.spec_revisions {
        if !candidate.spec_revisions.contains(old_spec) {
            return Err(StoreError::InvalidArgument(
                "existing spec revisions are immutable and cannot be removed".to_owned(),
            ));
        }
    }
    for old_record in &current.records {
        let Some(next_record) = candidate
            .records
            .iter()
            .find(|record| record.record_id == old_record.record_id)
        else {
            return Err(StoreError::InvalidArgument(
                "records cannot be removed from a project snapshot".to_owned(),
            ));
        };
        if next_record.spec != old_record.spec
            || next_record.created_at != old_record.created_at
            || next_record.created_sequence != old_record.created_sequence
            || !extensions_preserved(&old_record.extensions, &next_record.extensions)
        {
            return Err(StoreError::InvalidArgument(
                "record identity, original spec, creation facts and extensions are immutable"
                    .to_owned(),
            ));
        }
    }
    for old_behavior in &current.behavior_revisions {
        if !candidate.behavior_revisions.contains(old_behavior) {
            return Err(StoreError::InvalidArgument(
                "behavior revisions are immutable and cannot be removed".to_owned(),
            ));
        }
    }
    if !candidate.rule_bindings.starts_with(&current.rule_bindings)
        || !decisions_preserved(&current.decisions, &candidate.decisions)
        || !candidate.scenarios.starts_with(&current.scenarios)
        || !candidate.evidence.starts_with(&current.evidence)
        || !candidate.proposals.starts_with(&current.proposals)
        || current
            .operation_receipts
            .iter()
            .any(|(id, fingerprint)| candidate.operation_receipts.get(id) != Some(fingerprint))
    {
        return Err(StoreError::InvalidArgument(
            "rule bindings, scenarios, evidence, proposals and committed operations are append-only; decision transitions must preserve intent"
                .to_owned(),
        ));
    }
    for binding in &candidate.rule_bindings[current.rule_bindings.len()..] {
        if !scope_matches_confirmation(&binding.scope, current)
            || !binding_scope_matches_target(binding)
        {
            return Err(StoreError::InvalidArgument(
                "rule binding target and scope do not match the confirmed project records"
                    .to_owned(),
            ));
        }
    }
    for decision in &candidate.decisions[current.decisions.len()..] {
        if decision
            .scope
            .as_ref()
            .is_some_and(|scope| !scope_matches_confirmation(scope, current))
        {
            return Err(StoreError::InvalidArgument(
                "decision scope no longer matches the confirmed project records".to_owned(),
            ));
        }
    }
    Ok(())
}

fn scope_matches_confirmation(
    scope: &crate::tool_project::ScopeSelection,
    current: &ProjectSnapshot,
) -> bool {
    use crate::tool_project::ScopeKind::{
        AllExistingAndFuture, FutureRecords, IncompleteAndFuture, SingleRecord,
    };

    if scope.confirmed_generation != current.generation {
        return false;
    }
    if scope.validate().is_err() {
        return false;
    }
    let actual: HashSet<&str> = scope.frozen_record_ids.iter().map(String::as_str).collect();
    match scope.kind {
        SingleRecord => {
            actual.len() == 1
                && current
                    .records
                    .iter()
                    .any(|record| actual.contains(record.record_id.as_str()))
        }
        FutureRecords => actual.is_empty(),
        AllExistingAndFuture => {
            let expected: HashSet<&str> = current
                .records
                .iter()
                .map(|record| record.record_id.as_str())
                .collect();
            actual == expected
        }
        IncompleteAndFuture => {
            let expected: HashSet<&str> = current
                .records
                .iter()
                .filter(|record| {
                    current
                        .spec_revisions
                        .iter()
                        .find(|spec| {
                            spec.spec_id == record.spec.spec_id
                                && spec.revision == record.spec.revision
                        })
                        .and_then(|spec| spec.stage(&record.current_stage_id))
                        .is_some_and(|stage| !stage.terminal)
                })
                .map(|record| record.record_id.as_str())
                .collect();
            actual == expected
        }
    }
}

fn binding_scope_matches_target(binding: &crate::tool_project::RuleBinding) -> bool {
    use crate::tool_project::ScopeKind::SingleRecord;

    match (&binding.record_id, binding.scope.kind) {
        (Some(record_id), SingleRecord) => {
            binding.scope.frozen_record_ids.len() == 1
                && binding.scope.frozen_record_ids[0] == *record_id
        }
        (None, kind) if kind != SingleRecord => true,
        _ => false,
    }
}

fn extensions_preserved(old: &BTreeMap<String, Value>, new: &BTreeMap<String, Value>) -> bool {
    old.iter().all(|(key, value)| new.get(key) == Some(value))
}

fn decisions_preserved(
    current: &[crate::tool_project::DecisionRecord],
    candidate: &[crate::tool_project::DecisionRecord],
) -> bool {
    use crate::tool_project::DecisionStatus::{Active, Pending, Superseded, Withdrawn};

    if candidate.len() < current.len() {
        return false;
    }
    let appended = &candidate[current.len()..];
    for (previous, next) in current.iter().zip(candidate) {
        if previous == next {
            continue;
        }
        let transition_is_allowed = matches!(
            (previous.status, next.status),
            (Pending, Active | Superseded | Withdrawn) | (Active, Superseded | Withdrawn)
        );
        if !transition_is_allowed {
            return false;
        }
        let mut preserved_intent = next.clone();
        preserved_intent.status = previous.status;
        if &preserved_intent != previous {
            return false;
        }
        if next.status == Superseded
            && !appended
                .iter()
                .any(|replacement| replacement.supersedes.contains(&previous.decision_id))
        {
            return false;
        }
    }
    for replacement in appended {
        for superseded_id in &replacement.supersedes {
            let Some(superseded) = candidate
                .iter()
                .find(|decision| decision.decision_id == *superseded_id)
            else {
                return false;
            };
            if superseded.status != Superseded {
                return false;
            }
        }
    }
    true
}

fn request_fingerprint(snapshot: &ProjectSnapshot) -> Result<String, StoreError> {
    let mut request = snapshot.clone();
    request.operation_receipts.clear();
    let bytes = serde_json::to_vec(&request)
        .map_err(|error| StoreError::InvalidArgument(error.to_string()))?;
    Ok(sha256_hex(&bytes))
}

fn snapshot_file_name(generation: u64, operation_id: &str) -> String {
    let operation_hash = sha256_hex(operation_id.as_bytes());
    format!("g{generation:020}-{}.json", &operation_hash[..24])
}

fn pointer_bytes(pointer: &CurrentPointer) -> Result<Vec<u8>, StoreError> {
    serde_json::to_vec(pointer).map_err(|error| StoreError::InvalidArgument(error.to_string()))
}

fn unsupported_feature(value: &Value) -> Option<String> {
    value
        .get("required_features")
        .and_then(Value::as_array)
        .and_then(|features| {
            features.iter().find_map(|feature| {
                feature
                    .as_str()
                    .filter(|feature| !SUPPORTED_REQUIRED_FEATURES.contains(feature))
                    .map(str::to_owned)
            })
        })
}

const SUPPORTED_REQUIRED_FEATURES: &[&str] = &[];

fn valid_operation_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= crate::tool_project::MAX_ID_LENGTH
        && value.as_bytes()[0].is_ascii_lowercase()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_.".contains(&byte)
        })
}

fn validate_operation_id(value: &str) -> Result<(), StoreError> {
    if valid_operation_id(value) {
        Ok(())
    } else {
        Err(StoreError::InvalidArgument(
            "operation ID must be a stable lowercase ID".to_owned(),
        ))
    }
}

fn valid_fingerprint(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_hex_digest(value: &str) -> bool {
    valid_fingerprint(value)
}

fn valid_basename(value: &str) -> bool {
    if !value.ends_with(".json") || value.is_empty() || value.len() > 256 {
        return false;
    }
    let path = Path::new(value);
    path.components().count() == 1
        && matches!(path.components().next(), Some(Component::Normal(_)))
        && !value.contains('\\')
}

fn normalized_root(path: &Path) -> Result<PathBuf, StoreError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err(StoreError::UnsafePath(
            "project path must be absolute and cannot contain dot traversal".to_owned(),
        ));
    }
    let name = path.file_name().ok_or_else(|| {
        StoreError::UnsafePath("filesystem root is not a project directory".to_owned())
    })?;
    let parent = path.parent().ok_or_else(|| {
        StoreError::UnsafePath("project directory needs an existing parent".to_owned())
    })?;
    let canonical_parent = fs::canonicalize(parent).map_err(StoreError::Io)?;
    Ok(canonical_parent.join(name))
}

fn open_store_directory_chain(path: &Path) -> io::Result<(File, Vec<File>)> {
    #[cfg(windows)]
    {
        let mut current = PathBuf::new();
        let mut guards = Vec::new();
        for component in path.components() {
            match component {
                Component::Prefix(prefix) => current.push(prefix.as_os_str()),
                Component::RootDir => {
                    current.push(component.as_os_str());
                    guards.push(open_store_directory(&current)?);
                }
                Component::Normal(name) => {
                    current.push(name);
                    guards.push(open_store_directory(&current)?);
                }
                Component::CurDir | Component::ParentDir => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "pinned project parent cannot contain traversal components",
                    ));
                }
            }
        }
        let directory = guards
            .last()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing parent path"))?
            .try_clone()?;
        return Ok((directory, guards));
    }
    #[cfg(unix)]
    {
        if !path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "pinned project parent must be an absolute path",
            ));
        }
        let mut directory = open_store_directory(Path::new("/"))?;
        let mut current_path = PathBuf::from("/");
        for component in path.components() {
            match component {
                Component::RootDir => {}
                Component::Normal(name) => {
                    directory = open_child_directory_name(&directory, &current_path, name)?;
                    current_path.push(name);
                }
                Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "pinned project parent cannot contain traversal components",
                    ));
                }
            }
        }
        Ok((directory, Vec::new()))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "directory-handle operations are unsupported on this platform",
        ))
    }
}

fn directory_path_from_handle(directory: &File, fallback: &Path) -> io::Result<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;

        let _ = fallback;
        return Ok(PathBuf::from(format!(
            "/proc/self/fd/{}",
            directory.as_raw_fd()
        )));
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::fd::AsRawFd;

        let _ = fallback;
        return Ok(PathBuf::from(format!("/dev/fd/{}", directory.as_raw_fd())));
    }
    #[cfg(windows)]
    {
        let _ = directory;
        return Ok(fallback.to_path_buf());
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (directory, fallback);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "pinned parent-directory paths are unsupported on this platform",
        ))
    }
}

fn ensure_real_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "directory path is a symlink or is not a directory",
        ));
    }
    Ok(())
}

fn is_unsafe_path_error(error: &io::Error) -> bool {
    if matches!(
        error.kind(),
        io::ErrorKind::InvalidInput | io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    ) {
        return true;
    }
    #[cfg(target_os = "linux")]
    if error.raw_os_error() == Some(40) {
        return true;
    }
    #[cfg(target_os = "macos")]
    if error.raw_os_error() == Some(62) {
        return true;
    }
    false
}

fn map_unsafe_directory_error(error: io::Error) -> StoreError {
    if is_unsafe_path_error(&error) {
        StoreError::UnsafePath(error.to_string())
    } else if error.kind() == io::ErrorKind::Unsupported {
        StoreError::UnsupportedPlatform
    } else {
        StoreError::Io(error)
    }
}

fn open_store_directory(path: &Path) -> io::Result<File> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::fs::OpenOptionsExt;

        let mut options = OpenOptions::new();
        options
            .read(true)
            .custom_flags(no_follow_flag() | directory_flag() | close_on_exec_flag());
        let directory = options.open(path)?;
        if !directory.metadata()?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "project path is not a real directory",
            ));
        }
        Ok(directory)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
            FILE_SHARE_WRITE,
        };

        let mut options = OpenOptions::new();
        options
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
        let directory = options.open(path)?;
        let metadata = directory.metadata()?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "project path is a symlink or is not a directory",
            ));
        }
        Ok(directory)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "directory-handle operations are unsupported on this platform",
        ))
    }
}

fn same_directory_identity(left: &File, right: &File) -> io::Result<bool> {
    let left = left.metadata()?;
    let right = right.metadata()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        Ok(left.dev() == right.dev() && left.ino() == right.ino())
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        let left_identity = left.volume_serial_number().zip(left.file_index());
        let right_identity = right.volume_serial_number().zip(right.file_index());
        match (left_identity, right_identity) {
            (Some(left), Some(right)) => Ok(left == right),
            _ => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "the platform did not provide stable project directory identity",
            )),
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (left, right);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "stable project directory identity is unsupported on this platform",
        ))
    }
}

fn open_child_directory(parent: &File, parent_path: &Path, name: &str) -> io::Result<File> {
    open_child_directory_name(parent, parent_path, OsStr::new(name))
}

fn open_child_directory_name(parent: &File, parent_path: &Path, name: &OsStr) -> io::Result<File> {
    #[cfg(unix)]
    {
        let _ = parent_path;
        let directory = open_file_at(
            parent,
            name,
            directory_flag() | no_follow_flag() | close_on_exec_flag(),
            0,
        )?;
        if !directory.metadata()?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "project child path is not a real directory",
            ));
        }
        Ok(directory)
    }
    #[cfg(windows)]
    {
        open_store_directory(&parent_path.join(name))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (parent, parent_path, name);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "directory-handle operations are unsupported on this platform",
        ))
    }
}

fn read_file_from_directory(
    directory: &File,
    directory_path: &Path,
    name: &str,
    limit: usize,
) -> io::Result<Vec<u8>> {
    #[cfg(unix)]
    {
        let _ = directory_path;
        let mut file = open_file_at(
            directory,
            OsStr::new(name),
            no_follow_flag() | close_on_exec_flag() | nonblocking_flag(),
            0,
        )?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "project file path is not a regular file",
            ));
        }
        read_limited(&mut file, limit)
    }
    #[cfg(windows)]
    {
        read_bounded_no_follow(&directory_path.join(name), limit)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (directory, directory_path, name, limit);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "safe file access is unsupported on this platform",
        ))
    }
}

fn open_lock_file(directory: &File, path: &Path) -> Result<File, StoreError> {
    #[cfg(unix)]
    {
        let _ = path;
        return open_file_at(
            directory,
            OsStr::new(LOCK_FILE),
            read_write_flag() | create_flag() | no_follow_flag() | close_on_exec_flag(),
            0o600,
        )
        .map_err(|error| {
            if is_unsafe_path_error(&error) {
                StoreError::UnsafePath(error.to_string())
            } else {
                StoreError::Io(error)
            }
        });
    }
    #[cfg(windows)]
    {
        let _ = directory;
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;

        ensure_regular_file_or_missing(path).map_err(|error| {
            if is_unsafe_path_error(&error) {
                StoreError::UnsafePath(error.to_string())
            } else {
                StoreError::Io(error)
            }
        })?;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .create(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        return options.open(path).map_err(StoreError::Io);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (directory, path);
        Err(StoreError::UnsupportedPlatform)
    }
}

#[cfg(unix)]
fn open_file_at(directory: &File, name: &OsStr, flags: i32, mode: u32) -> io::Result<File> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::raw::{c_char, c_int};
    use std::os::unix::ffi::OsStrExt;

    extern "C" {
        fn openat(directory: c_int, path: *const c_char, flags: c_int, ...) -> c_int;
    }

    let path = CString::new(name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "file name contains NUL"))?;
    let descriptor = unsafe { openat(directory.as_raw_fd(), path.as_ptr(), flags, mode as c_int) };
    if descriptor < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(descriptor) })
    }
}

#[cfg(all(unix, target_os = "linux"))]
fn directory_flag() -> i32 {
    0o200000
}

#[cfg(all(unix, target_os = "macos"))]
fn directory_flag() -> i32 {
    0x0010_0000
}

#[cfg(all(unix, target_os = "linux"))]
fn close_on_exec_flag() -> i32 {
    0o2000000
}

#[cfg(all(unix, target_os = "macos"))]
fn close_on_exec_flag() -> i32 {
    0x0100_0000
}

#[cfg(all(unix, target_os = "linux"))]
fn nonblocking_flag() -> i32 {
    0o4000
}

#[cfg(all(unix, target_os = "macos"))]
fn nonblocking_flag() -> i32 {
    0x0000_0004
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn directory_flag() -> i32 {
    0
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn close_on_exec_flag() -> i32 {
    0
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn nonblocking_flag() -> i32 {
    0
}

#[cfg(target_os = "linux")]
fn create_flag() -> i32 {
    0o100
}

#[cfg(target_os = "macos")]
fn create_flag() -> i32 {
    0x0000_0200
}

#[cfg(target_os = "linux")]
fn exclusive_flag() -> i32 {
    0o200
}

#[cfg(target_os = "macos")]
fn exclusive_flag() -> i32 {
    0x0000_0800
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn create_flag() -> i32 {
    0
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn exclusive_flag() -> i32 {
    0
}

#[cfg(unix)]
fn read_write_flag() -> i32 {
    0x2
}

#[cfg(unix)]
fn write_only_flag() -> i32 {
    0x1
}

#[cfg(all(unix, any(target_os = "linux", target_os = "macos")))]
struct AnchoredTempFile<'a> {
    directory: &'a File,
    file: Option<File>,
    name: String,
    published: bool,
}

#[cfg(not(all(unix, any(target_os = "linux", target_os = "macos"))))]
struct AnchoredTempFile<'a> {
    file: Option<NamedTempFile>,
    _directory_path: &'a Path,
}

impl<'a> AnchoredTempFile<'a> {
    fn new(directory: &'a File, directory_path: &'a Path, prefix: &str) -> io::Result<Self> {
        #[cfg(all(unix, any(target_os = "linux", target_os = "macos")))]
        {
            let _ = directory_path;
            let (file, name) = create_temp_file_at(directory, prefix)?;
            return Ok(Self {
                directory,
                file: Some(file),
                name,
                published: false,
            });
        }
        #[cfg(not(all(unix, any(target_os = "linux", target_os = "macos"))))]
        {
            let _ = directory;
            Ok(Self {
                file: Some(NamedTempFile::new_in(directory_path)?),
                _directory_path: directory_path,
            })
        }
    }

    fn as_file(&self) -> &File {
        #[cfg(all(unix, any(target_os = "linux", target_os = "macos")))]
        {
            self.file.as_ref().expect("temporary file is retained")
        }
        #[cfg(not(all(unix, any(target_os = "linux", target_os = "macos"))))]
        {
            self.file
                .as_ref()
                .expect("temporary file is retained")
                .as_file()
        }
    }

    fn as_file_mut(&mut self) -> &mut File {
        #[cfg(all(unix, any(target_os = "linux", target_os = "macos")))]
        {
            self.file.as_mut().expect("temporary file is retained")
        }
        #[cfg(not(all(unix, any(target_os = "linux", target_os = "macos"))))]
        {
            self.file
                .as_mut()
                .expect("temporary file is retained")
                .as_file_mut()
        }
    }

    fn persist_replace(mut self, name: &str, destination: &Path) -> io::Result<()> {
        #[cfg(all(unix, any(target_os = "linux", target_os = "macos")))]
        {
            let _ = destination;
            rename_replace_at(self.directory, &self.name, name)?;
            self.published = true;
            Ok(())
        }
        #[cfg(not(all(unix, any(target_os = "linux", target_os = "macos"))))]
        {
            persist_replace(
                self.file.take().expect("temporary file is retained"),
                destination,
            )
        }
    }

    fn persist_noclobber(mut self, name: &str, destination: &Path) -> io::Result<()> {
        #[cfg(all(unix, any(target_os = "linux", target_os = "macos")))]
        {
            let _ = destination;
            rename_noreplace_at(self.directory, OsStr::new(&self.name), OsStr::new(name))?;
            self.published = true;
            Ok(())
        }
        #[cfg(not(all(unix, any(target_os = "linux", target_os = "macos"))))]
        {
            persist_noclobber(
                self.file.take().expect("temporary file is retained"),
                destination,
            )
        }
    }
}

impl Write for AnchoredTempFile<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.as_file_mut().write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.as_file_mut().flush()
    }
}

#[cfg(all(unix, any(target_os = "linux", target_os = "macos")))]
impl Drop for AnchoredTempFile<'_> {
    fn drop(&mut self) {
        if !self.published {
            let _ = unlink_file_at(self.directory, &self.name);
        }
    }
}

#[cfg(all(unix, any(target_os = "linux", target_os = "macos")))]
fn create_temp_file_at(directory: &File, prefix: &str) -> io::Result<(File, String)> {
    for _ in 0..128 {
        let sequence = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let name = format!(".tool-{prefix}-{}-{sequence:016x}", std::process::id());
        match open_file_at(
            directory,
            OsStr::new(&name),
            write_only_flag()
                | create_flag()
                | exclusive_flag()
                | no_follow_flag()
                | close_on_exec_flag(),
            0o600,
        ) {
            Ok(file) => return Ok((file, name)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "cannot allocate a unique project-store temporary file",
    ))
}

#[cfg(all(unix, any(target_os = "linux", target_os = "macos")))]
fn rename_replace_at(directory: &File, source: &str, destination: &str) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    use std::os::raw::{c_char, c_int};

    extern "C" {
        fn renameat(
            old_directory: c_int,
            old_path: *const c_char,
            new_directory: c_int,
            new_path: *const c_char,
        ) -> c_int;
    }
    let source = CString::new(source)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "source name contains NUL"))?;
    let destination = CString::new(destination).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "destination name contains NUL")
    })?;
    let descriptor = directory.as_raw_fd();
    let result = unsafe {
        renameat(
            descriptor,
            source.as_ptr(),
            descriptor,
            destination.as_ptr(),
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(all(unix, any(target_os = "linux", target_os = "macos")))]
fn rename_noreplace_at(directory: &File, source: &OsStr, destination: &OsStr) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    use std::os::raw::{c_char, c_int};
    use std::os::unix::ffi::OsStrExt;

    let source = CString::new(source.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "source name contains NUL"))?;
    let destination = CString::new(destination.as_bytes()).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "destination name contains NUL")
    })?;
    let descriptor = directory.as_raw_fd();
    #[cfg(target_os = "linux")]
    {
        use std::os::raw::c_uint;
        const RENAME_NOREPLACE: c_uint = 1;
        extern "C" {
            fn renameat2(
                old_directory: c_int,
                old_path: *const c_char,
                new_directory: c_int,
                new_path: *const c_char,
                flags: c_uint,
            ) -> c_int;
        }
        let result = unsafe {
            renameat2(
                descriptor,
                source.as_ptr(),
                descriptor,
                destination.as_ptr(),
                RENAME_NOREPLACE,
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    #[cfg(target_os = "macos")]
    {
        const RENAME_EXCL: u32 = 0x0000_0004;
        extern "C" {
            fn renameatx_np(
                old_directory: c_int,
                old_path: *const c_char,
                new_directory: c_int,
                new_path: *const c_char,
                flags: u32,
            ) -> c_int;
        }
        let result = unsafe {
            renameatx_np(
                descriptor,
                source.as_ptr(),
                descriptor,
                destination.as_ptr(),
                RENAME_EXCL,
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

fn rename_noreplace_at_names(
    directory: &File,
    directory_path: &Path,
    source: &OsStr,
    destination: &OsStr,
) -> io::Result<()> {
    #[cfg(all(unix, any(target_os = "linux", target_os = "macos")))]
    {
        let _ = directory_path;
        rename_noreplace_at(directory, source, destination)
    }
    #[cfg(windows)]
    {
        let anchored_path = directory_path_from_handle(directory, directory_path)?;
        rename_noreplace(
            &anchored_path.join(source),
            &anchored_path.join(destination),
        )
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (directory, directory_path, source, destination);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "atomic no-replace directory publication is unavailable",
        ))
    }
}

#[cfg(all(unix, any(target_os = "linux", target_os = "macos")))]
fn unlink_file_at(directory: &File, name: &str) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    use std::os::raw::{c_char, c_int};

    extern "C" {
        fn unlinkat(directory: c_int, path: *const c_char, flags: c_int) -> c_int;
    }
    let path = CString::new(name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "file name contains NUL"))?;
    let result = unsafe { unlinkat(directory.as_raw_fd(), path.as_ptr(), 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn ensure_regular_file_or_missing(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "file path is a symlink or not a regular file",
            ))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn read_bounded(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    ensure_regular_file_or_missing(path)?;
    let mut file = File::open(path)?;
    read_limited(&mut file, limit)
}

#[cfg(windows)]
fn read_bounded_no_follow(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (path, limit);
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "safe no-follow reads are unsupported on this platform",
        ));
    }
    ensure_regular_file_or_missing(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(no_follow_flag());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let mut file = options.open(path)?;
    read_limited(&mut file, limit)
}

fn read_limited(reader: &mut impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take((limit as u64) + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "project-store file exceeds the supported size limit",
        ));
    }
    Ok(bytes)
}

fn ensure_snapshot_size(bytes: &[u8]) -> Result<(), StoreError> {
    if bytes.len() > MAX_SNAPSHOT_BYTES {
        return Err(StoreError::InvalidArgument(
            "project snapshot exceeds the supported size limit".to_owned(),
        ));
    }
    Ok(())
}

fn write_immutable_file(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let parent = path.parent().ok_or_else(|| {
        StoreError::InvalidArgument("snapshot path has no parent directory".to_owned())
    })?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    match persist_noclobber(temporary, path) {
        Ok(()) => {}
        Err(_) if path.exists() => {
            let existing = read_bounded(path, MAX_SNAPSHOT_BYTES)?;
            if existing != bytes {
                return Err(StoreError::ObjectCollision(path.display().to_string()));
            }
        }
        Err(error) => return Err(StoreError::Io(error)),
    }
    Ok(())
}

fn persist_noclobber(temporary: NamedTempFile, destination: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};

        let (file, temporary_path) = temporary.keep().map_err(|error| error.error)?;
        drop(file);
        let source: Vec<u16> = temporary_path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        let destination: Vec<u16> = destination
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        let result = unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_WRITE_THROUGH,
            )
        };
        if result != 0 {
            Ok(())
        } else {
            let error = io::Error::last_os_error();
            let _ = fs::remove_file(temporary_path);
            Err(error)
        }
    }
    #[cfg(not(windows))]
    {
        temporary
            .persist_noclobber(destination)
            .map(|_| ())
            .map_err(|error| error.error)
    }
}

fn persist_replace(temporary: NamedTempFile, destination: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };

        let (file, temporary_path) = temporary.keep().map_err(|error| error.error)?;
        drop(file);
        let source: Vec<u16> = temporary_path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        let destination: Vec<u16> = destination
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        let result = unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if result != 0 {
            Ok(())
        } else {
            let error = io::Error::last_os_error();
            let _ = fs::remove_file(temporary_path);
            Err(error)
        }
    }
    #[cfg(not(windows))]
    {
        temporary
            .persist(destination)
            .map(|_| ())
            .map_err(|error| error.error)
    }
}

fn write_replacement_file(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let parent = path.parent().ok_or_else(|| {
        StoreError::InvalidArgument("pointer path has no parent directory".to_owned())
    })?;
    ensure_real_directory(parent)?;
    ensure_regular_file_or_missing(path)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    persist_replace(temporary, path).map_err(StoreError::Io)?;
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    digest(&SHA256, bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn read_only(
    reason: ReadOnlyReason,
    raw_pointer_bytes: Option<Vec<u8>>,
    raw_snapshot_bytes: Option<Vec<u8>>,
) -> ReadOnlyProject {
    ReadOnlyProject {
        reason,
        raw_pointer_bytes,
        raw_snapshot_bytes,
    }
}

fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()
    }
    #[cfg(windows)]
    {
        let _ = path;
        Ok(())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "directory durability is unsupported on this platform",
        ))
    }
}

fn sync_directory_handle(directory: &File) -> io::Result<()> {
    #[cfg(unix)]
    {
        directory.sync_all()
    }
    #[cfg(windows)]
    {
        let _ = directory;
        Ok(())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = directory;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "directory durability is unsupported on this platform",
        ))
    }
}

#[cfg(windows)]
fn rename_noreplace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_WRITE_THROUGH,
        )
    };
    if result != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn no_follow_flag() -> i32 {
    0o400000
}

#[cfg(target_os = "macos")]
fn no_follow_flag() -> i32 {
    0x0000_0100
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn no_follow_flag() -> i32 {
    0
}

#[cfg(windows)]
struct StoreFileLock {
    _directory: File,
    _file: File,
    _overlapped: windows_sys::Win32::System::IO::OVERLAPPED,
}

#[cfg(all(unix, not(windows)))]
struct StoreFileLock {
    _directory: File,
    _file: File,
}

#[cfg(not(any(unix, windows)))]
struct StoreFileLock;

impl StoreFileLock {
    fn acquire(store: &ProjectStore) -> Result<Self, StoreError> {
        #[cfg(not(any(unix, windows)))]
        {
            let _ = store;
            return Err(StoreError::UnsupportedPlatform);
        }
        let directory = store
            .open_project_directory()
            .map_err(map_unsafe_directory_error)?;
        let path = store.root.join(LOCK_FILE);
        let file = open_lock_file(&directory, &path)?;
        if !file.metadata()?.is_file() {
            return Err(StoreError::UnsafePath(
                "project lock path is not a regular file".to_owned(),
            ));
        }
        let deadline = Instant::now() + LOCK_WAIT_LIMIT;
        #[cfg(windows)]
        let mut overlapped = unsafe { std::mem::zeroed() };
        loop {
            #[cfg(windows)]
            let locked = try_lock_file(&file, &mut overlapped)?;
            #[cfg(unix)]
            let locked = try_lock_file(&file).map_err(StoreError::Io)?;
            if locked {
                #[cfg(windows)]
                return Ok(Self {
                    _directory: directory,
                    _file: file,
                    _overlapped: overlapped,
                });
                #[cfg(unix)]
                return Ok(Self {
                    _directory: directory,
                    _file: file,
                });
            }
            if Instant::now() >= deadline {
                return Err(StoreError::LockBusy);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn directory(&self) -> &File {
        &self._directory
    }
}

#[cfg(unix)]
fn try_lock_file(file: &File) -> io::Result<bool> {
    use std::os::fd::AsRawFd;
    use std::os::raw::c_int;

    const LOCK_EXCLUSIVE: c_int = 2;
    const LOCK_NONBLOCKING: c_int = 4;
    extern "C" {
        fn flock(fd: c_int, operation: c_int) -> c_int;
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
    file: &File,
    overlapped: &mut windows_sys::Win32::System::IO::OVERLAPPED,
) -> io::Result<bool> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_LOCK_VIOLATION};
    use windows_sys::Win32::Storage::FileSystem::{
        LockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
    };

    let result = unsafe {
        LockFileEx(
            file.as_raw_handle() as _,
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            1,
            0,
            overlapped,
        )
    };
    if result != 0 {
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

fn checkpoint(
    #[cfg(test)] requested: Option<StoreFaultPoint>,
    #[cfg(test)] point: StoreFaultPoint,
) -> Result<(), StoreError> {
    #[cfg(test)]
    if requested == Some(point) {
        return Err(StoreError::InjectedInterruption(point));
    }
    Ok(())
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;

    #[test]
    fn current_pointer_replacement_is_anchored_to_the_open_store_directory() {
        use std::os::unix::fs::symlink;

        let temporary_root = tempfile::tempdir().unwrap();
        let root = temporary_root.path().join("project");
        let external = temporary_root.path().join("external");
        let moved_root = temporary_root.path().join("moved-project");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&external).unwrap();
        fs::write(root.join(CURRENT_FILE), b"original pointer").unwrap();
        fs::write(external.join(CURRENT_FILE), b"external data").unwrap();

        let pinned_root = open_store_directory(&root).unwrap();
        let mut pointer_temp = AnchoredTempFile::new(&pinned_root, &root, "current").unwrap();
        pointer_temp.write_all(b"new pointer").unwrap();
        pointer_temp.as_file().sync_all().unwrap();
        let temp_name = pointer_temp.name.clone();
        fs::hard_link(root.join(&temp_name), external.join(&temp_name)).unwrap();

        fs::rename(&root, &moved_root).unwrap();
        symlink(&external, &root).unwrap();

        pointer_temp
            .persist_replace(CURRENT_FILE, &root.join(CURRENT_FILE))
            .unwrap();

        assert_eq!(
            fs::read(external.join(CURRENT_FILE)).unwrap(),
            b"external data"
        );
        assert_eq!(
            fs::read(moved_root.join(CURRENT_FILE)).unwrap(),
            b"new pointer"
        );
        fs::remove_file(external.join(temp_name)).unwrap();
    }

    #[test]
    fn project_creation_uses_the_pinned_parent_for_staging_and_publication() {
        use std::os::unix::fs::symlink;

        let temporary_root = tempfile::tempdir().unwrap();
        let parent_path = temporary_root.path().join("project-parent");
        let external_parent = temporary_root.path().join("external-parent");
        let moved_parent = temporary_root.path().join("moved-project-parent");
        fs::create_dir(&parent_path).unwrap();
        fs::create_dir(&external_parent).unwrap();
        let parent_directory = Arc::new(open_store_directory(&parent_path).unwrap());
        let root = parent_path.join("project");

        fs::rename(&parent_path, &moved_parent).unwrap();
        symlink(&external_parent, &parent_path).unwrap();

        let initial = crate::tool_project::ProjectSnapshot::new(
            "pinned-parent-project".to_owned(),
            "Pinned parent project".to_owned(),
            chrono::DateTime::parse_from_rfc3339("2026-10-01T08:00:00+00:00").unwrap(),
            crate::tool_project::ProjectDateSettings {
                calendar_utc_offset_seconds: 0,
            },
            crate::tool_project::studio_order_template(),
            crate::tool_project::BehaviorPolicy {
                timer: crate::tool_project::TimerPolicy::PauseStagesMarkedPaused,
                due_date: crate::tool_project::DateDuePolicy::KeepOriginal,
                reminder: crate::tool_project::ReminderPolicy::WaitingBeforeDue {
                    days_before_due: 1,
                },
            },
        )
        .unwrap();
        let store = ProjectStore::create_in_pinned_parent(
            root,
            parent_directory,
            Arc::new(Vec::new()),
            OsString::from("project"),
            &initial,
            "create-pinned-parent-project",
        )
        .unwrap();

        assert!(moved_parent.join("project/CURRENT").is_file());
        assert!(!external_parent.join("project").exists());
        match store.load().unwrap() {
            StoreLoad::Writable(snapshot) => {
                assert_eq!(snapshot.project_id, "pinned-parent-project");
                assert_eq!(snapshot.project_name, "Pinned parent project");
            }
            StoreLoad::ReadOnly(read_only) => {
                panic!(
                    "pinned project should remain readable: {:?}",
                    read_only.reason
                )
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn unix_parent_directory_walk_rejects_intermediate_symlinks() {
        use std::os::unix::fs::symlink;

        let temporary_root = tempfile::tempdir().unwrap();
        let canonical_root = fs::canonicalize(temporary_root.path()).unwrap();
        let real_parent = canonical_root.join("real-container/store-parent");
        let redirected_parent = canonical_root.join("redirected-container");
        fs::create_dir_all(&real_parent).unwrap();
        symlink(canonical_root.join("real-container"), &redirected_parent).unwrap();

        let candidate = redirected_parent.join("store-parent");
        assert!(open_store_directory_chain(&candidate).is_err());
    }
}
