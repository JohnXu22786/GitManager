//! Local host helpers. Controller registration and native dialogs belong to the
//! studio; generated programs never receive filesystem operations.
use crate::product_contract::{
    canonical_bytes, canonical_digest, valid_id, Digest, IdentityDomain,
};
use crate::product_store::{ProductStore, ProjectSnapshot, StoreError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

// One private implementation of descriptor-relative/no-reparse I/O. No raw
// Directory or file handle is part of the public helper contract.
#[path = "product_store/files.rs"]
mod files;

pub(crate) const MAX_LOCAL_BYTES: usize = 64 * 1024 * 1024;
const MAX_RECENT_BYTES: usize = 1024 * 1024;
const MAX_RECENT_TOOLS: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueKind {
    Unavailable,
    UnsafePath,
    Corrupt,
    Unsupported,
    Collision,
    Busy,
    Limit,
    Incompatible,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationIssue {
    pub kind: IssueKind,
    pub message: String,
    pub next_step: String,
    pub detail: String,
}
pub type Result<T> = std::result::Result<T, OperationIssue>;
impl OperationIssue {
    pub(crate) fn new(kind: IssueKind, detail: impl ToString) -> Self {
        let (message, next_step) = match kind {
            IssueKind::Unavailable => ("This location is not available.", "Reconnect the drive or choose an available folder. Your saved work has not been replaced."),
            IssueKind::UnsafePath => ("This location cannot be used safely.", "Choose the original folder or a new ordinary folder, rather than a linked or replaced location."),
            IssueKind::Corrupt => ("These saved bytes could not be verified.", "Keep this copy. Open a verified backup or choose another saved copy; do not replace current work."),
            IssueKind::Unsupported => ("This saved format is not supported here.", "Use a compatible version of GitManager. Keep the original file unchanged."),
            IssueKind::Collision => ("There is already saved work at that location.", "Choose a new name or folder. Recovery creates a separate tool and does not overwrite existing work."),
            IssueKind::Busy => ("Another operation is using this location.", "Let it finish, then try again."),
            IssueKind::Limit => ("This operation exceeds a safe size or count limit.", "Keep the original work. Choose one backup to inspect or another backup location; nothing is automatically deleted."),
            IssueKind::Incompatible => ("This tool cannot safely use the saved data in this version.", "Keep the data and use a compatible tool version. Restoring behavior must keep current business data."),
        };
        Self {
            kind,
            message: message.into(),
            next_step: next_step.into(),
            detail: detail.to_string(),
        }
    }
}
impl fmt::Display for OperationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.message, self.next_step)
    }
}
impl std::error::Error for OperationIssue {}
impl From<io::Error> for OperationIssue {
    fn from(error: io::Error) -> Self {
        let kind = match error.kind() {
            io::ErrorKind::AlreadyExists => IssueKind::Collision,
            io::ErrorKind::WouldBlock => IssueKind::Busy,
            io::ErrorKind::InvalidInput => IssueKind::UnsafePath,
            io::ErrorKind::Unsupported => IssueKind::Unsupported,
            _ => IssueKind::Unavailable,
        };
        Self::new(kind, error)
    }
}
impl From<StoreError> for OperationIssue {
    fn from(error: StoreError) -> Self {
        let kind = match &error {
            StoreError::Io(e) => return Self::from(io::Error::new(e.kind(), error.to_string())),
            StoreError::UnsupportedFormat(_) => IssueKind::Unsupported,
            StoreError::Incompatible(_) | StoreError::Runtime(_) => IssueKind::Incompatible,
            StoreError::Conflict(_) => IssueKind::Collision,
            _ => IssueKind::Corrupt,
        };
        Self::new(kind, error)
    }
}
impl From<crate::product_contract::ContractError> for OperationIssue {
    fn from(e: crate::product_contract::ContractError) -> Self {
        Self::new(IssueKind::Corrupt, e)
    }
}
impl From<serde_json::Error> for OperationIssue {
    fn from(e: serde_json::Error) -> Self {
        Self::new(IssueKind::Corrupt, e)
    }
}

