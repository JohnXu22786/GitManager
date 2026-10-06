//! Verified local recovery helpers. These never roll behavior back on current
//! data. The controller owns automatic checkpoint hooks and explicit recovery
//! confirmation; its registration is intentionally separate from this module.
use crate::product_contract::{
    canonical_bytes, canonical_digest, Digest, IdentityDomain, RuntimeLimits,
};
use crate::product_decisions::{validate_bundle, IntentArchive, IntentionBundle};
use crate::product_locations::{
    CreatedTool, Folder, IssueKind, OperationIssue, RecentTools, Result, SelectedFile,
    ToolIdentity, ToolLocations,
};
use crate::product_runtime::LocalRuntime;
use crate::product_store::{ProductStore, ProjectSnapshot, StoreError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MAX_BACKUP_BYTES: usize = 64 * 1024 * 1024;
const MAX_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;
const MAX_SCAN_FILES: usize = 4096;
const MAX_AUTOMATIC_CHECKS: usize = 64;
const MAGIC: &str = "gitmanager.generated-tool-backup";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    snapshot: ProjectSnapshot,
    intentions: IntentionBundle,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    magic: String,
    version: u32,
    digest: Digest,
    payload: Payload,
}

/// Constructed only after format, checksum, snapshot/runtime and exact
/// intention-object reachability checks. This proves integrity, not authorship
/// or that imported historical checks authorize a new behavior adoption.
#[derive(Clone, Debug)]
pub struct VerifiedBackup {
    envelope: Envelope,
}
#[derive(Clone, Debug)]
pub struct BackupSummary {
    pub digest: Digest,
    pub identity: ToolIdentity,
    pub label: String,
    pub revision: u64,
    pub records: usize,
    pub events: usize,
    pub outputs: usize,
    pub decisions: usize,
    pub intention_objects: usize,
}
impl VerifiedBackup {
    pub fn capture(store: &ProductStore) -> Result<Self> {
        // CURRENT selects one immutable committed snapshot. Do not enumerate
        // the project directory: orphan/interrupted writes are not commits.
        let snapshot = store.load()?;
        let intentions = IntentArchive::new(store.clone())
            .export_for(&snapshot)
            .map_err(|e| OperationIssue::new(IssueKind::Corrupt, e))?;
        let payload = Payload {
            snapshot,
            intentions,
        };
        let digest = canonical_digest(IdentityDomain::Evidence, &payload)?;
        let backup = Self {
            envelope: Envelope {
                magic: MAGIC.into(),
                version: 1,
                digest,
                payload,
            },
        };
        backup.validate()?;
        Ok(backup)
    }
    fn validate(&self) -> Result<()> {
        let envelope = &self.envelope;
        if envelope.magic != MAGIC {
            return Err(OperationIssue::new(
                IssueKind::Corrupt,
                "not a generated-tool backup",
            ));
        }
        if envelope.version != 1 {
            return Err(OperationIssue::new(
                IssueKind::Unsupported,
                "backup format version",
            ));
        }
        if canonical_digest(IdentityDomain::Evidence, &envelope.payload)? != envelope.digest {
            return Err(OperationIssue::new(
                IssueKind::Corrupt,
                "backup checksum mismatch",
            ));
        }
        let snapshot = self.snapshot();
        if canonical_bytes(snapshot)?.len() > MAX_SNAPSHOT_BYTES
            || canonical_bytes(envelope)?.len() > MAX_BACKUP_BYTES
        {
            return Err(OperationIssue::new(
                IssueKind::Limit,
                "backup or snapshot byte limit",
            ));
        }
        snapshot.validate()?;
        validate_bundle(snapshot, &envelope.payload.intentions)
            .map_err(|e| OperationIssue::new(IssueKind::Corrupt, e))?;
        LocalRuntime::default()
            .resume(
                snapshot.program()?,
                &snapshot.data,
                &snapshot.session,
                snapshot.clock_day,
                0,
                RuntimeLimits::default(),
                &snapshot.artifacts,
            )
            .map_err(|e| OperationIssue::new(IssueKind::Incompatible, format!("{e:?}")))?;
        Ok(())
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_BACKUP_BYTES {
            return Err(OperationIssue::new(
                IssueKind::Limit,
                "backup intake byte limit",
            ));
        }
        let backup = Self {
            envelope: serde_json::from_slice(bytes)?,
        };
        backup.validate()?;
        // Exact canonical equality also rejects duplicate map members, even
        // in nested maps for which a typed JSON decoder would retain one value.
        if canonical_bytes(&backup.envelope)? != bytes {
            return Err(OperationIssue::new(
                IssueKind::Corrupt,
                "noncanonical or duplicate backup content",
            ));
        }
        Ok(backup)
    }
    pub fn read(path: &Path) -> Result<Self> {
        Self::from_bytes(&SelectedFile::new(path)?.read(MAX_BACKUP_BYTES)?)
    }
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        self.validate()?;
        Ok(canonical_bytes(&self.envelope)?)
    }
    pub fn snapshot(&self) -> &ProjectSnapshot {
        &self.envelope.payload.snapshot
    }
    pub fn digest(&self) -> &Digest {
        &self.envelope.digest
    }
    pub fn summary(&self) -> Result<BackupSummary> {
        let snapshot = self.snapshot();
        Ok(BackupSummary {
            digest: self.digest().clone(),
            identity: ToolIdentity::from_snapshot(snapshot)?,
            label: snapshot.program()?.program.label.clone(),
            revision: snapshot.revision,
            records: snapshot.data.records.len(),
            events: snapshot.data.events.len(),
            outputs: snapshot.artifacts.len(),
            decisions: snapshot.decisions.decisions.len(),
            intention_objects: self.envelope.payload.intentions.object_count(),
        })
    }
    /// User-selected export. Existing files are always preserved, even if their
    /// bytes happen to match. No compression or archive path extraction occurs.
    pub fn export_new(&self, path: &Path) -> Result<BackupSummary> {
        let file = SelectedFile::new(path)?;
        file.write_new(&self.to_bytes()?)?;
        let verified = Self::from_bytes(&file.read(MAX_BACKUP_BYTES)?)?;
        if verified.digest() != self.digest() {
            return Err(OperationIssue::new(
                IssueKind::Corrupt,
                "export readback changed",
            ));
        }
        verified.summary()
    }
    /// A fresh, separate target only. This cannot overwrite current data or
    /// serve as the current-data behavior-withdrawal operation.
    pub fn recover_tool(
        &self,
        path: &Path,
        recent: &RecentTools,
        opened_unix_ms: u64,
    ) -> Result<CreatedTool> {
        self.validate()?;
        recent.create_and_remember(path, opened_unix_ms, |verify_instance| {
            self.recover_checked(path, verify_instance)
        })
    }
    // Low-level component boundary. Ordinary recovery uses recover_tool so a
    // missing recent path cannot inherit the original instance's checkpoints.
    pub(crate) fn recover_new(&self, path: &Path) -> Result<ProductStore> {
        self.recover_checked(path, &|| Ok(()))
    }
    pub(crate) fn recover_checked(
        &self,
        path: &Path,
        verify_instance: &dyn Fn() -> Result<()>,
    ) -> Result<ProductStore> {
        self.validate()?;
        let destination = SelectedFile::new(path)?;
        let payload = &self.envelope.payload;
        let store = ProductStore::create_recovered_with(path, &payload.snapshot, |fresh| {
            destination
                .check_parent()
                .map_err(|e| StoreError::Conflict(e.to_string()))?;
            verify_instance().map_err(|e| StoreError::Conflict(e.to_string()))?;
            let archive = IntentArchive::new(fresh.clone());
            // The archive restores and re-reads the exact reachable objects before
            // returning; CURRENT still does not exist at this point.
            archive
                .restore_for(&payload.snapshot, &payload.intentions)
                .map_err(|e| StoreError::Corrupt(e.to_string()))?;
            destination
                .check_parent()
                .map_err(|e| StoreError::Conflict(e.to_string()))?;
            verify_instance().map_err(|e| StoreError::Conflict(e.to_string()))
        })?;
        destination.check_parent()?;
        if store.load()? != payload.snapshot {
            return Err(OperationIssue::new(
                IssueKind::Corrupt,
                "recovered snapshot differs",
            ));
        }
        Ok(store)
    }
}

