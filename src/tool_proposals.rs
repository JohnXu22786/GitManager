//! Explicit, bounded data exchange with an external proposal author.
//!
//! Export returns bytes only. Callers must explicitly select synthetic examples
//! or review and sanitize every selected field/history value and free-text string
//! before constructing `ExportSelection`. This is a projection boundary, not an
//! automatic personal-data redactor. No task paths, repository content, business
//! records, stored notes, or arbitrary extensions are selected from the project.
//!
//! Schema acceptance is not execution evidence. Rehearsal and adoption preparation
//! delegate to `tool_decisions`; this module never persists or adopts a change.

use crate::tool_decisions::{self, ChangePreview, ChangeRequest, DecisionError, RehearsalCase};
use crate::tool_project::*;
use crate::tool_proposal_input::{self, InputError};
use chrono::{DateTime, FixedOffset, NaiveDate};
use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fmt;
use std::io::Read;

pub const PROPOSAL_FORMAT: &str = "gitmanager.proposal";
pub const EXCHANGE_VERSION: u32 = 1;
pub const MAX_CANDIDATES: usize = 8;
pub const MAX_CASES_PER_CANDIDATE: usize = 8;
pub const MAX_STEPS_PER_CASE: usize = 64;
pub const MAX_OBJECT_MEMBERS: usize = 256;
const MAX_TOTAL_CASES: usize = 32;
const MAX_TOTAL_STEPS: usize = 512;
const MAX_JSON_NODES: usize = 32_768;
const MAX_EXAMPLE_RECORDS: usize = 16;
const MAX_EXAMPLE_EVENTS: usize = 128;
const MAX_EXAMPLE_SPECS: usize = 32;
const MAX_SELECTED_INTENTS: usize = 128;
/// Typed local metadata stored through the existing lossless extension contract.
pub const PROVENANCE_KEY: &str = "tool_proposals_provenance";

/// A read-only projection of an explicitly associated existing task. The
/// controller resolves `TaskRecord.id` and obtains a *fresh* source fingerprint
/// through the existing task-verification API. Do not use a previous review or
/// verification result as proof of current source identity. Missing/moved tasks
/// are represented by `None`; no legacy task data is changed or exported here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskIdentity {
    pub task_id: String,
    pub candidate_fingerprint: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskAssociationStatus {
    Unlinked,
    Current,
    Missing,
    Changed,
}

