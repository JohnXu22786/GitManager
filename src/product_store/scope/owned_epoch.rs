//! Private regenerated execution recipes. A continuation has an identity seed,
//! never an initialization capability. Disk manifests cannot construct these.
use super::common_fact_admission::{refuse, ProofMeter};
use super::*;
use std::sync::Arc;

pub(crate) struct VerifiedOwnedTransitionExecution {
    pub(super) continuation: Arc<owned_continuation::VerifiedOwnedContinuation>,
    pub(super) target: CapturedProgram,
    pub(super) metadata: owned_lowering::Metadata,
    pub(super) manifest: OwnedCompositionV2,
    pub(super) original: Arc<PreparedOwnedPreview>,
    pub(super) initialization: OwnedInitializationV2,
}
impl VerifiedOwnedTransitionExecution {
    pub(crate) fn current(&self) -> &ProjectSnapshot {
        self.continuation.current()
    }
    pub(crate) fn seed(&self) -> &DataSnapshot {
        &self.current().data
    }
    pub(crate) fn target(&self) -> &CapturedProgram {
        &self.target
    }
}
#[derive(Clone)]
pub(super) enum OwnedExecutionRecipe {
    Initial(Arc<PreparedOwnedPreview>),
    Transition(Arc<VerifiedOwnedTransitionExecution>),
}
impl OwnedExecutionRecipe {
    pub fn initial(&self) -> &PreparedOwnedPreview {
        match self {
            Self::Initial(p) => p,
            Self::Transition(p) => &p.original,
        }
    }
    pub(crate) fn target(&self) -> &CapturedProgram {
        match self {
            Self::Initial(p) => &p.target,
            Self::Transition(p) => &p.target,
        }
    }
    pub(crate) fn seed(&self) -> &DataSnapshot {
        match self {
            Self::Initial(p) => &p.initialized,
            Self::Transition(p) => p.seed(),
        }
    }
    pub fn basis(&self) -> &ProjectSnapshot {
        match self {
            Self::Initial(p) => &p.proof.snapshot,
            Self::Transition(p) => p.current(),
        }
    }
    pub fn metadata(&self) -> &owned_lowering::Metadata {
        match self {
            Self::Initial(p) => &p.metadata,
            Self::Transition(p) => &p.metadata,
        }
    }
    pub fn active(&self) -> bool {
        match self {
            Self::Initial(_) => true,
            Self::Transition(p) => !p.manifest.active.is_empty(),
        }
    }
    pub fn request(&self) -> &OwnedPreviewRequest {
        // Population and FutureWork introduction always come from immutable L.
        &self.initial().proof.request
    }
    pub fn projections(&self) -> &[OwnedProjectionRecipe] {
        match self {
            Self::Initial(p) => &p.proof.report.projections,
            Self::Transition(p) => p.continuation.projections(),
        }
    }
    pub fn completed(&self, row: &Record) -> Result<bool> {
        match self {
            Self::Initial(p) => owned_lowering::completed(&p.proof, row),
            Self::Transition(p) => p.continuation.completed(row),
        }
    }
    pub fn capture_expression(&self, projection: &OwnedProjectionRecipe) -> Result<Expr> {
        match self {
            Self::Initial(p) => p.metadata.live(projection, &projection.binding),
            Self::Transition(p) => {
                p.continuation
                    .capture_expression(projection, &projection.binding, self.active())
            }
        }
    }
    pub fn projection_present(&self, projection: &OwnedProjectionRecipe) -> bool {
        self.active() || projection.baseline.is_some()
    }
    pub fn scope(&self) -> DecisionScope {
        owned_coverage::expected_scope(self.initial())
    }
}
#[derive(Clone)]
pub(super) struct OwnedProducerEpoch {
    pub output: Digest,
    pub adoption_revision: u64,
}
pub(super) fn err(reason: &str) -> StoreError {
    refuse("owned producer epoch", reason)
}

