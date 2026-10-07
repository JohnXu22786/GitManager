//! Historical replay correspondence is separate from live adoption authority.
//! It adds independently derived system cells without changing business input.
use super::*;
use compiler::error;

#[derive(Clone, Debug)]
pub(super) struct VerifiedCorrespondence {
    pub proof: ScopeCorrespondence,
    pub seed: DataSnapshot,
    pub frames: Vec<history::ReplayFrame>,
    pub operation_seed: DataSnapshot,
}

pub(super) fn derive(
    context: &ScopedExecutionContext,
    source: &CapturedProgram,
    original: &DataSnapshot,
    target: &CapturedProgram,
    day: i32,
    initialized: &DataSnapshot,
) -> Result<VerifiedCorrespondence> {
    let mut working = context.snapshot.clone();
    retain(&mut working, source)?;
    let snapshot = &working;
    let target_id = revision(target)?;
    let manifest = snapshot
        .scope
        .compositions
        .get(&target_id)
        .ok_or_else(|| error("historical correspondence target is not managed"))?;
    let mut frames = context.frames_for(source, original)?;
    let operation_seed = original.clone();
    let mut data = initialized.clone();
    for id in &manifest.layers {
        if frames.iter().any(|frame| &frame.layer == id) {
            continue;
        }
        let layer = &snapshot.scope.layers[id];
        let declared = layer.request.lifecycles.iter().all(|l| {
            data.schema
                .iter()
                .find(|e| e.id == l.entity)
                .is_some_and(|e| {
                    ["member", "sealed"].iter().all(|suffix| {
                        e.fields
                            .iter()
                            .any(|f| f.id == compiler::key(id, &l.entity, suffix))
                    })
                })
        });
        let initialized = declared
            && data
                .records
                .iter()
                .filter(|r| {
                    layer
                        .request
                        .lifecycles
                        .iter()
                        .any(|l| l.entity == r.entity)
                })
                .all(|r| {
                    r.values
                        .contains_key(&compiler::key(id, &r.entity, "member"))
                        && r.values
                            .contains_key(&compiler::key(id, &r.entity, "sealed"))
                });
        if data.generation >= layer.basis.data.generation
            && day >= layer.basis.day
            && data.events.starts_with(&layer.basis.data.events)
            && initialized
        {
            continue;
        }
        // Only a genuine earlier input in this actual project history has a
        // defensible correspondence. No selected synthetic ID is guessed.
        if data.generation > layer.basis.data.generation
            || day > layer.basis.day
            || !layer.basis.data.events.starts_with(&original.events)
        {
            return Err(error(
                "historical input has no actual frozen-history correspondence",
            ));
        }
        for row in &original.records {
            let later = layer
                .basis
                .data
                .records
                .iter()
                .find(|r| r.entity == row.entity && r.id == row.id)
                .ok_or_else(|| {
                    error("historical record is absent from the actual frozen cohort")
                })?;
            let mut system_keys = BTreeSet::new();
            for (id, proof) in &snapshot.scope.layers {
                if proof
                    .request
                    .lifecycles
                    .iter()
                    .any(|l| l.entity == row.entity)
                {
                    system_keys.extend(history::metadata_keys(id, proof, &row.entity)?);
                }
            }
            if later.created_program != row.created_program
                || later.revision < row.revision
                || (row.archived
                    && (!later.archived
                        || row.revision != later.revision
                        || row
                            .values
                            .iter()
                            .filter(|(key, _)| !system_keys.contains(*key))
                            .any(|(key, value)| later.values.get(key) != Some(value))))
            {
                return Err(error(
                    "historical record birth or archive provenance differs",
                ));
            }
        }
        let before = data.clone();
        let (initialized, additions) = history::initialize_frame(
            snapshot,
            layer,
            id,
            &before,
            &revision(source)?,
            target,
            day,
            "CapturedForHistoricalReplay",
        )?;
        frames.push(history::ReplayFrame {
            layer: id.clone(),
            before,
            additions,
        });
        data = initialized;
    }
    if frames.is_empty() {
        return Err(error("no historical layer correspondence was established"));
    }
    data = merged_data(target, &data)?;
    let proof = ScopeCorrespondence {
        source: source.clone(),
        original: original.clone(),
        target: target_id,
        day,
        projected: data.identity()?,
        operation_seed: operation_seed.identity()?,
        scenario: None,
    };
    Ok(VerifiedCorrespondence {
        proof,
        seed: data,
        frames,
        operation_seed,
    })
}

pub(super) fn validate(snapshot: &ProjectSnapshot) -> Result<()> {
    if snapshot.scope.correspondences.len() > MAX_ITEMS {
        return Err(error("historical correspondence inventory exceeds bounds"));
    }
    let mut proofs = BTreeMap::new();
    for (id, receipt) in &snapshot.scope.correspondences {
        let proof = &receipt.proof;
        if &proof.identity()? != id
            || !valid_id(&receipt.operation)
            || receipt.revision > snapshot.revision
        {
            return Err(error("invalid historical correspondence identity"));
        }
        let recorded = snapshot
            .adoptions
            .iter()
            .find(|a| a.plan.id == receipt.operation)
            .ok_or_else(|| error("historical correspondence has no atomic recording receipt"))?;
        let target = program(snapshot, &proof.target)?;
        if recorded.revision != receipt.revision
            || ((recorded.active != proof.target || recorded.plan.target != target.artifact)
                && !snapshot.scope.rehearsals.values().any(|p| {
                    p.recorded_by == receipt.operation && p.manifest.output == proof.target
                }))
        {
            return Err(error(
                "historical correspondence is not reachable from its recording target",
            ));
        }
        proofs.insert(id.clone(), proof.clone());
    }
    ScopedExecutionContext::registry(snapshot).with_correspondences(&proofs)?;
    Ok(())
}

pub(in crate::product_store) fn install(
    snapshot: &mut ProjectSnapshot,
    proofs: &BTreeMap<Digest, ScopeCorrespondence>,
    operation: &str,
    revision: u64,
) -> Result<()> {
    for (id, proof) in proofs {
        if &proof.identity()? != id {
            return Err(error("correspondence identity changed"));
        }
        if let Some(existing) = snapshot.scope.correspondences.get(id) {
            if existing.proof != *proof {
                return Err(error("correspondence collision"));
            }
            continue;
        }
        if snapshot.scope.correspondences.len() >= MAX_ITEMS {
            return Err(error("historical correspondence inventory exceeds bounds"));
        }
        retain(snapshot, &proof.source)?;
        snapshot.scope.correspondences.insert(
            id.clone(),
            ScopeCorrespondenceReceipt {
                proof: proof.clone(),
                operation: operation.into(),
                revision,
            },
        );
    }
    Ok(())
}
