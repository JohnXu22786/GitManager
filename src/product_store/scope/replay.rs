//! A non-serializable admission context, reconstructed from verified retained
//! sources/receipts or an exact fresh preparation. It is never provider input.
use super::*;
use crate::product_runtime::ReplayAdmission;

/// Comparison-only projection derived from an independently regenerated source.
/// Raw observations, artifacts and visible provenance are never rewritten.
#[derive(Default)]
pub(crate) struct ProvenanceColumns {
    pub outputs: BTreeMap<Id, BTreeSet<Id>>,
    pub views: BTreeMap<Id, BTreeSet<Id>>,
}
impl ProvenanceColumns {
    pub fn output(&self, output: &str, column: &str) -> bool {
        self.outputs
            .get(output)
            .is_some_and(|ids| ids.contains(column))
    }
    pub fn view(&self, view: &str, column: &str) -> bool {
        self.views.get(view).is_some_and(|ids| ids.contains(column))
    }
}

#[derive(Clone)]
pub struct ScopedExecutionContext {
    pub(super) snapshot: ProjectSnapshot,
    adoption_target: Digest,
    projected_seeds: BTreeSet<Digest>,
    correspondences: BTreeMap<Digest, correspondence::VerifiedCorrespondence>,
}
impl ScopedExecutionContext {
    pub fn committed(snapshot: &ProjectSnapshot) -> Result<Self> {
        snapshot.validate()?;
        Self::registry(snapshot).with_correspondences(
            &snapshot
                .scope
                .correspondences
                .iter()
                .map(|(id, receipt)| (id.clone(), receipt.proof.clone()))
                .collect(),
        )
    }
    pub(super) fn registry(snapshot: &ProjectSnapshot) -> Self {
        Self {
            snapshot: rehearsal::replay_snapshot(snapshot),
            adoption_target: snapshot.active_revision.clone(),
            projected_seeds: snapshot
                .scope
                .rehearsals
                .values()
                .map(|proof| proof.seed.clone())
                .collect(),
            correspondences: BTreeMap::new(),
        }
    }
    pub(crate) fn with_correspondences(
        mut self,
        proofs: &BTreeMap<Digest, ScopeCorrespondence>,
    ) -> Result<Self> {
        if proofs.len() > MAX_ITEMS {
            return Err(compiler::error("correspondence inventory exceeds bounds"));
        }
        let mut pending: BTreeMap<_, _> = proofs
            .iter()
            .filter(|(id, _)| !self.correspondences.contains_key(*id))
            .map(|(id, proof)| (id.clone(), proof.clone()))
            .collect();
        while !pending.is_empty() {
            let mut progressed = false;
            for (id, proof) in pending.clone() {
                if proof.identity()? != id {
                    return Err(compiler::error("correspondence digest changed"));
                }
                let source = &proof.source;
                let target = program(&self.snapshot, &proof.target)?;
                let checked = if let Some(pair) = &proof.scenario {
                    if pair.original.seed != proof.original || pair.original.clock_day != proof.day
                    {
                        return Err(compiler::error(
                            "correspondence scenario differs from its original input",
                        ));
                    }
                    let mut mapped = pair.projected.clone();
                    mapped.seed = merged_data(target, &proof.original)?;
                    let Ok((checked, actual)) =
                        self.project_scenario(source, &pair.original, target, &mapped)
                    else {
                        continue;
                    };
                    if actual != pair.projected {
                        return Err(compiler::error("projected scenario does not regenerate"));
                    }
                    checked
                } else {
                    let mapped = merged_data(target, &proof.original)?;
                    let Ok((checked, _)) =
                        self.project_seed(source, &proof.original, target, &mapped, proof.day)
                    else {
                        continue;
                    };
                    checked
                };
                let Some(derived) = checked.correspondences.get(&id) else {
                    continue;
                };
                if derived.proof != proof {
                    return Err(compiler::error(
                        "correspondence does not independently regenerate",
                    ));
                }
                self.insert_correspondence(derived.clone())?;
                pending.remove(&id);
                progressed = true;
            }
            if !progressed {
                return Err(compiler::error("historical correspondence is forged, cyclic or lacks an authenticated original input"));
            }
        }
        Ok(self)
    }
    fn insert_correspondence(
        &mut self,
        checked: correspondence::VerifiedCorrespondence,
    ) -> Result<()> {
        let id = checked.proof.identity()?;
        if self.correspondences.len() >= MAX_ITEMS && !self.correspondences.contains_key(&id) {
            return Err(compiler::error("correspondence inventory exceeds bounds"));
        }
        if self.correspondences.values().any(|prior| {
            prior.proof.projected == checked.proof.projected
                && ((prior.proof.target == checked.proof.target && prior.frames != checked.frames)
                    || (prior.proof.scenario.is_some()
                        && prior.proof.scenario.as_ref().map(|s| &s.projected)
                            == checked.proof.scenario.as_ref().map(|s| &s.projected)
                        && prior.proof.operation_seed != checked.proof.operation_seed))
        }) {
            return Err(compiler::error("ambiguous historical correspondence"));
        }
        self.projected_seeds.insert(checked.proof.projected.clone());
        self.correspondences.insert(id, checked);
        Ok(())
    }
    pub(crate) fn correspondence_proofs(&self) -> BTreeMap<Digest, ScopeCorrespondence> {
        self.correspondences
            .iter()
            .map(|(id, checked)| (id.clone(), checked.proof.clone()))
            .collect()
    }
    pub(super) fn frames_for(
        &self,
        source: &CapturedProgram,
        seed: &DataSnapshot,
    ) -> Result<Vec<history::ReplayFrame>> {
        let source = revision(source)?;
        let seed = seed.identity()?;
        Ok(self
            .correspondences
            .values()
            .find(|c| c.proof.target == source && c.proof.projected == seed)
            .map(|c| c.frames.clone())
            .unwrap_or_default())
    }
    pub(crate) fn has_scenario_correspondence(&self, scenario: &ScenarioSpec) -> bool {
        self.correspondences.values().any(|c| {
            c.proof
                .scenario
                .as_ref()
                .is_some_and(|pair| pair.matches_frame(scenario))
        })
    }
    fn operation_seed<'a>(
        &'a self,
        source: &CapturedProgram,
        scenario: &'a ScenarioSpec,
    ) -> Result<&'a DataSnapshot> {
        source.validate()?;
        if let Some(checked) = self.correspondences.values().find(|c| {
            c.proof
                .scenario
                .as_ref()
                .is_some_and(|pair| pair.matches_frame(scenario))
        }) {
            return Ok(&checked.operation_seed);
        }
        if self.correspondences.values().any(|c| {
            c.proof
                .scenario
                .as_ref()
                .is_some_and(|pair| pair.projected.id == scenario.id)
        }) {
            return Err(compiler::error("projected scenario changed its authenticated input frame; operation identity is unverified"));
        }
        Ok(&scenario.seed)
    }
    pub(crate) fn project_scenario(
        &self,
        source: &CapturedProgram,
        original: &ScenarioSpec,
        target: &CapturedProgram,
        mapped: &ScenarioSpec,
    ) -> Result<(Self, ScenarioSpec)> {
        original.validate(&source.program)?;
        mapped.validate(&target.program)?;
        if original.clock_day != mapped.clock_day || original.random_seed != mapped.random_seed {
            return Err(compiler::error(
                "historical correspondence changed the replay clock or seed",
            ));
        }
        let (mut context, seed) = self.project_seed(
            source,
            &original.seed,
            target,
            &mapped.seed,
            mapped.clock_day,
        )?;
        let mut actual = mapped.clone();
        actual.seed = seed;
        let target_id = revision(target)?;
        let original_id = original.seed.identity()?;
        let projected_id = actual.seed.identity()?;
        if let Some(derived) = context
            .correspondences
            .values()
            .find(|c| {
                c.proof.target == target_id
                    && c.proof.projected == projected_id
                    && c.proof.original.identity().ok().as_ref() == Some(&original_id)
                    && c.proof.source == *source
            })
            .cloned()
        {
            let namespace = canonical_digest(
                IdentityDomain::Scenario,
                &(
                    "scope-replay/1",
                    &source.binding,
                    &target.binding,
                    original.identity()?,
                    &mapped.session,
                    &mapped.inputs,
                    &mapped.validity,
                ),
            )?;
            actual.id = format!("scope-replay-{}", &namespace.as_str()[..40]);
            let mut bound = derived;
            bound.operation_seed = self.operation_seed(source, original)?.clone();
            bound.proof.operation_seed = bound.operation_seed.identity()?;
            bound.proof.scenario = Some(ScopeScenarioCorrespondence {
                original: original.clone(),
                projected: actual.clone(),
            });
            context.insert_correspondence(bound)?;
        }
        Ok((context, actual))
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
        let mut working = rehearsal::replay_snapshot(snapshot);
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
        let mut seeds: BTreeSet<_> = snapshot
            .scope
            .rehearsals
            .values()
            .map(|proof| proof.seed.clone())
            .collect();
        seeds.insert(checked.initialized.identity()?);
        let mut proofs: BTreeMap<_, _> = snapshot
            .scope
            .correspondences
            .iter()
            .map(|(id, receipt)| (id.clone(), receipt.proof.clone()))
            .collect();
        proofs.extend(prepared.correspondences.clone());
        Self {
            snapshot: working,
            adoption_target: prepared.manifest.output.clone(),
            projected_seeds: seeds,
            correspondences: BTreeMap::new(),
        }
        .with_correspondences(&proofs)
    }
    /// Replay a verified prospective implementation while retaining current
    /// adoption authority. Prospective cohort data stays inside this context.
    pub fn rehearsed(snapshot: &ProjectSnapshot, prepared: &PreparedScopedChange) -> Result<Self> {
        rehearsal::eligible(prepared)?;
        let mut context = Self::prepared(snapshot, prepared)?;
        context.adoption_target = snapshot.active_revision.clone();
        Ok(context)
    }
    pub(crate) fn provenance_columns(&self, source: &CapturedProgram) -> Result<ProvenanceColumns> {
        source.validate()?;
        let id = revision(source)?;
        let Some(manifest) = self.snapshot.scope.compositions.get(&id) else {
            compiler::reject_reserved(&source.program)?;
            return Ok(ProvenanceColumns::default());
        };
        if program(&self.snapshot, &id)? != source
            || compiler::compile(&self.snapshot, manifest)? != *source
        {
            return Err(compiler::error(
                "comparison source does not regenerate from its trusted manifest",
            ));
        }
        let business = program(&self.snapshot, &manifest.business)?;
        compiler::reject_reserved(&business.program)?;
        let mut result = ProvenanceColumns::default();
        for output in &source.program.outputs {
            let original = business
                .program
                .outputs
                .iter()
                .find(|o| o.id == output.id)
                .ok_or_else(|| compiler::error("compiler introduced an unrecognized output"))?;
            result.outputs.insert(
                output.id.clone(),
                output
                    .columns
                    .iter()
                    .filter(|c| !original.columns.iter().any(|old| old.id == c.id))
                    .map(|c| c.id.clone())
                    .collect(),
            );
        }
        fn columns(view: &ViewDefinition) -> &[Column] {
            match &view.kind {
                ViewKind::List { columns, .. } | ViewKind::Detail { columns, .. } => columns,
                _ => &[],
            }
        }
        for view in &source.program.views {
            let original = business
                .program
                .views
                .iter()
                .find(|v| v.id == view.id)
                .ok_or_else(|| compiler::error("compiler introduced an unrecognized view"))?;
            result.views.insert(
                view.id.clone(),
                columns(view)
                    .iter()
                    .filter(|c| !columns(original).iter().any(|old| old.id == c.id))
                    .map(|c| c.id.clone())
                    .collect(),
            );
        }
        Ok(result)
    }
    pub fn contains_managed_source(&self, source: &CapturedProgram) -> bool {
        revision(source).ok().is_some_and(|id| {
            self.snapshot.scope.compositions.contains_key(&id)
                && program(&self.snapshot, &id).is_ok_and(|p| p == source)
        })
    }
    pub fn admit_target(&self, target: &CapturedProgram) -> Result<()> {
        target.validate()?;
        let id = revision(target)?;
        if self.snapshot.scope.compositions.is_empty() {
            return compiler::reject_reserved(&target.program);
        }
        if id == self.adoption_target
            && !self.snapshot.scope.compositions.contains_key(&id)
            && program(&self.snapshot, &id)? == target
        {
            return compiler::reject_reserved(&target.program);
        }
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
    /// Authenticate the ordinary additive mapping, then apply only exact
    /// retained initialization transitions whose frozen input equals this seed.
    /// Business values, identities and events remain unchanged. Return the
    /// actual replay seed so evidence never names a different scenario.
    pub fn project_seed(
        &self,
        source: &CapturedProgram,
        original: &DataSnapshot,
        target: &CapturedProgram,
        mapped: &DataSnapshot,
        day: i32,
    ) -> Result<(Self, DataSnapshot)> {
        self.verify_seed(source, original, day)?;
        let authenticated = self.known_seed(&original.identity()?)?;
        if crate::product_runtime::has_protected_fields(target) && !authenticated {
            return Err(compiler::error(
                "an unverified raw seed cannot grant scope metadata authority",
            ));
        }
        // Use the interpreter's exact typed additive merge, including active
        // entity labels and constraints, while preserving every business fact.
        let expected = merged_data(target, original)?;
        if expected != *mapped {
            return Err(compiler::error(
                "scope scene mapping changed protected seed facts",
            ));
        }
        let mut initialized = original.clone();
        let mut lineage = vec![];
        let mut seen = BTreeSet::new();
        let mut next = Some(revision(target)?);
        while let Some(id) = next {
            if !seen.insert(id.clone()) {
                return Err(compiler::error("cyclic scope projection lineage"));
            }
            let Some(manifest) = self.snapshot.scope.compositions.get(&id) else {
                break;
            };
            lineage.push(manifest);
            next = manifest.previous.clone();
        }
        for manifest in lineage.into_iter().rev() {
            if initialized.identity()? != manifest.basis.data.identity()?
                || (manifest.transition == ScopeTransition::Adoption && day != manifest.basis.day)
            {
                continue;
            }
            let target = program(&self.snapshot, &manifest.output)?;
            initialized = if manifest.transition == ScopeTransition::Adoption {
                let id = manifest
                    .layers
                    .last()
                    .ok_or_else(|| compiler::error("initialization layer missing"))?;
                let layer = &self.snapshot.scope.layers[id];
                let (data, receipt) = history::initialize(&self.snapshot, layer, target)?;
                if !self.snapshot.scope.initializations.contains(&receipt) {
                    return Err(compiler::error(
                        "initialization projection differs from retained receipt",
                    ));
                }
                data
            } else {
                merged_data(target, &initialized)?
            };
        }
        // Keep the authenticated input frame unless an exact retained
        // transition above changed it. The interpreter already merges ordinary
        // target schemas on start; manufacturing another schema-only seed here
        // would lose durable provenance when an older scene is captured again.
        let mut result = self.clone();
        // Ordinary synthetic scenes remain usable as ordinary scenes, but
        // projecting one does not manufacture authority for a later layer.
        if authenticated {
            result.projected_seeds.insert(initialized.identity()?);
        }
        if result.verify_seed(target, &initialized, day).is_err() {
            let derived =
                correspondence::derive(self, source, original, target, day, &initialized)?;
            initialized = derived.seed.clone();
            result.insert_correspondence(derived)?;
        } else if initialized != *original {
            let operation_seed = original.clone();
            let checked = correspondence::VerifiedCorrespondence {
                proof: ScopeCorrespondence {
                    source: source.clone(),
                    original: original.clone(),
                    target: revision(target)?,
                    day,
                    projected: initialized.identity()?,
                    operation_seed: operation_seed.identity()?,
                    scenario: None,
                },
                seed: initialized.clone(),
                frames: result.frames_for(target, &initialized)?,
                operation_seed,
            };
            result.insert_correspondence(checked)?;
        }
        result.verify_seed(target, &initialized, day)?;
        Ok((result, initialized))
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
        if target.binding.project_id != self.snapshot.data.project_id
            || data.project_id != self.snapshot.data.project_id
        {
            return Err(compiler::error("scope replay belongs to another project"));
        }
        let Some(manifest) = self.snapshot.scope.compositions.get(&id) else {
            return compiler::reject_reserved(&target.program);
        };
        if program(&self.snapshot, &id)? != target
            || data.project_id != self.snapshot.data.project_id
        {
            return Err(compiler::error("scope source or project identity mismatch"));
        }
        let mut projected = self.snapshot.clone();
        projected.active_revision = id.clone();
        projected.data = data.clone();
        projected.clock_day = day;
        // Live receipt validation is unchanged. A historical replay may use
        // only a separately authenticated, independently derived input frame.
        let live = history::verify_history_layers(&projected, &manifest.layers);
        if live.is_ok() {
            return live;
        }
        for checked in self
            .correspondences
            .values()
            .filter(|c| c.proof.target == id)
        {
            if day >= checked.proof.day
                && data.generation >= checked.seed.generation
                && data.events.starts_with(&checked.seed.events)
                && history::verify_history_frames(&projected, &manifest.layers, &checked.frames)
                    .is_ok()
            {
                return Ok(());
            }
        }
        live
    }
}
impl ReplayAdmission for ScopedExecutionContext {
    fn replay_operation_ids(
        &self,
        target: &CapturedProgram,
        scenario: &ScenarioSpec,
    ) -> std::result::Result<Vec<Id>, AdapterError> {
        self.verify_seed(target, &scenario.seed, scenario.clock_day)
            .map_err(|e| AdapterError::Unsupported(e.to_string()))?;
        let mut original = scenario.clone();
        original.seed = self
            .operation_seed(target, scenario)
            .map_err(|e| AdapterError::Unsupported(e.to_string()))?
            .clone();
        Ok(original.replay_operation_ids()?)
    }

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
