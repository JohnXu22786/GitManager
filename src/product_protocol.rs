//! Presentation/input boundary. Display models never authorize adoption: the
//! controller retains the immutable plan and checks exact current identities.
use crate::product_contract::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestContext {
    pub request_id: Id,
    pub session_id: Id,
    pub project_id: Option<Id>,
    pub generation: Option<u64>,
    pub program: Option<Digest>,
    pub data: Option<Digest>,
    pub session: Option<Digest>,
    pub input_epoch: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceRequest {
    pub context: RequestContext,
    pub action: WorkspaceAction,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceAction {
    Create {
        need: String,
    },
    Open,
    Close,
    Daily {
        input: SemanticInput,
    },
    Change {
        utterance: String,
    },
    Compare {
        hypothesis: Id,
    },
    RestartComparison {
        witness: Id,
    },
    Replay {
        witness: Id,
        input: SemanticInput,
    },
    Choose {
        witness: Id,
        outcome: DecisionOutcome,
    },
    SetScope {
        choice: Id,
        scope: DecisionScope,
        rationale: Option<String>,
    },
    Adopt {
        plan: Id,
    },
    PreviewRecovery {
        decision: Id,
    },
    ConfirmRecovery {
        plan: Id,
    },
    Export {
        artifact: Digest,
    },
    Cancel {
        operation: Id,
    },
    Back,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeView {
    pub program: AppDefinition,
    pub observation: ViewObservation,
    pub artifacts: Vec<LocalArtifact>,
    pub retained_records: Vec<Record>,
    pub read_only: bool,
    pub issues: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveToolView {
    pub project_id: Id,
    pub name: String,
    pub generation: u64,
    pub program: Digest,
    pub data: Digest,
    pub session: Digest,
    pub runtime: RuntimeView,
    pub capabilities: RuntimeCapabilities,
    pub decisions: Vec<ScopedDecision>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutcomeView {
    pub artifact: ArtifactRef,
    pub runtime: RuntimeView,
    pub state: EvidenceState,
    pub origin: ExecutionOrigin,
    pub evidence: Option<Digest>,
    pub uncovered: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComparisonView {
    pub witness_id: Id,
    pub question: String,
    pub before: OutcomeView,
    pub after: OutcomeView,
    pub inputs: Vec<SemanticInput>,
    pub replay_index: usize,
    pub minimization: Option<MinimalityCertificate>,
    pub unknowns: Vec<String>,
    pub stale: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeView {
    pub choice_id: Id,
    pub witness_id: Id,
    pub outcome: DecisionOutcome,
    pub proposed_scope: DecisionScope,
    pub rationale: Option<String>,
    pub impact: Vec<String>,
    pub rehearsal_state: EvidenceState,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdoptionView {
    pub plan_id: Id,
    pub target: ArtifactRef,
    pub compatibility: CompatibilityReport,
    pub checks: Vec<DecisionCheck>,
    pub retired_decisions: Vec<Id>,
    pub issues: Vec<String>,
    /// Controller-computed availability; never used to reconstruct a plan.
    pub can_confirm: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OperationOutcome {
    Running,
    PreviewReady,
    AwaitingScope,
    AwaitingConsent(String),
    Committed { generation: u64 },
    Rejected(String),
    Retryable(String),
    Cancelled,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationStatus {
    pub delivery_id: u64,
    pub context: RequestContext,
    pub outcome: OperationOutcome,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceView {
    pub session_id: Id,
    pub next_operation_id: Id,
    pub active: Option<ActiveToolView>,
    pub comparison: Option<ComparisonView>,
    pub scope: Option<ScopeView>,
    pub adoption: Option<AdoptionView>,
    pub operation: Option<OperationStatus>,
}