/// Displayable association state, never a global gate on local daily work.
pub fn task_association_status(
    expected: Option<&TaskIdentity>,
    current: Option<&TaskIdentity>,
) -> TaskAssociationStatus {
    match (expected, current) {
        (None, _) => TaskAssociationStatus::Unlinked,
        (Some(_), None) => TaskAssociationStatus::Missing,
        (Some(expected), Some(current)) if expected == current => TaskAssociationStatus::Current,
        _ => TaskAssociationStatus::Changed,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExampleDisclosure {
    Synthetic,
    ExplicitlySelectedSanitized,
}

#[derive(Clone, Debug)]
pub struct SelectedExample {
    pub disclosure: ExampleDisclosure,
    pub case: RehearsalCase,
}

#[derive(Clone, Debug)]
pub struct SelectedIntent {
    pub decision_id: String,
    pub sanitized_intent: String,
}

/// Every free-text string and example is caller-selected for this export. Supply
/// a reviewed summary of each active intent; stored rationale, original words,
/// outcomes and extension metadata are never copied automatically.
#[derive(Clone, Debug)]
pub struct ExportSelection {
    pub original_request: String,
    pub confirmed_intents: Vec<SelectedIntent>,
    pub examples: Vec<SelectedExample>,
    pub task: Option<TaskIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalCandidate {
    pub candidate_id: String,
    pub rule_keys: Vec<RuleKey>,
    /// A suggestion only. The caller supplies the actual U03 frozen scope.
    pub suggested_scope: ScopeKind,
    pub cases: Vec<RehearsalCase>,
}

/// Version-one wire contract composes the shared domain types. Candidate plans
/// correspond by index to `envelope.candidate_policies`. No executable payloads,
/// extension blobs, imported evidence or external supersession authority exist.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalPackage {
    pub format: String,
    pub format_version: u32,
    pub runtime_semantics_version: u32,
    pub requirements_fingerprint: String,
    pub task: Option<TaskIdentity>,
    pub envelope: ProposalEnvelope,
    pub candidates: Vec<ProposalCandidate>,
}

#[derive(Clone, Debug)]
pub struct RequirementExport {
    bytes: Vec<u8>,
    template: ProposalPackage,
    original_request: String,
}
impl RequirementExport {
    pub fn json(&self) -> &[u8] {
        &self.bytes
    }
    /// A complete example of the exact supported wire shape. External authors
    /// may change policies/cases/rationale, but must retain request/base identity.
    pub fn proposal_template(&self) -> ProposalPackage {
        self.template.clone()
    }
}

/// A schema-accepted, still untrusted proposal. Private fields prevent mutation
/// between import and replay; callers can only inspect the shared envelope and
/// candidate descriptions, not construct a verified result.
#[derive(Clone, Debug)]
pub struct AcceptedProposal {
    package: ProposalPackage,
    original_request: String,
    fingerprint: String,
}
impl AcceptedProposal {
    pub fn envelope(&self) -> &ProposalEnvelope {
        &self.package.envelope
    }
    pub fn candidates(&self) -> &[ProposalCandidate] {
        &self.package.candidates
    }
    pub fn task(&self) -> Option<&TaskIdentity> {
        self.package.task.as_ref()
    }
}

/// IDs, scope and supersession are local user/controller choices, never external
/// instructions. Keep this exact selection with its replay through confirmation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplaySelection {
    pub candidate_id: String,
    pub request_id: String,
    pub operation_id: String,
    pub decision_id: String,
    pub revision_id: String,
    pub scope: ScopeSelection,
    pub supersedes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ProposalReplay {
    selection: ReplaySelection,
    package_fingerprint: String,
    request: ChangeRequest,
    preview: ChangePreview,
}
impl ProposalReplay {
    pub fn preview(&self) -> &ChangePreview {
        &self.preview
    }
    pub fn request(&self) -> &ChangeRequest {
        &self.request
    }
}

/// An integrity/freshness binding, not proof of external author authenticity.
/// Native evidence fingerprints keep their original runtime meaning.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalProvenance {
    pub version: u32,
    pub decision_id: String,
    pub proposal_id: String,
    pub source: ProposalSource,
    pub candidate_id: String,
    pub proposal_fingerprint: String,
    pub requirements_fingerprint: String,
    pub baseline_fingerprint: String,
    pub task: Option<TaskIdentity>,
    pub evidence_binding_fingerprint: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedProposalAssociation {
    pub provenance: ProposalProvenance,
    pub task_status: TaskAssociationStatus,
}

/// The U03 prepared snapshot, augmented only with typed local provenance.
/// Store and retry this exact immutable candidate. Validate the association
/// immediately before submitting the normal store commit/retry.
#[derive(Clone, Debug)]
pub struct PreparedProposalChange {
    operation_id: String,
    decision_id: String,
    snapshot: ProjectSnapshot,
}
impl PreparedProposalChange {
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub fn expected_generation(&self) -> u64 {
        self.snapshot.generation
    }
    pub fn snapshot(&self) -> &ProjectSnapshot {
        &self.snapshot
    }
    pub fn validate_for_commit(
        &self,
        current_task: Option<&TaskIdentity>,
    ) -> Result<(), ProposalError> {
        let saved = saved_proposal_association(&self.snapshot, &self.decision_id, current_task)?
            .ok_or_else(|| schema("prepared proposal provenance is missing"))?;
        require_current_association(saved.task_status)
    }
}

/// Inspect after reopening and before reusing associated proposal evidence.
/// Missing/changed associations are stale; ordinary records and new local-only
/// U03 changes remain usable. This checks provenance integrity only: U03 still
/// owns native replay, active conflicts and freshness.
pub fn saved_proposal_association(
    snapshot: &ProjectSnapshot,
    decision_id: &str,
    current_task: Option<&TaskIdentity>,
) -> Result<Option<SavedProposalAssociation>, ProposalError> {
    let decision = snapshot
        .decisions
        .iter()
        .find(|d| d.decision_id == decision_id)
        .ok_or_else(|| schema("proposal decision no longer exists"))?;
    let Some(value) = decision.extensions.get(PROVENANCE_KEY) else {
        if snapshot.evidence.iter().any(|e| {
            decision.evidence_ids.contains(&e.run_id) && e.extensions.contains_key(PROVENANCE_KEY)
        }) {
            return Err(schema(
                "decision provenance is missing from associated evidence",
            ));
        }
        return Ok(None);
    };
    let provenance: ProposalProvenance =
        serde_json::from_value(value.clone()).map_err(|e| schema(e.to_string()))?;
    if provenance.version != 1
        || provenance.source == ProposalSource::LocalBuiltIn
        || provenance.decision_id != decision.decision_id
    {
        return Err(schema("unsupported saved proposal provenance"));
    }
    id(&provenance.proposal_id)?;
    id(&provenance.candidate_id)?;
    for identity in [
        &provenance.proposal_fingerprint,
        &provenance.requirements_fingerprint,
        &provenance.baseline_fingerprint,
        &provenance.evidence_binding_fingerprint,
    ] {
        hash_identity(identity)?;
    }
    check_task(provenance.task.as_ref())?;
    let mut evidence = Vec::new();
    for run_id in &decision.evidence_ids {
        let saved = snapshot
            .evidence
            .iter()
            .find(|e| e.run_id == *run_id)
            .ok_or_else(|| schema("associated native evidence is missing"))?;
        if saved.source != EvidenceSource::NativeExecution
            || saved.extensions.get(PROVENANCE_KEY) != Some(value)
        {
            return Err(schema("associated evidence provenance is inconsistent"));
        }
        let mut native = saved.clone();
        native.extensions.clear();
        evidence.push(native);
    }
    if (evidence.is_empty() && decision.choice == DecisionChoice::Adopt)
        || evidence_binding(&provenance, &evidence)? != provenance.evidence_binding_fingerprint
    {
        return Err(schema("saved proposal evidence binding does not match"));
    }
    let task_status = task_association_status(provenance.task.as_ref(), current_task);
    Ok(Some(SavedProposalAssociation {
        provenance,
        task_status,
    }))
}

fn evidence_binding(
    provenance: &ProposalProvenance,
    evidence: &[Evidence],
) -> Result<String, ProposalError> {
    let mut context = provenance.clone();
    context.evidence_binding_fingerprint.clear();
    fingerprint(&(context, evidence))
}

#[derive(Debug)]
pub enum ProposalError {
    Input(InputError),
    Schema(String),
    Limit(&'static str),
    UnsupportedFormat,
    WrongProject,
    StaleBase,
    StaleRequirements,
    StaleCandidate,
    TaskAssociation(TaskAssociationStatus),
    Validation(ProjectValidationError),
    Decision(DecisionError),
}
impl fmt::Display for ProposalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(e) => write!(f, "{e}"),
            Self::Schema(message) => write!(f, "unsupported proposal data: {message}"),
            Self::Limit(kind) => write!(f, "proposal exceeds the {kind} limit"),
            Self::UnsupportedFormat => write!(f, "unsupported proposal format or runtime version"),
            Self::WrongProject => write!(f, "proposal belongs to another project"),
            Self::StaleBase => write!(f, "project changed; export a fresh requirement package"),
            Self::StaleRequirements => write!(
                f,
                "proposal does not match the selected requirement package"
            ),
            Self::StaleCandidate => write!(f, "candidate or selection changed; rehearse again"),
            Self::TaskAssociation(state) => write!(
                f,
                "task association is {state:?}; local work remains available"
            ),
            Self::Validation(e) => write!(f, "{e}"),
            Self::Decision(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for ProposalError {}
impl From<ProjectValidationError> for ProposalError {
    fn from(error: ProjectValidationError) -> Self {
        Self::Validation(error)
    }
}
impl From<DecisionError> for ProposalError {
    fn from(error: DecisionError) -> Self {
        Self::Decision(error)
    }
}
fn schema(message: impl Into<String>) -> ProposalError {
    ProposalError::Schema(message.into())
}

/// Return the selected requirement package; neither write it nor send it.
/// Tool specifications are exported through a known-field projection, including
/// user-configured labels, but with every extension removed.
pub fn export_requirements(
    snapshot: &ProjectSnapshot,
    mut selection: ExportSelection,
) -> Result<RequirementExport, ProposalError> {
    snapshot.validate()?;
    text(&selection.original_request)?;
    check_task(selection.task.as_ref())?;
    bounded(
        selection.examples.len(),
        1,
        MAX_CASES_PER_CANDIDATE,
        "selected examples",
    )?;
    bounded(
        selection.confirmed_intents.len(),
        0,
        MAX_SELECTED_INTENTS,
        "selected intents",
    )?;
    bounded(
        snapshot.spec_revisions.len(),
        1,
        MAX_EXAMPLE_SPECS,
        "spec revisions",
    )?;
    let active: BTreeSet<_> = snapshot
        .decisions
        .iter()
        .filter(|d| d.status == DecisionStatus::Active)
        .map(|d| d.decision_id.as_str())
        .collect();
    let mut selected_ids = BTreeSet::new();
    let mut intents = Vec::new();
    for selected in &selection.confirmed_intents {
        text(&selected.sanitized_intent)?;
        if !active.contains(selected.decision_id.as_str())
            || !selected_ids.insert(selected.decision_id.as_str())
        {
            return Err(schema("select each active intent exactly once"));
        }
        let decision = snapshot
            .decisions
            .iter()
            .find(|d| d.decision_id == selected.decision_id)
            .expect("active decision was checked");
        intents.push(json!({"decision_id": selected.decision_id, "intent_revision": decision.intent_revision, "intent": selected.sanitized_intent, "scope_kind": decision.scope.as_ref().map(|scope| scope.kind)}));
    }
    if selected_ids != active {
        return Err(schema(
            "provide an explicitly reviewed summary of every active intent",
        ));
    }
    let mut case_ids = BTreeSet::new();
    for selected in &mut selection.examples {
        project_case(&mut selected.case);
        validate_case(&selected.case, snapshot)?;
        if !case_ids.insert(&selected.case.scenario.scenario_id) {
            return Err(schema("duplicate example ID"));
        }
    }
    let baseline = baseline_fingerprint(snapshot)?;
    let specs: Vec<_> = snapshot
        .spec_revisions
        .iter()
        .cloned()
        .map(project_spec)
        .collect();
    let examples: Vec<_> = selection
        .examples
        .iter()
        .map(|selected| json!({"disclosure": selected.disclosure, "case": selected.case}))
        .collect();
    let requirements = json!({
        "format": "gitmanager.requirements", "format_version": EXCHANGE_VERSION,
        "project_id": snapshot.project_id, "baseline_fingerprint": baseline,
        "runtime_semantics_version": RUNTIME_SEMANTICS_VERSION,
        "spec_revisions": specs, "original_request": selection.original_request,
        "confirmed_intents": intents, "examples": examples, "task": selection.task,
    });
    let template = ProposalPackage {
        format: PROPOSAL_FORMAT.into(),
        format_version: EXCHANGE_VERSION,
        runtime_semantics_version: RUNTIME_SEMANTICS_VERSION,
        requirements_fingerprint: fingerprint(&requirements)?,
        task: selection.task,
        envelope: ProposalEnvelope {
            proposal_id: "proposal-1".into(),
            project_id: snapshot.project_id.clone(),
            baseline_fingerprint: baseline,
            source: ProposalSource::ExternalHarness,
            candidate_policies: vec![selection.examples[0].case.scenario.candidate_policy.clone()],
            rationale: "Describe the proposed outcome for the selected examples".into(),
            unknowns: vec![],
            imported_claims: Default::default(),
            extensions: Default::default(),
        },
        candidates: vec![ProposalCandidate {
            candidate_id: "candidate-1".into(),
            rule_keys: vec![RuleKey::Timer, RuleKey::DeliveryTarget, RuleKey::Reminder],
            suggested_scope: match &selection.examples[0].case.target {
                tool_decisions::RehearsalTarget::Record(_) => ScopeKind::SingleRecord,
                tool_decisions::RehearsalTarget::FutureRecord(_) => ScopeKind::FutureRecords,
            },
            cases: selection
                .examples
                .into_iter()
                .map(|selected| selected.case)
                .collect(),
        }],
    };
    validate_package(&template, snapshot)?;
    check_json_budget(&serde_json::to_value(&template).map_err(|e| schema(e.to_string()))?)?;
    let mut document = requirements;
    document["proposal_template"] =
        serde_json::to_value(&template).map_err(|e| schema(e.to_string()))?;
    document["supported_format"] = json!({
        "rule_keys": ["timer", "delivery_target", "reminder"],
        "timer": ["pause_stages_marked_paused", "count_paused_stages"],
        "due_date": ["keep_original", "extend_by_paused_days"],
        "reminder": {"never": [], "waiting_before_due": {"days_before_due": "integer 0..3650"}, "waiting_after_days": {"days_waiting": "integer 1..3650"}},
        "field_roles": ["title", "work_description", "work_started_on", "promised_date", "notes", "custom"],
        "scenario_steps": ["create_record", "edit_record", "transition_stage", "advance_date", "observe"],
        "limits": {"bytes": tool_proposal_input::MAX_INPUT_BYTES, "depth": tool_proposal_input::MAX_NESTING_DEPTH, "members_per_object": MAX_OBJECT_MEMBERS, "json_nodes": MAX_JSON_NODES, "candidates": MAX_CANDIDATES, "cases_per_candidate": MAX_CASES_PER_CANDIDATE, "total_cases": MAX_TOTAL_CASES, "steps_per_case": MAX_STEPS_PER_CASE, "total_steps": MAX_TOTAL_STEPS, "records_per_case": MAX_EXAMPLE_RECORDS, "events_per_case": MAX_EXAMPLE_EVENTS, "spec_revisions": MAX_EXAMPLE_SPECS},
        "strict_shape": "Return only the complete proposal_template shape. Candidate plans correspond by index to envelope.candidate_policies. All fields are required, including null task. No unknown members or extensions. Dates and timestamps use the template's canonical representation. No files, scripts, commands, dependencies, imported evidence or supersedes. imported_claims permits only passed:boolean and summary:string; both are untrusted. Local replay and explicit user choice are required."
    });
    let bytes = serde_json::to_vec_pretty(&document).map_err(|e| schema(e.to_string()))?;
    if bytes.len() > tool_proposal_input::MAX_INPUT_BYTES {
        return Err(ProposalError::Limit("export bytes"));
    }
    Ok(RequirementExport {
        bytes,
        template,
        original_request: selection.original_request,
    })
}

/// Consume the existing bounded syntax reader before strict semantic validation.
/// An accepted package has no native evidence and cannot write project state.
pub fn import_proposal(
    reader: impl Read,
    snapshot: &ProjectSnapshot,
    requirements: &RequirementExport,
    current_task: Option<&TaskIdentity>,
) -> Result<AcceptedProposal, ProposalError> {
    let value = tool_proposal_input::read_json(reader).map_err(ProposalError::Input)?;
    check_json_budget(&value)?;
    let mut package: ProposalPackage =
        serde_json::from_value(value.clone()).map_err(|e| schema(e.to_string()))?;
    // The shared storage contracts deliberately retain extension fields and some
    // enum deserializers ignore unknown members. The exchange is narrower: remove
    // every extension, then compare the entire typed serialization with the input.
    // This rejects unknown nested fields, ignored enum parameters and omissions,
    // without maintaining a second parallel domain schema or custom JSON parser.
    package.envelope.extensions.clear();
    for candidate in &mut package.candidates {
        for case in &mut candidate.cases {
            project_case(case);
        }
    }
    if serde_json::to_value(&package).map_err(|e| schema(e.to_string()))? != value {
        return Err(schema(
            "fields must match the complete supported format exactly",
        ));
    }
    if package.format != PROPOSAL_FORMAT
        || package.format_version != EXCHANGE_VERSION
        || package.runtime_semantics_version != RUNTIME_SEMANTICS_VERSION
    {
        return Err(ProposalError::UnsupportedFormat);
    }
    if package.envelope.project_id != snapshot.project_id {
        return Err(ProposalError::WrongProject);
    }
    ensure_current(snapshot, &package, current_task)?;
    if package.envelope.baseline_fingerprint != requirements.template.envelope.baseline_fingerprint
    {
        return Err(ProposalError::StaleBase);
    }
    if package.requirements_fingerprint != requirements.template.requirements_fingerprint
        || package.task != requirements.template.task
    {
        return Err(ProposalError::StaleRequirements);
    }
    validate_package(&package, snapshot)?;
    let fingerprint = fingerprint(&package)?;
    Ok(AcceptedProposal {
        package,
        original_request: requirements.original_request.clone(),
        fingerprint,
    })
}

pub fn rehearse_proposal(
    snapshot: &ProjectSnapshot,
    accepted: &AcceptedProposal,
    selection: &ReplaySelection,
    current_task: Option<&TaskIdentity>,
    as_of_date: NaiveDate,
    now: DateTime<FixedOffset>,
) -> Result<ProposalReplay, ProposalError> {
    ensure_current(snapshot, &accepted.package, current_task)?;
    let index = accepted
        .package
        .candidates
        .iter()
        .position(|c| c.candidate_id == selection.candidate_id)
        .ok_or(ProposalError::StaleCandidate)?;
    let plan = &accepted.package.candidates[index];
    let mut cases = plan.cases.clone();
    // Restore trusted, local spec extensions only after exact projected-spec
    // equality was checked at import. No external extension becomes executable.
    for case in &mut cases {
        case.scenario.spec_revisions = snapshot.spec_revisions.clone();
    }
    let request = ChangeRequest {
        request_id: selection.request_id.clone(),
        operation_id: selection.operation_id.clone(),
        decision_id: selection.decision_id.clone(),
        revision_id: selection.revision_id.clone(),
        rule_keys: plan.rule_keys.clone(),
        candidate_policy: accepted.package.envelope.candidate_policies[index].clone(),
        scope: selection.scope.clone(),
        original_request: accepted.original_request.clone(),
        rationale: accepted.package.envelope.rationale.clone(),
        unresolved_questions: accepted.package.envelope.unknowns.clone(),
        supersedes: selection.supersedes.clone(),
        cases,
    };
    let preview = tool_decisions::rehearse_change(snapshot, &request, as_of_date, now)?;
    Ok(ProposalReplay {
        selection: selection.clone(),
        package_fingerprint: accepted.fingerprint.clone(),
        request,
        preview,
    })
}

/// Call only after the user chooses this result. Adds typed provenance to the
/// actual U03 prepared candidate; no write occurs here. The controller still
/// fences session/project/request/input epoch.
pub fn prepare_proposal_adoption(
    snapshot: &ProjectSnapshot,
    accepted: &AcceptedProposal,
    replay: &ProposalReplay,
    current_selection: &ReplaySelection,
    current_task: Option<&TaskIdentity>,
    as_of_date: NaiveDate,
    now: DateTime<FixedOffset>,
) -> Result<PreparedProposalChange, ProposalError> {
    ensure_current(snapshot, &accepted.package, current_task)?;
    if accepted.fingerprint != replay.package_fingerprint || &replay.selection != current_selection
    {
        return Err(ProposalError::StaleCandidate);
    }
    let native = tool_decisions::prepare_adoption(
        snapshot,
        &replay.request,
        &replay.preview,
        &current_selection.request_id,
        as_of_date,
        now,
    )?;
    attach_provenance(&native, accepted, current_selection, current_task)
}

/// Retain a user's both/neither/defer choice and provenance without activating
/// any rule. The U03 unresolved-choice validation remains authoritative.
pub fn prepare_proposal_unresolved(
    snapshot: &ProjectSnapshot,
    accepted: &AcceptedProposal,
    replay: &ProposalReplay,
    current_selection: &ReplaySelection,
    current_task: Option<&TaskIdentity>,
    choice: DecisionChoice,
    now: DateTime<FixedOffset>,
) -> Result<PreparedProposalChange, ProposalError> {
    ensure_current(snapshot, &accepted.package, current_task)?;
    if accepted.fingerprint != replay.package_fingerprint || &replay.selection != current_selection
    {
        return Err(ProposalError::StaleCandidate);
    }
    let native = tool_decisions::prepare_unresolved(snapshot, &replay.request, choice, now)?;
    attach_provenance(&native, accepted, current_selection, current_task)
}

fn attach_provenance(
    native: &tool_decisions::PreparedChange,
    accepted: &AcceptedProposal,
    current_selection: &ReplaySelection,
    current_task: Option<&TaskIdentity>,
) -> Result<PreparedProposalChange, ProposalError> {
    let mut candidate = native.snapshot().clone();
    let evidence_ids = candidate
        .decisions
        .iter()
        .find(|d| d.decision_id == current_selection.decision_id)
        .ok_or_else(|| schema("native prepared decision is missing"))?
        .evidence_ids
        .clone();
    let evidence: Vec<_> = evidence_ids
        .iter()
        .map(|id| {
            candidate
                .evidence
                .iter()
                .find(|e| e.run_id == *id)
                .cloned()
                .ok_or_else(|| schema("native prepared evidence is missing"))
        })
        .collect::<Result<_, _>>()?;
    let mut provenance = ProposalProvenance {
        version: 1,
        decision_id: current_selection.decision_id.clone(),
        proposal_id: accepted.package.envelope.proposal_id.clone(),
        source: accepted.package.envelope.source,
        candidate_id: current_selection.candidate_id.clone(),
        proposal_fingerprint: accepted.fingerprint.clone(),
        requirements_fingerprint: accepted.package.requirements_fingerprint.clone(),
        baseline_fingerprint: accepted.package.envelope.baseline_fingerprint.clone(),
        task: accepted.package.task.clone(),
        evidence_binding_fingerprint: String::new(),
    };
    provenance.evidence_binding_fingerprint = evidence_binding(&provenance, &evidence)?;
    let value = serde_json::to_value(&provenance).map_err(|e| schema(e.to_string()))?;
    let decision = candidate
        .decisions
        .iter_mut()
        .find(|d| d.decision_id == current_selection.decision_id)
        .ok_or_else(|| schema("native prepared decision is missing"))?;
    decision
        .extensions
        .insert(PROVENANCE_KEY.into(), value.clone());
    for evidence in &mut candidate.evidence {
        if decision.evidence_ids.contains(&evidence.run_id) {
            evidence
                .extensions
                .insert(PROVENANCE_KEY.into(), value.clone());
        }
    }
    candidate.validate()?;
    let prepared = PreparedProposalChange {
        operation_id: native.operation_id().to_owned(),
        decision_id: current_selection.decision_id.clone(),
        snapshot: candidate,
    };
    prepared.validate_for_commit(current_task)?;
    Ok(prepared)
}

fn validate_package(
    package: &ProposalPackage,
    snapshot: &ProjectSnapshot,
) -> Result<(), ProposalError> {
    let envelope = &package.envelope;
    id(&envelope.proposal_id)?;
    text(&envelope.rationale)?;
    if envelope.source == ProposalSource::LocalBuiltIn {
        return Err(schema("external input cannot claim a local source"));
    }
    bounded(
        envelope.unknowns.len(),
        0,
        MAX_PROPOSAL_UNKNOWN_ITEMS,
        "unknown items",
    )?;
    for unknown in &envelope.unknowns {
        text(unknown)?;
    }
    for (key, value) in &envelope.imported_claims {
        match (key.as_str(), value) {
            ("passed", Value::Bool(_)) => {}
            ("summary", Value::String(value)) => text(value)?,
            _ => {
                return Err(schema(
                    "unsupported imported claim; claims are never evidence",
                ))
            }
        }
    }
    bounded(package.candidates.len(), 1, MAX_CANDIDATES, "candidates")?;
    if package.candidates.len() != envelope.candidate_policies.len() {
        return Err(schema("each candidate must have exactly one policy"));
    }
    let mut ids = BTreeSet::new();
    let mut total_cases = 0;
    let mut total_steps = 0;
    for (candidate, policy) in package.candidates.iter().zip(&envelope.candidate_policies) {
        id(&candidate.candidate_id)?;
        if !ids.insert(&candidate.candidate_id) {
            return Err(schema("duplicate candidate ID"));
        }
        policy.validate()?;
        bounded(candidate.rule_keys.len(), 1, 3, "rule keys")?;
        if candidate
            .rule_keys
            .iter()
            .enumerate()
            .any(|(i, key)| candidate.rule_keys[..i].contains(key))
        {
            return Err(schema("duplicate rule key"));
        }
        bounded(
            candidate.cases.len(),
            1,
            MAX_CASES_PER_CANDIDATE,
            "candidate cases",
        )?;
        total_cases += candidate.cases.len();
        let mut scenarios = BTreeSet::new();
        for case in &candidate.cases {
            validate_case(case, snapshot)?;
            if !scenarios.insert(&case.scenario.scenario_id) {
                return Err(schema("duplicate scenario ID"));
            }
            total_steps += case.scenario.steps.len();
        }
    }
    bounded(total_cases, 1, MAX_TOTAL_CASES, "total cases")?;
    bounded(total_steps, 0, MAX_TOTAL_STEPS, "total steps")?;
    Ok(())
}

fn validate_case(case: &RehearsalCase, snapshot: &ProjectSnapshot) -> Result<(), ProposalError> {
    let scenario = &case.scenario;
    bounded(
        scenario.records.len(),
        0,
        MAX_EXAMPLE_RECORDS,
        "example records",
    )?;
    bounded(
        scenario.event_history.len(),
        0,
        MAX_EXAMPLE_EVENTS,
        "example events",
    )?;
    bounded(
        scenario.spec_revisions.len(),
        1,
        MAX_EXAMPLE_SPECS,
        "example specs",
    )?;
    bounded(
        scenario.steps.len(),
        0,
        MAX_STEPS_PER_CASE,
        "scenario steps",
    )?;
    bounded(
        scenario.expected.len(),
        0,
        MAX_STEPS_PER_CASE,
        "scenario expectations",
    )?;
    let target = match &case.target {
        tool_decisions::RehearsalTarget::Record(id)
        | tool_decisions::RehearsalTarget::FutureRecord(id) => id,
    };
    id(target)?;
    let specs: Vec<_> = snapshot
        .spec_revisions
        .iter()
        .cloned()
        .map(project_spec)
        .collect();
    if scenario.project_id != snapshot.project_id {
        return Err(ProposalError::WrongProject);
    }
    if scenario.base_generation != snapshot.generation
        || scenario.spec_revisions != specs
        || scenario.active_spec != snapshot.active_spec
        || scenario.date_settings != snapshot.date_settings
        || scenario.project_created_at != snapshot.created_at
    {
        return Err(ProposalError::StaleBase);
    }
    scenario.validate()?;
    // Resolve the target through the actual shared validator, including records
    // created by a supported step in the copied scenario.
    let mut observed = scenario.clone();
    observed.steps.push(ScenarioStep::Observe {
        record_id: target.clone(),
    });
    observed.validate()?;
    Ok(())
}

fn ensure_current(
    snapshot: &ProjectSnapshot,
    package: &ProposalPackage,
    current_task: Option<&TaskIdentity>,
) -> Result<(), ProposalError> {
    if snapshot.project_id != package.envelope.project_id {
        return Err(ProposalError::WrongProject);
    }
    if baseline_fingerprint(snapshot)? != package.envelope.baseline_fingerprint {
        return Err(ProposalError::StaleBase);
    }
    check_task(package.task.as_ref())?;
    let state = task_association_status(package.task.as_ref(), current_task);
    require_current_association(state)
}

fn require_current_association(state: TaskAssociationStatus) -> Result<(), ProposalError> {
    if matches!(
        state,
        TaskAssociationStatus::Missing | TaskAssociationStatus::Changed
    ) {
        return Err(ProposalError::TaskAssociation(state));
    }
    Ok(())
}

fn baseline_fingerprint(snapshot: &ProjectSnapshot) -> Result<String, ProposalError> {
    snapshot.validate()?;
    fingerprint(&(snapshot, RUNTIME_SEMANTICS_VERSION))
}
fn fingerprint(value: &impl Serialize) -> Result<String, ProposalError> {
    // Value's sorted object maps provide a canonical order, including extensions.
    let value = serde_json::to_value(value).map_err(|e| schema(e.to_string()))?;
    let bytes = serde_json::to_vec(&value).map_err(|e| schema(e.to_string()))?;
    Ok(digest(&SHA256, &bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
fn project_spec(mut spec: ToolSpec) -> ToolSpec {
    spec.extensions.clear();
    for field in &mut spec.fields {
        field.extensions.clear();
    }
    for stage in &mut spec.stages {
        stage.extensions.clear();
    }
    spec
}
fn project_case(case: &mut RehearsalCase) {
    case.scenario.extensions.clear();
    for spec in &mut case.scenario.spec_revisions {
        *spec = project_spec(spec.clone());
    }
    for record in &mut case.scenario.records {
        record.extensions.clear();
    }
    for event in &mut case.scenario.event_history {
        event.extensions.clear();
    }
}
fn check_task(task: Option<&TaskIdentity>) -> Result<(), ProposalError> {
    if let Some(task) = task {
        id(&task.task_id)?;
        hash_identity(&task.candidate_fingerprint)?;
    }
    Ok(())
}
fn hash_identity(value: &str) -> Result<(), ProposalError> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(schema("fingerprint must be a SHA-256 identity"));
    }
    Ok(())
}
fn text(value: &str) -> Result<(), ProposalError> {
    if value.trim().is_empty() || value.len() > 16_384 {
        return Err(schema("text must be nonempty and at most 16384 bytes"));
    }
    Ok(())
}
fn id(value: &str) -> Result<(), ProposalError> {
    if value.is_empty()
        || value.len() > MAX_ID_LENGTH
        || !value.as_bytes()[0].is_ascii_lowercase()
        || !value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_.".contains(&b))
    {
        return Err(schema(
            "identity must be a bounded lowercase ID, never a path",
        ));
    }
    Ok(())
}
fn bounded(
    count: usize,
    minimum: usize,
    maximum: usize,
    name: &'static str,
) -> Result<(), ProposalError> {
    if count < minimum || count > maximum {
        return Err(ProposalError::Limit(name));
    }
    Ok(())
}
fn check_json_budget(value: &Value) -> Result<(), ProposalError> {
    fn visit(value: &Value, nodes: &mut usize) -> Result<(), ProposalError> {
        *nodes += 1;
        if *nodes > MAX_JSON_NODES {
            return Err(ProposalError::Limit("JSON nodes"));
        }
        match value {
            Value::Object(map) => {
                bounded(map.len(), 0, MAX_OBJECT_MEMBERS, "object members")?;
                for child in map.values() {
                    visit(child, nodes)?;
                }
            }
            Value::Array(array) => {
                for child in array {
                    visit(child, nodes)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    visit(value, &mut 0)
}
