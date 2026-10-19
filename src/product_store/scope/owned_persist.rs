//! Whole-family reconstruction. All disk recipes are untrusted until the full
//! ordered source/decision/receipt chain and every retained recipe regenerate.
use super::common_fact_admission::{refuse, ProofMeter};
use super::*;
use std::sync::{atomic::AtomicBool, Arc};

pub(crate) use super::owned_epoch::VerifiedOwnedTransitionExecution;
pub(super) use super::owned_epoch::{OwnedExecutionRecipe, OwnedProducerEpoch};
pub(super) struct ReconstructedOwned {
    pub snapshot: Digest,
    pub recipes: BTreeMap<Digest, OwnedExecutionRecipe>,
    pub epochs: Vec<OwnedProducerEpoch>,
    pub proofs: BTreeMap<Digest, Arc<PreparedOwnedPreview>>,
    pub manifests: BTreeMap<Digest, OwnedCompositionV2>,
    pub initializations: BTreeMap<Digest, OwnedInitializationV2>,
}
struct GraphHistory {
    prior: BTreeMap<u64, DecisionGraph>,
    after: BTreeMap<u64, DecisionGraph>,
    births: BTreeMap<u64, BTreeSet<Id>>,
}
fn err(reason: &str) -> StoreError {
    refuse("owned persisted proof", reason)
}
fn graph_history(snapshot: &ProjectSnapshot) -> Result<GraphHistory> {
    let mut graph = snapshot.decisions.clone();
    let mut result = GraphHistory {
        prior: BTreeMap::new(),
        after: BTreeMap::new(),
        births: BTreeMap::new(),
    };
    for receipt in snapshot.adoptions.iter().rev() {
        result.after.insert(receipt.revision, graph.clone());
        let required: BTreeSet<_> = graph
            .decisions
            .iter()
            .filter(|d| d.status == DecisionStatus::Active)
            .map(|d| d.id.clone())
            .collect();
        if required != receipt.plan.required_decisions.iter().cloned().collect() {
            return Err(err("required decisions differ from the committed graph"));
        }
        let mut born = BTreeSet::new();
        if graph.identity()? != receipt.plan.expected_decisions {
            graph.revision = graph
                .revision
                .checked_sub(1)
                .ok_or_else(|| err("decision graph predecessor missing"))?;
            for id in &receipt.plan.retire_decisions {
                let decision = graph
                    .decisions
                    .iter_mut()
                    .find(|d| &d.id == id)
                    .ok_or_else(|| err("retired decision missing"))?;
                match &decision.status {
                    DecisionStatus::Withdrawn { adoption } if adoption == &receipt.plan.id => {}
                    DecisionStatus::Superseded { .. } => {}
                    _ => return Err(err("invalid decision retirement transition")),
                }
                decision.status = if matches!(
                    decision.outcome,
                    DecisionOutcome::Accept { .. } | DecisionOutcome::KeepCurrent
                ) {
                    DecisionStatus::Active
                } else {
                    DecisionStatus::Pending
                };
                decision.revision = decision
                    .revision
                    .checked_sub(1)
                    .ok_or_else(|| err("decision revision predecessor missing"))?;
            }
            while canonical_digest(IdentityDomain::Decision, &graph)?
                != receipt.plan.expected_decisions
            {
                let d = graph
                    .decisions
                    .pop()
                    .ok_or_else(|| err("prior decision graph does not reconstruct"))?;
                if d.revision != 1
                    || receipt.plan.retire_decisions.contains(&d.id)
                    || !matches!(d.status, DecisionStatus::Active | DecisionStatus::Pending)
                {
                    return Err(err("decision birth altered terminal history"));
                }
                born.insert(d.id);
            }
            graph.validate()?;
        } else if !receipt.plan.retire_decisions.is_empty() {
            return Err(err("unchanged graph claims retirement"));
        }
        result.prior.insert(receipt.revision, graph.clone());
        result.births.insert(receipt.revision, born);
    }
    if graph.revision != 0 || !graph.decisions.is_empty() {
        return Err(err("decision history lacks its empty original checkpoint"));
    }
    Ok(result)
}
fn retained_state(state: &OwnedScopeStateV2, revision: u64) -> OwnedScopeStateV2 {
    let adoptions: Vec<_> = state
        .adoptions
        .iter()
        .filter(|a| a.revision <= revision)
        .cloned()
        .collect();
    let compositions: BTreeMap<_, _> = state
        .compositions
        .iter()
        .filter(|(id, _)| adoptions.iter().any(|a| &a.composition == *id))
        .map(|(id, v)| (id.clone(), v.clone()))
        .collect();
    let layers: BTreeMap<_, _> = state
        .layers
        .iter()
        .filter(|(id, _)| compositions.values().any(|m| m.layers.contains(id)))
        .map(|(id, v)| (id.clone(), v.clone()))
        .collect();
    OwnedScopeStateV2 {
        version: OWNED_SCOPE_VERSION,
        initializations: state
            .initializations
            .iter()
            .filter(|i| layers.contains_key(&i.layer))
            .cloned()
            .collect(),
        layers,
        compositions,
        adoptions,
        rehearsals: state
            .rehearsals
            .iter()
            .filter(|(_, p)| p.recorded_revision <= revision)
            .map(|(id, p)| (id.clone(), p.clone()))
            .collect(),
        correspondences: state
            .correspondences
            .iter()
            .filter(|(_, p)| p.revision <= revision)
            .map(|(id, p)| (id.clone(), p.clone()))
            .collect(),
    }
}
#[derive(Clone, Copy)]
enum BasisPurpose {
    FirstEntry,
    Continuation,
}
fn historical_basis(
    snapshot: &ProjectSnapshot,
    basis: &OwnedBasisV2,
    graphs: &GraphHistory,
    purpose: BasisPurpose,
    meter: &mut ProofMeter,
) -> Result<ProjectSnapshot> {
    let frozen = &basis.frozen;
    if basis.container_format != super::super::FORMAT
        || frozen.revision >= snapshot.revision
        || basis.sources.is_empty()
        || basis.sources.len() > MAX_ITEMS
        || basis.artifacts.len() > MAX_ITEMS
    {
        return Err(err("unsupported, future or over-budget owned basis"));
    }
    frozen.data.validate()?;
    basis.graph.validate()?;
    if frozen.data.project_id != snapshot.data.project_id
        || frozen.decisions != basis.graph.identity()?
    {
        return Err(err("foreign basis or graph mismatch"));
    }
    let expected_graph = snapshot
        .adoptions
        .iter()
        .filter(|a| a.revision <= frozen.revision)
        .last()
        .and_then(|a| graphs.after.get(&a.revision));
    let empty = DecisionGraph {
        version: CONTRACT_VERSION,
        revision: 0,
        decisions: vec![],
    };
    if expected_graph.unwrap_or(&empty) != &basis.graph {
        return Err(err("basis decision checkpoint differs from ordered births"));
    }
    if basis.sources.len() > snapshot.programs.len()
        || basis
            .sources
            .iter()
            .zip(&snapshot.programs)
            .any(|(id, source)| revision(source).ok().as_ref() != Some(id))
    {
        return Err(err("historical source order is not a retained prefix"));
    }
    let mut seen = BTreeSet::new();
    let programs = basis
        .sources
        .iter()
        .map(|id| {
            if !seen.insert(id) {
                return Err(err("duplicate basis source"));
            }
            Ok(program(snapshot, id)?.clone())
        })
        .collect::<Result<Vec<_>>>()?;
    let history = retained_state(snapshot.owned_scope()?, frozen.revision);
    let scope = match basis.family {
        OwnedBasisFamilyV2::ScalarEmpty => {
            if history != OwnedScopeStateV2::default() {
                return Err(err("owned lineage cannot reset to scalar"));
            }
            ScopeEnvelope::ScalarV1(ScopeState::default())
        }
        OwnedBasisFamilyV2::OwnedV2 => {
            if history.rehearsals.is_empty() && history.layers.is_empty() {
                return Err(err("naked historical owned basis"));
            }
            ScopeEnvelope::OwnedV2(history)
        }
    };
    let result = ProjectSnapshot {
        magic: snapshot.magic.clone(),
        version: basis.container_format,
        revision: frozen.revision,
        programs,
        active_revision: frozen.active.clone(),
        data: frozen.data.clone(),
        session: frozen.session.clone(),
        clock_day: frozen.day,
        decisions: basis.graph.clone(),
        adoptions: snapshot
            .adoptions
            .iter()
            .filter(|a| a.revision <= frozen.revision)
            .cloned()
            .collect(),
        operations: snapshot
            .operations
            .iter()
            .filter(|(_, r)| r.revision <= frozen.revision)
            .map(|(id, r)| (id.clone(), r.clone()))
            .collect(),
        artifacts: basis.artifacts.clone(),
        scope,
    };
    if canonical_digest(IdentityDomain::Data, &result)? != frozen.snapshot {
        return Err(err("complete historical snapshot does not reconstruct"));
    }
    result.validate_container()?;
    if let ScopeEnvelope::OwnedV2(state) = &result.scope {
        let retained: BTreeSet<_> = basis.sources.iter().collect();
        for manifest in state
            .compositions
            .values()
            .chain(state.rehearsals.values().map(|r| &r.manifest))
        {
            for id in std::iter::once(&manifest.output)
                .chain(std::iter::once(&manifest.business))
                .chain(manifest.basis.sources.iter())
            {
                meter.charge(1)?;
                if !retained.contains(id) {
                    return Err(err("historical container omitted a retained proof source"));
                }
            }
        }
        validate_recording_continuation(&result, meter)?;
    }
    if matches!(purpose, BasisPurpose::Continuation) {
        if !matches!(&result.scope, ScopeEnvelope::OwnedV2(s) if s.layers.len() == 1 && s.initializations.len() == 1 && !s.adoptions.is_empty())
        {
            return Err(err(
                "continuation basis lacks committed original layer and initialization",
            ));
        }
    } else {
        compiler::reject_reserved(&result.program()?.program)?;
        if matches!(basis.family, OwnedBasisFamilyV2::ScalarEmpty) {
            for source in &result.programs {
                compiler::reject_reserved(&source.program)?;
            }
        }
        if result
            .data
            .schema
            .iter()
            .any(|e| e.fields.iter().any(|f| f.id.starts_with(RESERVED_PREFIX)))
        {
            return Err(err("first-layer basis contains protected live storage"));
        }
    }
    result.session.validate(&result.program()?.program)?;
    validate_day(result.clock_day)?;
    // No recursively validating the same disk family. Ordered closure above and
    // the outer full-family pass authenticate all earlier retained recipes.
    Ok(result)
}
fn inventory(proof: &VerifiedOwnedPreviewPlan) -> Result<OwnedInventoryV2> {
    Ok(OwnedInventoryV2 {
        structure: analyze_owned_sources(
            proof.snapshot.program()?,
            &proof.candidate,
            &proof.request.inventory.sources,
        )?,
        certificates: proof.report.certificates.clone(),
        projections: proof.report.projections.clone(),
        phases: proof.report.phases.clone(),
        captured: proof.captured.clone(),
        outcome_comparison: proof.report.outcome_comparison.clone(),
    })
}
pub(super) fn initialization(
    layer: &OwnedLayerV2,
    prepared: &PreparedOwnedPreview,
) -> Result<OwnedInitializationV2> {
    let before = &layer.basis.frozen.data;
    let data = &prepared.initialized;
    let additions = before
        .records
        .iter()
        .filter(|r| r.entity == prepared.metadata.root)
        .map(|row| {
            let after = data
                .records
                .iter()
                .find(|r| r.entity == row.entity && r.id == row.id)
                .ok_or_else(|| err("initialization removed business row"))?;
            let values = after
                .values
                .iter()
                .filter(|(id, _)| !row.values.contains_key(*id))
                .map(|(id, v)| (id.clone(), v.clone()))
                .collect();
            Ok(MetadataAddition {
                record: history::row_ref(row),
                values,
            })
        })
        .collect::<Result<_>>()?;
    let auxiliary = data
        .schema
        .iter()
        .filter(|e| !before.schema.iter().any(|b| b.id == e.id))
        .cloned()
        .collect();
    Ok(OwnedInitializationV2 {
        version: OWNED_SCOPE_VERSION,
        operation: layer.operation.clone(),
        layer: layer.identity()?,
        before_data: before.identity()?,
        after_data: data.identity()?,
        before_schema: before.schema_identity()?,
        after_schema: data.schema_identity()?,
        additions,
        auxiliary,
        business_projection: canonical_digest(IdentityDomain::Data, &before.records)?,
        original_events: canonical_digest(IdentityDomain::Data, &before.events)?,
        original_records: before.records.len(),
        original_event_count: before.events.len(),
        capture_day: layer.basis.frozen.day,
        capture_program: layer.basis.frozen.active.clone(),
        prior_snapshot: layer.basis.frozen.snapshot.clone(),
        adoption_revision: layer
            .basis
            .frozen
            .revision
            .checked_add(1)
            .ok_or_else(|| err("basis revision overflow"))?,
    })
}
fn reconstruct_recipe(
    snapshot: &ProjectSnapshot,
    layer: &OwnedLayerV2,
    manifest: &OwnedCompositionV2,
    init: &OwnedInitializationV2,
    graphs: &GraphHistory,
    meter: &mut ProofMeter,
) -> Result<PreparedOwnedPreview> {
    meter.charge(canonical_bytes(&(layer, manifest, init))?.len() as u64)?;
    let id = layer.identity()?;
    if layer.version != OWNED_SCOPE_VERSION
        || layer.lowering != OWNED_LOWERING_VERSION
        || !valid_id(&layer.operation)
        || manifest.version != OWNED_SCOPE_VERSION
        || manifest.lowering != OWNED_LOWERING_VERSION
        || manifest.runtime != RUNTIME_VERSION
        || manifest.driver != DRIVER_VERSION
        || init.version != OWNED_SCOPE_VERSION
    {
        return Err(err(
            "unknown owned semantics, lowering, initialization or runtime",
        ));
    }
    if manifest.transition != ScopeTransition::Adoption
        || manifest.previous.is_some()
        || !manifest.correspondences.is_empty()
    {
        return Err(err(
            "owned evolution/withdrawal reconstruction is not yet supported",
        ));
    }
    if manifest.basis != layer.basis
        || manifest.operation != layer.operation
        || manifest.business != layer.candidate
        || manifest.layers != [id.clone()]
        || manifest.active != BTreeSet::from([id.clone()])
    {
        return Err(err(
            "composition changed exact layer/basis/active inventory",
        ));
    }
    let basis = historical_basis(
        snapshot,
        &layer.basis,
        graphs,
        BasisPurpose::FirstEntry,
        meter,
    )?;
    if matches!(&basis.scope, ScopeEnvelope::OwnedV2(s) if !s.layers.is_empty()) {
        return Err(err(
            "second committed owned layer is unsupported, including after withdrawal",
        ));
    }
    let candidate = program(snapshot, &layer.candidate)?;
    // Freshness covers the entire authenticated retained schema and every row.
    for auxiliary in &layer.request.auxiliary {
        if basis.data.schema.iter().any(|e| e.id == auxiliary.entity)
            || basis
                .data
                .records
                .iter()
                .any(|r| r.entity == auxiliary.entity)
        {
            return Err(err(
                "auxiliary identity already exists in retained schema/records",
            ));
        }
    }
    let (_accepted, proof) = common_fact_admission::reconstruct_and_derive_retained(
        &basis,
        candidate,
        &layer.request,
        &layer.lifecycle,
        meter,
    )?;
    if inventory(&proof)? != layer.inventory {
        return Err(err(
            "complete ownership, structure, capture or read-surface inventory differs",
        ));
    }
    let (target, metadata) = owned_lowering::compile_owned(&proof, &id, meter)?;
    if revision(&target)? != manifest.output || program(snapshot, &manifest.output)? != &target {
        return Err(err("owned compiled source/provenance does not regenerate"));
    }
    let data = owned_preview::initialize(&proof, &target, &metadata, meter)?;
    let prepared = PreparedOwnedPreview::from_retained(proof, target, metadata, data)?;
    if initialization(layer, &prepared)? != *init {
        return Err(err(
            "complete owned initialization receipt does not regenerate",
        ));
    }
    let runtime = LocalRuntime::with_cancellation(meter.cancelled());
    runtime.resume(
        &prepared.target,
        &prepared.initialized,
        &layer.basis.frozen.session,
        layer.basis.frozen.day,
        0,
        meter.runtime()?,
        &basis.artifacts,
    )?;
    Ok(prepared)
}
fn linked<'a>(
    snapshot: &'a ProjectSnapshot,
    operation: &str,
    revision: u64,
) -> Result<&'a AdoptionReceipt> {
    let matches: Vec<_> = snapshot
        .adoptions
        .iter()
        .filter(|a| a.plan.id == operation && a.revision == revision)
        .collect();
    if matches.len() != 1
        || snapshot.operations.get(operation).map(|r| r.revision) != Some(revision)
    {
        return Err(err(
            "owned receipt lacks unique ordinary adoption/operation link",
        ));
    }
    Ok(matches[0])
}
fn bind_plan(
    snapshot: &ProjectSnapshot,
    linked: &AdoptionReceipt,
    manifest: &OwnedCompositionV2,
    layer: &OwnedLayerV2,
    initialized: &DataSnapshot,
    prospective: bool,
) -> Result<()> {
    let b = &manifest.basis.frozen;
    let target = if prospective {
        program(snapshot, &b.active)?
    } else {
        program(snapshot, &manifest.output)?
    };
    if linked.previous != b.active
        || linked.active != revision(target)?
        || linked.plan.current_source != program(snapshot, &b.active)?.binding
        || linked.plan.target != target.artifact
        || linked.plan.expected_data != b.data.identity()?
        || linked.plan.expected_generation != b.data.generation
        || linked.plan.expected_session != b.session.identity()?
        || linked.plan.expected_decisions != b.decisions
        || linked.plan.scope != layer.scope()
        || b.revision.checked_add(1) != Some(linked.revision)
    {
        return Err(err("owned plan differs from exact historical frame/scope"));
    }
    let runtime = LocalRuntime::default();
    if runtime.compatibility_at(target, &b.data, b.day)? != linked.plan.compatibility {
        return Err(err("historical compatibility report differs"));
    }
    if !prospective
        && runtime.compatibility_at(target, initialized, b.day)?.state
            != CompatibilityState::Compatible
    {
        return Err(err("initialized owned runtime is incompatible"));
    }
    Ok(())
}
fn check_correspondence(
    proof: &OwnedCorrespondenceV2,
    manifest: &OwnedCompositionV2,
    init: &OwnedInitializationV2,
    prepared: &PreparedOwnedPreview,
) -> Result<()> {
    let b = &manifest.basis.frozen;
    if proof.version != OWNED_SCOPE_VERSION
        || proof.source != *prepared.proof.snapshot.program()?
        || proof.original != b.data
        || proof.target != manifest.output
        || proof.day != b.day
        || proof.projected != init.after_data
        || proof.operation_seed != b.data.identity()?
        || proof.layer != init.layer
        || proof.composition != manifest.identity()?
        || proof.initialization != init.identity()?
        || proof.basis != b.snapshot
        || proof.session != b.session
        || proof.decisions != b.decisions
        || proof.random_seed != 0
    {
        return Err(err("owned correspondence frame/recipe differs"));
    }
    if let Some(pair) = &proof.scenario {
        let mut expected = pair.original.clone();
        expected.seed = prepared.initialized.clone();
        if pair.original.seed != b.data
            || pair.original.session != b.session
            || pair.original.clock_day != b.day
            || pair.original.random_seed != 0
            || pair.projected != expected
        {
            return Err(err(
                "owned correspondence changed original full input frame",
            ));
        }
        pair.original.validate(&proof.source.program)?;
        pair.projected.validate(&prepared.target.program)?;
    }
    Ok(())
}
pub(super) fn reconstruct(
    snapshot: &ProjectSnapshot,
    meter: &mut ProofMeter,
) -> Result<ReconstructedOwned> {
    let state = snapshot.owned_scope()?;
    if state.layers.len() > 1
        || state.compositions.len() > MAX_ITEMS
        || state.rehearsals.len() > MAX_ITEMS
        || state.correspondences.len() > MAX_ITEMS
        || state.initializations.len() != state.layers.len()
        || state.adoptions.len() != state.compositions.len()
        || (state.layers.is_empty() && state.rehearsals.is_empty())
        || (state.layers.is_empty() != state.compositions.is_empty())
    {
        return Err(err("invalid owned family inventory or naked entry"));
    }
    meter.charge(canonical_bytes(state)?.len() as u64)?;
    for (id, layer) in &state.layers {
        if layer.identity()? != *id {
            return Err(err("owned layer map key differs"));
        }
    }
    for (id, composition) in &state.compositions {
        if composition.identity()? != *id {
            return Err(err("owned composition map key differs"));
        }
    }
    for (id, proof) in &state.rehearsals {
        if proof.version != OWNED_SCOPE_VERSION || proof.identity()? != *id {
            return Err(err("owned rehearsal version/key differs"));
        }
    }
    for (id, proof) in &state.correspondences {
        if proof.identity()? != *id {
            return Err(err("owned correspondence receipt key differs"));
        }
    }
    validate_revision_ownership(snapshot)?;
    let graphs = graph_history(snapshot)?;
    let mut active = revision(
        snapshot
            .programs
            .first()
            .ok_or_else(|| err("original source missing"))?,
    )?;
    let mut seen_operations = BTreeSet::new();
    let mut revisions = BTreeSet::new();
    for a in &snapshot.adoptions {
        if a.previous != active
            || a.plan.current_source != program(snapshot, &active)?.binding
            || a.plan.target != program(snapshot, &a.active)?.artifact
            || !seen_operations.insert(&a.plan.id)
            || snapshot.operations.get(&a.plan.id).map(|o| o.revision) != Some(a.revision)
        {
            return Err(err("ordered ordinary source/operation chain differs"));
        }
        active = a.active.clone();
    }
    if active != snapshot.active_revision {
        return Err(err("active source differs from ordered adoption tail"));
    }
    let mut result = ReconstructedOwned {
        snapshot: canonical_digest(IdentityDomain::Data, snapshot)?,
        recipes: BTreeMap::new(),
        epochs: vec![],
        proofs: BTreeMap::new(),
        manifests: BTreeMap::new(),
        initializations: BTreeMap::new(),
    };
    let mut previous_revision = None;
    for receipt in &state.adoptions {
        if receipt.version != OWNED_SCOPE_VERSION
            || !revisions.insert(receipt.revision)
            || previous_revision.is_some_and(|r| receipt.revision <= r)
        {
            return Err(err(
                "unknown, reordered or duplicate owned adoption receipt",
            ));
        }
        previous_revision = Some(receipt.revision);
        let manifest = state
            .compositions
            .get(&receipt.composition)
            .ok_or_else(|| err("adoption composition missing"))?;
        let [id] = manifest.layers.as_slice() else {
            return Err(err("committed composition needs one original layer"));
        };
        let layer = state
            .layers
            .get(id)
            .ok_or_else(|| err("committed layer missing"))?;
        let init = state
            .initializations
            .iter()
            .find(|r| &r.layer == id)
            .ok_or_else(|| err("committed initialization missing"))?;
        let adoption = linked(snapshot, &manifest.operation, receipt.revision)?;
        let after = graphs
            .after
            .get(&adoption.revision)
            .ok_or_else(|| err("adoption graph checkpoint missing"))?;
        let born = graphs
            .births
            .get(&adoption.revision)
            .ok_or_else(|| err("adoption graph birth checkpoint missing"))?;
        let operation = snapshot
            .operations
            .get(&adoption.plan.id)
            .ok_or_else(|| err("adoption operation missing"))?;
        if receipt.plan != adoption.plan.identity()? {
            return Err(err("owned plan identity differs"));
        }
        let recipe = if result.epochs.is_empty() {
            if receipt.initialization != Some(init.identity()?)
                || operation.request
                    != owned_wire::adoption_request(&adoption.plan, after, manifest, init)?
            {
                return Err(err("initial adoption request/initializer differs"));
            }
            let actual: BTreeSet<_> = receipt.decisions.iter().cloned().collect();
            let expected: BTreeSet<_> = after
                .decisions
                .iter()
                .filter(|d| {
                    born.contains(&d.id)
                        && d.status == DecisionStatus::Active
                        && d.scope == layer.scope()
                })
                .map(|d| d.id.clone())
                .collect();
            if actual.len() != receipt.decisions.len() || actual != expected
                || actual.iter().any(|id| !after.decisions.iter().any(|d| &d.id == id && matches!(&d.outcome, DecisionOutcome::Accept { artifact } if artifact == &adoption.plan.target.program_digest)))
            { return Err(err("owned accepted decisions differ from exact graph births")); }
            let initial = Arc::new(reconstruct_recipe(
                snapshot, layer, manifest, init, &graphs, meter,
            )?);
            result
                .proofs
                .insert(manifest.output.clone(), initial.clone());
            OwnedExecutionRecipe::Initial(initial)
        } else {
            if receipt.initialization.is_some()
                || !receipt.decisions.is_empty()
                || !born.is_empty()
                || operation.request
                    != owned_wire::transition_request(&adoption.plan, after, manifest)?
            {
                return Err(err(
                    "transition has initialization, decision birth or wrong request",
                ));
            }
            let historical = historical_basis(
                snapshot,
                &manifest.basis,
                &graphs,
                BasisPurpose::Continuation,
                meter,
            )?;
            result.validate_endpoint(
                snapshot,
                &historical.data,
                historical.clock_day,
                historical.revision,
                meter,
            )?;
            if graphs.prior.get(&adoption.revision) != Some(&historical.decisions) {
                return Err(err(
                    "transition prior graph differs from authenticated basis",
                ));
            }
            owned_epoch::validate_transition_graph(
                &historical,
                manifest,
                after,
                Some(&adoption.plan.retire_decisions),
            )?;
            let transition = reconstruct_transition(
                snapshot,
                &historical,
                &result,
                layer,
                init,
                manifest,
                meter,
            )?;
            OwnedExecutionRecipe::Transition(Arc::new(transition))
        };
        bind_plan(snapshot, adoption, manifest, layer, recipe.seed(), false)?;
        if LocalRuntime::default().compatibility_at(
            recipe.target(),
            recipe.seed(),
            manifest.basis.frozen.day,
        )? != receipt.initialized_compatibility
        {
            return Err(err("owned compatibility receipt differs"));
        }
        if result
            .recipes
            .insert(manifest.output.clone(), recipe)
            .is_some()
        {
            return Err(err("ambiguous owned executable"));
        }
        result
            .manifests
            .insert(manifest.output.clone(), manifest.clone());
        result
            .initializations
            .insert(manifest.output.clone(), init.clone());
        result.epochs.push(OwnedProducerEpoch {
            output: manifest.output.clone(),
            adoption_revision: receipt.revision,
        });
    }
    let mut recordings = BTreeSet::new();
    for proof in state.rehearsals.values() {
        // Initial alternatives are separate recordings, never a synthetic pair
        // of layers. Evolution pairs require the later checked evolution route.
        if !recordings.insert(&proof.recorded_by)
            || proof.witnesses.is_empty()
            || proof.witnesses.len() > MAX_ITEMS
        {
            return Err(err(
                "ambiguous owned rehearsal recording or empty witnesses",
            ));
        }
        let layer = proof
            .layer
            .as_ref()
            .ok_or_else(|| err("prospective owned layer recipe missing"))?;
        let init = proof
            .initialization
            .as_ref()
            .ok_or_else(|| err("prospective owned initialization missing"))?;
        let a = linked(snapshot, &proof.recorded_by, proof.recorded_revision)?;
        let born = graphs
            .births
            .get(&a.revision)
            .ok_or_else(|| err("recording birth checkpoint missing"))?;
        let after_graph = graphs
            .after
            .get(&a.revision)
            .ok_or_else(|| err("recording graph checkpoint missing"))?;
        let expected: BTreeSet<_> = after_graph
            .decisions
            .iter()
            .filter(|d| born.contains(&d.id) && d.status == DecisionStatus::Pending)
            .map(|d| d.id.clone())
            .collect();
        if proof.witnesses.keys().cloned().collect::<BTreeSet<_>>() != expected {
            return Err(err(
                "prospective witnesses differ from pending decision births",
            ));
        }
        for (id, witness) in &proof.witnesses {
            let d = after_graph
                .decisions
                .iter()
                .find(|d| &d.id == id)
                .ok_or_else(|| err("prospective decision missing"))?;
            if d.witness != *witness
                || d.scope != layer.scope()
                || !matches!(
                    d.outcome,
                    DecisionOutcome::EitherAcceptable
                        | DecisionOutcome::BothNeeded
                        | DecisionOutcome::NeitherFits
                        | DecisionOutcome::Deferred
                )
            {
                return Err(err("prospective decision witness/outcome/scope differs"));
            }
        }
        let correspondence: Vec<_> = state
            .correspondences
            .values()
            .filter(|r| r.operation == proof.recorded_by)
            .map(|r| r.proof.clone())
            .collect();
        let request = owned_wire::identity(
            "owned-decision-recording/2",
            &(
                &a.plan,
                &after_graph,
                owned_wire::recording_request(proof, &correspondence)?,
            ),
        )?;
        if snapshot
            .operations
            .get(&a.plan.id)
            .ok_or_else(|| err("recording operation missing"))?
            .request
            != request
        {
            return Err(err("owned recording request inventory differs"));
        }
        let prepared = reconstruct_recipe(snapshot, layer, &proof.manifest, init, &graphs, meter)?;
        bind_plan(
            snapshot,
            a,
            &proof.manifest,
            layer,
            &prepared.initialized,
            true,
        )?;
        if prepared.initialized.identity()? != proof.seed
            || LocalRuntime::default().compatibility_at(
                &prepared.target,
                &prepared.initialized,
                proof.manifest.basis.frozen.day,
            )? != proof.compatibility
        {
            return Err(err("prospective owned seed/compatibility differs"));
        }
        let prepared = Arc::new(prepared);
        if result
            .recipes
            .insert(
                proof.manifest.output.clone(),
                OwnedExecutionRecipe::Initial(prepared.clone()),
            )
            .is_some()
        {
            return Err(err("prospective recipe aliases committed executable"));
        }
        if result
            .proofs
            .insert(proof.manifest.output.clone(), prepared)
            .is_some()
        {
            return Err(err("prospective recipe aliases another executable"));
        }
        result
            .manifests
            .insert(proof.manifest.output.clone(), proof.manifest.clone());
        result
            .initializations
            .insert(proof.manifest.output.clone(), init.clone());
    }
    for receipt in state.correspondences.values() {
        let prepared = result
            .proofs
            .get(&receipt.proof.target)
            .ok_or_else(|| err("correspondence target recipe missing"))?;
        let manifest = result
            .manifests
            .get(&receipt.proof.target)
            .ok_or_else(|| err("correspondence manifest missing"))?;
        let init = result
            .initializations
            .get(&receipt.proof.target)
            .ok_or_else(|| err("correspondence initialization missing"))?;
        check_correspondence(&receipt.proof, manifest, init, prepared)?;
        linked(snapshot, &receipt.operation, receipt.revision)?;
        if !state.rehearsals.values().any(|r| {
            r.recorded_by == receipt.operation
                && r.recorded_revision == receipt.revision
                && r.manifest.output == receipt.proof.target
        }) {
            return Err(err("correspondence lacks exact owned recording linkage"));
        }
    }
    // Every protected source and live storage cell must belong to reconstructed
    // committed proofs. Prospective namespaces never enter the live schema.
    let mut declared = BTreeMap::new();
    for source in &snapshot.programs {
        let id = revision(source)?;
        if !result.recipes.contains_key(&id) {
            compiler::reject_reserved(&source.program)?;
        }
        if state.compositions.values().any(|m| m.output == id) {
            for e in &source.program.entities {
                for f in &e.fields {
                    if f.id.starts_with(RESERVED_PREFIX) {
                        declared.insert((e.id.clone(), f.id.clone()), f.value_type.clone());
                    }
                }
            }
        }
    }
    for e in &snapshot.data.schema {
        for f in &e.fields {
            if f.id.starts_with(RESERVED_PREFIX)
                && declared.get(&(e.id.clone(), f.id.clone())) != Some(&f.value_type)
            {
                return Err(err("unregistered or prospective protected storage"));
            }
        }
    }
    if state.layers.is_empty() {
        compiler::reject_reserved(&snapshot.program()?.program)?;
        for p in result.proofs.values() {
            for auxiliary in &p.proof.request.auxiliary {
                if snapshot
                    .data
                    .schema
                    .iter()
                    .any(|e| e.id == auxiliary.entity)
                    || snapshot
                        .data
                        .records
                        .iter()
                        .any(|r| r.entity == auxiliary.entity)
                {
                    return Err(err("prospective auxiliary state leaked into live data"));
                }
            }
        }
    } else if !state
        .compositions
        .values()
        .any(|m| m.output == snapshot.active_revision)
    {
        return Err(err("committed owned lineage lost active history envelope"));
    }
    for a in &snapshot.adoptions {
        if a.active != a.previous
            && (result.recipes.contains_key(&a.active) || result.recipes.contains_key(&a.previous))
            && !state.adoptions.iter().any(|r| r.revision == a.revision)
        {
            return Err(err("managed source changed without owned adoption receipt"));
        }
    }
    validate_recording_continuation(snapshot, meter)?;
    if !result.epochs.is_empty() {
        result.validate_endpoint(
            snapshot,
            &snapshot.data,
            snapshot.clock_day,
            snapshot.revision,
            meter,
        )?;
        let epoch = result
            .epochs
            .last()
            .ok_or_else(|| err("committed epoch missing"))?;
        if epoch.output != snapshot.active_revision {
            return Err(err("active source is not committed epoch tail"));
        }
        let recipe = result
            .recipes
            .get(&epoch.output)
            .ok_or_else(|| err("active recipe missing"))?;
        if snapshot.revision == epoch.adoption_revision
            && (snapshot.session != recipe.basis().session
                || snapshot.clock_day != recipe.basis().clock_day
                || snapshot.artifacts != recipe.basis().artifacts)
        {
            return Err(err("adoption changed exact session, day or artifacts"));
        }
    }
    Ok(result)
}
pub(super) fn validate(snapshot: &ProjectSnapshot) -> Result<()> {
    let mut meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
    reconstruct(snapshot, &mut meter).map(|_| ())
}

