//! Current-data P5 capabilities. Wire recipes and successful diagnostics are
//! never transition or initialization authority.
use super::common_fact_admission::{refuse, ProofMeter};
use super::owned_persist::VerifiedOwnedTransitionExecution;
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// One lexical callback borrows the actual successful preparation and gate.
/// No constructor or registry/meter borrow is exposed to policy callers.
pub(crate) struct OwnedPolicyCheck<'gate> {
    execution: &'gate VerifiedOwnedTransitionExecution,
    registry: Option<&'gate owned_persist::ReconstructedOwned>,
    meter: &'gate mut ProofMeter,
}
impl<'gate> OwnedPolicyCheck<'gate> {
    pub(crate) fn execution(&mut self) -> Result<&'gate VerifiedOwnedTransitionExecution> {
        self.meter.charge(3)?;
        Ok(self.execution)
    }
    pub(crate) fn charge(&mut self, units: u64) -> Result<()> {
        self.meter.charge(units)
    }
    pub(crate) fn surface(
        &mut self,
        source: &CapturedProgram,
    ) -> Result<owned_continuation::VerifiedOwnedPolicySurface> {
        self.meter.charge(5)?;
        let execution = self.execution;
        let layer_id = execution.continuation.original_layer();
        let layer = execution
            .current()
            .owned_scope()?
            .layers
            .get(layer_id)
            .ok_or_else(|| refuse("owned policy", "original layer missing"))?;
        match self.registry {
            Some(registry) => {
                // current() is the verified execution's immutable clone. Keep
                // the complete-snapshot association, not pointer equality.
                registry.check_snapshot(execution.current())?;
                policy_surface_from_registry(execution, registry, source, layer, self.meter)
            }
            None => {
                // Archive fallback retains its fresh old-position reconstruction;
                // an endpoint registry cannot cover a historical prefix.
                let registry = owned_persist::reconstruct(execution.current(), self.meter)?;
                policy_surface_from_registry(execution, &registry, source, layer, self.meter)
            }
        }
    }
}

