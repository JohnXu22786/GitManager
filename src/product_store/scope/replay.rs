//! A non-serializable admission context, reconstructed from verified retained
//! sources/receipts or an exact fresh preparation. It is never provider input.
use super::*;
use crate::product_runtime::ReplayAdmission;

#[derive(Clone)]
pub struct ScopedExecutionContext {
    snapshot: ProjectSnapshot,
    adoption_target: Digest,
    projected_seeds: BTreeSet<Digest>,
}
impl ScopedExecutionContext {
    pub fn committed(snapshot: &ProjectSnapshot) -> Result<Self> {
        snapshot.validate()?;
        Ok(Self {
            snapshot: snapshot.clone(),
            adoption_target: snapshot.active_revision.clone(),
            projected_seeds: BTreeSet::new(),
        })
    }
    pub fn prepared(snapshot: &ProjectSnapshot, prepared: &PreparedScopedChange) -> Result<Self> {
        if canonical_digest(IdentityDomain::Data, snapshot)? != prepared.basis {
            return Err(StoreError::Conflict("scope rehearsal basis changed".into()));
        }
        let checked = if let Some(layer) = &prepared.layer {
            prepare(
                snapshot,
                &prepared.candidate,
                &layer.request,
                &layer.operation,
            )?
        } else if let Some(mappings) = &prepared.evolution {
            prepare_evolution(
                snapshot,
                &prepared.candidate,
                mappings,
                &prepared.manifest.operation,
            )?
        } else {
            let previous = snapshot
                .scope
                .compositions
                .get(&snapshot.active_revision)
                .ok_or_else(|| compiler::error("scope predecessor missing"))?;
            let ids: Vec<_> = previous
                .active
                .difference(&prepared.manifest.active)
                .cloned()
                .collect();
            prepare_withdrawal(snapshot, &ids, &prepared.manifest.operation)?
        };
        if checked.target != prepared.target
            || checked.manifest != prepared.manifest
            || checked.initialized != prepared.initialized
            || checked.receipt != prepared.receipt
        {
            return Err(compiler::error(
                "scope preparation does not independently regenerate",
            ));
        }
        let mut working = snapshot.clone();
        retain(&mut working, &prepared.candidate)?;
        retain(&mut working, &prepared.target)?;
        if let Some(layer) = &prepared.layer {
            working
                .scope
                .layers
                .insert(layer.identity()?, layer.clone());
        }
        if let Some(receipt) = &prepared.receipt {
            working.scope.initializations.push(receipt.clone());
        }
        working
            .scope
            .compositions
            .insert(prepared.manifest.output.clone(), prepared.manifest.clone());
        // No adoption/decision receipt is invented for this isolated context.
        Ok(Self {
            snapshot: working,
            adoption_target: prepared.manifest.output.clone(),
            projected_seeds: BTreeSet::from([checked.initialized.identity()?]),
        })
    }
    pub fn contains_managed_source(&self, source: &CapturedProgram) -> bool {
        revision(source).ok().is_some_and(|id| {
            self.snapshot.scope.compositions.contains_key(&id)
                && program(&self.snapshot, &id).is_ok_and(|p| p == source)
        })
    }
    pub fn admit_target(&self, target: &CapturedProgram) -> Result<()> {
        target.validate()?;
        if self.snapshot.scope.layers.is_empty() {
            return compiler::reject_reserved(&target.program);
        }
        let id = revision(target)?;
        if id != self.adoption_target
            || !self.snapshot.scope.compositions.contains_key(&id)
            || program(&self.snapshot, &id)? != target
        {
            return Err(compiler::error(
                "managed work needs a freshly verified preserved-history envelope",
            ));
        }
        Ok(())
    }
    /// Permit only the deterministic additive schema projection of an already
    /// authenticated scene seed. Business values, identities and history remain
    /// exact; arbitrary metadata-bearing synthetic seeds are never admitted.
    pub fn project_seed(
        &self,
        source: &CapturedProgram,
        original: &DataSnapshot,
        target: &CapturedProgram,
        mapped: &DataSnapshot,
        day: i32,
    ) -> Result<Self> {
        self.verify_seed(source, original, day)?;
        if crate::product_runtime::has_protected_fields(target)
            && !self.known_seed(&original.identity()?)?
        {
            return Err(compiler::error(
                "an unverified raw seed cannot grant scope metadata authority",
            ));
        }
        let mut expected = original.clone();
        for entity in &target.program.entities {
            if let Some(stored) = expected.schema.iter_mut().find(|e| e.id == entity.id) {
                for field in &entity.fields {
                    if !stored.fields.iter().any(|f| f.id == field.id) {
                        stored.fields.push(field.clone());
                    }
                }
            } else {
                expected.schema.push(entity.clone());
            }
        }
        if expected != *mapped {
            return Err(compiler::error(
                "scope scene mapping changed protected seed facts",
            ));
        }
        let mut result = self.clone();
        result.projected_seeds.insert(mapped.identity()?);
        Ok(result)
    }
    pub fn verify_seed(
        &self,
        target: &CapturedProgram,
        data: &DataSnapshot,
        day: i32,
    ) -> Result<()> {
        if crate::product_runtime::has_protected_fields(target) {
            let digest = data.identity()?;
            let known = self.known_seed(&digest)?;
            if !known {
                return Err(compiler::error("metadata-bearing scene seed lacks an authenticated frozen snapshot or exact initialization receipt"));
            }
        }
        self.verify_state(target, data, day)
    }
    fn known_seed(&self, digest: &Digest) -> Result<bool> {
        // Evolution and withdrawal add no layer initialization receipt. Their
        // exact initialized seeds remain reproducible from verified immutable
        // basis/source pairs after restart and subsequent business writes.
        for manifest in self.snapshot.scope.compositions.values() {
            if matches!(
                manifest.transition,
                ScopeTransition::Evolution | ScopeTransition::Withdrawal
            ) && merged_data(
                program(&self.snapshot, &manifest.output)?,
                &manifest.basis.data,
            )?
            .identity()?
                == *digest
            {
                return Ok(true);
            }
        }
        Ok(self.projected_seeds.contains(digest)
            || self.snapshot.data.identity()? == *digest
            || self
                .snapshot
                .scope
                .initializations
                .iter()
                .any(|r| r.after_data == *digest)
            || self
                .snapshot
                .scope
                .compositions
                .values()
                .any(|m| m.basis.data.identity().ok().as_ref() == Some(digest))
            || self
                .snapshot
                .adoptions
                .iter()
                .any(|a| a.plan.expected_data == *digest))
    }
    fn verify_state(&self, target: &CapturedProgram, data: &DataSnapshot, day: i32) -> Result<()> {
        target.validate()?;
        data.validate()?;
        validate_day(day)?;
        let id = revision(target)?;
        let Some(manifest) = self.snapshot.scope.compositions.get(&id) else {
            return compiler::reject_reserved(&target.program);
        };
        if program(&self.snapshot, &id)? != target
            || data.project_id != self.snapshot.data.project_id
        {
            return Err(compiler::error("scope source or project identity mismatch"));
        }
        let mut projected = self.snapshot.clone();
        projected.active_revision = id;
        projected.data = data.clone();
        projected.clock_day = day;
        projected
            .scope
            .layers
            .retain(|id, _| manifest.layers.contains(id));
        projected
            .scope
            .initializations
            .retain(|r| manifest.layers.contains(&r.layer));
        for layer in projected.scope.layers.values() {
            if data.generation < layer.basis.data.generation {
                return Err(compiler::error("scene precedes the frozen cohort and needs explicit source-qualified correspondence"));
            }
        }
        history::verify_history(&projected)
    }
}
impl ReplayAdmission for ScopedExecutionContext {
    fn validate_seed(
        &self,
        target: &CapturedProgram,
        data: &DataSnapshot,
        day: i32,
    ) -> std::result::Result<(), AdapterError> {
        self.verify_seed(target, data, day)
            .map_err(|e| AdapterError::Unsupported(e.to_string()))
    }
    fn validate_state(
        &self,
        target: &CapturedProgram,
        data: &DataSnapshot,
        day: i32,
    ) -> std::result::Result<(), AdapterError> {
        self.verify_state(target, data, day)
            .map_err(|e| AdapterError::Unsupported(e.to_string()))
    }
}