fn basis_recipe(snapshot: &ProjectSnapshot) -> Result<OwnedBasisV2> {
    Ok(OwnedBasisV2 {
        container_format: super::super::FORMAT,
        family: match &snapshot.scope {
            ScopeEnvelope::ScalarV1(s) if *s == ScopeState::default() => {
                OwnedBasisFamilyV2::ScalarEmpty
            }
            ScopeEnvelope::OwnedV2(s) if s.layers.is_empty() && !s.rehearsals.is_empty() => {
                OwnedBasisFamilyV2::OwnedV2
            }
            _ => return Err(err("first owned recipe needs an empty committed lineage")),
        },
        frozen: basis(snapshot)?,
        sources: snapshot
            .programs
            .iter()
            .map(revision)
            .collect::<std::result::Result<_, _>>()?,
        graph: snapshot.decisions.clone(),
        artifacts: snapshot.artifacts.clone(),
    })
}
/// Private, non-authoritative recipe extraction for the later locked writer.
/// No public prepared/adoption conversion or store mutation is exposed here.
pub(super) fn recipe(
    preview: &PreparedOwnedPreview,
    operation: &str,
    meter: &mut ProofMeter,
) -> Result<(
    OwnedLayerV2,
    OwnedCompositionV2,
    OwnedInitializationV2,
    PreparedOwnedPreview,
)> {
    if !valid_id(operation) || preview.proof.snapshot.operations.contains_key(operation) {
        return Err(err("invalid or reused owned operation"));
    }
    let original = &preview.proof;
    let layer = OwnedLayerV2 {
        version: OWNED_SCOPE_VERSION,
        lowering: OWNED_LOWERING_VERSION,
        operation: operation.into(),
        basis: basis_recipe(&original.snapshot)?,
        candidate: revision(&original.candidate)?,
        request: original.request.clone(),
        lifecycle: original.accepted.recipe(),
        inventory: inventory(original)?,
    };
    let proof = common_fact_admission::derive_retained(
        &original.snapshot,
        &original.candidate,
        &original.request,
        &original.accepted,
        meter,
    )?;
    let (target, metadata) = owned_lowering::compile_owned(&proof, &layer.identity()?, meter)?;
    let data = owned_preview::initialize(&proof, &target, &metadata, meter)?;
    let manifest = OwnedCompositionV2 {
        version: OWNED_SCOPE_VERSION,
        lowering: OWNED_LOWERING_VERSION,
        basis: layer.basis.clone(),
        transition: ScopeTransition::Adoption,
        business: layer.candidate.clone(),
        layers: vec![layer.identity()?],
        active: BTreeSet::from([layer.identity()?]),
        previous: None,
        operation: operation.into(),
        runtime: RUNTIME_VERSION.into(),
        driver: DRIVER_VERSION.into(),
        output: revision(&target)?,
        correspondences: vec![],
    };
    let prepared = PreparedOwnedPreview::from_retained(proof, target, metadata, data)?;
    let init = initialization(&layer, &prepared)?;
    Ok((layer, manifest, init, prepared))
}
#[cfg(test)]
pub(crate) fn owned_snapshot_for_test(
    preview: &PreparedOwnedPreview,
    operation: &str,
    prospective: bool,
) -> Result<ProjectSnapshot> {
    let mut meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
    let (layer, manifest, init, prepared) = recipe(preview, operation, &mut meter)?;
    let mut snapshot = preview.proof.snapshot.clone();
    retain(&mut snapshot, &preview.proof.candidate)?;
    retain(&mut snapshot, &prepared.target)?;
    let next = snapshot
        .revision
        .checked_add(1)
        .ok_or_else(|| err("revision overflow"))?;
    let target = if prospective {
        snapshot.program()?.clone()
    } else {
        prepared.target.clone()
    };
    let plan = AdoptionPlan {
        version: CONTRACT_VERSION,
        id: operation.into(),
        project_id: snapshot.data.project_id.clone(),
        expected_generation: snapshot.data.generation,
        expected_data: snapshot.data.identity()?,
        expected_decisions: snapshot.decisions.identity()?,
        expected_session: snapshot.session.identity()?,
        current_source: snapshot.program()?.binding.clone(),
        target: target.artifact.clone(),
        scope: layer.scope(),
        compatibility: LocalRuntime::default().compatibility_at(
            &target,
            &snapshot.data,
            snapshot.clock_day,
        )?,
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
    let mut state = match &snapshot.scope {
        ScopeEnvelope::ScalarV1(_) => OwnedScopeStateV2::default(),
        ScopeEnvelope::OwnedV2(state) => state.clone(),
    };
    let request;
    if prospective {
        let witness =
            canonical_digest(IdentityDomain::Evidence, &"test-only-pending-scene-package")?;
        let decision_id = format!("{operation}-decision");
        snapshot.decisions.revision += 1;
        snapshot.decisions.decisions.push(ScopedDecision {
            id: decision_id.clone(),
            revision: 1,
            request: "Synthetic owned pending choice".into(),
            rationale: None,
            scope: layer.scope(),
            outcome: DecisionOutcome::Deferred,
            status: DecisionStatus::Pending,
            obligations: vec![],
            scenarios: vec![canonical_digest(
                IdentityDomain::Scenario,
                &"test-only-scene",
            )?],
            witness: witness.clone(),
            supersedes: vec![],
        });
        let rehearsal = OwnedRehearsalV2 {
            version: OWNED_SCOPE_VERSION,
            manifest: manifest.clone(),
            seed: prepared.initialized.identity()?,
            layer: Some(layer),
            initialization: Some(init),
            compatibility: LocalRuntime::default().compatibility_at(
                &prepared.target,
                &prepared.initialized,
                snapshot.clock_day,
            )?,
            recorded_by: operation.into(),
            recorded_revision: next,
            witnesses: [(decision_id, witness)].into(),
        };
        request = owned_wire::identity(
            "owned-decision-recording/2",
            &(
                &plan,
                &snapshot.decisions,
                owned_wire::recording_request(&rehearsal, &[])?,
            ),
        )?;
        state.rehearsals.insert(rehearsal.identity()?, rehearsal);
    } else {
        request = owned_wire::adoption_request(&plan, &snapshot.decisions, &manifest, &init)?;
        state.layers.insert(layer.identity()?, layer);
        state
            .compositions
            .insert(manifest.identity()?, manifest.clone());
        state.initializations.push(init.clone());
        state.adoptions.push(OwnedAdoptionV2 {
            version: OWNED_SCOPE_VERSION,
            decisions: vec![],
            plan: plan.identity()?,
            composition: manifest.identity()?,
            initialization: Some(init.identity()?),
            initialized_compatibility: LocalRuntime::default().compatibility_at(
                &prepared.target,
                &prepared.initialized,
                snapshot.clock_day,
            )?,
            revision: next,
        });
        snapshot.data = prepared.initialized;
    }
    snapshot.adoptions.push(AdoptionReceipt {
        previous: snapshot.active_revision.clone(),
        active: revision(&target)?,
        plan,
        revision: next,
    });
    snapshot.operations.insert(
        operation.into(),
        OperationReceipt {
            operation: operation.into(),
            request,
            revision: next,
        },
    );
    snapshot.active_revision = revision(&target)?;
    snapshot.revision = next;
    snapshot.scope = ScopeEnvelope::OwnedV2(state);
    Ok(snapshot)
}

#[cfg(test)]
pub(crate) fn owned_historical_basis_for_test(
    snapshot: &ProjectSnapshot,
    basis: &OwnedBasisV2,
) -> Result<()> {
    let mut meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
    historical_basis(
        snapshot,
        basis,
        &graph_history(snapshot)?,
        BasisPurpose::FirstEntry,
        &mut meter,
    )
    .map(|_| ())
}
#[cfg(test)]
pub(crate) fn owned_retained_preview_for_test(
    store: &ProductStore,
    candidate: &CapturedProgram,
    request: &OwnedPreviewRequest,
    accepted: &AcceptedPreviewLifecycle,
) -> Result<PreparedOwnedPreview> {
    let mut meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
    let proof = common_fact_admission::derive_retained(
        &store.load()?,
        candidate,
        request,
        accepted,
        &mut meter,
    )?;
    let (target, metadata) = owned_lowering::compile(&proof, &mut meter)?;
    let data = owned_preview::initialize(&proof, &target, &metadata, &mut meter)?;
    PreparedOwnedPreview::from_retained(proof, target, metadata, data)
}

/// A recording changes proof/decision inventories only. Later ordinary inputs
/// may change business work, but their events must continue that preserved frame.
fn validate_recording_continuation(
    snapshot: &ProjectSnapshot,
    meter: &mut ProofMeter,
) -> Result<()> {
    let state = snapshot.owned_scope()?;
    if !state.layers.is_empty() {
        return Ok(()); // The committed initializer and owned history validate this case.
    }
    let Some(recording) = state
        .rehearsals
        .values()
        .filter(|p| p.recorded_revision <= snapshot.revision)
        .max_by_key(|p| p.recorded_revision)
    else {
        return Ok(());
    };
    let basis = &recording.manifest.basis;
    let b = &basis.frozen;
    meter.charge(canonical_bytes(&snapshot.data)?.len() as u64)?;
    if snapshot.active_revision != b.active
        || snapshot.clock_day < b.day
        || snapshot.data.schema != b.data.schema
        || !snapshot.artifacts.starts_with(&basis.artifacts)
    {
        return Err(err(
            "prospective recording changed live source/schema/artifacts/clock",
        ));
    }
    if snapshot.revision == recording.recorded_revision {
        if snapshot.data != b.data
            || snapshot.session != b.session
            || snapshot.clock_day != b.day
            || snapshot.artifacts != basis.artifacts
        {
            return Err(err("prospective recording changed its exact live frame"));
        }
        return Ok(());
    }
    if !snapshot.data.events.starts_with(&b.data.events) {
        return Err(err("ordinary continuation rewrote pre-recording events"));
    }
    let source = snapshot.program()?;
    let mut frame = b.data.clone();
    let mut revisions = revision_history::RevisionHistory::new(&frame, |_| true);
    let mut prior_revision = recording.recorded_revision;
    let mut prior_day = b.day;
    for event in &snapshot.data.events[b.data.events.len()..] {
        meter.charge(1)?;
        let receipt = snapshot
            .operations
            .get(&event.operation_id)
            .ok_or_else(|| err("ordinary event lacks its retained operation"))?;
        let id = canonical_digest(
            IdentityDomain::Input,
            &(&event.operation_id, &source.artifact.program_digest),
        )?;
        if receipt.revision <= prior_revision
            || receipt.revision > snapshot.revision
            || snapshot
                .adoptions
                .iter()
                .any(|a| a.revision == receipt.revision)
            || event.program != source.artifact.program_digest
            || !source.program.actions.iter().any(|a| a.id == event.action)
            || event.id != format!("event-{}", &id.as_str()[..32])
            || frame.generation.checked_add(1) != Some(event.sequence)
            || event.day < prior_day
            || event.day > snapshot.clock_day
        {
            return Err(err(
                "ordinary event is not a post-recording runtime continuation",
            ));
        }
        prior_revision = receipt.revision;
        prior_day = event.day;
        for change in &event.changes {
            meter.charge(1)?;
            if let Some(row) = frame
                .records
                .iter_mut()
                .find(|r| r.entity == change.entity && r.id == change.record)
            {
                if change.before.as_ref() != Some(&row.values) || row.archived {
                    return Err(err("ordinary continuation changed event before-values"));
                }
                row.values = change.after.clone();
                row.archived = change.archived;
                row.revision = row
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| err("ordinary revision overflow"))?;
            } else {
                if change.before.is_some() {
                    return Err(err("ordinary continuation omitted a record birth"));
                }
                frame.records.push(Record {
                    entity: change.entity.clone(),
                    id: change.record.clone(),
                    revision: 1,
                    created_program: event.program.clone(),
                    archived: change.archived,
                    values: change.after.clone(),
                });
            }
        }
        revisions.event(
            event
                .changes
                .iter()
                .map(|c| (c.entity.clone(), c.record.clone()))
                .collect(),
            0,
            meter,
        )?;
        frame.generation = event.sequence;
        frame.events.push(event.clone());
        frame.validate()?;
    }
    if frame.generation != snapshot.data.generation
        || frame.records.len() != snapshot.data.records.len()
    {
        return Err(err(
            "ordinary continuation changed generation or record inventory",
        ));
    }
    for (expected, actual) in frame.records.iter().zip(&snapshot.data.records) {
        if expected.entity != actual.entity
            || expected.id != actual.id
            || expected.values != actual.values
            || expected.archived != actual.archived
            || expected.created_program != actual.created_program
            || actual.revision < expected.revision
        {
            return Err(err(
                "ordinary current row differs from post-recording events",
            ));
        }
    }
    revisions.validate(&snapshot.data, meter)
}