#[derive(Clone)]
pub struct PreparedOwnedTransitionV2 {
    pub(super) execution: Arc<VerifiedOwnedTransitionExecution>,
    pub(super) budget: ProofBudget,
    pub(super) cancelled: Arc<AtomicBool>,
    compatibility: CompatibilityReport,
}
impl PartialEq for PreparedOwnedTransitionV2 {
    fn eq(&self, other: &Self) -> bool {
        self.execution.manifest == other.execution.manifest
            && self.execution.target == other.execution.target
            && self.execution.metadata == other.execution.metadata
            && self.execution.current() == other.execution.current()
            && self.execution.initialization == other.execution.initialization
            && self.compatibility == other.compatibility
            && self.budget.units == other.budget.units
            && self.budget.elapsed_millis == other.budget.elapsed_millis
            && Arc::ptr_eq(&self.cancelled, &other.cancelled)
    }
}
impl Eq for PreparedOwnedTransitionV2 {}
impl PreparedOwnedTransitionV2 {
    pub fn target(&self) -> &CapturedProgram {
        &self.execution.target
    }
    pub fn seed(&self) -> &DataSnapshot {
        self.execution.seed()
    }
    pub fn scope(&self) -> DecisionScope {
        self.basis()
            .owned_scope()
            .expect("verified owned transition basis")
            .layers
            .get(self.execution.continuation.original_layer())
            .expect("verified original layer")
            .scope()
    }
    pub fn operation_id(&self) -> &str {
        &self.execution.manifest.operation
    }
    pub(crate) fn original_layer(&self) -> &Digest {
        self.execution.continuation.original_layer()
    }
    pub(crate) fn proof_budget(&self) -> ProofBudget {
        self.budget
    }
    pub(crate) fn cancellation(&self) -> Arc<AtomicBool> {
        self.cancelled.clone()
    }
    pub(crate) fn check_policy<T>(
        &self,
        check: impl for<'gate> FnOnce(&mut OwnedPolicyCheck<'gate>) -> Result<T>,
        meter: &mut ProofMeter,
    ) -> Result<T> {
        meter.charge(6)?;
        let mut context = OwnedPolicyCheck {
            execution: &self.execution,
            registry: None,
            meter,
        };
        check(&mut context)
    }
    pub(crate) fn basis(&self) -> &ProjectSnapshot {
        self.execution.current()
    }
    pub(crate) fn is_withdrawal(&self) -> bool {
        self.execution.manifest.transition == ScopeTransition::Withdrawal
    }
    pub(crate) fn check_basis(&self, current: &ProjectSnapshot) -> Result<()> {
        let mut meter = ProofMeter::new(self.budget, self.cancelled.clone())?;
        meter.charge(1)?;
        if current != self.basis() {
            return Err(StoreError::Conflict("owned transition preparation is stale; source, data, session, day or decisions changed".into()));
        }
        Ok(())
    }
    pub(crate) fn original_frame(
        &self,
        current: &ProjectSnapshot,
        scene: &ScenarioSpec,
    ) -> Result<()> {
        self.check_basis(current)?;
        scene.validate(&current.program()?.program)?;
        if scene.seed != current.data
            || scene.session != current.session
            || scene.clock_day != current.clock_day
            || scene.random_seed != 0
        {
            return Err(refuse(
                "owned transition experience",
                "scene differs from exact current frame",
            ));
        }
        scene.validate(&self.target().program)?;
        Ok(())
    }
    fn request(&self, plan: &AdoptionPlan, graph: &DecisionGraph) -> Result<Digest> {
        owned_wire::transition_request(plan, graph, &self.execution.manifest)
    }
}
fn finish(
    execution: Arc<VerifiedOwnedTransitionExecution>,
    budget: ProofBudget,
    cancelled: Arc<AtomicBool>,
    meter: &mut ProofMeter,
) -> Result<PreparedOwnedTransitionV2> {
    if execution.seed() != &execution.current().data {
        return Err(refuse(
            "owned transition",
            "current data changed during compilation",
        ));
    }
    let compatibility = LocalRuntime::with_cancellation(cancelled.clone()).compatibility_at(
        &execution.target,
        execution.seed(),
        execution.current().clock_day,
    )?;
    if compatibility.state != CompatibilityState::Compatible {
        return Err(StoreError::Incompatible(compatibility));
    }
    meter.charge(1)?;
    Ok(PreparedOwnedTransitionV2 {
        execution,
        budget,
        cancelled,
        compatibility,
    })
}
pub(crate) fn prepare_owned_evolution<F>(
    current: &ProjectSnapshot,
    before: &CapturedProgram,
    after: &CapturedProgram,
    sources: &OwnedSourceRequest,
    layer: &Digest,
    operation: &str,
    withdrawal: bool,
    budget: ProofBudget,
    cancelled: Arc<AtomicBool>,
    secondary: Option<Arc<AtomicBool>>,
    verify: F,
) -> Result<PreparedOwnedTransitionV2>
where
    F: for<'gate> FnOnce(&mut OwnedPolicyCheck<'gate>) -> Result<()>,
{
    let mut meter = ProofMeter::new(budget, cancelled.clone())?;
    if let Some(token) = &secondary {
        meter.watch_cancellation(token);
    }
    meter.charge(0)?;
    let registry = owned_persist::reconstruct(current, &mut meter)?;
    let execution = owned_persist::transition_from_registry(
        current, &registry, before, after, sources, layer, operation, withdrawal, &mut meter,
    )?;
    meter.charge(8)?;
    {
        let mut context = OwnedPolicyCheck {
            execution: &execution,
            registry: Some(&registry),
            meter: &mut meter,
        };
        verify(&mut context)?;
    }
    drop(registry);
    finish(execution, budget, cancelled, &mut meter)
}
pub(crate) fn prepare_owned_withdrawal<F>(
    current: &ProjectSnapshot,
    layer: &Digest,
    operation: &str,
    budget: ProofBudget,
    cancelled: Arc<AtomicBool>,
    secondary: Option<Arc<AtomicBool>>,
    verify: F,
) -> Result<PreparedOwnedTransitionV2>
where
    F: for<'gate> FnOnce(&mut OwnedPolicyCheck<'gate>) -> Result<()>,
{
    let (before, after, sources) =
        owned_persist::effective_pair(current, &current.active_revision)?;
    prepare_owned_evolution(
        current, before, after, sources, layer, operation, true, budget, cancelled, secondary,
        verify,
    )
}

