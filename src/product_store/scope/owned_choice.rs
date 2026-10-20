//! Opaque first-owned preparation and one locked publication path. Wire recipes
//! remain untrusted; neither a serialized proof nor a preview grants this token.
use super::common_fact_admission::{refuse, ProofMeter};
use super::*;
use std::sync::{atomic::AtomicBool, Arc};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OwnedChoiceMode {
    Activate,
    Record(Vec<Id>),
}
#[derive(Clone)]
pub struct PreparedOwnedScopedChangeV2 {
    pub(super) execution: Arc<PreparedOwnedPreview>,
    pub(super) layer: OwnedLayerV2,
    pub(super) manifest: OwnedCompositionV2,
    pub(super) initialization: OwnedInitializationV2,
    compatibility: CompatibilityReport,
    pub(super) budget: ProofBudget,
    pub(super) cancelled: Arc<AtomicBool>,
}
impl PartialEq for PreparedOwnedScopedChangeV2 {
    fn eq(&self, other: &Self) -> bool {
        self.layer == other.layer
            && self.manifest == other.manifest
            && self.initialization == other.initialization
            && self.compatibility == other.compatibility
            && self.execution.target == other.execution.target
            && self.execution.initialized == other.execution.initialized
            && self.execution.proof.snapshot == other.execution.proof.snapshot
            && self.execution.proof.candidate == other.execution.proof.candidate
            && self.execution.proof.report == other.execution.proof.report
            && self.execution.metadata == other.execution.metadata
            && self.budget.units == other.budget.units
            && self.budget.elapsed_millis == other.budget.elapsed_millis
            && Arc::ptr_eq(&self.cancelled, &other.cancelled)
    }
}
impl Eq for PreparedOwnedScopedChangeV2 {}
impl PreparedOwnedScopedChangeV2 {
    pub fn target(&self) -> &CapturedProgram {
        &self.execution.target
    }
    pub fn initialized(&self) -> &DataSnapshot {
        &self.execution.initialized
    }
    pub fn scope(&self) -> DecisionScope {
        self.layer.scope()
    }
    pub fn operation_id(&self) -> &str {
        &self.layer.operation
    }
    pub(crate) fn basis(&self) -> &ProjectSnapshot {
        &self.execution.proof.snapshot
    }
    pub(crate) fn check_basis(&self, current: &ProjectSnapshot) -> Result<()> {
        ProofMeter::new(self.budget, self.cancelled.clone())?;
        if current != self.basis() {
            return Err(StoreError::Conflict(
                "owned preparation is stale; source, data, session, day or decisions changed"
                    .into(),
            ));
        }
        Ok(())
    }
    pub(crate) fn original_frame(
        &self,
        current: &ProjectSnapshot,
        scene: &ScenarioSpec,
    ) -> Result<ScenarioSpec> {
        self.check_basis(current)?;
        scene.validate(&current.program()?.program)?;
        if scene.seed != current.data
            || scene.session != current.session
            || scene.clock_day != current.clock_day
            || scene.random_seed != 0
        {
            return Err(refuse(
                "owned experience",
                "scene differs from exact basis frame",
            ));
        }
        let mut projected = scene.clone();
        projected.seed = self.initialized().clone();
        projected.validate(&self.target().program)?;
        Ok(projected)
    }
    fn fresh(&self, current: &ProjectSnapshot) -> Result<Self> {
        self.check_basis(current)?;
        let proof = &self.execution.proof;
        let regenerated = prepare_owned(
            current,
            &proof.candidate,
            &proof.request,
            &proof.accepted,
            self.operation_id(),
            self.budget,
            self.cancelled.clone(),
        )?;
        if &regenerated != self {
            return Err(refuse(
                "owned preparation",
                "full durable recipe does not regenerate",
            ));
        }
        Ok(regenerated)
    }
    fn rehearsal(
        &self,
        graph: &DecisionGraph,
        ids: &[Id],
        plan: &AdoptionPlan,
    ) -> Result<OwnedRehearsalV2> {
        let mut witnesses = BTreeMap::new();
        for id in ids {
            let d = graph
                .decisions
                .iter()
                .find(|d| &d.id == id)
                .ok_or_else(|| refuse("owned recording", "pending decision missing"))?;
            if self
                .basis()
                .decisions
                .decisions
                .iter()
                .any(|old| old.id == *id)
                || d.status != DecisionStatus::Pending
                || d.scope != self.scope()
                || !matches!(
                    d.outcome,
                    DecisionOutcome::EitherAcceptable
                        | DecisionOutcome::BothNeeded
                        | DecisionOutcome::NeitherFits
                        | DecisionOutcome::Deferred
                )
                || witnesses.insert(id.clone(), d.witness.clone()).is_some()
            {
                return Err(refuse(
                    "owned recording",
                    "not an exact new nonbinary choice",
                ));
            }
        }
        if witnesses.is_empty() {
            return Err(refuse("owned recording", "no pending choice"));
        }
        Ok(OwnedRehearsalV2 {
            version: OWNED_SCOPE_VERSION,
            manifest: self.manifest.clone(),
            seed: self.initialized().identity()?,
            layer: Some(self.layer.clone()),
            initialization: Some(self.initialization.clone()),
            compatibility: self.compatibility.clone(),
            recorded_by: plan.id.clone(),
            recorded_revision: self.initialization.adoption_revision,
            witnesses,
        })
    }
    fn request(
        &self,
        mode: &OwnedChoiceMode,
        plan: &AdoptionPlan,
        graph: &DecisionGraph,
    ) -> Result<Digest> {
        match mode {
            OwnedChoiceMode::Activate => {
                owned_wire::adoption_request(plan, graph, &self.manifest, &self.initialization)
            }
            OwnedChoiceMode::Record(ids) => owned_wire::identity(
                "owned-decision-recording/2",
                &(
                    plan,
                    graph,
                    owned_wire::recording_request(&self.rehearsal(graph, ids, plan)?, &[])?,
                ),
            ),
        }
    }
}
pub(crate) fn prepare_owned(
    current: &ProjectSnapshot,
    candidate: &CapturedProgram,
    request: &OwnedPreviewRequest,
    accepted: &AcceptedPreviewLifecycle,
    operation: &str,
    budget: ProofBudget,
    cancelled: Arc<AtomicBool>,
) -> Result<PreparedOwnedScopedChangeV2> {
    let mut meter = ProofMeter::new(budget, cancelled.clone())?;
    let (layer, manifest, initialization, execution, compatibility) = owned_preview::initial_owned_recipe(
        current, candidate, request, accepted, operation, &mut meter,
    )?;
    Ok(PreparedOwnedScopedChangeV2 {
        execution: Arc::new(execution),
        layer,
        manifest,
        initialization,
        compatibility,
        budget,
        cancelled,
    })
}
impl ProductStore {
    fn check_owned_plan(
        &self,
        current: &ProjectSnapshot,
        plan: &AdoptionPlan,
        target: &CapturedProgram,
        prepared: &PreparedOwnedScopedChangeV2,
        mode: &OwnedChoiceMode,
    ) -> Result<()> {
        if matches!(mode, OwnedChoiceMode::Record(_)) {
            return self.check_plan(current, plan, target);
        }
        plan.validate()?;
        target.validate()?;
        if plan.target != target.artifact
            || target != prepared.target()
            || plan.current_source != current.program()?.binding
            || plan.expected_data != current.data.identity()?
            || plan.expected_generation != current.data.generation
            || plan.expected_decisions != current.decisions.identity()?
            || plan.expected_session != current.session.identity()?
            || plan.project_id != current.data.project_id
        {
            return Err(StoreError::Conflict(
                "owned plan differs from the exact uninitialized basis".into(),
            ));
        }
        let runtime = LocalRuntime::default();
        // The raw basis intentionally lacks required protected cells. Preserve
        // that honest report; only the privately regenerated initializer can
        // establish compatible execution, never an ordinary schema merge.
        if runtime.compatibility_at(target, &current.data, current.clock_day)? != plan.compatibility
            || runtime.compatibility_at(target, prepared.initialized(), current.clock_day)?
                != prepared.compatibility
            || prepared.compatibility.state != CompatibilityState::Compatible
        {
            return Err(refuse(
                "owned plan",
                "raw or initialized compatibility changed",
            ));
        }
        Ok(())
    }
    pub(crate) fn prepare_owned_adoption(
        &self,
        plan: AdoptionPlan,
        prepared: &PreparedOwnedScopedChangeV2,
        mode: &OwnedChoiceMode,
    ) -> Result<PreparedAdoption> {
        let current = self.load()?;
        prepared.check_basis(&current)?;
        let target = match mode {
            OwnedChoiceMode::Activate => prepared.target(),
            OwnedChoiceMode::Record(_) => current.program()?,
        };
        if plan.scope != prepared.scope()
            || matches!(mode, OwnedChoiceMode::Activate) && plan.id != prepared.operation_id()
        {
            return Err(refuse(
                "owned plan",
                "operation or exact frozen scope differs",
            ));
        }
        self.check_owned_plan(&current, &plan, target, prepared, mode)?;
        Ok(PreparedAdoption {
            expected_revision: current.revision,
            target: revision(target)?,
            plan,
        })
    }
    pub(crate) fn adopt_owned_verified<F, V>(
        &self,
        expected_revision: u64,
        prepared: &PreparedOwnedScopedChangeV2,
        mode: &OwnedChoiceMode,
        adoption: &PreparedAdoption,
        decisions: &DecisionGraph,
        verify: F,
        verify_retry: V,
        #[cfg(test)] fault: Option<FaultPoint>,
    ) -> Result<ProjectSnapshot>
    where
        F: FnOnce(
            &ProjectSnapshot,
            &CapturedProgram,
            &DecisionGraph,
            &AdoptionPlan,
            &ProjectSnapshot,
        ) -> Result<()>,
        V: FnOnce(&ProjectSnapshot) -> Result<()>,
    {
        let _lock = self.root.lock()?;
        let current = self.load()?;
        let request = prepared.request(mode, &adoption.plan, decisions)?;
        if let Some(receipt) = current.operations.get(&adoption.plan.id) {
            if receipt.request != request {
                return Err(StoreError::Conflict(
                    "owned operation ID belongs to a different request".into(),
                ));
            }
            verify_retry(&current)?;
            // Finish checked persistence barriers after an acknowledgment/sync failure.
            let pointer_bytes = self.root.read_and_sync("CURRENT", MAX_STORE_BYTES)?;
            let pointer: Pointer = serde_json::from_slice(&pointer_bytes)
                .map_err(|e| StoreError::Corrupt(e.to_string()))?;
            let bytes = self.root.read_and_sync(
                &format!("object-{}.json", pointer.object.as_str()),
                MAX_STORE_BYTES,
            )?;
            if bytes != canonical_bytes(&current)? {
                return Err(StoreError::Conflict("retry CURRENT changed".into()));
            }
            let packages: BTreeSet<_> = current
                .decisions
                .decisions
                .iter()
                .map(|d| d.witness.clone())
                .chain(
                    current
                        .adoptions
                        .iter()
                        .filter_map(|a| a.plan.evidence.first().cloned()),
                )
                .collect();
            for digest in packages {
                let bytes = self.root.read_and_sync(
                    &format!("extension-{}.json", digest.as_str()),
                    MAX_INTENTION_OBJECT_BYTES,
                )?;
                if Self::extension_digest(&bytes)? != digest {
                    return Err(StoreError::Corrupt("retry package changed".into()));
                }
            }
            self.root.sync()?;
            self.pinned()?;
            return Ok(current);
        }
        if expected_revision != current.revision || adoption.expected_revision != current.revision {
            return Err(StoreError::Conflict(
                "owned adoption revision changed".into(),
            ));
        }
        let fresh = prepared.fresh(&current)?;
        let target = match mode {
            OwnedChoiceMode::Activate => fresh.target(),
            OwnedChoiceMode::Record(_) => current.program()?,
        };
        if adoption.target != revision(target)?
            || adoption.plan.scope != fresh.scope()
            || matches!(mode, OwnedChoiceMode::Activate) && adoption.plan.id != fresh.operation_id()
        {
            return Err(refuse(
                "owned adoption",
                "target, scope or operation differs",
            ));
        }
        self.check_owned_plan(&current, &adoption.plan, target, &fresh, mode)?;
        decisions.validate()?;
        let mut next = current.clone();
        retain(&mut next, &fresh.execution.proof.candidate)?;
        retain(&mut next, fresh.target())?;
        let mut state = match &current.scope {
            ScopeEnvelope::ScalarV1(s) if *s == ScopeState::default() => {
                OwnedScopeStateV2::default()
            }
            ScopeEnvelope::OwnedV2(s) if s.layers.is_empty() && !s.rehearsals.is_empty() => {
                s.clone()
            }
            _ => return Err(refuse("owned adoption", "first committed lineage required")),
        };
        next.revision = current
            .revision
            .checked_add(1)
            .ok_or_else(|| refuse("owned adoption", "revision overflow"))?;
        match mode {
            OwnedChoiceMode::Activate => {
                state
                    .layers
                    .insert(fresh.layer.identity()?, fresh.layer.clone());
                state
                    .compositions
                    .insert(fresh.manifest.identity()?, fresh.manifest.clone());
                state.initializations.push(fresh.initialization.clone());
                state.adoptions.push(OwnedAdoptionV2 {
                    version: OWNED_SCOPE_VERSION,
                    decisions: decisions
                        .decisions
                        .iter()
                        .filter(|d| {
                            d.status == DecisionStatus::Active
                                && !current.decisions.decisions.iter().any(|old| old.id == d.id)
                        })
                        .map(|d| d.id.clone())
                        .collect(),
                    plan: adoption.plan.identity()?,
                    composition: fresh.manifest.identity()?,
                    initialization: Some(fresh.initialization.identity()?),
                    initialized_compatibility: fresh.compatibility.clone(),
                    revision: next.revision,
                });
                next.data = fresh.initialized().clone();
                next.active_revision = fresh.manifest.output.clone();
            }
            OwnedChoiceMode::Record(ids) => {
                let rehearsal = fresh.rehearsal(decisions, ids, &adoption.plan)?;
                state.rehearsals.insert(rehearsal.identity()?, rehearsal);
            }
        }
        next.scope = ScopeEnvelope::OwnedV2(state);
        next.decisions = decisions.clone();
        next.adoptions.push(AdoptionReceipt {
            plan: adoption.plan.clone(),
            previous: current.active_revision.clone(),
            active: next.active_revision.clone(),
            revision: next.revision,
        });
        next.operations.insert(
            adoption.plan.id.clone(),
            OperationReceipt {
                operation: adoption.plan.id.clone(),
                request,
                revision: next.revision,
            },
        );
        verify(&current, target, decisions, &adoption.plan, &next)?;
        next.validate()?;
        LocalRuntime::with_cancellation(prepared.cancelled.clone()).resume(
            next.program()?,
            &next.data,
            &next.session,
            next.clock_day,
            0,
            RuntimeLimits::default(),
            &next.artifacts,
        )?;
        self.save(
            &next,
            true,
            #[cfg(test)]
            fault,
        )?;
        Ok(next)
    }
}