pub struct OpenedTool {
    pub store: ProductStore,
    pub snapshot: ProjectSnapshot,
    pub summary: BackupSummary,
}
/// Local and offline. Recent-tool callers supply the saved identity; a
/// deliberately selected new folder can be inspected without a prior identity.
pub fn open_verified(path: &Path, expected: Option<&ToolIdentity>) -> Result<OpenedTool> {
    let folder = Folder::open(path)?;
    let store = ProductStore::open(path)?;
    let backup = VerifiedBackup::capture(&store)?;
    let summary = backup.summary()?;
    if expected.is_some_and(|expected| summary.identity != *expected) {
        return Err(OperationIssue::new(
            IssueKind::Collision,
            "this folder identifies a different tool",
        ));
    }
    folder.check()?;
    Ok(OpenedTool {
        store,
        snapshot: backup.snapshot().clone(),
        summary,
    })
}

pub struct CheckpointShelf {
    folder: Folder,
    identity: ToolIdentity,
}
#[derive(Clone, Debug)]
pub struct CheckpointReceipt {
    pub path: PathBuf,
    pub digest: Digest,
    pub summary: BackupSummary,
}
pub struct CheckpointSearch {
    pub recovery: Option<VerifiedBackup>,
    pub issues: Vec<OperationIssue>,
}
impl CheckpointShelf {
    /// `instance` is the stable RecentTool.id, retained during deliberate
    /// relocation. Separate recovered tools get separate recent IDs/shelves,
    /// even though exact recovery preserves their original project/record IDs.
    pub fn for_tool(
        locations: &ToolLocations,
        identity: &ToolIdentity,
        instance: &Digest,
    ) -> Result<Self> {
        locations.check()?;
        let key = canonical_digest(IdentityDomain::Evidence, &(identity, instance))?;
        let folder = Folder::ensure(&locations.path().join("checkpoints").join(key.as_str()))?;
        locations.check()?;
        Ok(Self {
            folder,
            identity: identity.clone(),
        })
    }
    pub fn path(&self) -> &Path {
        self.folder.path()
    }
    fn name(backup: &VerifiedBackup) -> String {
        format!(
            "checkpoint-{:020}-{}.gmbak",
            backup.snapshot().revision,
            backup.digest().as_str()
        )
    }
    /// Call after opening and after each successful committed save/adoption,
    /// and before a risky operation. A failure is a backup warning, never an
    /// instruction to undo an already committed save or delete older backups.
    pub fn capture(&self, store: &ProductStore) -> Result<CheckpointReceipt> {
        let backup = VerifiedBackup::capture(store)?;
        let summary = backup.summary()?;
        if summary.identity != self.identity {
            return Err(OperationIssue::new(
                IssueKind::Collision,
                "checkpoint tool identity mismatch",
            ));
        }
        let name = Self::name(&backup);
        match self.folder.publish(&name, &backup.to_bytes()?, false) {
            Ok(()) => (),
            Err(e) if e.kind == IssueKind::Collision => (),
            Err(e) => return Err(e),
        }
        let actual = VerifiedBackup::from_bytes(&self.folder.read(&name, MAX_BACKUP_BYTES)?)?;
        if actual.digest() != backup.digest() {
            return Err(OperationIssue::new(
                IssueKind::Corrupt,
                "checkpoint readback mismatch",
            ));
        }
        // Also finish the sync on an idempotent retry after interrupted
        // publication. No mutable checkpoint index can advertise partial data.
        self.folder.sync()?;
        Ok(CheckpointReceipt {
            path: self.path().join(name),
            digest: summary.digest.clone(),
            summary,
        })
    }
    pub fn newest_verified(&self) -> Result<CheckpointSearch> {
        let mut names: Vec<_> = self
            .folder
            .names(MAX_SCAN_FILES)?
            .into_iter()
            .filter(|name| {
                name.is_ascii()
                    && name.len() == 102
                    && name.starts_with("checkpoint-")
                    && name.ends_with(".gmbak")
                    && name[11..31].bytes().all(|b| b.is_ascii_digit())
                    && &name[31..32] == "-"
                    && Digest::try_from(name[32..96].to_string()).is_ok()
            })
            .collect();
        names.sort_by(|a, b| b.cmp(a));
        let truncated = names.len() > MAX_AUTOMATIC_CHECKS;
        let mut issues = Vec::new();
        for name in names.into_iter().take(MAX_AUTOMATIC_CHECKS) {
            let candidate = self
                .folder
                .read(&name, MAX_BACKUP_BYTES)
                .and_then(|bytes| VerifiedBackup::from_bytes(&bytes));
            match candidate {
                Ok(backup)
                    if Self::name(&backup) == name
                        && backup.summary()?.identity == self.identity =>
                {
                    return Ok(CheckpointSearch {
                        recovery: Some(backup),
                        issues,
                    });
                }
                Ok(_) => issues.push(OperationIssue::new(
                    IssueKind::Corrupt,
                    format!("checkpoint identity mismatch: {name}"),
                )),
                Err(error) => issues.push(error),
            }
        }
        if truncated {
            issues.push(OperationIssue::new(
                IssueKind::Limit,
                "older checkpoints remain unchecked; choose a backup file to verify it",
            ));
        }
        Ok(CheckpointSearch {
            recovery: None,
            issues,
        })
    }
}