fn safe_absolute(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path.as_os_str().len() > 4096
        || path.components().count() > 128
        || path
            .components()
            .any(|p| matches!(p, Component::ParentDir | Component::CurDir))
    {
        return Err(OperationIssue::new(
            IssueKind::UnsafePath,
            "an absolute non-traversing location is required",
        ));
    }
    #[cfg(windows)]
    for component in path.components() {
        if let Component::Normal(name) = component {
            safe_filename(name.to_str().ok_or_else(|| {
                OperationIssue::new(IssueKind::UnsafePath, "non-text path component")
            })?)?;
        }
        if let Component::Prefix(prefix) = component {
            if !matches!(
                prefix.kind(),
                std::path::Prefix::Disk(_)
                    | std::path::Prefix::VerbatimDisk(_)
                    | std::path::Prefix::UNC(_, _)
                    | std::path::Prefix::VerbatimUNC(_, _)
            ) {
                return Err(OperationIssue::new(
                    IssueKind::UnsafePath,
                    "device namespaces are not tool locations",
                ));
            }
        }
    }
    Ok(())
}

fn safe_filename(name: &str) -> Result<()> {
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    // Microsoft naming rules reserve these in every directory, including the
    // superscript digits and names followed by an extension.
    let numbered = stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"));
    let reserved = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || numbered
        .is_some_and(|n| ["1", "2", "3", "4", "5", "6", "7", "8", "9", "¹", "²", "³"].contains(&n));
    if name.is_empty()
        || name.ends_with(['.', ' '])
        || reserved
        || name.chars().any(|c| {
            c.is_control() || matches!(c, ':' | '\\' | '/' | '<' | '>' | '"' | '|' | '?' | '*')
        })
    {
        return Err(OperationIssue::new(
            IssueKind::UnsafePath,
            "choose an ordinary file name, without a device name or alternate stream",
        ));
    }
    Ok(())
}

pub(crate) struct Folder {
    path: PathBuf,
    directory: files::Directory,
}
impl Folder {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        safe_absolute(path)?;
        Ok(Self {
            path: path.into(),
            directory: files::Directory::open(path)?,
        })
    }
    pub(crate) fn ensure(path: &Path) -> Result<Self> {
        safe_absolute(path)?;
        let mut missing = Vec::new();
        let mut current = path;
        let mut directory = loop {
            match files::Directory::open(current) {
                Ok(directory) => break directory,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    missing.push(
                        current
                            .file_name()
                            .ok_or_else(|| {
                                OperationIssue::new(IssueKind::UnsafePath, "missing root")
                            })?
                            .to_os_string(),
                    );
                    current = current.parent().ok_or_else(|| {
                        OperationIssue::new(IssueKind::UnsafePath, "missing parent")
                    })?;
                }
                Err(e) => return Err(e.into()),
            }
        };
        for name in missing.into_iter().rev() {
            directory = match directory.create_child(&name) {
                Ok(child) => child,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => directory.child(&name)?,
                Err(e) => return Err(e.into()),
            };
        }
        let result = Self {
            path: path.into(),
            directory,
        };
        result.check()?;
        Ok(result)
    }
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
    pub(crate) fn check(&self) -> Result<()> {
        if !self.directory.same(&files::Directory::open(&self.path)?)? {
            return Err(OperationIssue::new(
                IssueKind::UnsafePath,
                "the selected directory was replaced",
            ));
        }
        Ok(())
    }
    pub(crate) fn read(&self, name: &str, limit: usize) -> Result<Vec<u8>> {
        self.check()?;
        safe_filename(name)?;
        if limit > MAX_LOCAL_BYTES {
            return Err(OperationIssue::new(IssueKind::Limit, "file bound"));
        }
        // This metadata is only for a useful diagnosis. The descriptor-relative
        // read below independently rechecks type, link count and actual length.
        let metadata = fs::symlink_metadata(self.path.join(name))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(OperationIssue::new(
                IssueKind::UnsafePath,
                "choose a regular local file",
            ));
        }
        if metadata.len() > limit as u64 {
            return Err(OperationIssue::new(
                IssueKind::Limit,
                "the selected file exceeds the byte limit",
            ));
        }
        let bytes = self.directory.read(name, limit)?;
        self.check()?;
        Ok(bytes)
    }
    pub(crate) fn publish(&self, name: &str, bytes: &[u8], replace: bool) -> Result<()> {
        self.check()?;
        safe_filename(name)?;
        if bytes.len() > MAX_LOCAL_BYTES {
            return Err(OperationIssue::new(IssueKind::Limit, "file bound"));
        }
        self.directory.publish(name, bytes, replace)?;
        self.check()
    }
    fn lock(&self) -> Result<fs::File> {
        self.check()?;
        Ok(self.directory.lock()?)
    }
    pub(crate) fn sync(&self) -> Result<()> {
        self.check()?;
        self.directory.sync()?;
        self.check()
    }
    pub(crate) fn names(&self, limit: usize) -> Result<Vec<String>> {
        self.check()?;
        let mut names = Vec::new();
        for entry in fs::read_dir(&self.path)? {
            if names.len() == limit {
                return Err(OperationIssue::new(
                    IssueKind::Limit,
                    "too many files in this backup location",
                ));
            }
            let name = entry?
                .file_name()
                .into_string()
                .map_err(|_| OperationIssue::new(IssueKind::UnsafePath, "non-text filename"))?;
            names.push(name);
        }
        self.check()?;
        Ok(names)
    }
}