/// Operation receipt revisions select one producer. Merely retaining another
/// matching program digest never authorizes that producer in this interval.
// The receipt proof is common to the native wrapper and the single lexical
// cold historical endpoint expansion. No flag or serialization witness escapes.
macro_rules! validate_interval_body {
    ($snapshot:ident, $recipe:ident, $adoption_revision:expr, $end_revision:ident,
     $data:ident, $meter:ident, [$($history:tt)*]) => {{
    if $end_revision < $adoption_revision || !$data.events.starts_with(&$recipe.seed().events) {
        return Err(crate::product_store::scope::owned_epoch::err("producer endpoint precedes its exact adopted seed"));
    }
    let mut prior_revision = $adoption_revision;
    for event in &$data.events[$recipe.seed().events.len()..] {
        $meter.charge(1)?;
        let receipt = $snapshot
            .operations
            .get(&event.operation_id)
            .ok_or_else(|| crate::product_store::scope::owned_epoch::err("event lacks its retained operation receipt"))?;
        if receipt.revision <= prior_revision
            || receipt.revision > $end_revision
            || $snapshot
                .adoptions
                .iter()
                .any(|a| a.revision == receipt.revision)
            || event.program != $recipe.target().artifact.program_digest
        {
            return Err(crate::product_store::scope::owned_epoch::err(
                "event is outside its actual producer operation interval",
            ));
        }
        prior_revision = receipt.revision;
    }
    $($history)*
    }};
}
pub(super) use validate_interval_body;

// One activation/floor plus argument, result and cleanup ownership for the
// direct current-initialization lookup performed by reconstructed loans.
pub(super) const ACTIVE_INITIALIZATION_LOOKUP_FRAME_PAYMENT: u64 = 1 + 4 * (
    std::mem::size_of::<&ProjectSnapshot>()
        + std::mem::size_of::<&mut ProofMeter>()
        + std::mem::size_of::<u64>()
        + std::mem::size_of::<bool>()
) as u64;

pub(super) fn active_initialization<'a>(
    snapshot: &'a ProjectSnapshot,
    meter: &mut ProofMeter,
) -> Result<Option<(&'a OwnedInitializationV2, u64)>> {
    let state = snapshot.owned_scope()?;
    let mut active_composition = None;
    for composition in state.compositions.values() {
        meter.charge(1)?;
        if composition.output == snapshot.active_revision {
            if active_composition.replace(composition).is_some() {
                return Err(err("active output has ambiguous owned compositions"));
            }
        }
    }
    let Some(composition) = active_composition else {
        return Ok(None);
    };
    let composition_bytes = canonical_bytes(&("owned-scope-composition/2", composition))?;
    meter.charge(composition_bytes.len() as u64)?;
    let composition_id = crate::product_contract::bytes_digest(IdentityDomain::Adoption, &composition_bytes);
    let mut active_adoption = None;
    for adoption in &state.adoptions {
        meter.charge(1)?;
        if adoption.composition == composition_id {
            if active_adoption.replace(adoption).is_some() {
                return Err(err("active composition has ambiguous adoption receipts"));
            }
        }
    }
    let Some(adoption) = active_adoption else {
        return Ok(None);
    };
    let Some(initialization_id) = &adoption.initialization else {
        return Ok(None);
    };
    let mut active_initialization = None;
    for initialization in &state.initializations {
        meter.charge(1)?;
        if initialization.adoption_revision == adoption.revision {
            let bytes = canonical_bytes(&("owned-scope-initialization/2", initialization))?;
            meter.charge(bytes.len() as u64)?;
            let identity = crate::product_contract::bytes_digest(IdentityDomain::Adoption, &bytes);
            if &identity == initialization_id && active_initialization.replace(initialization).is_some() {
                return Err(err("active adoption initialization is ambiguous"));
            }
        }
    }
    let Some(initialization) = active_initialization else {
        return Err(err("active adoption initialization is missing"));
    };
    Ok(Some((initialization, adoption.revision)))
}

