use super::*;
use compiler::error;

struct AdoptionHistory<'a> {
    links: BTreeMap<Digest, (&'a AdoptionReceipt, &'a ScopedAdoptionReceipt)>,
}
#[derive(Default)]
struct DecisionBirths {
    active: BTreeSet<Id>,
    pending: BTreeSet<Id>,
}

/// Recover only the host's existing graph transitions. Exact prior graph
/// digests anchor births independently of editable required-ID arrays. The
/// temporary reverse walk never changes retained terminal history.
fn decision_births(snapshot: &ProjectSnapshot) -> Result<BTreeMap<u64, DecisionBirths>> {
    let mut graph = snapshot.decisions.clone();
    let mut graph_identity = graph.identity()?;
    let mut births = BTreeMap::new();
    for adoption in snapshot.adoptions.iter().rev() {
        if !snapshot
            .scope
            .adoptions
            .iter()
            .any(|receipt| receipt.revision == adoption.revision)
        {
            rehearsal::verify_recording_request(snapshot, adoption, &graph)?;
        }
        let required: BTreeSet<_> = graph
            .decisions
            .iter()
            .filter(|decision| decision.status == DecisionStatus::Active)
            .map(|decision| decision.id.clone())
            .collect();
        if required != adoption.plan.required_decisions.iter().cloned().collect() {
            return Err(error(
                "adoption requirements differ from its exact committed graph",
            ));
        }
        let mut added = DecisionBirths::default();
        if graph_identity != adoption.plan.expected_decisions {
            graph.revision = graph
                .revision
                .checked_sub(1)
                .ok_or_else(|| error("decision graph revision has no predecessor"))?;
            for id in &adoption.plan.retire_decisions {
                let decision = graph
                    .decisions
                    .iter_mut()
                    .find(|decision| &decision.id == id)
                    .ok_or_else(|| error("retired decision missing from retained graph"))?;
                match &decision.status {
                    DecisionStatus::Withdrawn {
                        adoption: operation,
                    } if operation == &adoption.plan.id => {}
                    DecisionStatus::Superseded { .. } => {}
                    _ => return Err(error("retirement differs from its graph transition")),
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
                    .ok_or_else(|| error("retired decision has no prior revision"))?;
            }
            // Each appended ID is removed once across this bounded walk.
            // Validate reciprocity after matching the prior graph: a successor
            // is temporarily present while undoing its retirement edge.
            while canonical_digest(IdentityDomain::Decision, &graph)?
                != adoption.plan.expected_decisions
            {
                let decision = graph
                    .decisions
                    .pop()
                    .ok_or_else(|| error("retained prior decision graph does not regenerate"))?;
                if decision.revision != 1
                    || adoption.plan.retire_decisions.contains(&decision.id)
                    || !matches!(
                        decision.status,
                        DecisionStatus::Active | DecisionStatus::Pending
                    )
                {
                    return Err(error("decision birth rewrites earlier terminal history"));
                }
                if decision.status == DecisionStatus::Active {
                    added.active.insert(decision.id);
                } else {
                    added.pending.insert(decision.id);
                }
            }
            graph.validate()?;
            graph_identity = adoption.plan.expected_decisions.clone();
        } else if !adoption.plan.retire_decisions.is_empty() {
            return Err(error("unchanged graph cannot claim a retirement"));
        }
        births.insert(adoption.revision, added);
    }
    if graph.revision != 0 || !graph.decisions.is_empty() {
        return Err(error("decision graph lacks its original empty checkpoint"));
    }
    Ok(births)
}

/// One bounded map connects plans, exact captures, operation revisions,
/// scoped receipts, decision births and the live tail. Local digests do not
/// attest approval against wholesale replacement of the store and its evidence.
fn adoption_history(snapshot: &ProjectSnapshot) -> Result<AdoptionHistory<'_>> {
    let state = &snapshot.scope;
    let mut history = AdoptionHistory {
        links: BTreeMap::new(),
    };
    if state.layers.is_empty() && state.rehearsals.is_empty() && state.correspondences.is_empty() {
        return Ok(history);
    }
    let sources: BTreeMap<_, _> = snapshot
        .programs
        .iter()
        .map(|source| Ok((revision(source)?, source)))
        .collect::<Result<_>>()?;
    let mut manifests = BTreeMap::new();
    for (output, manifest) in &state.compositions {
        if manifests
            .insert(
                canonical_digest(IdentityDomain::Adoption, manifest)?,
                output,
            )
            .is_some()
        {
            return Err(error("ambiguous composition identity"));
        }
    }
    let mut receipts = BTreeMap::new();
    for receipt in &state.adoptions {
        if receipts.insert(receipt.revision, receipt).is_some() {
            return Err(error("duplicate scoped adoption revision link"));
        }
    }
    let births = decision_births(snapshot)?;
    let mut active = revision(
        snapshot
            .programs
            .first()
            .ok_or_else(|| error("initial source missing"))?,
    )?;
    let mut operations = BTreeSet::new();
    for linked in &snapshot.adoptions {
        let previous = sources
            .get(&active)
            .ok_or_else(|| error("prior active capture missing"))?;
        let target = sources
            .get(&linked.active)
            .ok_or_else(|| error("adopted capture missing"))?;
        if linked.previous != active
            || linked.plan.current_source != previous.binding
            || linked.plan.target != target.artifact
            || !operations.insert(&linked.plan.id)
            || snapshot
                .operations
                .get(&linked.plan.id)
                .map(|operation| operation.revision)
                != Some(linked.revision)
        {
            return Err(error(
                "adoption is inconsistent with its ordered source and operation history",
            ));
        }
        if let Some(receipt) = receipts.get(&linked.revision) {
            let output = manifests
                .get(&receipt.composition)
                .ok_or_else(|| error("scoped receipt has no composition"))?;
            let manifest = &state.compositions[*output];
            if linked.active != **output
                || linked.plan.identity()? != receipt.plan
                || linked.plan.id != manifest.operation
                || manifest.basis.active != active
                || linked.plan.expected_data != manifest.basis.data.identity()?
                || linked.plan.expected_generation != manifest.basis.data.generation
                || linked.plan.expected_session != manifest.basis.session.identity()?
                || linked.plan.expected_decisions != manifest.basis.decisions
                || manifest.basis.revision.checked_add(1) != Some(linked.revision)
                || history
                    .links
                    .insert((*output).clone(), (linked, *receipt))
                    .is_some()
            {
                return Err(error(
                    "scoped plan differs from its exact history checkpoint",
                ));
            }
            let expected: BTreeSet<_> = snapshot
                .decisions
                .decisions
                .iter()
                .filter(|decision| {
                    births[&linked.revision].active.contains(&decision.id)
                        && decision.scope == linked.plan.scope
                })
                .map(|decision| decision.id.clone())
                .collect();
            let mut actual = BTreeSet::new();
            for id in &receipt.decisions {
                let decision = snapshot
                    .decisions
                    .decisions
                    .iter()
                    .find(|decision| &decision.id == id)
                    .ok_or_else(|| error("scoped receipt decision missing"))?;
                if !actual.insert(id.clone())
                    || !matches!(&decision.outcome, DecisionOutcome::Accept { artifact } if artifact == &target.artifact.program_digest)
                {
                    return Err(error(
                        "scoped decision differs from its exact accepted artifact",
                    ));
                }
            }
            if actual != expected {
                return Err(error(
                    "scoped receipt differs from the exact newly activated intention set",
                ));
            }
        } else if linked.active != active
            && (state.compositions.contains_key(&active)
                || state.compositions.contains_key(&linked.active))
        {
            return Err(error(
                "managed source change has no scoped adoption receipt",
            ));
        }
        active = linked.active.clone();
    }
    let mut rehearsal_operations: BTreeMap<&Id, Vec<&ManagedRehearsal>> = BTreeMap::new();
    for proof in state.rehearsals.values() {
        rehearsal_operations
            .entry(&proof.recorded_by)
            .or_default()
            .push(proof);
        let witnesses = proof.witnesses.keys().cloned().collect();
        if births
            .get(&proof.recorded_revision)
            .map(|born| &born.pending)
            != Some(&witnesses)
        {
            return Err(error(
                "rehearsal witnesses differ from their exact recording decision births",
            ));
        }
    }
    for proofs in rehearsal_operations.values() {
        if proofs.len() > 2
            || (proofs.len() == 2
                && proofs.iter().any(|proof| {
                    proof.manifest.transition != ScopeTransition::Evolution
                        || proof.manifest.basis != proofs[0].manifest.basis
                        || proof.manifest.layers != proofs[0].manifest.layers
                        || proof.manifest.active != proofs[0].manifest.active
                        || proof.witnesses != proofs[0].witnesses
                        || proof.recorded_revision != proofs[0].recorded_revision
                }))
        {
            return Err(error(
                "paired rehearsal has ambiguous recording or preservation authority",
            ));
        }
    }
    if active != snapshot.active_revision
        || history.links.len() != state.compositions.len()
        || history.links.len() != receipts.len()
    {
        return Err(error(
            "active source or scoped inventory differs from the ordered history tail",
        ));
    }
    Ok(history)
}