impl ProductStore {
    fn check_owned_transition_plan(
        &self,
        current: &ProjectSnapshot,
        plan: &AdoptionPlan,
        prepared: &PreparedOwnedTransitionV2,
    ) -> Result<()> {
        plan.validate()?;
        prepared.target().validate()?;
        if plan.id != prepared.operation_id()
            || plan.scope != prepared.scope()
            || plan.target != prepared.target().artifact
            || plan.current_source != current.program()?.binding
            || plan.expected_data != current.data.identity()?
            || plan.expected_generation != current.data.generation
            || plan.expected_decisions != current.decisions.identity()?
            || plan.expected_session != current.session.identity()?
            || plan.project_id != current.data.project_id
            || prepared.seed() != &current.data
        {
            return Err(StoreError::Conflict(
                "owned transition plan differs from exact current data and source".into(),
            ));
        }
        let compatibility = LocalRuntime::default().compatibility_at(
            prepared.target(),
            &current.data,
            current.clock_day,
        )?;
        if compatibility != plan.compatibility
            || compatibility != prepared.compatibility
            || compatibility.state != CompatibilityState::Compatible
        {
            return Err(refuse(
                "owned transition",
                "current-data compatibility changed",
            ));
        }
        Ok(())
    }
    pub(crate) fn prepare_owned_transition_adoption(
        &self,
        plan: AdoptionPlan,
        prepared: &PreparedOwnedTransitionV2,
        graph: &DecisionGraph,
    ) -> Result<PreparedAdoption> {
        let current = self.load()?;
        prepared.check_basis(&current)?;
        self.check_owned_transition_plan(&current, &plan, prepared)?;
        owned_epoch::validate_transition_graph(
            &current,
            &prepared.execution.manifest,
            graph,
            Some(&plan.retire_decisions),
        )?;
        Ok(PreparedAdoption {
            expected_revision: current.revision,
            target: revision(prepared.target())?,
            plan,
        })
    }
    pub(crate) fn adopt_owned_transition_verified<F, V>(
        &self,
        expected_revision: u64,
        prepared: &PreparedOwnedTransitionV2,
        adoption: &PreparedAdoption,
        decisions: &DecisionGraph,
        secondary: Option<Arc<AtomicBool>>,
        verify: F,
        verify_retry: V,
        #[cfg(test)] fault: Option<FaultPoint>,
    ) -> Result<ProjectSnapshot>
    where
        F: for<'gate> FnOnce(
            &ProjectSnapshot,
            &PreparedOwnedTransitionV2,
            &DecisionGraph,
            &AdoptionPlan,
            &ProjectSnapshot,
            &mut OwnedPolicyCheck<'gate>,
        ) -> Result<()>,
        V: Fn(&ProjectSnapshot) -> Result<()>,
    {
        #[cfg(test)]
        adoption_diagnostic::phase(adoption_diagnostic::Phase::LockedLoad);
        let _lock = self.root.lock()?;
        let current = self.load()?;
        #[cfg(test)]
        adoption_diagnostic::phase(adoption_diagnostic::Phase::Request);
        let request = prepared.request(&adoption.plan, decisions)?;
        if let Some(receipt) = current.operations.get(&adoption.plan.id) {
            if receipt.request != request {
                return Err(StoreError::Conflict(
                    "owned operation ID belongs to a different request".into(),
                ));
            }
            #[cfg(test)]
            adoption_diagnostic::phase(adoption_diagnostic::Phase::Retry);
            self.confirm_owned_transition_current(&current, &verify_retry)?;
            return Ok(current);
        }
        if expected_revision != current.revision || adoption.expected_revision != current.revision {
            return Err(StoreError::Conflict(
                "owned transition revision changed".into(),
            ));
        }
        #[cfg(test)]
        adoption_diagnostic::phase(adoption_diagnostic::Phase::Regenerate);
        let mut meter = ProofMeter::new(prepared.budget, prepared.cancelled.clone())?;
        if let Some(token) = &secondary {
            meter.watch_cancellation(token);
        }
        meter.charge(0)?;
        if &current != prepared.basis() {
            return Err(StoreError::Conflict(
                "owned transition basis changed".into(),
            ));
        }
        let proof = &prepared.execution.continuation;
        // Prepay the retained local owner header and its cleanup.
        meter.charge(2)?;
        let registry = owned_persist::reconstruct(&current, &mut meter)?;
        let execution = owned_persist::transition_from_registry(
            &current,
            &registry,
            proof.before(),
            proof.after(),
            &proof.request().inventory.sources,
            proof.original_layer(),
            prepared.operation_id(),
            prepared.is_withdrawal(),
            &mut meter,
        )?;
        let fresh = finish(
            execution, prepared.budget, prepared.cancelled.clone(), &mut meter,
        )?;
        if &fresh != prepared {
            return Err(refuse(
                "owned transition",
                "complete recipe does not regenerate",
            ));
        }
        #[cfg(test)]
        adoption_diagnostic::phase(adoption_diagnostic::Phase::Plan);
        if adoption.target != revision(fresh.target())? {
            return Err(refuse("owned transition", "target differs"));
        }
        self.check_owned_transition_plan(&current, &adoption.plan, &fresh)?;
        #[cfg(test)]
        adoption_diagnostic::phase(adoption_diagnostic::Phase::Graph);
        owned_epoch::validate_transition_graph(
            &current,
            &fresh.execution.manifest,
            decisions,
            Some(&adoption.plan.retire_decisions),
        )?;
        let mut next = current.clone();
        retain(&mut next, fresh.execution.continuation.before())?;
        retain(&mut next, fresh.execution.continuation.after())?;
        retain(&mut next, fresh.target())?;
        next.revision = current
            .revision
            .checked_add(1)
            .ok_or_else(|| refuse("owned transition", "revision overflow"))?;
        let mut state = current.owned_scope()?.clone();
        state.compositions.insert(
            fresh.execution.manifest.identity()?,
            fresh.execution.manifest.clone(),
        );
        state.adoptions.push(OwnedAdoptionV2 {
            version: OWNED_SCOPE_VERSION,
            decisions: vec![],
            plan: adoption.plan.identity()?,
            composition: fresh.execution.manifest.identity()?,
            initialization: None,
            initialized_compatibility: fresh.compatibility.clone(),
            revision: next.revision,
        });
        next.scope = ScopeEnvelope::OwnedV2(state);
        next.active_revision = fresh.execution.manifest.output.clone();
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
                request: request.clone(),
                revision: next.revision,
            },
        );
        meter.charge(8)?;
        {
            let mut check = OwnedPolicyCheck {
                execution: &fresh.execution,
                registry: Some(&registry),
                meter: &mut meter,
            };
            verify(
                &current,
                &fresh,
                decisions,
                &adoption.plan,
                &next,
                &mut check,
            )?;
        }
        drop(registry);
        #[cfg(test)]
        adoption_diagnostic::phase(adoption_diagnostic::Phase::NextValidate);
        next.validate()?;
        #[cfg(test)]
        adoption_diagnostic::phase(adoption_diagnostic::Phase::Resume);
        LocalRuntime::with_cancellation(
            secondary
                .clone()
                .unwrap_or_else(|| prepared.cancelled.clone()),
        )
        .resume(
            next.program()?,
            &next.data,
            &next.session,
            next.clock_day,
            0,
            meter.runtime()?,
            &next.artifacts,
        )?;
        meter.charge(0)?;
        #[cfg(test)]
        adoption_diagnostic::phase(adoption_diagnostic::Phase::Save);
        let result = self.save_with_checkpoint(
            &next,
            true,
            || {
                #[cfg(test)]
                match fault {
                    Some(FaultPoint::CancelPrimaryBeforePointer) => {
                        prepared.cancelled.store(true, Ordering::Release)
                    }
                    Some(FaultPoint::CancelSecondaryBeforePointer) => {
                        secondary
                            .as_ref()
                            .expect("test secondary token supplied")
                            .store(true, Ordering::Release);
                    }
                    _ => {}
                }
                #[cfg(test)]
                adoption_diagnostic::phase(adoption_diagnostic::Phase::Pointer);
                meter.charge(0)
            },
            #[cfg(test)]
            fault,
        );
        match result {
            Ok(()) => Ok(next),
            Err(error) => {
                // The pointer is the commit linearization point. Complete
                // confirmation even when cancellation arrived after activation.
                #[cfg(test)]
                adoption_diagnostic::phase(adoption_diagnostic::Phase::ErrorLoad);
                let actual = self.load()?;
                if actual
                    .operations
                    .get(&adoption.plan.id)
                    .is_some_and(|r| r.request == request)
                {
                    #[cfg(test)]
                    adoption_diagnostic::phase(adoption_diagnostic::Phase::ErrorConfirm);
                    self.confirm_owned_transition_current(&actual, &verify_retry)?;
                }
                Err(error)
            }
        }
    }
    fn confirm_owned_transition_current<V>(
        &self,
        current: &ProjectSnapshot,
        verify: &V,
    ) -> Result<()>
    where
        V: Fn(&ProjectSnapshot) -> Result<()>,
    {
        verify(current)?;
        let bytes = self.root.read_and_sync("CURRENT", MAX_STORE_BYTES)?;
        let pointer: Pointer =
            serde_json::from_slice(&bytes).map_err(|e| StoreError::Corrupt(e.to_string()))?;
        let bytes = self.root.read_and_sync(
            &format!("object-{}.json", pointer.object.as_str()),
            MAX_STORE_BYTES,
        )?;
        if bytes != canonical_bytes(current)? {
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
        Ok(())
    }
}