fn validate_revision_ownership(snapshot: &ProjectSnapshot) -> Result<()> {
    let mut revisions = BTreeSet::new();
    for (id, operation) in &snapshot.operations {
        if id != &operation.operation
            || operation.revision == 0
            || operation.revision > snapshot.revision
            || !revisions.insert(operation.revision)
        {
            return Err(err("operation revision is missing, duplicated or foreign"));
        }
    }
    let mut previous = 0;
    for adoption in &snapshot.adoptions {
        if adoption.revision <= previous || adoption.revision > snapshot.revision {
            return Err(err("ordinary adoptions are not strictly chronological"));
        }
        previous = adoption.revision;
    }
    Ok(())
}
impl ReconstructedOwned {
    pub(super) fn check_snapshot(&self, snapshot: &ProjectSnapshot) -> Result<()> {
        if self.snapshot != canonical_digest(IdentityDomain::Data, snapshot)? {
            return Err(err(
                "execution registry belongs to another complete snapshot",
            ));
        }
        Ok(())
    }
    pub fn resolve(&self, source: &CapturedProgram) -> Result<&OwnedExecutionRecipe> {
        let recipe = self
            .recipes
            .get(&revision(source)?)
            .ok_or_else(|| err("source is not a checked owned recipe"))?;
        if recipe.target() != source {
            return Err(err("source capture/provenance differs from checked recipe"));
        }
        Ok(recipe)
    }
    pub fn effective_pair<'a>(
        &'a self,
        snapshot: &'a ProjectSnapshot,
        output: &Digest,
    ) -> Result<(
        &'a CapturedProgram,
        &'a CapturedProgram,
        &'a OwnedSourceRequest,
    )> {
        match self
            .recipes
            .get(output)
            .ok_or_else(|| err("effective recipe missing"))?
        {
            OwnedExecutionRecipe::Initial(p) => Ok((
                program(snapshot, &p.proof.request.inventory.sources.before)?,
                program(snapshot, &p.proof.request.inventory.sources.after)?,
                &p.proof.request.inventory.sources,
            )),
            OwnedExecutionRecipe::Transition(p) => Ok((
                p.continuation.before(),
                p.continuation.after(),
                &p.continuation.request().inventory.sources,
            )),
        }
    }
    pub fn checked_withdrawals(&self) -> Vec<&Arc<VerifiedOwnedTransitionExecution>> {
        self.epochs
            .iter()
            .filter_map(|epoch| match self.recipes.get(&epoch.output) {
                Some(OwnedExecutionRecipe::Transition(p))
                    if p.manifest.transition == ScopeTransition::Withdrawal =>
                {
                    Some(p)
                }
                _ => None,
            })
            .collect()
    }
    /// Every boundary is an exact authenticated data snapshot. The shared-row
    /// max-flow proof finishes at that boundary before the next segment reseeds.
    pub fn validate_endpoint(
        &self,
        snapshot: &ProjectSnapshot,
        data: &DataSnapshot,
        day: i32,
        revision: u64,
        meter: &mut ProofMeter,
    ) -> Result<()> {
        self.check_snapshot(snapshot)?;
        for (index, epoch) in self.epochs.iter().enumerate() {
            if epoch.adoption_revision > revision {
                return Err(err("history endpoint predates registered producer"));
            }
            let recipe = self
                .recipes
                .get(&epoch.output)
                .ok_or_else(|| err("registered producer recipe missing"))?;
            let (endpoint, endpoint_day, endpoint_revision) =
                if let Some(next) = self.epochs.get(index + 1) {
                    let next_recipe = self
                        .recipes
                        .get(&next.output)
                        .ok_or_else(|| err("next producer missing"))?;
                    (
                        next_recipe.seed(),
                        next_recipe.basis().clock_day,
                        next_recipe.basis().revision,
                    )
                } else {
                    (data, day, revision)
                };
            owned_epoch::validate_interval(
                snapshot,
                recipe,
                epoch.adoption_revision,
                endpoint_revision,
                endpoint,
                endpoint_day,
                meter,
            )?;
        }
        Ok(())
    }
    /// Additional withdrawal-only value induction over actual protected history.
    /// Pure P4 validity does not call this stricter eligibility check.
    pub fn validate_policy_frames(
        &self,
        snapshot: &ProjectSnapshot,
        surface: &owned_continuation::VerifiedOwnedPolicySurface,
        meter: &mut ProofMeter,
    ) -> Result<()> {
        self.check_snapshot(snapshot)?;
        let state = snapshot.owned_scope()?;
        if !state.layers.contains_key(surface.original_layer()) || self.epochs.is_empty() {
            return Err(err(
                "policy surface is not bound to this committed original layer",
            ));
        }
        // The type cannot be caller-constructed; the source still has to belong
        // to this exact reconstructed prefix, including provenance.
        let source = program(snapshot, surface.source())?;
        self.resolve(source)?;
        for (index, epoch) in self.epochs.iter().enumerate() {
            let recipe = self
                .recipes
                .get(&epoch.output)
                .ok_or_else(|| err("policy producer recipe missing"))?;
            let (endpoint, day) = if let Some(next) = self.epochs.get(index + 1) {
                let next_recipe = self
                    .recipes
                    .get(&next.output)
                    .ok_or_else(|| err("policy successor recipe missing"))?;
                (next_recipe.seed(), next_recipe.basis().clock_day)
            } else {
                (&snapshot.data, snapshot.clock_day)
            };
            let mut visit = |frame: &DataSnapshot, meter: &mut ProofMeter| {
                meter.charge(canonical_bytes(frame)?.len() as u64)?;
                surface.validate_frame(frame)
            };
            owned_replay::validate_recipe_history_with(
                recipe,
                recipe.seed(),
                recipe.basis().clock_day,
                endpoint,
                day,
                meter,
                &mut visit,
            )?;
        }
        Ok(())
    }
    /// Authenticate an archived frame under the prefix ending at its genuine
    /// producer. No future implementation may lend event capacity or provenance.
    pub fn validate_historical_frame(
        &self,
        snapshot: &ProjectSnapshot,
        source: &CapturedProgram,
        data: &DataSnapshot,
        day: i32,
        meter: &mut ProofMeter,
    ) -> Result<()> {
        self.check_snapshot(snapshot)?;
        let output = revision(source)?;
        let index = self
            .epochs
            .iter()
            .position(|e| e.output == output)
            .ok_or_else(|| err("historical source is not a committed producer"))?;
        self.resolve(source)?;
        for (i, epoch) in self.epochs.iter().enumerate().take(index + 1) {
            let recipe = self
                .recipes
                .get(&epoch.output)
                .ok_or_else(|| err("historical producer missing"))?;
            let (endpoint, endpoint_day, end_revision) = if i < index {
                let next = self
                    .epochs
                    .get(i + 1)
                    .ok_or_else(|| err("historical successor missing"))?;
                let next_recipe = self
                    .recipes
                    .get(&next.output)
                    .ok_or_else(|| err("historical successor recipe missing"))?;
                (
                    next_recipe.seed(),
                    next_recipe.basis().clock_day,
                    next_recipe.basis().revision,
                )
            } else {
                let end = self
                    .epochs
                    .get(i + 1)
                    .map(|e| e.adoption_revision - 1)
                    .unwrap_or(snapshot.revision);
                (data, day, end)
            };
            owned_epoch::validate_interval(
                snapshot,
                recipe,
                epoch.adoption_revision,
                end_revision,
                endpoint,
                endpoint_day,
                meter,
            )?;
        }
        Ok(())
    }
}
fn reconstruct_transition(
    snapshot: &ProjectSnapshot,
    current: &ProjectSnapshot,
    registry: &ReconstructedOwned,
    layer: &OwnedLayerV2,
    initialization: &OwnedInitializationV2,
    manifest: &OwnedCompositionV2,
    meter: &mut ProofMeter,
) -> Result<VerifiedOwnedTransitionExecution> {
    let last = registry
        .epochs
        .last()
        .ok_or_else(|| err("transition has no committed predecessor"))?;
    let predecessor = registry
        .manifests
        .get(&last.output)
        .ok_or_else(|| err("predecessor manifest missing"))?;
    let original = registry
        .proofs
        .get(
            &registry
                .epochs
                .first()
                .ok_or_else(|| err("original epoch missing"))?
                .output,
        )
        .ok_or_else(|| err("original initializer recipe missing"))?
        .clone();
    let id = layer.identity()?;
    if manifest.version != OWNED_SCOPE_VERSION
        || manifest.lowering != OWNED_TRANSITION_LOWERING_VERSION
        || manifest.runtime != RUNTIME_VERSION
        || manifest.driver != DRIVER_VERSION
        || !valid_id(&manifest.operation)
        || manifest.previous.as_ref() != Some(&last.output)
        || current.active_revision != last.output
        || manifest.layers != [id.clone()]
        || manifest.basis.frozen != basis(current)?
        || manifest.basis.graph != current.decisions
        || manifest.basis.artifacts != current.artifacts
        || manifest.basis.sources
            != current
                .programs
                .iter()
                .map(revision)
                .collect::<std::result::Result<Vec<_>, _>>()?
        || manifest.basis.family != OwnedBasisFamilyV2::OwnedV2
    {
        return Err(err(
            "transition dispatch, immediate predecessor or complete basis differs",
        ));
    }
    let (prior_before, prior_after, prior_sources) =
        registry.effective_pair(snapshot, &last.output)?;
    let (before, after, sources) = match manifest.transition {
        ScopeTransition::Evolution => {
            if manifest.active != predecessor.active {
                return Err(err("implementation changed active layer set"));
            }
            let [correspondence] = manifest.correspondences.as_slice() else {
                return Err(err(
                    "evolution needs one exact implementation correspondence",
                ));
            };
            if correspondence.version != OWNED_SCOPE_VERSION
                || correspondence.source != last.output
                || correspondence.target != manifest.business
                || correspondence.sources.after != manifest.business
                || correspondence.sources.root_entity != layer.request.inventory.sources.root_entity
            {
                return Err(err("evolution ordinary pair correspondence differs"));
            }
            (
                program(snapshot, &correspondence.sources.before)?,
                program(snapshot, &correspondence.sources.after)?,
                &correspondence.sources,
            )
        }
        ScopeTransition::Withdrawal => {
            if predecessor.active != BTreeSet::from([id])
                || !manifest.active.is_empty()
                || manifest.correspondences != predecessor.correspondences
                || manifest.business != predecessor.business
            {
                return Err(err(
                    "withdrawal changed effective correspondence or inactive lineage",
                ));
            }
            (prior_before, prior_after, prior_sources)
        }
        _ => return Err(err("unsupported owned transition kind")),
    };
    let continuation = owned_continuation::verify(
        current,
        &original,
        layer,
        initialization,
        prior_before,
        prior_after,
        before,
        after,
        sources,
        meter,
    )?;
    let (target, metadata) = owned_transition_lowering::compile(
        &continuation,
        manifest,
        !manifest.active.is_empty(),
        meter,
    )?;
    if revision(&target)? != manifest.output || program(snapshot, &manifest.output)? != &target {
        return Err(err("transition source bytes/provenance do not regenerate"));
    }
    Ok(VerifiedOwnedTransitionExecution {
        continuation: Arc::new(continuation),
        target,
        metadata,
        manifest: manifest.clone(),
        original,
        initialization: initialization.clone(),
    })
}

