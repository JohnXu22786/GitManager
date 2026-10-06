//! Separate generated-project format with immutable snapshots and one atomic
//! activation pointer. Legacy order projects are never read or rewritten here.
mod files;
mod json;
pub mod scope;
mod upgrade;
use crate::product_contract::*;
use crate::product_runtime::{merged_data, LocalRuntime};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path};
use std::sync::Arc;
pub(crate) use upgrade::LegacyUpgrade;
pub use upgrade::{UpgradeProgress, UpgradeSummary};

const MAGIC: &str = "gitmanager.generated-project";
const FORMAT: u32 = 2;
const MAX_STORE_BYTES: usize = 16 * 1024 * 1024;
type Result<T> = std::result::Result<T, StoreError>;
#[derive(Debug)]
pub enum StoreError {
    Io(io::Error),
    Invalid(String),
    UnsupportedFormat(u32),
    UpgradeRequired,
    RestartRequired,
    Conflict(String),
    Incompatible(CompatibilityReport),
    Runtime(AdapterError),
    Corrupt(String),
    #[cfg(test)]
    Interrupted(FaultPoint),
}
impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for StoreError {}
impl From<io::Error> for StoreError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<ContractError> for StoreError {
    fn from(e: ContractError) -> Self {
        Self::Invalid(e.to_string())
    }
}
impl From<AdapterError> for StoreError {
    fn from(e: AdapterError) -> Self {
        Self::Runtime(e)
    }
}
impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> Self {
        Self::Corrupt(e.to_string())
    }
}
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultPoint {
    AfterObject,
    BeforePointer,
    AfterPointer,
    AfterUpgradeObject,
    AfterUpgradeJournal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationReceipt {
    pub operation: Id,
    pub request: Digest,
    pub revision: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdoptionReceipt {
    pub plan: AdoptionPlan,
    pub previous: Digest,
    pub active: Digest,
    pub revision: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectSnapshot {
    pub magic: String,
    pub version: u32,
    pub revision: u64,
    pub programs: Vec<CapturedProgram>,
    pub active_revision: Digest,
    pub data: DataSnapshot,
    pub session: SessionState,
    pub clock_day: i32,
    pub decisions: DecisionGraph,
    pub adoptions: Vec<AdoptionReceipt>,
    pub operations: BTreeMap<Id, OperationReceipt>,
    pub artifacts: Vec<LocalArtifact>,
    pub scope: scope::ScopeState,
}
fn revision(program: &CapturedProgram) -> std::result::Result<Digest, ContractError> {
    canonical_digest(IdentityDomain::Source, program)
}
impl ProjectSnapshot {
    pub fn program(&self) -> Result<&CapturedProgram> {
        self.programs
            .iter()
            .find(|p| revision(p).ok().as_ref() == Some(&self.active_revision))
            .ok_or_else(|| StoreError::Corrupt("active program revision missing".into()))
    }
    pub fn validate(&self) -> Result<()> {
        if self.magic != MAGIC {
            return Err(StoreError::Corrupt("not a generated-tool project".into()));
        }
        if self.version != FORMAT {
            return Err(StoreError::UnsupportedFormat(self.version));
        }
        if self.programs.is_empty()
            || self.programs.len() > MAX_ITEMS
            || self.adoptions.len() > MAX_ITEMS
            || self.operations.len() > MAX_COLLECTION
            || self.artifacts.len() > MAX_ITEMS
        {
            return Err(StoreError::Invalid(
                "project history size limit reached".into(),
            ));
        }
        self.data.validate()?;
        self.decisions.validate()?;
        validate_day(self.clock_day)?;
        let mut revisions = std::collections::BTreeSet::new();
        let mut digests = std::collections::BTreeSet::new();
        for program in &self.programs {
            program.validate()?;
            if program.binding.project_id != self.data.project_id
                || !revisions.insert(revision(program)?)
            {
                return Err(StoreError::Corrupt(
                    "duplicate or foreign program revision".into(),
                ));
            }
            digests.insert(program.artifact.program_digest.clone());
        }
        for record in &self.data.records {
            if !digests.contains(&record.created_program) {
                return Err(StoreError::Corrupt(
                    "record producer revision missing".into(),
                ));
            }
        }
        for event in &self.data.events {
            if !digests.contains(&event.program) {
                return Err(StoreError::Corrupt(
                    "event producer revision missing".into(),
                ));
            }
        }
        self.session.validate(&self.program()?.program)?;
        for (id, receipt) in &self.operations {
            if !valid_id(id)
                || id != &receipt.operation
                || receipt.revision == 0
                || receipt.revision > self.revision
            {
                return Err(StoreError::Corrupt("invalid operation receipt".into()));
            }
        }
        let mut last = 0;
        for adoption in &self.adoptions {
            adoption.plan.validate()?;
            if adoption.revision <= last
                || adoption.revision > self.revision
                || !revisions.contains(&adoption.previous)
                || !revisions.contains(&adoption.active)
            {
                return Err(StoreError::Corrupt("invalid adoption history".into()));
            }
            last = adoption.revision;
        }
        for artifact in &self.artifacts {
            artifact.validate()?;
        }
        let emitted: Vec<_> = self
            .data
            .events
            .iter()
            .flat_map(|e| e.outputs.iter())
            .collect();
        if emitted != self.artifacts.iter().map(|a| &a.digest).collect::<Vec<_>>() {
            return Err(StoreError::Corrupt(
                "artifact inventory differs from committed output history".into(),
            ));
        }

        if self.artifacts.iter().map(|a| a.bytes.len()).sum::<usize>() > MAX_OUTPUT_BYTES {
            return Err(StoreError::Invalid("retained output byte limit".into()));
        }
        scope::verify_snapshot(self)?;
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pointer {
    magic: String,
    version: u32,
    project_id: Id,
    revision: u64,
    object: Digest,
}
/// Prepared locally against exact current identities. This is a rehearsal, not
/// authorization: the host must obtain the user's choice before calling adopt.
#[derive(Clone, Debug)]
pub struct PreparedAdoption {
    plan: AdoptionPlan,
    expected_revision: u64,
    target: Digest,
}
impl PreparedAdoption {
    pub fn plan(&self) -> &AdoptionPlan {
        &self.plan
    }
}
#[derive(Clone)]
pub struct ProductStore {
    parent: Arc<files::Directory>,
    root: Arc<files::Directory>,
    name: std::ffi::OsString,
    upgrade_requires_reopen: Arc<std::sync::atomic::AtomicBool>,
}
impl ProductStore {
    fn location(path: &Path) -> Result<(files::Directory, std::ffi::OsString)> {
        if !path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(StoreError::Invalid(
                "choose an absolute project location without traversal".into(),
            ));
        }
        let name = path
            .file_name()
            .ok_or_else(|| StoreError::Invalid("project needs a directory name".into()))?
            .to_os_string();
        // Resolve the user-selected parent once, then pin it. The actual project
        // entry is opened without following symlinks.
        let parent = fs::canonicalize(
            path.parent()
                .ok_or_else(|| StoreError::Invalid("project needs a parent".into()))?,
        )?;
        Ok((files::Directory::open(&parent)?, name))
    }
    pub fn create(
        path: impl AsRef<Path>,
        program: &CapturedProgram,
        clock_day: i32,
    ) -> Result<Self> {
        program.validate()?;
        validate_day(clock_day)?;
        let snapshot = ProjectSnapshot {
            magic: MAGIC.into(),
            version: FORMAT,
            revision: 0,
            programs: vec![program.clone()],
            active_revision: revision(program)?,
            data: DataSnapshot::empty(&program.binding.project_id, &program.program)?,
            session: SessionState::initial(&program.program)?,
            clock_day,
            decisions: DecisionGraph {
                version: CONTRACT_VERSION,
                revision: 0,
                decisions: vec![],
            },
            adoptions: vec![],
            operations: BTreeMap::new(),
            artifacts: vec![],
            scope: scope::ScopeState::default(),
        };
        // Ordinary creation and recovery share the same fresh-only activation.
        Self::create_recovered(path, &snapshot)
    }
    /// Recover a complete snapshot into a new directory. This never replaces
    /// current work and is not a behavior rollback on current business data.
    /// Snapshots with intention references need `create_recovered_with` so the
    /// trusted host can verify and restore their immutable packages first.
    pub fn create_recovered(path: impl AsRef<Path>, snapshot: &ProjectSnapshot) -> Result<Self> {
        if !snapshot.decisions.decisions.is_empty()
            || snapshot
                .adoptions
                .iter()
                .any(|a| !a.plan.evidence.is_empty())
        {
            return Err(StoreError::Invalid(
                "this recovery needs its verified intention packages; use the complete backup"
                    .into(),
            ));
        }
        Self::create_recovered_with(path, snapshot, |_| Ok(()))
    }

    /// Trusted host-only recovery boundary. Validate the complete intention
    /// bundle before calling, then restore and verify it in `before_activate`.
    /// The callback receives a fresh, pinned store without a CURRENT pointer;
    /// it must not grant authority to imported check flags. Callback failure
    /// leaves an unactivated folder, never overwrites a destination and never
    /// deletes the original project or any backup.
    pub fn create_recovered_with<F>(
        path: impl AsRef<Path>,
        snapshot: &ProjectSnapshot,
        before_activate: F,
    ) -> Result<Self>
    where
        F: FnOnce(&ProductStore) -> Result<()>,
    {
        snapshot.validate()?;
        if canonical_bytes(snapshot)?.len() > MAX_STORE_BYTES {
            return Err(StoreError::Invalid(
                "recovery snapshot exceeds the byte limit".into(),
            ));
        }
        LocalRuntime::default().resume(
            snapshot.program()?,
            &snapshot.data,
            &snapshot.session,
            snapshot.clock_day,
            0,
            RuntimeLimits::default(),
            &snapshot.artifacts,
        )?;
        let (parent, name) = Self::location(path.as_ref())?;
        let root = parent.create_child(&name)?;
        let store = Self {
            parent: Arc::new(parent),
            root: Arc::new(root),
            name,
            upgrade_requires_reopen: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        let _lock = store.root.lock()?;
        store.require_unactivated()?;
        before_activate(&store)?;
        store.pinned()?;
        store.save(
            snapshot,
            false,
            #[cfg(test)]
            None,
        )?;
        Ok(store)
    }

    fn require_unactivated(&self) -> Result<()> {
        match self.root.read("CURRENT", MAX_STORE_BYTES) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
            Ok(_) => Err(StoreError::Conflict(
                "destination already contains a project; choose a new location".into(),
            )),
        }
    }

    /// Store bounded canonical JSON in the fixed immutable extension namespace.
    /// This is content storage only: the intention layer validates the object
    /// type, references and provenance; CURRENT remains activation authority.
    pub fn stage_extension(&self, bytes: &[u8]) -> Result<Digest> {
        self.pinned()?;
        let digest = Self::extension_digest(bytes)?;
        let name = format!("extension-{}.json", digest.as_str());
        match self.root.publish(&name, bytes, false) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                if self.read_extension(&digest)? != bytes {
                    return Err(StoreError::Corrupt(
                        "immutable intention object collision".into(),
                    ));
                }
                // A previous or concurrent publisher may have renamed the
                // object without completing its directory sync yet.
                self.root.sync()?;
            }
            Err(e) => return Err(e.into()),
        }
        self.pinned()?;
        Ok(digest)
    }

    /// Read exactly the requested bounded, canonical, content-addressed object.
    pub fn read_extension(&self, digest: &Digest) -> Result<Vec<u8>> {
        self.pinned()?;
        let bytes = self.root.read(
            &format!("extension-{}.json", digest.as_str()),
            MAX_WIRE_BYTES,
        )?;
        if Self::extension_digest(&bytes)? != *digest {
            return Err(StoreError::Corrupt(
                "intention object checksum mismatch".into(),
            ));
        }
        self.pinned()?;
        Ok(bytes)
    }

    fn extension_digest(bytes: &[u8]) -> Result<Digest> {
        if bytes.len() > MAX_WIRE_BYTES {
            return Err(StoreError::Invalid(
                "intention object exceeds the byte limit".into(),
            ));
        }
        let value: serde_json::Value = json::parse(bytes)?;
        if canonical_bytes(&value)? != bytes {
            return Err(StoreError::Corrupt(
                "intention object is not canonical JSON".into(),
            ));
        }
        // Canonical equality makes this exactly bytes_digest(Evidence, bytes),
        // matching the typed intention object's canonical identity.
        Ok(canonical_digest(IdentityDomain::Evidence, &value)?)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let (parent, name) = Self::location(path.as_ref())?;
        let root = parent.child(&name)?;
        Ok(Self {
            parent: Arc::new(parent),
            root: Arc::new(root),
            name,
            upgrade_requires_reopen: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        })
    }
    fn pinned(&self) -> Result<()> {
        if !self.root.same(&self.parent.child(&self.name)?)? {
            return Err(StoreError::Conflict(
                "project directory was replaced; reopen the chosen location".into(),
            ));
        }
        Ok(())
    }
    pub fn load(&self) -> Result<ProjectSnapshot> {
        if self
            .upgrade_requires_reopen
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StoreError::RestartRequired);
        }
        self.pinned()?;
        let pointer: Pointer = json::parse(&self.root.read("CURRENT", 16 * 1024)?)?;
        if pointer.magic != MAGIC {
            return Err(StoreError::Corrupt("not a generated-tool project".into()));
        }
        if pointer.version == 1 {
            return Err(StoreError::UpgradeRequired);
        }
        if pointer.version != FORMAT {
            return Err(StoreError::UnsupportedFormat(pointer.version));
        }
        let bytes = self.root.read(
            &format!("object-{}.json", pointer.object.as_str()),
            MAX_STORE_BYTES,
        )?;
        let snapshot: ProjectSnapshot = json::parse(&bytes)?;
        if canonical_digest(IdentityDomain::Data, &snapshot)? != pointer.object {
            return Err(StoreError::Corrupt(
                "snapshot checksum mismatch; use a verified checkpoint".into(),
            ));
        }
        snapshot.validate()?;
        if snapshot.revision != pointer.revision || snapshot.data.project_id != pointer.project_id {
            return Err(StoreError::Corrupt(
                "pointer does not match snapshot".into(),
            ));
        }
        // Reopening verifies usable current state, rather than trusting a stored
        // flag or recomputing any historical event under the active program.
        LocalRuntime::default().start(
            snapshot.program()?,
            &snapshot.data,
            &snapshot.session,
            snapshot.clock_day,
            0,
            RuntimeLimits::default(),
        )?;
        Ok(snapshot)
    }
    fn save(
        &self,
        snapshot: &ProjectSnapshot,
        replace_current: bool,
        #[cfg(test)] fault: Option<FaultPoint>,
    ) -> Result<()> {
        self.pinned()?;
        if !replace_current {
            self.require_unactivated()?;
        }
        snapshot.validate()?;
        let bytes = canonical_bytes(snapshot)?;
        if bytes.len() > MAX_STORE_BYTES {
            return Err(StoreError::Invalid(
                "project snapshot byte limit reached; current work was preserved".into(),
            ));
        }
        let object = canonical_digest(IdentityDomain::Data, snapshot)?;
        let name = format!("object-{}.json", object.as_str());
        match self.root.publish(&name, &bytes, false) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                if self.root.read(&name, MAX_STORE_BYTES)? != bytes {
                    return Err(StoreError::Corrupt("immutable snapshot collision".into()));
                }
            }
            Err(e) => return Err(e.into()),
        }
        #[cfg(test)]
        if fault == Some(FaultPoint::AfterObject) {
            return Err(StoreError::Interrupted(FaultPoint::AfterObject));
        }
        self.pinned()?;
        let pointer = Pointer {
            magic: MAGIC.into(),
            version: FORMAT,
            project_id: snapshot.data.project_id.clone(),
            revision: snapshot.revision,
            object,
        };
        #[cfg(test)]
        if fault == Some(FaultPoint::BeforePointer) {
            return Err(StoreError::Interrupted(FaultPoint::BeforePointer));
        }
        // The preflight avoids staging into a known competing project; only
        // atomic no-clobber publication closes a subsequent activation race.
        self.root
            .publish("CURRENT", &canonical_bytes(&pointer)?, replace_current)?;
        #[cfg(test)]
        if fault == Some(FaultPoint::AfterPointer) {
            return Err(StoreError::Interrupted(FaultPoint::AfterPointer));
        }
        Ok(())
    }
    pub fn apply(
        &self,
        expected_revision: u64,
        operation: &str,
        input: &SemanticInput,
        limits: RuntimeLimits,
    ) -> Result<ProjectSnapshot> {
        self.apply_inner(
            expected_revision,
            operation,
            input,
            limits,
            #[cfg(test)]
            None,
        )
    }
    #[cfg(test)]
    pub fn apply_with_fault(
        &self,
        expected_revision: u64,
        operation: &str,
        input: &SemanticInput,
        limits: RuntimeLimits,
        fault: FaultPoint,
    ) -> Result<ProjectSnapshot> {
        self.apply_inner(expected_revision, operation, input, limits, Some(fault))
    }
    fn apply_inner(
        &self,
        expected_revision: u64,
        operation: &str,
        input: &SemanticInput,
        limits: RuntimeLimits,
        #[cfg(test)] fault: Option<FaultPoint>,
    ) -> Result<ProjectSnapshot> {
        if !valid_id(operation) {
            return Err(StoreError::Invalid("invalid operation ID".into()));
        }
        validate_input_shape(input)?;
        limits.validate()?;
        let _lock = self.root.lock()?;
        let mut snapshot = self.load()?;
        let request = canonical_digest(IdentityDomain::Input, input)?;
        if let Some(receipt) = snapshot.operations.get(operation) {
            return if receipt.request == request {
                Ok(snapshot)
            } else {
                Err(StoreError::Conflict(
                    "operation ID was used for a different input".into(),
                ))
            };
        }
        if snapshot.revision != expected_revision {
            return Err(StoreError::Conflict(
                "project changed; reload before applying this input".into(),
            ));
        }
        let before = snapshot.clone();
        let runtime = LocalRuntime::default();
        let mut run = runtime.resume(
            snapshot.program()?,
            &snapshot.data,
            &snapshot.session,
            snapshot.clock_day,
            0,
            limits,
            &snapshot.artifacts,
        )?;
        runtime.apply(&mut run, input, operation)?;
        snapshot.data = runtime.data(&run).clone();
        snapshot.session = runtime.session(&run).clone();
        snapshot.clock_day = run.clock_day();
        snapshot.artifacts = run.artifacts().to_vec();
        snapshot.revision = snapshot
            .revision
            .checked_add(1)
            .ok_or_else(|| StoreError::Invalid("project revision overflow".into()))?;
        snapshot.operations.insert(
            operation.into(),
            OperationReceipt {
                operation: operation.into(),
                request,
                revision: snapshot.revision,
            },
        );
        scope::verify_transition(&before, &snapshot)?;
        self.save(
            &snapshot,
            true,
            #[cfg(test)]
            fault,
        )?;
        Ok(snapshot)
    }
    /// Convenience rehearsal for an explicit whole-program switch. Scoped
    /// changes need the controller's own plan and executable implementation.
    pub fn prepare_switch(&self, target: &CapturedProgram, id: &str) -> Result<PreparedAdoption> {
        let snapshot = self.load()?;
        let compatibility =
            LocalRuntime::default().compatibility_at(target, &snapshot.data, snapshot.clock_day)?;
        if compatibility.state != CompatibilityState::Compatible {
            return Err(StoreError::Incompatible(compatibility));
        }
        let plan = AdoptionPlan {
            version: CONTRACT_VERSION,
            id: id.into(),
            project_id: snapshot.data.project_id.clone(),
            expected_generation: snapshot.data.generation,
            expected_data: snapshot.data.identity()?,
            expected_decisions: snapshot.decisions.identity()?,
            expected_session: snapshot.session.identity()?,
            current_source: snapshot.program()?.binding.clone(),
            target: target.artifact.clone(),
            scope: DecisionScope {
                operations: target
                    .program
                    .actions
                    .iter()
                    .map(|a| a.id.clone())
                    .collect(),
                population: Population::All,
                conditions: Values::new(),
                excluded_records: vec![],
                unknowns: vec![],
            },
            compatibility,
            required_decisions: snapshot
                .decisions
                .decisions
                .iter()
                .filter(|d| d.status == DecisionStatus::Active)
                .map(|d| d.id.clone())
                .collect(),
            checks: vec![],
            evidence: vec![],
            retire_decisions: vec![],
        };
        self.prepare_adoption(plan, target)
    }
    pub fn prepare_adoption(
        &self,
        plan: AdoptionPlan,
        target: &CapturedProgram,
    ) -> Result<PreparedAdoption> {
        let current = self.load()?;
        self.check_plan(&current, &plan, target)?;
        Ok(PreparedAdoption {
            plan,
            expected_revision: current.revision,
            target: revision(target)?,
        })
    }
    pub fn prepare_recovery(
        &self,
        recovery: &RecoveryPlan,
        target: &CapturedProgram,
    ) -> Result<PreparedAdoption> {
        recovery.validate()?;
        let current = self.load()?;
        if recovery.preserve_current_data != current.data.identity()?
            || recovery.preserve_current_events
                != canonical_digest(IdentityDomain::Data, &current.data.events)?
        {
            return Err(StoreError::Conflict(
                "recovery must preserve the current records and events".into(),
            ));
        }
        for id in &recovery.withdraw_decisions {
            if !current.decisions.decisions.iter().any(|d| &d.id == id) {
                return Err(StoreError::Invalid("unknown decision to withdraw".into()));
            }
        }
        self.prepare_adoption(recovery.adoption.clone(), target)
    }
    fn check_plan(
        &self,
        current: &ProjectSnapshot,
        plan: &AdoptionPlan,
        target: &CapturedProgram,
    ) -> Result<()> {
        plan.validate()?;
        target.validate()?;
        if plan.target != target.artifact
            || plan.current_source != current.program()?.binding
            || plan.expected_data != current.data.identity()?
            || plan.expected_generation != current.data.generation
            || plan.expected_decisions != current.decisions.identity()?
            || plan.expected_session != current.session.identity()?
            || plan.project_id != current.data.project_id
        {
            return Err(StoreError::Conflict(
                "adoption rehearsal is stale or targets different source/data".into(),
            ));
        }
        let report =
            LocalRuntime::default().compatibility_at(target, &current.data, current.clock_day)?;
        if report != plan.compatibility || report.state != CompatibilityState::Compatible {
            return Err(StoreError::Incompatible(report));
        }
        Ok(())
    }
    /// No active intentions may be silently skipped. The decision controller
    /// uses adopt_verified to independently rerun them while holding this save's
    /// exact current state. Imported check records alone never grant authority.
    pub fn adopt(
        &self,
        expected_revision: u64,
        prepared: &PreparedAdoption,
        target: &CapturedProgram,
        decisions: &DecisionGraph,
    ) -> Result<ProjectSnapshot> {
        self.adopt_verified(expected_revision,prepared,target,decisions,|current,target,next,plan| {
            if plan.scope.population!=Population::All || !plan.scope.conditions.is_empty() || !plan.scope.excluded_records.is_empty() || !plan.scope.unknowns.is_empty() || plan.scope.operations!=target.program.actions.iter().map(|a|a.id.clone()).collect() {return Err(StoreError::Invalid("scoped implementation needs independent controller verification".into()));}
            if current.decisions!=*next || current.decisions.decisions.iter().any(|d|d.status==DecisionStatus::Active) {return Err(StoreError::Invalid("active decisions require independent controller verification before adoption".into()));}Ok(())
        })
    }
    /// `verify` is trusted host code, not model output. It must execute active
    /// obligations and confirm scope/retirement against these exact arguments.
    pub fn adopt_verified<F>(
        &self,
        expected_revision: u64,
        prepared: &PreparedAdoption,
        target: &CapturedProgram,
        decisions: &DecisionGraph,
        verify: F,
    ) -> Result<ProjectSnapshot>
    where
        F: FnOnce(&ProjectSnapshot, &CapturedProgram, &DecisionGraph, &AdoptionPlan) -> Result<()>,
    {
        let _lock = self.root.lock()?;
        let mut current = self.load()?;
        let request = canonical_digest(
            IdentityDomain::Adoption,
            &(&prepared.plan, target, decisions),
        )?;
        if let Some(receipt) = current.operations.get(&prepared.plan.id) {
            return if receipt.request == request {
                Ok(current)
            } else {
                Err(StoreError::Conflict(
                    "adoption ID belongs to another operation".into(),
                ))
            };
        }
        if expected_revision != current.revision
            || prepared.expected_revision != current.revision
            || prepared.target != revision(target)?
        {
            return Err(StoreError::Conflict(
                "project changed after adoption rehearsal".into(),
            ));
        }
        if !current.scope.layers.is_empty() && revision(target)? != current.active_revision {
            return Err(StoreError::Invalid(
                "Managed history requires a verified scoped or managed-evolution preparation"
                    .into(),
            ));
        }
        scope::reject_unmanaged_target(&current, target)?;
        self.check_plan(&current, &prepared.plan, target)?;
        decisions.validate()?;
        verify(&current, target, decisions, &prepared.plan)?;
        let previous = current.active_revision.clone();
        let target_revision = revision(target)?;
        if !current
            .programs
            .iter()
            .any(|p| revision(p).ok().as_ref() == Some(&target_revision))
        {
            current.programs.push(target.clone());
        }
        // Only schema/behavior changes. Current records and all historical event
        // values are retained byte-for-byte, including later optional fields.
        current.data = merged_data(target, &current.data)?;
        current.active_revision = target_revision.clone();
        // A decision-only save must not discard the work-in-progress session.
        // A genuinely different program still starts with its own valid state.
        if target_revision != previous {
            current.session = SessionState::initial(&target.program)?;
        }
        current.decisions = decisions.clone();
        current.revision = current
            .revision
            .checked_add(1)
            .ok_or_else(|| StoreError::Invalid("project revision overflow".into()))?;
        current.adoptions.push(AdoptionReceipt {
            plan: prepared.plan.clone(),
            previous,
            active: target_revision,
            revision: current.revision,
        });
        current.operations.insert(
            prepared.plan.id.clone(),
            OperationReceipt {
                operation: prepared.plan.id.clone(),
                request,
                revision: current.revision,
            },
        );
        LocalRuntime::default().start(
            target,
            &current.data,
            &current.session,
            current.clock_day,
            0,
            RuntimeLimits::default(),
        )?;
        self.save(
            &current,
            true,
            #[cfg(test)]
            None,
        )?;
        Ok(current)
    }
}