pub(crate) struct SelectedFile {
    folder: Folder,
    name: String,
}
impl SelectedFile {
    pub(crate) fn check_parent(&self) -> Result<()> {
        self.folder.check()
    }
    pub(crate) fn new(path: &Path) -> Result<Self> {
        safe_absolute(path)?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| !n.is_empty())
            .ok_or_else(|| OperationIssue::new(IssueKind::UnsafePath, "choose a file name"))?
            .to_string();
        safe_filename(&name)?;
        let folder =
            Folder::open(path.parent().ok_or_else(|| {
                OperationIssue::new(IssueKind::UnsafePath, "missing file parent")
            })?)?;
        Ok(Self { folder, name })
    }
    pub(crate) fn read(&self, limit: usize) -> Result<Vec<u8>> {
        self.folder.read(&self.name, limit)
    }
    pub(crate) fn write_new(&self, bytes: &[u8]) -> Result<()> {
        let _lock = self.folder.lock()?;
        self.folder.publish(&self.name, bytes, false)
    }
}

pub struct ToolLocations {
    folder: Folder,
}
fn random_nonce() -> Result<[u8; 16]> {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut random = [0u8; 16];
    SystemRandom::new()
        .fill(&mut random)
        .map_err(|_| OperationIssue {
            kind: IssueKind::Unavailable,
            message: "A new tool identity could not be created safely.".into(),
            next_step: "Try again. Existing tools and backups were kept.".into(),
            detail: "system randomness unavailable".into(),
        })?;
    Ok(random)
}
impl ToolLocations {
    pub(crate) fn check(&self) -> Result<()> {
        self.folder.check()
    }
    pub fn default_location() -> Result<Self> {
        let root = default_root(std::env::consts::OS, |key| {
            std::env::var_os(key).map(PathBuf::from)
        })?;
        Self::create_default_at(&root)
    }
    pub(crate) fn create_default_at(path: &Path) -> Result<Self> {
        Ok(Self {
            folder: Folder::ensure(path)?,
        })
    }
    /// A deliberate folder selection never silently falls back to a default.
    pub fn chosen(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            folder: Folder::open(path.as_ref())?,
        })
    }
    pub fn path(&self) -> &Path {
        self.folder.path()
    }
    pub fn new_tool_path(&self) -> Result<PathBuf> {
        self.folder.check()?;
        let random = random_nonce()?;
        let suffix: String = random.iter().map(|b| format!("{b:02x}")).collect();
        // This is a suggestion, not a reservation. ProductStore's fresh-only
        // activation is still mandatory and detects a competing creation.
        Ok(self.path().join(format!("tool-{suffix}")))
    }
}
fn default_root(os: &str, env: impl Fn(&str) -> Option<PathBuf>) -> Result<PathBuf> {
    let home = || env("HOME").or_else(|| env("USERPROFILE"));
    let base = match os {
        "windows" => env("LOCALAPPDATA").or_else(|| home().map(|p| p.join("AppData/Local"))),
        "macos" => home().map(|p| p.join("Library/Application Support")),
        "linux" => env("XDG_DATA_HOME").or_else(|| home().map(|p| p.join(".local/share"))),
        _ => {
            return Err(OperationIssue::new(
                IssueKind::Unsupported,
                "no private default for this platform",
            ))
        }
    }
    .ok_or_else(|| {
        OperationIssue::new(
            IssueKind::Unavailable,
            "no private profile location is configured",
        )
    })?;
    safe_absolute(&base)?;
    Ok(base.join("GitManager/generated-tools"))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolIdentity {
    pub project_id: String,
    pub first_program: Digest,
}
impl ToolIdentity {
    pub fn from_snapshot(snapshot: &ProjectSnapshot) -> Result<Self> {
        snapshot.validate()?;
        Ok(Self {
            project_id: snapshot.data.project_id.clone(),
            first_program: canonical_digest(IdentityDomain::Source, &snapshot.programs[0])?,
        })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecentTool {
    pub id: Digest,
    pub identity: ToolIdentity,
    pub label: String,
    pub path: PathBuf,
    pub last_opened_unix_ms: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecentAvailability {
    Located,
    Missing,
    Replaced,
    Unverified,
}
#[derive(Clone, Debug)]
pub struct RecentEntry {
    pub tool: RecentTool,
    pub availability: RecentAvailability,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecentIndex {
    magic: String,
    version: u32,
    entries: Vec<RecentTool>,
}
/// The tool exists and is usable even if updating the separate recent list
/// failed. Callers must surface that warning without retrying fresh creation.
pub struct CreatedTool {
    pub store: ProductStore,
    pub registration: Result<Digest>,
}
pub struct RecentTools {
    folder: Folder,
}
impl RecentTools {
    /// Existing application metadata folder, separate from project schemas.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            folder: Folder::open(path.as_ref())?,
        })
    }
    fn read(&self) -> Result<RecentIndex> {
        // Do not mistake inaccessible/corrupt metadata for an empty history.
        self.folder.check()?;
        let bytes = match self
            .folder
            .directory
            .read("recent-tools.json", MAX_RECENT_BYTES)
        {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Ok(RecentIndex {
                    magic: "gitmanager.recent-tools".into(),
                    version: 1,
                    entries: vec![],
                })
            }
            Err(e) => return Err(e.into()),
        };
        self.folder.check()?;
        let index: RecentIndex = serde_json::from_slice(&bytes)?;
        if index.version != 1 {
            return Err(OperationIssue::new(
                IssueKind::Unsupported,
                "recent metadata version",
            ));
        }
        if index.magic != "gitmanager.recent-tools"
            || canonical_bytes(&index)? != bytes
            || index.entries.len() > MAX_RECENT_TOOLS
        {
            return Err(OperationIssue::new(
                IssueKind::Corrupt,
                "recent metadata integrity or count",
            ));
        }
        let mut ids = BTreeSet::new();
        let mut paths = BTreeSet::new();
        for entry in &index.entries {
            safe_absolute(&entry.path)?;
            if !ids.insert(&entry.id)
                || !paths.insert(&entry.path)
                || !valid_id(&entry.identity.project_id)
                || entry.label.len() > 4096
            {
                return Err(OperationIssue::new(
                    IssueKind::Corrupt,
                    "invalid recent entry",
                ));
            }
        }
        Ok(index)
    }
    fn write(&self, index: &RecentIndex) -> Result<()> {
        let bytes = canonical_bytes(index)?;
        if bytes.len() > MAX_RECENT_BYTES || index.entries.len() > MAX_RECENT_TOOLS {
            return Err(OperationIssue {
                kind: IssueKind::Limit,
                message: "The recent-tool list is full. Your tool and existing entries were kept."
                    .into(),
                next_step:
                    "You can keep using the open tool or use the folder picker to reopen it.".into(),
                detail: "recent metadata exceeds its entry or byte limit".into(),
            });
        }
        self.folder.publish("recent-tools.json", &bytes, true)
    }
    fn inspect(path: &Path) -> Result<(ToolIdentity, String)> {
        Folder::open(path)?.check()?;
        let snapshot = ProductStore::open(path)?.load()?;
        Ok((
            ToolIdentity::from_snapshot(&snapshot)?,
            snapshot.program()?.program.label.clone(),
        ))
    }
    fn same_location(previous: &Path, path: &Path, target: &Folder) -> Result<bool> {
        if previous == path {
            return Ok(true);
        }
        // Ask the actual filesystem about case/Unicode aliases. An old path
        // which cannot safely open cannot be used as this target by our opener.
        if let Ok(previous) = Folder::open(previous) {
            let same = previous.directory.same(&target.directory)?;
            previous.check()?;
            target.check()?;
            Ok(same)
        } else {
            Ok(false)
        }
    }
    pub fn remember(&self, path: &Path, opened_unix_ms: u64) -> Result<Digest> {
        let (identity, label) = Self::inspect(path)?;
        let target = Folder::open(path)?;
        let _lock = self.folder.lock()?;
        let mut index = self.read()?;
        let mut matching = None;
        for (position, entry) in index.entries.iter().enumerate() {
            if Self::same_location(&entry.path, path, &target)?
                && matching.replace(position).is_some()
            {
                return Err(OperationIssue::new(
                    IssueKind::Collision,
                    "multiple recent instances identify this directory",
                ));
            }
        }
        let id = if let Some(position) = matching {
            let entry = &mut index.entries[position];
            if entry.identity != identity {
                return Err(OperationIssue::new(
                    IssueKind::Collision,
                    "recent path now identifies another tool",
                ));
            }
            entry.label = label;
            entry.last_opened_unix_ms = opened_unix_ms;
            entry.id.clone()
        } else {
            Self::append_new(&mut index, path, identity, label, opened_unix_ms)?
        };
        target.check()?;
        self.write(&index)?;
        Ok(id)
    }
    fn append_new(
        index: &mut RecentIndex,
        path: &Path,
        identity: ToolIdentity,
        label: String,
        opened_unix_ms: u64,
    ) -> Result<Digest> {
        // Paths can be reused after relocation. A new instance needs a
        // fresh ID so it cannot share the relocated tool's backup shelf.
        let mut available = None;
        for _ in 0..8 {
            let candidate =
                canonical_digest(IdentityDomain::Evidence, &(&identity, random_nonce()?))?;
            if index.entries.iter().all(|entry| entry.id != candidate) {
                available = Some(candidate);
                break;
            }
        }
        let id = available.ok_or_else(|| OperationIssue {
            kind: IssueKind::Unavailable,
            message: "A separate tool identity could not be allocated.".into(),
            next_step: "Try again. Existing tools and backups were kept.".into(),
            detail: "fresh identifiers collided with existing recent entries".into(),
        })?;
        index.entries.push(RecentTool {
            id: id.clone(),
            identity,
            label,
            path: path.into(),
            last_opened_unix_ms: opened_unix_ms,
        });
        Ok(id)
    }
    /// Serialize the destination check, fresh creation and recent registration.
    /// In particular, a missing old path still belongs to its listed instance.
    pub(crate) fn create_and_remember(
        &self,
        path: &Path,
        opened_unix_ms: u64,
        create: impl FnOnce(&dyn Fn() -> Result<()>) -> Result<ProductStore>,
    ) -> Result<CreatedTool> {
        let destination = SelectedFile::new(path)?;
        let _lock = self.folder.lock()?;
        let mut index = self.read()?;
        if index.entries.iter().any(|entry| entry.path == path) {
            return Err(OperationIssue {
                kind: IssueKind::Collision,
                message: "That location is already listed as another tool instance.".into(),
                next_step: "Choose a new folder for this recovery, or locate the moved original first. Existing entries and backups were kept.".into(),
                detail: "fresh recovery destination is already in recent tools".into(),
            });
        }
        // The filesystem, not string case/Unicode guesses, decides aliases.
        // The callback runs after fresh mkdir but before CURRENT publication.
        let verify_instance = || {
            self.folder.check()?;
            destination.check_parent()?;
            let target = Folder::open(path)?;
            for entry in &index.entries {
                if Self::same_location(&entry.path, path, &target)? {
                    return Err(OperationIssue::new(
                        IssueKind::Collision,
                        "the recovery destination aliases an existing recent-tool location",
                    ));
                }
            }
            target.check()
        };
        destination.check_parent()?;
        let store = create(&verify_instance)?;
        let instance_check = verify_instance();
        let registration = (|| {
            instance_check?;
            let (identity, label) = Self::inspect(path)?;
            let id = Self::append_new(&mut index, path, identity, label, opened_unix_ms)?;
            self.write(&index)?;
            Ok(id)
        })().map_err(|mut issue: OperationIssue| {
            issue.message = "The recovered tool was saved, but its recent-tool entry could not be confirmed.".into();
            issue.next_step = "Keep using this tool, or reopen its selected folder. Do not repeat recovery into the same folder.".into();
            issue
        });
        Ok(CreatedTool {
            store,
            registration,
        })
    }
    pub fn relocate(&self, id: &Digest, path: &Path, opened_unix_ms: u64) -> Result<()> {
        let (identity, label) = Self::inspect(path)?;
        let target = Folder::open(path)?;
        let _lock = self.folder.lock()?;
        let mut index = self.read()?;
        for entry in &index.entries {
            if &entry.id != id && Self::same_location(&entry.path, path, &target)? {
                return Err(OperationIssue::new(
                    IssueKind::Collision,
                    "location is already listed",
                ));
            }
        }
        let entry = index
            .entries
            .iter_mut()
            .find(|e| &e.id == id)
            .ok_or_else(|| OperationIssue::new(IssueKind::Unavailable, "recent entry not found"))?;
        if identity != entry.identity {
            return Err(OperationIssue::new(
                IssueKind::Collision,
                "the selected folder contains another tool",
            ));
        }
        entry.path = path.into();
        entry.label = label;
        entry.last_opened_unix_ms = opened_unix_ms;
        target.check()?;
        self.write(&index)
    }
    pub fn list(&self) -> Result<Vec<RecentEntry>> {
        let mut entries = self.read()?.entries;
        entries.sort_by(|a, b| {
            b.last_opened_unix_ms
                .cmp(&a.last_opened_unix_ms)
                .then(a.id.cmp(&b.id))
        });
        Ok(entries.into_iter().map(|tool| {
            let availability = match Self::inspect(&tool.path) {
                Ok((identity, _)) if identity == tool.identity => RecentAvailability::Located,
                Ok(_) => RecentAvailability::Replaced,
                Err(_) if matches!(fs::symlink_metadata(&tool.path), Err(e) if e.kind() == io::ErrorKind::NotFound) => RecentAvailability::Missing,
                Err(_) => RecentAvailability::Unverified,
            };
            RecentEntry { tool, availability }
        }).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_are_profile_local_and_invalid_configuration_is_not_ignored() {
        let root = if cfg!(windows) {
            PathBuf::from(r"C:\Users\fixture")
        } else {
            PathBuf::from("/home/fixture")
        };
        for (os, relative) in [
            ("linux", ".local/share"),
            ("macos", "Library/Application Support"),
            ("windows", "AppData/Local"),
        ] {
            assert_eq!(
                default_root(os, |key| (key == "HOME").then(|| root.clone())).unwrap(),
                root.join(relative).join("GitManager/generated-tools")
            );
        }
        assert!(default_root("linux", |key| (key == "XDG_DATA_HOME")
            .then(|| PathBuf::from("relative")))
        .is_err());
        assert!(default_root("linux", |_| None).is_err());
        assert!(default_root("unknown", |_| None).is_err());
    }
}