pub(super) fn transition(
    current: &ProjectSnapshot,
    before: &CapturedProgram,
    after: &CapturedProgram,
    sources: &OwnedSourceRequest,
    layer_id: &Digest,
    operation: &str,
    withdrawal: bool,
    meter: &mut ProofMeter,
) -> Result<Arc<VerifiedOwnedTransitionExecution>> {
    let registry = reconstruct(current, meter)?;
    transition_from_registry(
        current, &registry, before, after, sources, layer_id, operation, withdrawal, meter,
    )
}
pub(super) fn transition_from_registry(
    current: &ProjectSnapshot,
    registry: &ReconstructedOwned,
    before: &CapturedProgram,
    after: &CapturedProgram,
    sources: &OwnedSourceRequest,
    layer_id: &Digest,
    operation: &str,
    withdrawal: bool,
    meter: &mut ProofMeter,
) -> Result<Arc<VerifiedOwnedTransitionExecution>> {
    registry.check_snapshot(current)?;
    let state = current.owned_scope()?;
    let layer = state
        .layers
        .get(layer_id)
        .ok_or_else(|| err("requested original layer missing"))?;
    let initialization = state
        .initializations
        .iter()
        .find(|i| &i.layer == layer_id)
        .ok_or_else(|| err("requested original initializer missing"))?;
    let last = registry
        .epochs
        .last()
        .ok_or_else(|| err("continuation requires a committed owned layer"))?;
    let predecessor = registry
        .manifests
        .get(&last.output)
        .ok_or_else(|| err("committed predecessor missing"))?;
    if current.operations.contains_key(operation) {
        return Err(StoreError::Conflict(
            "owned transition operation ID was already used".into(),
        ));
    }
    if current.active_revision != last.output || !valid_id(operation) {
        return Err(err("invalid or noncurrent transition operation"));
    }
    let (prior_before, prior_after, prior_sources) =
        registry.effective_pair(current, &last.output)?;
    if withdrawal
        && (predecessor.active != BTreeSet::from([layer_id.clone()])
            || before != prior_before
            || after != prior_after
            || sources != prior_sources)
    {
        return Err(err(
            "withdrawal is inactive or changed its exact effective pair",
        ));
    }
    let original = registry
        .proofs
        .get(
            &registry
                .epochs
                .first()
                .ok_or_else(|| err("original epoch missing"))?
                .output,
        )
        .ok_or_else(|| err("original preview missing"))?
        .clone();
    let continuation = owned_continuation::verify(
        current,
        &original,
        layer,
        initialization,
        prior_before,
        prior_after,
        before,
        after,
        sources,
        meter,
    )?;
    let mut manifest = OwnedCompositionV2 {
        version: OWNED_SCOPE_VERSION,
        lowering: OWNED_TRANSITION_LOWERING_VERSION,
        basis: OwnedBasisV2 {
            container_format: super::super::FORMAT,
            family: OwnedBasisFamilyV2::OwnedV2,
            frozen: basis(current)?,
            sources: current
                .programs
                .iter()
                .map(revision)
                .collect::<std::result::Result<_, _>>()?,
            graph: current.decisions.clone(),
            artifacts: current.artifacts.clone(),
        },
        transition: if withdrawal {
            ScopeTransition::Withdrawal
        } else {
            ScopeTransition::Evolution
        },
        business: revision(after)?,
        layers: vec![layer_id.clone()],
        active: if withdrawal {
            BTreeSet::new()
        } else {
            predecessor.active.clone()
        },
        previous: Some(last.output.clone()),
        operation: operation.into(),
        runtime: RUNTIME_VERSION.into(),
        driver: DRIVER_VERSION.into(),
        output: last.output.clone(), // Excluded by lowering3's recipe digest.
        correspondences: if withdrawal {
            predecessor.correspondences.clone()
        } else {
            vec![OwnedImplementationCorrespondenceV2 {
                version: OWNED_SCOPE_VERSION,
                source: last.output.clone(),
                target: revision(after)?,
                sources: sources.clone(),
            }]
        },
    };
    let (target, metadata) = owned_transition_lowering::compile(
        &continuation,
        &manifest,
        !manifest.active.is_empty(),
        meter,
    )?;
    manifest.output = revision(&target)?;
    Ok(Arc::new(VerifiedOwnedTransitionExecution {
        continuation: Arc::new(continuation),
        target,
        metadata,
        manifest,
        original,
        initialization: initialization.clone(),
    }))
}

