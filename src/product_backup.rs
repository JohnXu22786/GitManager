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
        // export_for just validated this exact owned snapshot and every
        // reachable bundle object with the same default execution context.
        // Neither value is exposed or mutated before becoming this envelope.
        backup.validate_envelope()?;
        backup.validate_runtime()?;
        Ok(backup)
    }
    fn validate(&self) -> Result<()> {
        self.validate_envelope()?;
        let snapshot = self.snapshot();
        snapshot.validate()?;
        validate_bundle(snapshot, &self.envelope.payload.intentions)
            .map_err(|e| OperationIssue::new(IssueKind::Corrupt, e))?;
        self.validate_runtime()
    }
    fn validate_envelope(&self) -> Result<()> {
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
        Ok(())
    }
    fn validate_runtime(&self) -> Result<()> {
        let snapshot = self.snapshot();
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
        let header: serde_json::Value = serde_json::from_slice(bytes)?;
        if header
            .get("payload")
            .and_then(|p| p.get("snapshot"))
            .and_then(|s| s.get("version"))
            .and_then(|v| v.as_u64())
            .is_some_and(|version| version != 1 && version != 2)
        {
            return Err(OperationIssue::new(
                IssueKind::Unsupported,
                "unsupported generated-project snapshot version",
            ));
        }
        if header
            .get("payload")
            .and_then(|p| p.get("snapshot"))
            .and_then(|s| s.get("version"))
            .and_then(|v| v.as_u64())
            == Some(1)
        {
            LegacyBackup::from_bytes(bytes)?;
            return Err(OperationIssue::new(
                IssueKind::UpgradeRequired,
                "legacy backup requires explicit fresh-destination upgrade",
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
        // Both constructors fully validate this private, immutable envelope.
        // Serialization cannot change its snapshot or reachable object bytes.
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
            identity: ToolIdentity {
                project_id: snapshot.data.project_id.clone(),
                first_program: canonical_digest(IdentityDomain::Source, &snapshot.programs[0])?,
            },
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
    checkpoint: Option<OpenedCheckpoint>,
}
// A single installation handoff, never retained by the daily-work controller.
// Its private backup has already passed every cold validation check.
struct OpenedCheckpoint {
    backup: VerifiedBackup,
    folder: Folder,
    current: Vec<u8>,
    object: Vec<u8>,
}
impl OpenedCheckpoint {
    fn still_current(&self, store: &ProductStore) -> Result<bool> {
        let snapshot = self.backup.snapshot();
        let name = format!(
            "object-{}.json",
            self.backup
                .envelope
                .payload
                .intentions
                .snapshot_digest()
                .as_str()
        );
        if self.folder.read("CURRENT", 16 * 1024)? != self.current
            || self.folder.read(&name, MAX_SNAPSHOT_BYTES)? != self.object
            || self.object != canonical_bytes(snapshot)?
            // load also verifies the pinned directory, complete current
            // object, runtime/driver/compiler identity and default limits.
            || store.load()? != *snapshot
            || !self.backup.envelope.payload.intentions
                .matches_store_objects(store)
                .map_err(|e| OperationIssue::new(IssueKind::Corrupt, e))?
        {
            return Ok(false);
        }
        // Fence the intention reads against a concurrent committed change.
        Ok(store.load()? == *snapshot
            && self.folder.read("CURRENT", 16 * 1024)? == self.current
            && self.folder.read(&name, MAX_SNAPSHOT_BYTES)? == self.object)
    }
}
impl OpenedTool {
    pub(crate) fn checkpoint(&mut self, shelf: &CheckpointShelf) -> Result<CheckpointReceipt> {
        // Consume on every attempt, including errors. A later action always
        // captures and validates again; corruption cannot hide behind a cache.
        if let Some(opened) = self.checkpoint.take() {
            if opened.still_current(&self.store).unwrap_or(false) {
                return shelf.publish_backup(&opened.backup);
            }
        }
        // Changed bytes or any freshness-read error must take the full
        // validation path. Its errors are reported as normal backup warnings.
        shelf.capture(&self.store)
    }
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
    let current = folder.read("CURRENT", 16 * 1024)?;
    let object = folder.read(
        &format!(
            "object-{}.json",
            backup
                .envelope
                .payload
                .intentions
                .snapshot_digest()
                .as_str()
        ),
        MAX_SNAPSHOT_BYTES,
    )?;
    folder.check()?;
    Ok(OpenedTool {
        store,
        snapshot: backup.snapshot().clone(),
        summary,
        checkpoint: Some(OpenedCheckpoint {
            backup,
            folder,
            current,
            object,
        }),
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
        self.publish_backup(&backup)
    }
    fn publish_backup(&self, backup: &VerifiedBackup) -> Result<CheckpointReceipt> {
        let summary = backup.summary()?;
        if summary.identity != self.identity {
            return Err(OperationIssue::new(
                IssueKind::Collision,
                "checkpoint tool identity mismatch",
            ));
        }
        let name = Self::name(backup);
        let bytes = backup.to_bytes()?;
        match self.folder.publish(&name, &bytes, false) {
            Ok(()) => (),
            Err(e) if e.kind == IssueKind::Collision => (),
            Err(e) => return Err(e),
        }
        // This is still a fresh bounded, safe-path read. Exact equality to the
        // already validated canonical bytes preserves every intake check; a
        // changed collision is rejected rather than trusted by name or digest.
        if self.folder.read(&name, MAX_BACKUP_BYTES)? != bytes {
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

/// The controller presents this gate on launch/open before enabling daily edits.
pub enum OpenGate {
    Ready(OpenedTool),
    UpgradeRequired(crate::product_store::UpgradeSummary),
}
pub fn inspect_open(path: &Path, expected: Option<&ToolIdentity>) -> Result<OpenGate> {
    let store = ProductStore::open(path)?;
    let summary = store.inspect_upgrade()?;
    verify_upgrade_identity(&summary, expected)?;
    if summary.restart_required {
        Ok(OpenGate::UpgradeRequired(summary))
    } else {
        Ok(OpenGate::Ready(open_verified(path, expected)?))
    }
}
fn verify_upgrade_identity(
    summary: &crate::product_store::UpgradeSummary,
    expected: Option<&ToolIdentity>,
) -> Result<()> {
    if expected.is_some_and(|identity| {
        identity.project_id != summary.project_id || identity.first_program != summary.first_program
    }) {
        return Err(OperationIssue::new(
            IssueKind::Collision,
            "upgrade folder belongs to another tool",
        ));
    }
    Ok(())
}
/// Explicit, offline upgrade with visible progress supplied by the controller.
/// Completion requires restart/reopen. No saved backup is deleted.
pub fn upgrade_open_verified<F>(
    path: &Path,
    expected: Option<&ToolIdentity>,
    progress: F,
) -> Result<crate::product_store::UpgradeSummary>
where
    F: FnMut(crate::product_store::UpgradeProgress),
{
    let store = ProductStore::open(path)?;
    verify_upgrade_identity(&store.inspect_upgrade()?, expected)?;
    let archive = IntentArchive::new(store.clone());
    let result = store.upgrade_generated_project_verified(progress, |target| {
        let identity =
            ToolIdentity::from_snapshot(target).map_err(|e| StoreError::Corrupt(e.to_string()))?;
        if expected.is_some_and(|expected| *expected != identity) {
            return Err(StoreError::Conflict("upgrade identity changed".into()));
        }
        archive
            .export_for(target)
            .map_err(|e| StoreError::Corrupt(e.to_string()))?;
        Ok(())
    })?;
    verify_upgrade_identity(&result, expected)?;
    Ok(result)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyPayload {
    snapshot: serde_json::Value,
    intentions: IntentionBundle,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyEnvelope {
    magic: String,
    version: u32,
    digest: Digest,
    payload: LegacyPayload,
}
/// Verified read-only intake. Upgrade/recovery always chooses a fresh folder;
/// the original backup is never rewritten or used as an ordinary writable store.
pub struct LegacyBackup {
    original: Vec<u8>,
    upgrade: crate::product_store::LegacyUpgrade,
    intentions: IntentionBundle,
}
pub enum BackupIntake {
    Current(VerifiedBackup),
    UpgradeRequired(LegacyBackup),
}
pub fn inspect_backup(bytes: &[u8]) -> Result<BackupIntake> {
    if bytes.len() > MAX_BACKUP_BYTES {
        return Err(OperationIssue::new(IssueKind::Limit, "backup intake limit"));
    }
    let header: serde_json::Value = serde_json::from_slice(bytes)?;
    if header
        .get("payload")
        .and_then(|p| p.get("snapshot"))
        .and_then(|s| s.get("version"))
        .and_then(|v| v.as_u64())
        == Some(1)
    {
        Ok(BackupIntake::UpgradeRequired(LegacyBackup::from_bytes(
            bytes,
        )?))
    } else {
        Ok(BackupIntake::Current(VerifiedBackup::from_bytes(bytes)?))
    }
}
impl LegacyBackup {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_BACKUP_BYTES {
            return Err(OperationIssue::new(
                IssueKind::Limit,
                "legacy backup intake limit",
            ));
        }
        let envelope: LegacyEnvelope = serde_json::from_slice(bytes)?;
        if envelope.magic != MAGIC || envelope.version != 1 {
            return Err(OperationIssue::new(
                IssueKind::Unsupported,
                "legacy backup format",
            ));
        }
        if canonical_bytes(&envelope)? != bytes
            || canonical_digest(IdentityDomain::Evidence, &envelope.payload)? != envelope.digest
        {
            return Err(OperationIssue::new(
                IssueKind::Corrupt,
                "legacy backup checksum/canonical content mismatch",
            ));
        }
        let upgrade = crate::product_store::LegacyUpgrade::decode(&canonical_bytes(
            &envelope.payload.snapshot,
        )?)?;
        let intentions = crate::product_decisions::upgrade_bundle_binding(
            &upgrade.original_digest,
            &upgrade.snapshot,
            &envelope.payload.intentions,
        )
        .map_err(|e| OperationIssue::new(IssueKind::Corrupt, e))?;
        Ok(Self {
            original: bytes.to_vec(),
            upgrade,
            intentions,
        })
    }
    pub fn original_bytes(&self) -> &[u8] {
        &self.original
    }
    /// On success, restart, open_verified the chosen path, then register that
    /// new instance. A failed pre-pointer attempt leaves only staged immutable
    /// files; the original backup and any existing tool remain untouched.
    pub fn upgrade_recover_new<P>(
        &self,
        path: &Path,
        progress: P,
    ) -> Result<crate::product_store::UpgradeSummary>
    where
        P: FnMut(crate::product_store::UpgradeProgress),
    {
        self.recover_inner(
            path,
            progress,
            #[cfg(test)]
            None,
        )
    }
    #[cfg(test)]
    pub fn upgrade_recover_with_fault<P>(
        &self,
        path: &Path,
        progress: P,
        fault: crate::product_store::FaultPoint,
    ) -> Result<crate::product_store::UpgradeSummary>
    where
        P: FnMut(crate::product_store::UpgradeProgress),
    {
        self.recover_inner(path, progress, Some(fault))
    }
    fn recover_inner<P>(
        &self,
        path: &Path,
        progress: P,
        #[cfg(test)] fault: Option<crate::product_store::FaultPoint>,
    ) -> Result<crate::product_store::UpgradeSummary>
    where
        P: FnMut(crate::product_store::UpgradeProgress),
    {
        let verified = Self::from_bytes(&self.original)?;
        Ok(ProductStore::recover_upgraded_with(
            path,
            &verified.upgrade,
            |store, snapshot| {
                IntentArchive::new(store.clone())
                    .restore_for(snapshot, &verified.intentions)
                    .map_err(|e| StoreError::Corrupt(e.to_string()))
            },
            progress,
            #[cfg(test)]
            fault,
        )?)
    }
}
