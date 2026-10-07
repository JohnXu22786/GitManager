use super::*;
use compiler::error;

pub(super) fn validate(snapshot: &ProjectSnapshot) -> Result<()> {
    let state = &snapshot.scope;
    if state.version != COMPILER_VERSION
        || state.layers.len() > MAX_LAYERS
        || state.compositions.len() > MAX_ITEMS
        || state.initializations.len() != state.layers.len()
        || state.adoptions.len() > MAX_ITEMS
        || state.rehearsals.len() > MAX_ITEMS
        || state.correspondences.len() > MAX_ITEMS
    {
        return Err(error("invalid scope-state version or inventory"));
    }
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
        let manifest_identity = canonical_digest(IdentityDomain::Adoption, manifest)?;
        let matching: Vec<_> = state
            .adoptions
            .iter()
            .filter(|a| a.composition == manifest_identity)
            .collect();
        if matching.len() != 1 {
            return Err(error("composition needs exactly one atomic receipt"));
        }
        let linked = snapshot
            .adoptions
            .iter()
            .find(|a| a.revision == matching[0].revision)
            .ok_or_else(|| error("scoped receipt lacks adoption"))?;
        if linked.active != *output
            || linked.plan.identity()? != matching[0].plan
            || linked.previous != manifest.basis.active
            || linked.plan.expected_data != manifest.basis.data.identity()?
            || linked.plan.expected_session != manifest.basis.session.identity()?
            || linked.plan.expected_decisions != manifest.basis.decisions
            || linked.revision != manifest.basis.revision + 1
        {
            return Err(error("scoped plan link mismatch"));
        }
        let mut decision_ids = BTreeSet::new();
        for id in &matching[0].decisions {
            let decision = snapshot
                .decisions
                .decisions
                .iter()
                .find(|d| &d.id == id)
                .ok_or_else(|| error("scoped receipt decision missing"))?;
            if !decision_ids.insert(id)
                || decision.scope != linked.plan.scope
                || !matches!(&decision.outcome,DecisionOutcome::Accept{artifact} if artifact==&target.artifact.program_digest)
            {
                return Err(error(
                    "scoped decision does not bind this exact frozen outcome",
                ));
            }
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
            if matching[0].initialization
                != Some(canonical_digest(IdentityDomain::Adoption, &initialization)?)
            {
                return Err(error("initialization receipt link mismatch"));
            }
            data
        } else {
            if matching[0].initialization.is_some() {
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
            || report != matching[0].initialized_compatibility
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
