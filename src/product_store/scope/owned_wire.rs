//! Closed, untrusted owned-v2 disk recipes. Deserializing any of these values
//! never grants preparation, execution, lifecycle acceptance or write authority.
use super::*;
use serde::{Deserialize, Serialize};

pub const OWNED_SCOPE_VERSION: u32 = 2;
pub const OWNED_LOWERING_VERSION: u32 = 2;
pub(crate) const SCOPE_VERIFIER_REVISION: u32 = 4;
pub(crate) const OWNED_TRANSITION_LOWERING_VERSION: u32 = 3;
pub(crate) const SUPPORTED_OWNED_LOWERINGS: [u32;2] = [OWNED_LOWERING_VERSION,OWNED_TRANSITION_LOWERING_VERSION];
pub type OwnedScopeRequestV2 = OwnedPreviewRequest;

pub(super) fn identity<T: Serialize>(tag: &str, value: &T) -> Result<Digest> {
    Ok(canonical_digest(IdentityDomain::Adoption, &(tag, value))?)
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnedBasisFamilyV2 {
    ScalarEmpty,
    OwnedV2,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedBasisV2 {
    pub container_format: u32,
    pub family: OwnedBasisFamilyV2,
    pub frozen: FrozenBasis,
    /// Exact historical capture order, including ordinary independent sources.
    pub sources: Vec<Digest>,
    pub graph: DecisionGraph,
    pub artifacts: Vec<LocalArtifact>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnedAcceptanceOriginV2 {
    ExplicitHostUi,
    SyntheticOperatorFixture,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedLifecycleV2 {
    pub version: u32,
    pub witnesses: Vec<LifecycleWitness>,
    pub evidence: Vec<RunEvidence>,
    pub disclosure: Digest,
    pub event: Digest,
    pub origin: OwnedAcceptanceOriginV2,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedInventoryV2 {
    pub structure: OwnedStructuralReport,
    pub certificates: Vec<CommonFactCertificate>,
    pub projections: Vec<OwnedProjectionRecipe>,
    pub phases: Vec<CommonFactAction>,
    pub captured: BTreeMap<Id, Values>,
    pub outcome_comparison: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedLayerV2 {
    pub version: u32,
    pub lowering: u32,
    pub operation: Id,
    pub basis: OwnedBasisV2,
    pub candidate: Digest,
    pub request: OwnedScopeRequestV2,
    pub lifecycle: OwnedLifecycleV2,
    pub inventory: OwnedInventoryV2,
}
impl OwnedLayerV2 {
    pub fn identity(&self) -> Result<Digest> {
        identity("owned-scope-layer/2", self)
    }
    pub fn scope(&self) -> DecisionScope {
        DecisionScope {
            operations: self.request.operations.clone(),
            population: match &self.request.population {
                ScopePopulation::All => Population::All,
                ScopePopulation::FutureWork => Population::CreatedAfter {
                    generation: self.basis.frozen.data.generation,
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
pub struct OwnedImplementationCorrespondenceV2 {
    pub version: u32,
    pub source: Digest,
    pub target: Digest,
    pub sources: OwnedSourceRequest,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedCompositionV2 {
    pub version: u32,
    pub lowering: u32,
    pub basis: OwnedBasisV2,
    pub transition: ScopeTransition,
    pub business: Digest,
    pub layers: Vec<Digest>,
    pub active: BTreeSet<Digest>,
    pub previous: Option<Digest>,
    pub operation: Id,
    pub runtime: String,
    pub driver: String,
    pub output: Digest,
    pub correspondences: Vec<OwnedImplementationCorrespondenceV2>,
}
impl OwnedCompositionV2 {
    pub fn identity(&self) -> Result<Digest> {
        identity("owned-scope-composition/2", self)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedInitializationV2 {
    pub version: u32,
    pub operation: Id,
    pub layer: Digest,
    pub before_data: Digest,
    pub after_data: Digest,
    pub before_schema: Digest,
    pub after_schema: Digest,
    pub additions: Vec<MetadataAddition>,
    pub auxiliary: Vec<EntityDefinition>,
    pub business_projection: Digest,
    pub original_events: Digest,
    pub original_records: usize,
    pub original_event_count: usize,
    pub capture_day: i32,
    pub capture_program: Digest,
    pub prior_snapshot: Digest,
    pub adoption_revision: u64,
}
impl OwnedInitializationV2 {
    pub fn identity(&self) -> Result<Digest> {
        identity("owned-scope-initialization/2", self)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedAdoptionV2 {
    pub version: u32,
    pub decisions: Vec<Id>,
    pub plan: Digest,
    pub composition: Digest,
    pub initialization: Option<Digest>,
    pub initialized_compatibility: CompatibilityReport,
    pub revision: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedRehearsalV2 {
    pub version: u32,
    pub manifest: OwnedCompositionV2,
    pub seed: Digest,
    pub layer: Option<OwnedLayerV2>,
    pub initialization: Option<OwnedInitializationV2>,
    pub compatibility: CompatibilityReport,
    pub recorded_by: Id,
    pub recorded_revision: u64,
    pub witnesses: BTreeMap<Id, Digest>,
}
impl OwnedRehearsalV2 {
    pub fn identity(&self) -> Result<Digest> {
        identity("owned-scope-rehearsal/2", self)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedCorrespondenceV2 {
    pub version: u32,
    pub source: CapturedProgram,
    pub original: DataSnapshot,
    pub target: Digest,
    pub day: i32,
    pub projected: Digest,
    pub operation_seed: Digest,
    pub scenario: Option<ScopeScenarioCorrespondence>,
    pub layer: Digest,
    pub composition: Digest,
    pub initialization: Digest,
    pub basis: Digest,
    pub session: SessionState,
    pub decisions: Digest,
    pub random_seed: u64,
}
impl OwnedCorrespondenceV2 {
    pub fn identity(&self) -> Result<Digest> {
        identity("owned-scope-correspondence/2", self)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedCorrespondenceReceiptV2 {
    pub proof: OwnedCorrespondenceV2,
    pub operation: Id,
    pub revision: u64,
}
impl OwnedCorrespondenceReceiptV2 {
    pub fn identity(&self) -> Result<Digest> {
        identity("owned-correspondence-receipt/2", self)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedScopeStateV2 {
    pub version: u32,
    pub layers: BTreeMap<Digest, OwnedLayerV2>,
    pub compositions: BTreeMap<Digest, OwnedCompositionV2>,
    pub initializations: Vec<OwnedInitializationV2>,
    pub adoptions: Vec<OwnedAdoptionV2>,
    pub rehearsals: BTreeMap<Digest, OwnedRehearsalV2>,
    pub correspondences: BTreeMap<Digest, OwnedCorrespondenceReceiptV2>,
}
impl Default for OwnedScopeStateV2 {
    fn default() -> Self {
        Self {
            version: OWNED_SCOPE_VERSION,
            layers: BTreeMap::new(),
            compositions: BTreeMap::new(),
            initializations: vec![],
            adoptions: vec![],
            rehearsals: BTreeMap::new(),
            correspondences: BTreeMap::new(),
        }
    }
}
/// One exact family-wide recording recipe. This binds all retained proofs,
/// including correspondence and witness inventories; it is not scalar adoption.
pub(super) fn recording_request(
    proof: &OwnedRehearsalV2,
    correspondences: &[OwnedCorrespondenceV2],
) -> Result<Digest> {
    identity("owned-rehearsal-recording/2", &(proof, correspondences))
}
pub(super) fn adoption_request(
    plan: &AdoptionPlan,
    graph: &DecisionGraph,
    composition: &OwnedCompositionV2,
    initialization: &OwnedInitializationV2,
) -> Result<Digest> {
    identity(
        "owned-adoption-recording/2",
        &(plan, graph, composition, initialization),
    )
}

/// Output is the result of the authenticated transition recipe, not an
/// initializer or first-entry adoption request.
pub(super) fn transition_request(plan: &AdoptionPlan, graph: &DecisionGraph, manifest: &OwnedCompositionV2) -> Result<Digest> {
    identity("owned-transition-recording/2", &(plan, graph, manifest))
}
