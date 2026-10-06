use super::*;

pub const COMPILER_VERSION: u32 = 1;
pub const RESERVED_PREFIX: &str = crate::product_runtime::PROTECTED_FIELD_PREFIX;
pub const MAX_LAYERS: usize = 16;

/// Suggestions carry no authority. The host resolves and verifies them against
/// the current snapshot before producing a private preparation handle.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScopePopulation {
    All,
    FutureWork,
    SelectedUnfinished { records: Vec<RecordRef> },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleBinding {
    pub entity: Id,
    /// Pure own-row predicate, with `record` bound to this entity. Its meaning
    /// must be disclosed and supported by the accepted business scene.
    pub completed: Expr,
    pub source: Digest,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EffectDestination {
    Update {
        action: Id,
        path: Vec<usize>,
        field: Id,
    },
    CreateValue {
        action: Id,
        path: Vec<usize>,
        field: Id,
    },
    EmitColumn {
        action: Id,
        path: Vec<usize>,
        column: Id,
    },
    ViewColumn {
        view: Id,
        column: Id,
    },
    /// Whole-population, pure operation result. No shared state write changes.
    EmitItems {
        action: Id,
        path: Vec<usize>,
    },
    Observable {
        observable: Id,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectPatchRequest {
    pub destination: EffectDestination,
    pub entity: Id,
    pub subject: Id,
    pub value_type: Type,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeRequest {
    pub population: ScopePopulation,
    pub operations: BTreeSet<Id>,
    pub excluded_records: Vec<RecordRef>,
    pub lifecycles: Vec<LifecycleBinding>,
    pub patches: Vec<EffectPatchRequest>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenBasis {
    pub snapshot: Digest,
    pub revision: u64,
    pub active: Digest,
    /// Retained immutable input enables independent initialization replay. This
    /// participates in the same overall bounded snapshot budget as all history.
    pub data: DataSnapshot,
    pub session: SessionState,
    pub day: i32,
    pub decisions: Digest,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectPatch {
    pub request: EffectPatchRequest,
    pub before: Expr,
    pub after: Expr,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeLayer {
    pub version: u32,
    pub operation: Id,
    pub basis: FrozenBasis,
    pub candidate: Digest,
    pub request: ScopeRequest,
    pub patches: Vec<EffectPatch>,
}
impl ScopeLayer {
    pub fn identity(&self) -> Result<Digest> {
        Ok(canonical_digest(IdentityDomain::Adoption, self)?)
    }
    pub fn scope(&self) -> DecisionScope {
        DecisionScope {
            operations: self.request.operations.clone(),
            population: match &self.request.population {
                ScopePopulation::All => Population::All,
                ScopePopulation::FutureWork => Population::CreatedAfter {
                    generation: self.basis.data.generation,
                },
                ScopePopulation::SelectedUnfinished { records } => Population::Records {
                    records: records.clone(),
                },
            },
            conditions: Values::new(),
            excluded_records: self.request.excluded_records.clone(),
            unknowns: vec![],
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionManifest {
    pub version: u32,
    pub basis: FrozenBasis,
    pub transition: ScopeTransition,
    pub rewrites: Vec<ScopeSlotMapping>,
    pub business: Digest,
    /// In insertion order, including withdrawn historical layers.
    pub layers: Vec<Digest>,
    pub active: BTreeSet<Digest>,
    pub previous: Option<Digest>,
    pub operation: Id,
    pub runtime: String,
    pub driver: String,
    pub output: Digest,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetadataAddition {
    pub record: RecordRef,
    pub values: Values,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetadataInitializationReceipt {
    pub operation: Id,
    pub layer: Digest,
    pub before_data: Digest,
    pub after_data: Digest,
    pub before_schema: Digest,
    pub after_schema: Digest,
    pub additions: Vec<MetadataAddition>,
    pub business_projection: Digest,
    pub original_events: Digest,
    pub original_records: usize,
    pub capture_day: i32,
    pub capture_program: Digest,
    pub prior_snapshot: Digest,
    pub adoption_revision: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopedAdoptionReceipt {
    pub decisions: Vec<Id>,
    pub plan: Digest,
    pub composition: Digest,
    pub initialization: Option<Digest>,
    pub initialized_compatibility: CompatibilityReport,
    pub revision: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeState {
    pub version: u32,
    pub layers: BTreeMap<Digest, ScopeLayer>,
    pub compositions: BTreeMap<Digest, CompositionManifest>,
    pub initializations: Vec<MetadataInitializationReceipt>,
    pub adoptions: Vec<ScopedAdoptionReceipt>,
    /// Replay-only prospective implementations; never the active layer set.
    pub rehearsals: BTreeMap<Digest, ManagedRehearsal>,
    pub correspondences: BTreeMap<Digest, ScopeCorrespondenceReceipt>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedRehearsal {
    pub version: u32,
    pub manifest: CompositionManifest,
    pub seed: Digest,
    pub layer: Option<ScopeLayer>,
    pub initialization: Option<MetadataInitializationReceipt>,
    pub compatibility: CompatibilityReport,
    pub recorded_by: Id,
    pub recorded_revision: u64,
    pub witnesses: BTreeMap<Id, Digest>,
}
/// A replay input correspondence, never a live initialization receipt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeCorrespondence {
    pub source: CapturedProgram,
    pub original: DataSnapshot,
    pub target: Digest,
    pub day: i32,
    pub projected: Digest,
    pub operation_seed: Digest,
    pub scenario: Option<ScopeScenarioCorrespondence>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeScenarioCorrespondence {
    pub original: ScenarioSpec,
    pub projected: ScenarioSpec,
}
impl ScopeCorrespondence {
    pub fn identity(&self) -> Result<Digest> {
        Ok(canonical_digest(IdentityDomain::Scenario, self)?)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeCorrespondenceReceipt {
    pub proof: ScopeCorrespondence,
    pub operation: Id,
    pub revision: u64,
}
impl Default for ScopeState {
    fn default() -> Self {
        Self {
            version: COMPILER_VERSION,
            layers: BTreeMap::new(),
            compositions: BTreeMap::new(),
            initializations: vec![],
            adoptions: vec![],
            rehearsals: BTreeMap::new(),
            correspondences: BTreeMap::new(),
        }
    }
}
/// Opaque preparation: source, seed, initialization and scope are regenerated
/// under the store lock. Serializing a manifest never creates this authority.
#[derive(Clone, Debug)]
pub struct PreparedScopedChange {
    pub(super) basis: Digest,
    pub(super) adoption: PreparedAdoption,
    pub(super) target: CapturedProgram,
    pub(super) candidate: CapturedProgram,
    pub(super) layer: Option<ScopeLayer>,
    pub(super) evolution: Option<Vec<ScopeSlotMapping>>,
    pub(super) manifest: CompositionManifest,
    pub(super) initialized: DataSnapshot,
    pub(super) receipt: Option<MetadataInitializationReceipt>,
    pub(super) compatibility: CompatibilityReport,
    pub(super) scope: DecisionScope,
    pub(crate) correspondences: BTreeMap<Digest, ScopeCorrespondence>,
}
impl PreparedScopedChange {
    pub fn operation_id(&self) -> &str {
        &self.manifest.operation
    }
    pub fn target(&self) -> &CapturedProgram {
        &self.target
    }
    /// Exact ordinary input capture, with its original author provenance.
    pub fn candidate(&self) -> &CapturedProgram {
        &self.candidate
    }
    pub fn seed(&self) -> &DataSnapshot {
        &self.initialized
    }
    pub fn scope(&self) -> &DecisionScope {
        &self.scope
    }
    pub fn initialization(&self) -> Option<&MetadataInitializationReceipt> {
        self.receipt.as_ref()
    }
    pub fn initialized_compatibility(&self) -> &CompatibilityReport {
        &self.compatibility
    }
    pub fn layer_id(&self) -> Result<Option<Digest>> {
        self.layer.as_ref().map(ScopeLayer::identity).transpose()
    }
}

/// A host-verified correspondence for one protected semantic result slot.
/// References name actual captured sources, not presentation labels.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeSlotMapping {
    pub layer: Digest,
    pub patch: usize,
    pub from_source: Digest,
    pub from: EffectDestination,
    pub to_source: Digest,
    pub to: EffectDestination,
    pub subject: Id,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeTransition {
    Adoption,
    Withdrawal,
    Evolution,
}

/// Read-only editing guidance. This is not a preparation or adoption authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeEditableContext {
    /// Exact compiled source whose ordinary logical coordinates are described.
    pub compiled_source: Digest,
    /// Honest host-authored live-rule projection with no protected metadata.
    pub editable: CapturedProgram,
    pub slots: Vec<ScopeEditableSlot>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ScopeEditableSlot {
    pub layer: Digest,
    pub patch: usize,
    pub active: bool,
    /// Logical location before host instrumentation, not a JSON pointer into
    /// the compiled source. The compiled_source binds its verified manifest.
    pub destination: EffectDestination,
    pub entity: Id,
    pub subject: Id,
    pub value_type: Type,
}
