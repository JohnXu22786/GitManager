//! Application-only boundary between the workspace and its controller.
//!
//! These are presentation/input types, not a second persisted project schema.
//! The controller owns session/operation IDs, clocks, paths, immutable previews,
//! freshness checks, execution and durable commits. Map U03 previews into these
//! displays, retaining the originals by preview ID. Never reconstruct adoption or
//! retry candidates from the display. A preview-ready reply is not a saved reply.

use crate::tool_project::*;
use chrono::{DateTime, FixedOffset, NaiveDate};
use std::collections::BTreeMap;

pub struct WorkspaceView<'a> {
    /// Change on every open/close/project switch, including reopening the same ID.
    pub session_id: &'a str,
    /// Fresh, globally unique stable lowercase ID; advance after accepting a request.
    pub next_operation_id: &'a str,
    pub project: Option<&'a ToolViewModel>,
    pub now: DateTime<FixedOffset>,
    pub as_of_date: NaiveDate,
    pub location: Option<&'a str>,
    pub is_example: bool,
    /// Effective scoped policy for each displayed record, resolved by the controller.
    /// Original saved requests, projected using the decision layer accessor.
    pub decision_requests: &'a BTreeMap<String, String>,
    /// The immutable spec revision each existing record was created with.
    pub record_specs: &'a BTreeMap<String, ToolSpec>,
    pub record_policies: &'a BTreeMap<String, BehaviorPolicy>,
    pub record_history: &'a [RecordEvent],
    pub operation: Option<&'a OperationStatus>,
    pub rehearsal: Option<&'a RehearsalView>,
    pub withdrawal: Option<&'a WithdrawalView>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestContext {
    pub request_id: String,
    pub session_id: String,
    pub project_id: Option<String>,
    pub generation: Option<u64>,
    pub record_id: Option<String>,
    pub record_revision: Option<u64>,
    pub input_epoch: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceRequest {
    pub context: RequestContext,
    pub action: WorkspaceAction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceAction {
    CreateProject {
        name: String,
        location: String,
        spec: ToolSpec,
    },
    OpenProject {
        location: String,
    },
    StartExample,
    CloseProject,
    Configure {
        name: String,
        spec: ToolSpec,
    },
    Record(ToolCommand),
    /// First, copied-record comparison only. It cannot authorize adoption.
    Compare {
        input: RuleInput,
    },
    /// Fresh scope-bound proof after the user has selected an outcome.
    Rehearse {
        input: RuleInput,
    },
    SaveDecision {
        choice: DecisionChoice,
        input: RuleInput,
        preview_id: Option<String>,
        candidate_id: Option<String>,
    },
    PreviewWithdrawal {
        decision_id: String,
    },
    ConfirmWithdrawal {
        preview_id: String,
    },
}

/// Finite form inputs. The controller builds copied shared Scenarios, freezes
/// scope, and executes both candidates; these fields never directly edit records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleInput {
    /// Explicit dimensions chosen by the user; never inferred from one record or other targets.
    pub rule_keys: Vec<RuleKey>,
    pub original_request: String,
    pub rationale: String,
    pub unresolved_questions: Vec<String>,
    pub supersedes: Vec<String>,
    pub scope: ScopeKind,
    pub candidates: [BehaviorPolicy; 2],
    pub as_of_date: NaiveDate,
    pub record_changes: BTreeMap<String, Option<FieldValue>>,
    pub stage_override: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationStatus {
    /// Monotonic delivery identity, including repeated identical retry outcomes.
    pub delivery_id: u64,
    pub context: RequestContext,
    pub outcome: OperationOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OperationOutcome {
    Running,
    PreviewReady,
    /// Only emit after the store confirms the exact operation's durable commit.
    Committed,
    /// Outcome is uncertain/retryable: retain and retry the exact operation.
    Failed(String),
    /// Definitely not committed; preserve input and allow a corrected request.
    Rejected(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservationComparison {
    pub before: EvidenceObservation,
    pub after: EvidenceObservation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictView {
    pub decision_id: String,
    pub message: String,
    pub expected: Vec<EvidenceObservation>,
    pub actual: Vec<EvidenceObservation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateView {
    pub candidate_id: String,
    pub label: String,
    pub policy: BehaviorPolicy,
    pub comparisons: Vec<ObservationComparison>,
    pub evidence: Vec<Evidence>,
    pub conflicts: Vec<ConflictView>,
    /// Set only after all applicable decisions and local scenarios were checked.
    pub ready_to_adopt: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RehearsalView {
    pub context: RequestContext,
    pub preview_id: String,
    /// Controller-owned fingerprint freshness; UI adds local-context checks.
    pub fresh: bool,
    pub scope: ScopeSelection,
    pub candidates: Vec<CandidateView>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetainedBindingView {
    pub record_id: Option<String>,
    pub rule_key: RuleKey,
    pub behavior_revision_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WithdrawalView {
    pub context: RequestContext,
    pub preview_id: String,
    pub decision_id: String,
    pub fresh: bool,
    pub ready_to_commit: bool,
    pub comparisons: Vec<ObservationComparison>,
    pub preserved_record_count: usize,
    pub preserved_event_count: usize,
    pub retained_bindings: Vec<RetainedBindingView>,
    pub conflicts: Vec<ConflictView>,
}

/// Suggested private storage root, never a temporary fallback. This does not
/// create directories or authorize writes; the controller validates every path.
pub fn default_tool_data_directory() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library/Application Support"));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let base = match std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        Some(path) => Some(PathBuf::from(path)),
        None => std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")),
    };
    base.filter(|path| path.is_absolute())
        .map(|base| base.join("GitManager/tools"))
}
