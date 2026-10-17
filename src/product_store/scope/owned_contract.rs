//! Untrusted structural suggestions, separate from the persisted scalar-v1
//! contract. None of these types can prepare, execute, or adopt a scoped change.
use crate::product_contract::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedSourceRequest {
    /// Exact CapturedProgram identities, including authored bytes/provenance.
    pub before: Digest,
    pub after: Digest,
    pub root_entity: Id,
    pub edits: Vec<OwnedSpanEdit>,
}
/// Paired nonempty spans initially retain their existing lexical binder
/// occurrences as an ordered, identically named/typed/scoped prefix. Additional
/// occurrences may follow; unsupported alpha-renaming/reordering is rejected.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedSpanEdit {
    pub id: Id,
    pub before: OwnedSpanEndpoint,
    pub after: OwnedSpanEndpoint,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedRootBinding {
    pub name: Id,
    pub value_type: Type,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedSpanEndpoint {
    pub source: Digest,
    pub action: Id,
    /// Path to the containing ForEach; empty means the action's statement list.
    pub list_path: Vec<usize>,
    pub start: usize,
    /// Zero is an explicit insertion/removal gap, never an unresolved endpoint.
    pub len: usize,
    pub kinds: Vec<OwnedStatementKind>,
    /// Evidence-domain digest of ("owned-span/1", the exact statement slice).
    pub digest: Digest,
    /// Exact adjacent statement digests. None means the list boundary.
    pub previous: Option<Digest>,
    pub next: Option<Digest>,
    /// Optional assertion about an already-in-scope root, not ownership proof.
    pub root: Option<OwnedRootBinding>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnedStatementKind {
    Create,
    Update,
    Archive,
    SetState,
    Collection,
    ForEach,
    Assert,
    Emit,
}
impl OwnedStatementKind {
    pub fn of(statement: &Statement) -> Self {
        match statement {
            Statement::Create { .. } => Self::Create,
            Statement::Update { .. } => Self::Update,
            Statement::Archive { .. } => Self::Archive,
            Statement::SetState { .. } => Self::SetState,
            Statement::Collection { .. } => Self::Collection,
            Statement::ForEach { .. } => Self::ForEach,
            Statement::Assert { .. } => Self::Assert,
            Statement::Emit { .. } => Self::Emit,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnedSourceSide {
    Before,
    After,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnedStructuralChangeKind {
    ApplicationLabelChanged,
    EntityAdded,
    ColumnAdded,
    ColumnChanged,
    ObservableAdded,
    ObservableChanged,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedStructuralChange {
    pub kind: OwnedStructuralChangeKind,
    pub container: Option<Id>,
    pub id: Id,
    /// Absence stays absence; no fabricated baseline observation or value.
    pub before: Option<Digest>,
    pub after: Digest,
    pub before_type: Option<Type>,
    pub after_type: Option<Type>,
}
/// An ordered common statement header. ForEach bodies are compared recursively
/// and may contain variant slots; this does not mark their whole subtree common.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedCommonStatement {
    pub action: Id,
    pub before: Vec<usize>,
    pub after: Vec<usize>,
    pub kind: OwnedStatementKind,
}
/// Derived across both complete programs, including unchanged writers. This is
/// inventory for later seal/ownership analysis; it does not enforce any seal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedWriter {
    pub side: OwnedSourceSide,
    pub source: Digest,
    pub action: Id,
    pub path: Vec<usize>,
    pub kind: OwnedStatementKind,
    pub entity: Id,
    /// None denotes archival of the whole record.
    pub field: Option<Id>,
    pub value_type: Option<Type>,
}
/// Read-only structural analysis, never a safety certificate or preparation.
/// Passive additions may require ownership, data-freshness, read-surface, and
/// lifecycle proofs that this analyzer deliberately does not attempt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedStructuralReport {
    pub before: Digest,
    pub after: Digest,
    pub root_entity: Id,
    pub edits: Vec<OwnedSpanEdit>,
    pub common_statements: Vec<OwnedCommonStatement>,
    pub writers: Vec<OwnedWriter>,
    pub changes: Vec<OwnedStructuralChange>,
}