/// Extract proposed arguments only. Callers must pass them to `transition`,
/// which reconstructs the complete family before granting any authority.
pub(super) fn effective_pair<'a>(
    snapshot: &'a ProjectSnapshot,
    target: &Digest,
) -> Result<(
    &'a CapturedProgram,
    &'a CapturedProgram,
    &'a OwnedSourceRequest,
)> {
    let state = snapshot.owned_scope()?;
    let manifest = state
        .compositions
        .values()
        .find(|m| &m.output == target)
        .ok_or_else(|| err("effective manifest missing"))?;
    let sources = if manifest.transition == ScopeTransition::Adoption {
        let [id] = manifest.layers.as_slice() else {
            return Err(err("effective original layer ambiguous"));
        };
        &state
            .layers
            .get(id)
            .ok_or_else(|| err("effective original layer missing"))?
            .request
            .inventory
            .sources
    } else if let [correspondence] = manifest.correspondences.as_slice() {
        &correspondence.sources
    } else if manifest.transition == ScopeTransition::Withdrawal
        && manifest.correspondences.is_empty()
    {
        let [id] = manifest.layers.as_slice() else {
            return Err(err("effective original layer ambiguous"));
        };
        &state
            .layers
            .get(id)
            .ok_or_else(|| err("effective original layer missing"))?
            .request
            .inventory
            .sources
    } else {
        return Err(err("effective ordinary source pair ambiguous"));
    };
    Ok((
        program(snapshot, &sources.before)?,
        program(snapshot, &sources.after)?,
        sources,
    ))
}