pub struct DoctorReport {
    pub current: Option<BackupSummary>,
    pub issue: Option<OperationIssue>,
    pub recovery: Option<VerifiedBackup>,
    pub checkpoint_issues: Vec<OperationIssue>,
}
/// Read-only diagnosis. A corrupt CURRENT/object is never silently repaired.
/// A displayed recovery option contains fully revalidated checkpoint bytes.
pub fn doctor(path: &Path, expected: &ToolIdentity, shelf: &CheckpointShelf) -> DoctorReport {
    let current = open_verified(path, Some(expected)).map(|opened| opened.summary);
    match current {
        Ok(current) => DoctorReport {
            current: Some(current),
            issue: None,
            recovery: None,
            checkpoint_issues: vec![],
        },
        Err(issue) => {
            let search = if expected != &shelf.identity {
                Err(OperationIssue::new(
                    IssueKind::Collision,
                    "checkpoint shelf belongs to another tool",
                ))
            } else {
                shelf.newest_verified()
            };
            match search {
                Ok(search) => DoctorReport {
                    current: None,
                    issue: Some(issue),
                    recovery: search.recovery,
                    checkpoint_issues: search.issues,
                },
                Err(error) => DoctorReport {
                    current: None,
                    issue: Some(issue),
                    recovery: None,
                    checkpoint_issues: vec![error],
                },
            }
        }
    }
}