pub(super) fn validate_interval(
    snapshot: &ProjectSnapshot,
    recipe: &OwnedExecutionRecipe,
    adoption_revision: u64,
    end_revision: u64,
    data: &DataSnapshot,
    day: i32,
    meter: &mut ProofMeter,
) -> Result<()> {
    let state = snapshot.owned_scope()?;
    // `reconstruct_with` has already checked each initialization identity
    // against its ordered adoption receipt before it validates any interval.
    // Select by the interval's authenticated revision bound directly: the
    // current composition may be a no-initialization transition even though a
    // later committed layer initialization advanced a protected row.
    let mut committed_initializations = Vec::new();
    for initialization in &state.initializations {
        meter.charge(1)?;
        if initialization.adoption_revision > end_revision { continue; }
        meter.charge(4 + std::mem::size_of::<&OwnedInitializationV2>() as u64)?;
        committed_initializations.push(initialization);
    }
    validate_interval_body!(snapshot, recipe, adoption_revision, end_revision, data, meter, [
    owned_replay::validate_recipe_history_with_initializations(
        recipe,
        recipe.seed(),
        recipe.basis().clock_day,
        data,
        day,
        meter,
        &committed_initializations,
        None,
        adoption_revision,
        end_revision,
        &mut |_, _| Ok(()),
    )
    ])
}

/// Closed structural graph mode check. Package-level whole-obligation ownership
/// is additionally required at DecisionEngine's historical archive boundary.
pub(super) fn validate_transition_graph(
    current: &ProjectSnapshot,
    manifest: &OwnedCompositionV2,
    graph: &DecisionGraph,
    retire: Option<&[Id]>,
) -> Result<Vec<Id>> {
    graph.validate()?;
    if manifest.transition == ScopeTransition::Evolution {
        if graph != &current.decisions || retire.is_some_and(|r| !r.is_empty()) {
            return Err(err(
                "implementation evolution changed the complete decision graph",
            ));
        }
        return Ok(vec![]);
    }
    if manifest.transition != ScopeTransition::Withdrawal {
        return Err(err("unsupported transition graph mode"));
    }
    let state = current.owned_scope()?;
    let [layer_id] = manifest.layers.as_slice() else {
        return Err(err("withdrawal requires one original owned layer"));
    };
    let layer = state
        .layers
        .get(layer_id)
        .ok_or_else(|| err("withdrawal original layer missing"))?;
    let adoption = current
        .adoptions
        .iter()
        .find(|a| a.plan.id == layer.operation)
        .ok_or_else(|| err("original owned adoption missing"))?;
    let receipt = state
        .adoptions
        .iter()
        .find(|a| a.revision == adoption.revision)
        .ok_or_else(|| err("original owned decision receipt missing"))?;
    let linked: BTreeSet<_> = receipt.decisions.iter().cloned().collect();
    if linked.len() != receipt.decisions.len() {
        return Err(err("duplicate original owned receipt decision"));
    }
    let mut ids = Vec::new();
    for id in linked {
        let decision = current
            .decisions
            .decisions
            .iter()
            .find(|d| d.id == id)
            .ok_or_else(|| err("original receipt-linked decision missing"))?;
        if decision.status == DecisionStatus::Active {
            ids.push(id);
        }
    }
    // BTreeSet gives the canonical ID order; graph declaration order is kept
    // separately when deriving the exact graph delta below.
    if retire.is_some_and(|r| r != ids.as_slice()) {
        return Err(err("withdrawal retirement inventory differs"));
    }
    let mut expected = current.decisions.clone();
    if !ids.is_empty() {
        expected.revision = expected
            .revision
            .checked_add(1)
            .ok_or_else(|| err("graph revision overflow"))?;
        for decision in &mut expected.decisions {
            if ids.contains(&decision.id) {
                decision.revision = decision
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| err("decision revision overflow"))?;
                decision.status = DecisionStatus::Withdrawn {
                    adoption: manifest.operation.clone(),
                };
            }
        }
    }
    if graph != &expected {
        return Err(err(
            "withdrawal changed independent or historical decisions",
        ));
    }
    Ok(ids)
}