// Assertion-only fixture bridge: no proof, source, registry or write capability
// leaves this cfg(test) method. It probes one real private mechanism after full
// reconstruction of the unmodified two-epoch fixture has already succeeded.
#[cfg(test)]
impl ProjectSnapshot {
    pub(crate) fn assert_owned_epoch_interval_rejects_unique_swapped_receipts_for_test(&self) {
        self.validate_container().unwrap();
        let mut meter =
            ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false))).unwrap();
        let registry = reconstruct(self, &mut meter).unwrap();
        assert_eq!(
            registry.epochs.len(),
            2,
            "bridge requires the genuine two-epoch fixture"
        );
        let epoch = &registry.epochs[0];
        let recipe = registry.recipes.get(&epoch.output).unwrap();
        let next = registry.recipes.get(&registry.epochs[1].output).unwrap();
        let endpoint = next.seed();
        let endpoint_revision = next.basis().revision;
        let old_revision = self.operations["old-complete"].revision;
        let new_revision = self.operations["new-epoch-add"].revision;
        assert!(epoch.adoption_revision < old_revision && old_revision <= endpoint_revision);
        assert!(new_revision > endpoint_revision);
        assert!(endpoint.events[recipe.seed().events.len()..]
            .iter()
            .any(|event| event.operation_id == "old-complete"
                && event.program == recipe.target().artifact.program_digest));
        assert!(self.data.events[endpoint.events.len()..]
            .iter()
            .any(|event| event.operation_id == "new-epoch-add"
                && event.program == next.target().artifact.program_digest));
        let mut swapped = self.clone();
        swapped.operations.get_mut("old-complete").unwrap().revision = new_revision;
        swapped
            .operations
            .get_mut("new-epoch-add")
            .unwrap()
            .revision = old_revision;
        validate_revision_ownership(&swapped).unwrap();
        let error = owned_epoch::validate_interval(
            &swapped,
            recipe,
            epoch.adoption_revision,
            endpoint_revision,
            endpoint,
            next.basis().clock_day,
            &mut meter,
        )
        .unwrap_err();
        assert!(
            matches!(&error, StoreError::Invalid(message)
            if message == "Common factual admission: owned producer epoch: event is outside its actual producer operation interval"),
            "{error}"
        );
    }
}

#[cfg(test)]
#[path = "owned_persist_registry_controls.rs"]
mod registry_policy_controls;
