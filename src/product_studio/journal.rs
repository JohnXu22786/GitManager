//! Bounded restart associations, never a second project store or provider grant.
use super::*;
use crate::product_locations::Folder;
use serde::{Deserialize, Serialize};
#[path = "journal_format.rs"]
mod format;
#[path = "journal_upgrade.rs"]
mod upgrade;
#[cfg(test)]
#[path = "journal_upgrade_tests.rs"]
mod upgrade_tests;
// Complete inherited context must fit the unchanged strict 1 MiB intake.
// Only startup's dedicated upgrader can read the previous disk format.
const JOURNAL_LIMIT: usize = MAX_WIRE_BYTES;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Association {
    pub path: PathBuf,
    pub identity: ToolIdentity,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Basis {
    pub snapshot: Digest,
    pub source: Digest,
    pub revision: u64,
    pub data_generation: u64,
    pub data: Digest,
    pub session: Digest,
    pub decisions: Digest,
    pub day: i32,
    pub runtime: String,
    pub driver: String,
}
impl Basis {
    pub fn capture(snapshot: &ProjectSnapshot) -> Result<Self, String> {
        Ok(Self {
            snapshot: canonical_digest(IdentityDomain::Evidence, snapshot).map_err(error)?,
            source: snapshot.active_revision.clone(),
            revision: snapshot.revision,
            data_generation: snapshot.data.generation,
            data: canonical_digest(IdentityDomain::Data, &snapshot.data).map_err(error)?,
            session: canonical_digest(IdentityDomain::Session, &snapshot.session).map_err(error)?,
            decisions: canonical_digest(IdentityDomain::Decision, &snapshot.decisions)
                .map_err(error)?,
            day: snapshot.clock_day,
            runtime: crate::product_runtime::RUNTIME_VERSION.into(),
            driver: crate::product_runtime::DRIVER_VERSION.into(),
        })
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProviderAssociation {
    pub request: DevelopmentRequest,
    pub provider: ProviderKind,
    pub profile: CapabilityProfile,
    pub wire_request: String,
    pub wire_source: String,
    pub issued: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modify: Option<(Association, Basis)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconcile: Option<ReconcileAssociation>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReconcileAssociation {
    pub tool: Association,
    pub basis: Basis,
    pub needs: Vec<Id>,
    pub evolution: Id,
    pub operation: Id,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Interrupted {
    Change {
        tool: Association,
        basis: Basis,
        plan: AdoptionPlan,
    },
    Create {
        tool: Association,
    },
    Daily {
        tool: Association,
        basis: Basis,
        operation: Id,
        input: SemanticInput,
    },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UnsavedInput {
    pub tool: Association,
    pub operation: Id,
    pub input: SemanticInput,
    pub summary: String,
    pub explanation: String,
}
/// An external publication intent is not a receipt and never authorizes replay.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FileAttempt {
    pub operation: Id,
    pub tool: Option<Association>,
    pub basis: Option<Basis>,
    pub destination: PathBuf,
    pub target: FileTarget,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum FileTarget {
    Output {
        inventory_index: usize,
        artifact: Digest,
        bytes_digest: Digest,
        byte_count: usize,
    },
    Backup {
        digest: Digest,
    },
    Recovery {
        backup: Digest,
        identity: ToolIdentity,
    },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RecoveryHandoff {
    pub operation: Id,
    pub original: Interrupted,
    pub recovered: Association,
}
impl RecoveryHandoff {
    pub fn review(&self) -> (String, String) {
        let (tool, summary) = match &self.original {
            Interrupted::Create { tool } => (tool, "Original tool creation".to_string()),
            Interrupted::Daily {
                tool,
                operation,
                input,
                ..
            } => (tool, format!("Original input {operation}: {input:?}")),
            Interrupted::Change { tool, plan, .. } => {
                (tool, format!("Original rule change {}: {plan:?}", plan.id))
            }
        };
        (format!("Your original interrupted attempt was kept and not repeated or confirmed. Original: {}. Separate working copy: {}", tool.path.display(), self.recovered.path.display()), summary)
    }
}
/// Existing task authority pins, not a second task registry.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TaskAssociation {
    pub tool: Association,
    pub task_id: Id,
    pub created_at: String,
    pub repository: PathBuf,
    pub worktree: PathBuf,
    pub external: Option<ExternalAssociation>,
    // Consumed context is evidence history, never pending launch/submit authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed: Option<CompletedTask>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CompletedTask {
    pub external: ExternalAssociation,
    pub capture: CapturedProgram,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExternalAssociation {
    pub pending: task_source_flow::PendingExternal,
    pub basis: Basis,
    pub provider: ProviderKind,
    pub disclosure: Digest,
    pub approval_operation: Id,
    // Written before invocation. A lost launch acknowledgement never retries.
    pub launch_attempted: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Journal {
    magic: String,
    version: u32,
    pub need: String,
    pub last: Option<Association>,
    pub pending: Option<Interrupted>,
    pub provider: Option<ProviderAssociation>,
    pub abandoned_creation: Option<Association>,
    pub last_unsaved: Option<UnsavedInput>,
    // Optional host metadata is preserved by the startup migration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_file: Option<FileAttempt>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recovery_handoffs: Vec<RecoveryHandoff>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<TaskAssociation>,
}
impl Default for Journal {
    fn default() -> Self {
        Self {
            magic: "gitmanager.generated-tool-host".into(),
            version: 2,
            need: String::new(),
            last: None,
            pending: None,
            provider: None,
            abandoned_creation: None,
            last_unsaved: None,
            local_file: None,
            recovery_handoffs: vec![],
            task: None,
        }
    }
}
impl Journal {
    fn validate(&self) -> Result<(), String> {
        if self.magic != "gitmanager.generated-tool-host"
            || self.version != 2
            || self.need.len() > MAX_TEXT_BYTES
        {
            return Err("The restart information is damaged or belongs to an unsupported version; it was kept unchanged".into());
        }
        let check = |tool: &Association| -> Result<(), String> {
            if !tool.path.is_absolute() || !valid_id(&tool.identity.project_id) {
                return Err("Invalid restart tool association".into());
            }
            Ok(())
        };
        if let Some(task) = &self.task {
            check(&task.tool)?;
            if !valid_id(&task.task_id)
                || task.created_at.is_empty()
                || task.created_at.len() > MAX_TEXT_BYTES
                || !task.repository.is_absolute()
                || !task.worktree.is_absolute()
            {
                return Err("Invalid linked task association".into());
            }
            if task.external.is_some() && task.completed.is_some() {
                return Err("A task cannot have pending and consumed authority together".into());
            }
            for external in task
                .external
                .iter()
                .chain(task.completed.iter().map(|c| &c.external))
            {
                let request = external.pending.request();
                request.validate().map_err(error)?;
                let disclosure =
                    task_source_host::Disclosure::new(request.clone(), external.provider)?;
                if disclosure.digest != external.disclosure
                    || !valid_id(&external.approval_operation)
                    || request.operation != DevelopmentOperation::Modify
                    || request.project_id != task.tool.identity.project_id
                    || request.context.data_digest.as_ref() != Some(&external.basis.data)
                    || request.context.session_digest.as_ref() != Some(&external.basis.session)
                    || canonical_digest(IdentityDomain::Decision, &request.decisions)
                        .map_err(error)?
                        != external.basis.decisions
                    || request
                        .sources
                        .first()
                        .and_then(|s| s.binding.task.as_ref())
                        .map(|t| &t.task_id)
                        != Some(&task.task_id)
                    || !request.sources.iter().any(|s| {
                        canonical_digest(IdentityDomain::Source, s).ok().as_ref()
                            == Some(&external.basis.source)
                    })
                {
                    return Err("Invalid authorized task handoff association".into());
                }
            }
        }
        if let Some(task) = &self.task {
            if let Some(completed) = &task.completed {
                completed.capture.validate().map_err(error)?;
                if !completed.external.launch_attempted
                    || completed.capture.binding.project_id != task.tool.identity.project_id
                    || completed.capture.binding.task.as_ref().map(|t| &t.task_id)
                        != Some(&task.task_id)
                    || !matches!(
                        completed.capture.binding.producer,
                        Producer::ExternalAuthor { .. }
                    )
                    || completed
                        .external
                        .pending
                        .request()
                        .sources
                        .first()
                        .map(|s| &s.binding.program_path)
                        != Some(&completed.capture.binding.program_path)
                {
                    return Err("Invalid consumed task source association".into());
                }
            }
        }
        if let Some(file) = &self.local_file {
            if !valid_id(&file.operation)
                || !file.destination.is_absolute()
                || file.destination.as_os_str().len() > 4096
                || file.destination.components().count() > 128
                || file.destination.components().any(|c| {
                    matches!(
                        c,
                        std::path::Component::ParentDir | std::path::Component::CurDir
                    )
                })
                || file.tool.is_some() != file.basis.is_some()
            {
                return Err("Invalid interrupted local-file association; files were kept".into());
            }
            if let Some(tool) = &file.tool {
                check(tool)?;
            }
            match &file.target {
                FileTarget::Output { byte_count, .. }
                    if file.tool.is_none()
                        || *byte_count > crate::product_locations::MAX_LOCAL_BYTES =>
                {
                    return Err("Invalid interrupted output association".into())
                }
                FileTarget::Backup { .. } if file.tool.is_none() => {
                    return Err("Invalid interrupted backup association".into())
                }
                FileTarget::Recovery { identity, .. } if !valid_id(&identity.project_id) => {
                    return Err("Invalid interrupted recovery identity".into())
                }
                _ => (),
            }
        }
        if let Some(unsaved) = &self.last_unsaved {
            check(&unsaved.tool)?;
            if !valid_id(&unsaved.operation)
                || unsaved.summary.len() > MAX_TEXT_BYTES
                || unsaved.explanation.len() > MAX_TEXT_BYTES
            {
                return Err("Invalid retained unsaved input".into());
            }
            validate_input_shape(&unsaved.input).map_err(error)?;
        }
        if let Some(tool) = &self.abandoned_creation {
            check(tool)?;
        }
        if let Some(tool) = &self.last {
            check(tool)?;
        }
        if let Some(ProviderAssociation {
            request,
            provider,
            profile,
            wire_request,
            wire_source,
            modify,
            reconcile,
            ..
        }) = &self.provider
        {
            request.validate().map_err(error)?;
            match (request.operation, modify, reconcile) {
                (DevelopmentOperation::Generate, None, None)
                    if request.sources.is_empty() && request.request == self.need =>
                {
                    ()
                }
                (DevelopmentOperation::Modify, Some((tool, basis)), None) => {
                    check(tool)?;
                    if request.project_id != tool.identity.project_id
                        || request
                            .sources
                            .last()
                            .and_then(|s| canonical_digest(IdentityDomain::Source, s).ok())
                            .as_ref()
                            != Some(&basis.source)
                        || request.context.data_digest.as_ref() != Some(&basis.data)
                        || request.context.session_digest.as_ref() != Some(&basis.session)
                    {
                        return Err("Invalid interrupted modification association".into());
                    }
                }
                (DevelopmentOperation::Reconcile, None, Some(binding)) => {
                    check(&binding.tool)?;
                    let basis = &binding.basis;
                    let unique: std::collections::BTreeSet<_> = binding.needs.iter().collect();
                    let prefix = format!("{}\nHost adoption operation ID: {}\nReturn the executable evolution using this exact evolution suggestion ID: {}\nPreserve both accepted needs: {}. Return an executable design, explicit input mappings and exact proposed retirements. Preserve every independent obligation.", self.need, binding.operation, binding.evolution, binding.needs.join(", "));
                    if binding.needs.is_empty()
                        || unique.len() != binding.needs.len()
                        || !valid_id(&binding.evolution)
                        || !valid_id(&binding.operation)
                        || binding.evolution == binding.operation
                        || request.project_id != binding.tool.identity.project_id
                        || request
                            .sources
                            .last()
                            .and_then(|s| canonical_digest(IdentityDomain::Source, s).ok())
                            .as_ref()
                            != Some(&basis.source)
                        || request.context.data_digest.as_ref() != Some(&basis.data)
                        || request.context.session_digest.as_ref() != Some(&basis.session)
                        || canonical_digest(IdentityDomain::Decision, &request.decisions)
                            .map_err(error)?
                            != basis.decisions
                        || !request.request.starts_with(&prefix)
                        || binding.needs.iter().any(|id| {
                            !request.decisions.decisions.iter().any(|d| {
                                &d.id == id
                                    && (d.status == DecisionStatus::Active
                                        || (d.status == DecisionStatus::Pending
                                            && matches!(
                                                d.outcome,
                                                DecisionOutcome::BothNeeded
                                                    | DecisionOutcome::EitherAcceptable
                                            )))
                            })
                        })
                        || request
                            .accepted_scenes
                            .iter()
                            .filter(|scene| binding.needs.contains(&scene.decision))
                            .count()
                            < 2
                    {
                        return Err("Invalid interrupted new-design association".into());
                    }
                }
                _ => return Err("Invalid interrupted generation association".into()),
            }
            let wire = encode_request(
                request,
                &ProviderOptions {
                    provider: *provider,
                    profile: *profile,
                    ..ProviderOptions::default()
                },
            )
            .map_err(error)?;
            if wire.digest()? != *wire_request || wire.source_digest != *wire_source {
                return Err("Interrupted generation identity changed".into());
            }
        }
        if self.recovery_handoffs.len() > 16 {
            return Err("The preserved recovery-attempt history is full. All previous attempts and files were kept.".into());
        }
        let mut operations = std::collections::BTreeSet::new();
        for handoff in &self.recovery_handoffs {
            check(&handoff.recovered)?;
            if !valid_id(&handoff.operation) || !operations.insert(&handoff.operation) {
                return Err("Invalid preserved recovery-attempt identity".into());
            }
        }
        for pending in self
            .pending
            .iter()
            .chain(self.recovery_handoffs.iter().map(|h| &h.original))
        {
            match pending {
                Interrupted::Change { tool, basis, plan } => {
                    check(tool)?;
                    plan.validate().map_err(error)?;
                    if plan.project_id != tool.identity.project_id
                        || plan.expected_generation != basis.data_generation
                        || plan.expected_data != basis.data
                        || plan.expected_session != basis.session
                    {
                        return Err("Invalid interrupted change association".into());
                    }
                }
                Interrupted::Create { tool } => check(tool)?,
                Interrupted::Daily {
                    tool,
                    operation,
                    input,
                    ..
                } => {
                    check(tool)?;
                    if !valid_id(operation) {
                        return Err("Invalid interrupted operation identity".into());
                    }
                    validate_input_shape(input).map_err(error)?;
                }
            }
        }
        Ok(())
    }
}
pub(super) struct JournalFile {
    folder: Folder,
    // One generated-tool host writer per profile. Release happens on the worker.
    _lock: crate::product_locations::WriteLock,
    pub value: Journal,
}
fn read_optional(folder: &Folder) -> Result<Option<Vec<u8>>, String> {
    folder.check().map_err(error)?;
    match std::fs::symlink_metadata(folder.path().join("session.json")) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            folder.check().map_err(error)?;
            Ok(None)
        }
        Err(e) => Err(error(e)),
        Ok(_) => folder
            .read("session.json", JOURNAL_LIMIT)
            .map(Some)
            .map_err(error),
    }
}
impl JournalFile {
    /// Upgrade handles are released before returning this inert byte identity.
    /// The caller must then open a fresh current-format host session.
    pub fn upgrade(root: &Path, report: impl FnMut(u8)) -> Result<Option<String>, String> {
        Ok(match upgrade::run(root, report)? {
            upgrade::Outcome::Current => None,
            upgrade::Outcome::RestartRequired { digest } => Some(digest),
        })
    }
    pub fn open(root: &Path) -> Result<Self, String> {
        Self::reopen(root, None)
    }
    pub fn reopen(root: &Path, activated: Option<&str>) -> Result<Self, String> {
        #[cfg(test)]
        upgrade::checkpoint(root, upgrade::TestPoint::BeforeOpen)?;
        let folder = Folder::ensure(&root.join("studio")).map_err(error)?;
        let lock = folder
            .lock()
            .map_err(|e| format!("Generated-tool restart information is busy: {e}"))?;
        let value = match read_optional(&folder)? {
            Some(bytes) => {
                if activated
                    .is_some_and(|expected| crate::product_provider::digest(&bytes) != expected)
                {
                    return Err("The activated restart information changed before fresh reopening; files were kept and writes are paused".into());
                }
                format::decode(&bytes)?
            }
            None => {
                if activated.is_some() {
                    return Err(
                        "The activated restart information is missing; writes are paused".into(),
                    );
                }
                let value = Journal::default();
                folder
                    .publish("session.json", &format::encode(&value)?, false)
                    .map_err(error)?;
                folder.sync().map_err(error)?;
                value
            }
        };
        let result = Self {
            folder,
            _lock: lock,
            value,
        };
        #[cfg(test)]
        upgrade::checkpoint(root, upgrade::TestPoint::Opened)?;
        Ok(result)
    }
    /// Machine callers read only the current atomic format. They never acquire
    /// a writer lock, create a directory, migrate or renew any association.
    pub fn read_only(root: &Path) -> Result<Journal, String> {
        let folder = Folder::open(&root.join("studio")).map_err(error)?;
        let bytes = folder.read("session.json", JOURNAL_LIMIT).map_err(error)?;
        format::decode(&bytes)
    }
    pub fn write(&mut self, value: Journal) -> Result<(), String> {
        let bytes = format::encode(&value)?;
        self.folder
            .publish("session.json", &bytes, true)
            .map_err(error)?;
        self.folder.sync().map_err(error)?;
        self.value = value;
        Ok(())
    }
}
