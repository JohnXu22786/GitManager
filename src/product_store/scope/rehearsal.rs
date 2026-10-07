//! Bounded replay authority for experienced nonbinary managed alternatives.
//! Recording this proof never activates its executable or changes live data.
use super::*;
use compiler::error;

fn nonbinary(outcome: &DecisionOutcome) -> bool {
    matches!(
        outcome,
        DecisionOutcome::EitherAcceptable
            | DecisionOutcome::BothNeeded
            | DecisionOutcome::NeitherFits
            | DecisionOutcome::Deferred
    )
}
pub(super) fn eligible(prepared: &PreparedScopedChange) -> Result<()> {
    let valid = match prepared.manifest.transition {
        ScopeTransition::Evolution => {
            prepared.layer.is_none() && prepared.receipt.is_none() && prepared.evolution.is_some()
        }
        ScopeTransition::Adoption => {
            prepared.layer.is_some() && prepared.receipt.is_some() && prepared.evolution.is_none()
        }
        ScopeTransition::Withdrawal => false,
    };
    if !valid {
        return Err(error(
            "rehearsal needs an exact evolution or new-layer preparation",
        ));
    }
    Ok(())
}
/// Stable identity also used for exact retries after a lost acknowledgement.
pub(in crate::product_store) fn rehearsal_request(
    prepared: &PreparedScopedChange,
    decisions: &[Id],
) -> Result<Digest> {
    eligible(prepared)?;
    Ok(canonical_digest(
        IdentityDomain::Adoption,
        &(
            "managed-rehearsal/1",
            &prepared.basis,
            &prepared.manifest,
            &prepared.candidate,
            &prepared.target,
            prepared.initialized.identity()?,
            &prepared.compatibility,
            decisions,
        ),
    )?)
}
pub(in crate::product_store) fn retain_rehearsal(
    current: &mut ProjectSnapshot,
    prepared: &PreparedScopedChange,
    next: &DecisionGraph,
    decisions: &[Id],
    plan: &AdoptionPlan,
) -> Result<()> {
    eligible(prepared)?;
    ScopedExecutionContext::prepared(current, prepared)?;
    if plan.target != current.program()?.artifact
        || plan.current_source != current.program()?.binding
        || current.scope.rehearsals.len() >= MAX_ITEMS
        || current
            .scope
            .rehearsals
            .contains_key(&prepared.manifest.output)
        || current
            .scope
            .compositions
            .contains_key(&prepared.manifest.output)
        || decisions.is_empty()
        || decisions.len() > MAX_ITEMS
    {
        return Err(error("retained rehearsal must keep the current executable and use a fresh bounded proof identity"));
    }
    let mut witnesses = BTreeMap::new();
    for id in decisions {
        let decision = next
            .decisions
            .iter()
            .find(|d| &d.id == id)
            .ok_or_else(|| error("rehearsal decision missing"))?;
        if current.decisions.decisions.iter().any(|d| &d.id == id)
            || decision.status != DecisionStatus::Pending
            || !nonbinary(&decision.outcome)
            || witnesses
                .insert(id.clone(), decision.witness.clone())
                .is_some()
        {
            return Err(error(
                "rehearsal must bind exact newly recorded nonbinary intentions",
            ));
        }
    }
    let proof = ManagedRehearsal {
        version: COMPILER_VERSION,
        manifest: prepared.manifest.clone(),
        seed: prepared.initialized.identity()?,
        layer: prepared.layer.clone(),
        initialization: prepared.receipt.clone(),
        compatibility: prepared.compatibility.clone(),
        recorded_by: plan.id.clone(),
        recorded_revision: current
            .revision
            .checked_add(1)
            .ok_or_else(|| error("revision exhausted"))?,
        witnesses,
    };
    retain(current, &prepared.candidate)?;
    retain(current, &prepared.target)?;
    current
        .scope
        .rehearsals
        .insert(prepared.manifest.output.clone(), proof);
    Ok(())
}
pub(super) fn validate(snapshot: &ProjectSnapshot) -> Result<()> {
    if snapshot.scope.rehearsals.len() > MAX_ITEMS {
        return Err(error("rehearsal inventory exceeds bounds"));
    }
    for (output, proof) in &snapshot.scope.rehearsals {
        let manifest = &proof.manifest;
        if proof.version != COMPILER_VERSION
            || manifest.version != COMPILER_VERSION
            || &manifest.output != output
            || manifest.runtime != RUNTIME_VERSION
            || manifest.driver != DRIVER_VERSION
            || manifest.basis.revision.checked_add(1) != Some(proof.recorded_revision)
            || proof.recorded_revision > snapshot.revision
            || proof.witnesses.is_empty()
            || proof.witnesses.len() > MAX_ITEMS
            || !valid_id(&manifest.operation)
            || !valid_id(&proof.recorded_by)
        {
            return Err(error("invalid retained rehearsal identity or transition"));
        }
        let previous = snapshot.scope.compositions.get(&manifest.basis.active);
        if manifest.previous.as_ref() != previous.map(|_| &manifest.basis.active) {
            return Err(error(
                "rehearsal predecessor differs from its adopted basis",
            ));
        }
        manifest.basis.data.validate()?;
        validate_day(manifest.basis.day)?;
        let baseline = program(snapshot, &manifest.basis.active)?;
        manifest.basis.session.validate(&baseline.program)?;
        let candidate = program(snapshot, &manifest.business)?;
        compiler::reject_reserved(&candidate.program)?;
        if candidate.binding.project_id != snapshot.data.project_id
            || manifest.basis.data.project_id != snapshot.data.project_id
        {
            return Err(error("rehearsal belongs to another project"));
        }
        let mut working = snapshot.clone();
        let target = program(snapshot, output)?;
        let seed = match manifest.transition {
            ScopeTransition::Evolution => {
                let previous = previous
                    .ok_or_else(|| error("evolution rehearsal has no adopted predecessor"))?;
                if proof.layer.is_some()
                    || proof.initialization.is_some()
                    || manifest.layers != previous.layers
                    || manifest.active != previous.active
                {
                    return Err(error("evolution rehearsal changed the adopted layer set"));
                }
                compiler::validate_mappings(snapshot, previous, candidate, &manifest.rewrites)?;
                merged_data(target, &manifest.basis.data)?
            }
            ScopeTransition::Adoption => {
                let layer = proof
                    .layer
                    .as_ref()
                    .ok_or_else(|| error("prospective layer proof missing"))?;
                let id = layer.identity()?;
                let mut layers = previous.map(|p| p.layers.clone()).unwrap_or_default();
                layers.push(id.clone());
                let mut active = previous.map(|p| p.active.clone()).unwrap_or_default();
                active.insert(id.clone());
                if layer.version != COMPILER_VERSION
                    || layer.basis != manifest.basis
                    || layer.operation != manifest.operation
                    || layer.candidate != manifest.business
                    || manifest.layers != layers
                    || manifest.layers.len() > MAX_LAYERS
                    || manifest.active != active
                    || manifest.rewrites != previous.map(|p| p.rewrites.clone()).unwrap_or_default()
                    || snapshot.scope.layers.contains_key(&id)
                {
                    return Err(error(
                        "prospective layer differs from its exact prepared envelope",
                    ));
                }
                working.active_revision = manifest.basis.active.clone();
                working.data = manifest.basis.data.clone();
                working.session = manifest.basis.session.clone();
                working.clock_day = manifest.basis.day;
                working.revision = manifest.basis.revision;
                // Reconstruct the adopted layer inventory at this frozen basis.
                working.scope.layers.retain(|id, _| {
                    previous.is_some_and(|predecessor| predecessor.layers.contains(id))
                });
                request_valid(&working, &layer.request)?;
                let before = compiler::business_program(&working)?;
                if compiler::derive_patches(&before, &candidate.program, &layer.request)?
                    != layer.patches
                {
                    return Err(error(
                        "prospective layer patches do not independently regenerate",
                    ));
                }
                working.scope.layers.insert(id, layer.clone());
                let (seed, receipt) = history::initialize(&working, layer, target)?;
                if proof.initialization.as_ref() != Some(&receipt) {
                    return Err(error(
                        "prospective initialization does not independently regenerate",
                    ));
                }
                seed
            }
            ScopeTransition::Withdrawal => {
                return Err(error(
                    "withdrawal cannot be retained as rehearsal authority",
                ))
            }
        };
        if compiler::compile(&working, manifest)? != *target {
            return Err(error(
                "rehearsal executable does not independently regenerate",
            ));
        }
        if seed.identity()? != proof.seed {
            return Err(error("rehearsal seed lacks exact provenance"));
        }
        let compatibility =
            LocalRuntime::default().compatibility_at(target, &seed, manifest.basis.day)?;
        if compatibility.state != CompatibilityState::Compatible
            || compatibility != proof.compatibility
        {
            return Err(error(
                "rehearsal compatibility does not independently regenerate",
            ));
        }
        validate_initial_runtime(target, &seed, manifest.basis.day)?;
        let receipt = snapshot
            .adoptions
            .iter()
            .find(|r| r.plan.id == proof.recorded_by)
            .ok_or_else(|| error("rehearsal has no recording receipt"))?;
        if receipt.revision != proof.recorded_revision
            || receipt.previous != manifest.basis.active
            || receipt.active != manifest.basis.active
            || receipt.plan.target != baseline.artifact
            || receipt.plan.current_source != baseline.binding
            || receipt.plan.expected_data != manifest.basis.data.identity()?
            || receipt.plan.expected_generation != manifest.basis.data.generation
            || receipt.plan.expected_session != manifest.basis.session.identity()?
            || receipt.plan.expected_decisions != manifest.basis.decisions
        {
            return Err(error(
                "rehearsal recording changed the active source or frozen basis",
            ));
        }
        for (id, witness) in &proof.witnesses {
            let decision = snapshot
                .decisions
                .decisions
                .iter()
                .find(|d| &d.id == id)
                .ok_or_else(|| error("retained rehearsal intention is missing"))?;
            if &decision.witness != witness
                || !nonbinary(&decision.outcome)
                || !matches!(
                    decision.status,
                    DecisionStatus::Pending | DecisionStatus::Superseded { .. }
                )
                || receipt.plan.required_decisions.contains(id)
            {
                return Err(error(
                    "rehearsal witness link is missing or was relabeled as active authority",
                ));
            }
        }
    }
    Ok(())
}
/// Build an isolated replay registry only after the snapshot proof gate passed.
pub(super) fn replay_snapshot(snapshot: &ProjectSnapshot) -> ProjectSnapshot {
    let mut replay = snapshot.clone();
    for (id, proof) in &snapshot.scope.rehearsals {
        if let Some(layer) = &proof.layer {
            // validate() proved the identity and absence from active layers.
            replay.scope.layers.insert(
                layer.identity().expect("verified rehearsal layer"),
                layer.clone(),
            );
        }
        if let Some(receipt) = &proof.initialization {
            replay.scope.initializations.push(receipt.clone());
        }
        replay
            .scope
            .compositions
            .entry(id.clone())
            .or_insert_with(|| proof.manifest.clone());
    }
    replay
}