pub(crate) fn transition_policy_surface(
    execution: &VerifiedOwnedTransitionExecution,
    source: &CapturedProgram,
    meter: &mut ProofMeter,
) -> Result<owned_continuation::VerifiedOwnedPolicySurface> {
    let layer_id = execution.continuation.original_layer();
    let layer = execution
        .current()
        .owned_scope()?
        .layers
        .get(layer_id)
        .ok_or_else(|| refuse("owned policy", "original layer missing"))?;
    let registry = owned_persist::reconstruct(execution.current(), meter)?;
    policy_surface_from_registry(execution, &registry, source, layer, meter)
}
fn policy_surface_from_registry(
    execution: &VerifiedOwnedTransitionExecution,
    registry: &owned_persist::ReconstructedOwned,
    source: &CapturedProgram,
    layer: &OwnedLayerV2,
    meter: &mut ProofMeter,
) -> Result<owned_continuation::VerifiedOwnedPolicySurface> {
    let surface = match registry.resolve(source)? {
        owned_persist::OwnedExecutionRecipe::Initial(original) => {
            owned_continuation::derive_policy_surface(original, layer, None, meter)?
        }
        owned_persist::OwnedExecutionRecipe::Transition(prior) => {
            owned_continuation::derive_policy_surface(
                &prior.original,
                layer,
                Some((&prior.continuation, &prior.target)),
                meter,
            )?
        }
    };
    registry.validate_policy_frames(execution.current(), &surface, meter)?;
    Ok(surface)
}
pub(crate) fn transition_policy_scope(
    execution: &VerifiedOwnedTransitionExecution,
) -> Result<DecisionScope> {
    Ok(execution
        .current()
        .owned_scope()?
        .layers
        .get(execution.continuation.original_layer())
        .ok_or_else(|| refuse("owned policy", "original layer missing"))?
        .scope())
}
pub(crate) fn transition_layer(execution: &VerifiedOwnedTransitionExecution) -> &Digest {
    execution.continuation.original_layer()
}
pub(crate) fn check_retained_transition_policies(
    snapshot: &ProjectSnapshot,
    mut check: impl for<'gate> FnMut(&mut OwnedPolicyCheck<'gate>) -> Result<()>,
) -> Result<()> {
    if !snapshot.owned_scope().is_ok_and(|state| {
        state.compositions.values().any(|m| {
            m.lowering == OWNED_TRANSITION_LOWERING_VERSION
                && m.transition == ScopeTransition::Withdrawal
        })
    }) {
        return Ok(());
    }
    let mut meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
    let registry = owned_persist::reconstruct(snapshot, &mut meter)?;
    for execution in registry.checked_withdrawals() {
        meter.charge(1)?;
        meter.charge(6)?;
        let mut context = OwnedPolicyCheck {
            execution,
            registry: None,
            meter: &mut meter,
        };
        check(&mut context)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "owned_transition_registry_controls.rs"]
mod policy_reference_tests;

#[cfg(test)]
#[path = "owned_policy_context_tests.rs"]
mod policy_context_tests;