pub(super) fn validate(snapshot: &ProjectSnapshot) -> Result<()> {
    let state = &snapshot.scope;
    if state.version != COMPILER_VERSION
        || state.layers.len() > MAX_LAYERS
        || state.compositions.len() > MAX_ITEMS
        || state.initializations.len() != state.layers.len()
        || state.adoptions.len() != state.compositions.len()
        || state.rehearsals.len() > MAX_ITEMS
        || state.correspondences.len() > MAX_ITEMS
    {
        return Err(error("invalid scope-state version or inventory"));
    }
    let history = adoption_history(snapshot)?;
    // Retained-basis reconstruction consults earlier compositions before the
    // semantic composition pass below. Reject broken references first so no
    // corrupted import/restart can turn those proof lookups into indexing panics.
    for manifest in state.compositions.values() {
        if manifest
            .layers
            .iter()
            .any(|id| !state.layers.contains_key(id))
        {
            return Err(error("composition references a missing layer proof"));
        }
    }
    for proof in state.rehearsals.values() {
        let own = proof.layer.as_ref().map(ScopeLayer::identity).transpose()?;
        if proof
            .manifest
            .layers
            .iter()
            .any(|id| !state.layers.contains_key(id) && own.as_ref() != Some(id))
        {
            return Err(error("rehearsal references a missing layer proof"));
        }
    }
    let mut declared = BTreeMap::new();
    for source in &snapshot.programs {
        let source_id = revision(source)?;
        if state.compositions.contains_key(&source_id) {
            for entity in &source.program.entities {
                for field in &entity.fields {
                    if field.id.starts_with(RESERVED_PREFIX) {
                        if let Some(previous) = declared.insert(
                            (entity.id.clone(), field.id.clone()),
                            field.value_type.clone(),
                        ) {
                            if previous != field.value_type {
                                return Err(error("protected field type changed"));
                            }
                        }
                    }
                }
            }
        } else if !state.rehearsals.contains_key(&source_id) {
            compiler::reject_reserved(&source.program)?;
        }
    }
    for entity in &snapshot.data.schema {
        for field in &entity.fields {
            if field.id.starts_with(RESERVED_PREFIX)
                && declared.get(&(entity.id.clone(), field.id.clone())) != Some(&field.value_type)
            {
                return Err(error("unregistered protected storage field"));
            }
        }
    }
    if state.layers.is_empty() {
        if !state.compositions.is_empty()
            || !state.initializations.is_empty()
            || !state.adoptions.is_empty()
        {
            return Err(error("orphan composition metadata"));
        }
        compiler::reject_reserved(&snapshot.program()?.program)?;
        rehearsal::validate(snapshot)?;
        correspondence::validate(snapshot)?;
        return Ok(());
    }
    if !state.compositions.contains_key(&snapshot.active_revision) {
        return Err(error(
            "active source has lost its protected history envelope",
        ));
    }
    let mut initialized = BTreeSet::new();
    let mut operations = BTreeSet::new();
    for receipt in &state.initializations {
        if !initialized.insert(&receipt.layer) || !operations.insert(&receipt.operation) {
            return Err(error("duplicate metadata initialization"));
        }
    }
    for (id, layer) in &state.layers {
        if layer.version != COMPILER_VERSION
            || layer.identity()? != *id
            || layer.operation.is_empty()
        {
            return Err(error("layer identity mismatch"));
        }
        layer.basis.data.validate()?;
        if layer.basis.data.project_id != snapshot.data.project_id
            || layer.basis.revision >= snapshot.revision
        {
            return Err(error("foreign or future scope basis"));
        }
        let candidate = program(snapshot, &layer.candidate)?;
        compiler::reject_reserved(&candidate.program)?;
        let mut basis = snapshot.clone();
        basis.active_revision = layer.basis.active.clone();
        basis.data = layer.basis.data.clone();
        basis.session = layer.basis.session.clone();
        basis.clock_day = layer.basis.day;
        basis.revision = layer.basis.revision;
        request_valid(&basis, &layer.request)?;
        let before = compiler::business_program(&basis)?;
        if compiler::derive_patches(&before, &candidate.program, &layer.request)? != layer.patches {
            return Err(error("layer patch proof does not independently regenerate"));
        }
        let receipt = state
            .initializations
            .iter()
            .find(|r| &r.layer == id)
            .ok_or_else(|| error("layer has no initialization"))?;
        let adoption = snapshot
            .adoptions
            .iter()
            .find(|a| a.plan.id == layer.operation)
            .ok_or_else(|| error("layer has no atomic adoption"))?;
        if adoption.previous != layer.basis.active
            || adoption.revision != receipt.adoption_revision
            || adoption.plan.expected_data != layer.basis.data.identity()?
            || adoption.plan.expected_generation != layer.basis.data.generation
            || adoption.plan.expected_session != layer.basis.session.identity()?
            || adoption.plan.expected_decisions != layer.basis.decisions
            || adoption.plan.scope != layer.scope()
        {
            return Err(error("layer/adoption basis linkage differs"));
        }
        let target = program(snapshot, &adoption.active)?;
        if history::initialize(snapshot, layer, target)?.1 != *receipt {
            return Err(error(
                "metadata initialization does not independently regenerate",
            ));
        }
    }
    for (output, manifest) in &state.compositions {
        if manifest.version != COMPILER_VERSION
            || &manifest.output != output
            || manifest.layers.len() > MAX_LAYERS
            || manifest.layers.is_empty()
            || manifest.runtime != RUNTIME_VERSION
            || manifest.driver != DRIVER_VERSION
        {
            return Err(error("unsupported composition or stale runtime"));
        }
        let unique: BTreeSet<_> = manifest.layers.iter().collect();
        if unique.len() != manifest.layers.len()
            || manifest
                .active
                .iter()
                .any(|id| !manifest.layers.contains(id))
        {
            return Err(error("ambiguous active layer inventory"));
        }
        let expected_previous = state.compositions.get(&manifest.basis.active);
        if manifest.previous.as_ref() != expected_previous.map(|_| &manifest.basis.active) {
            return Err(error(
                "composition predecessor differs from its exact adopted basis",
            ));
        }
        if let Some(previous) = &manifest.previous {
            let prior = state
                .compositions
                .get(previous)
                .ok_or_else(|| error("composition predecessor missing"))?;
            if !manifest.layers.starts_with(&prior.layers) {
                return Err(error("composition removed protected history"));
            }
        }
        manifest.basis.data.validate()?;
        if manifest.basis.revision >= snapshot.revision
            || manifest.basis.data.project_id != snapshot.data.project_id
        {
            return Err(error("invalid composition basis"));
        }
        match manifest.transition {
            ScopeTransition::Evolution => {
                let prior = manifest
                    .previous
                    .as_ref()
                    .and_then(|id| state.compositions.get(id))
                    .ok_or_else(|| error("evolution predecessor missing"))?;
                if manifest.layers != prior.layers || manifest.active != prior.active {
                    return Err(error("evolution changed independent active scope layers"));
                }
                compiler::validate_mappings(
                    snapshot,
                    prior,
                    program(snapshot, &manifest.business)?,
                    &manifest.rewrites,
                )?;
            }
            ScopeTransition::Withdrawal => {
                let prior = manifest
                    .previous
                    .as_ref()
                    .and_then(|id| state.compositions.get(id))
                    .ok_or_else(|| error("withdrawal predecessor missing"))?;
                if manifest.rewrites != prior.rewrites {
                    return Err(error("withdrawal changed protected mappings"));
                }
            }
            ScopeTransition::Adoption => {
                let prior = manifest
                    .previous
                    .as_ref()
                    .and_then(|id| state.compositions.get(id));
                if manifest.rewrites != prior.map(|p| p.rewrites.clone()).unwrap_or_default() {
                    return Err(error("scoped adoption changed prior mappings"));
                }
                let new = state
                    .layers
                    .values()
                    .find(|l| l.operation == manifest.operation)
                    .ok_or_else(|| error("adoption layer missing"))?;
                if manifest.business != new.candidate || manifest.basis != new.basis {
                    return Err(error(
                        "adoption composition differs from its exact layer candidate or basis",
                    ));
                }
                let mut expected = prior.map(|p| p.layers.clone()).unwrap_or_default();
                expected.push(new.identity()?);
                let mut active = prior.map(|p| p.active.clone()).unwrap_or_default();
                active.insert(new.identity()?);
                if manifest.layers != expected || manifest.active != active {
                    return Err(error("adoption changed an independent layer"));
                }
            }
        }
        let target = program(snapshot, output)?;
        if compiler::compile(snapshot, manifest)? != *target {
            return Err(error(
                "compiled source differs from independently regenerated envelope",
            ));
        }
        let (linked, matching) = history
            .links
            .get(output)
            .copied()
            .ok_or_else(|| error("composition lacks its validated history link"))?;
        if LocalRuntime::default().compatibility_at(
            target,
            &manifest.basis.data,
            manifest.basis.day,
        )? != linked.plan.compatibility
        {
            return Err(error(
                "adoption compatibility differs from its frozen input",
            ));
        }
        let layer = state
            .layers
            .values()
            .find(|l| l.operation == manifest.operation);
        let data = if let Some(layer) = layer {
            if manifest.transition != ScopeTransition::Adoption {
                return Err(error("layer receipt attached to the wrong transition"));
            }
            let (data, initialization) = history::initialize(snapshot, layer, target)?;
            if matching.initialization
                != Some(canonical_digest(IdentityDomain::Adoption, &initialization)?)
            {
                return Err(error("initialization receipt link mismatch"));
            }
            data
        } else {
            if matching.initialization.is_some() {
                return Err(error("withdrawal must not initialize metadata"));
            }
            let previous = manifest
                .previous
                .as_ref()
                .and_then(|id| state.compositions.get(id))
                .ok_or_else(|| error("withdrawal predecessor missing"))?;
            if manifest.transition == ScopeTransition::Withdrawal
                && (manifest.business != previous.business
                    || manifest.layers != previous.layers
                    || !manifest.active.is_subset(&previous.active)
                    || manifest.active == previous.active)
            {
                return Err(error("withdrawal changed independent layers or source"));
            }
            merged_data(target, &manifest.basis.data)?
        };
        let report = LocalRuntime::default().compatibility_at(target, &data, manifest.basis.day)?;
        if report.state != CompatibilityState::Compatible
            || report != matching.initialized_compatibility
        {
            return Err(error("initialized-data compatibility receipt mismatch"));
        }
        validate_initial_runtime(target, &data, manifest.basis.day)?;
    }
    rehearsal::validate(snapshot)?;
    correspondence::validate(snapshot)?;
    history::verify_history(snapshot)
}

pub(super) fn preserve(before: &ProjectSnapshot, after: &ProjectSnapshot) -> Result<()> {
    if before.scope != after.scope {
        return Err(error(
            "ordinary action changed protected composition history",
        ));
    }
    if !after.data.events.starts_with(&before.data.events)
        || !after.artifacts.starts_with(&before.artifacts)
    {
        return Err(error("ordinary action rewrote committed facts"));
    }
    history::verify_history(after)
}
