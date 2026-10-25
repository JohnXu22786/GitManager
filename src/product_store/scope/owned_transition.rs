//! Current-data P5 capabilities. Wire recipes and successful diagnostics are
//! never transition or initialization authority.
use super::common_fact_admission::{refuse, ProofMeter};
use super::owned_persist::VerifiedOwnedTransitionExecution;
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

// This capsule is minted only by the fresh locked writer below. Its contents
// never leave the fixed-result callback and are not policy-success authority.
pub(crate) struct LockedArchivePolicy<'gate> {
    registry: &'gate owned_persist::ReconstructedOwned,
    execution: &'gate VerifiedOwnedTransitionExecution,
}
use std::mem::size_of;
use super::owned_continuation::comparison_payment;
const LOCKED_ARCHIVE_TRANSPORT_PAYMENT: u64 = comparison_payment(64, [
    (size_of::<LockedArchivePolicy<'_>>(), 4),
    (size_of::<Option<LockedArchivePolicy<'_>>>(), 4),
    (size_of::<Option<&LockedArchivePolicy<'_>>>(), 8),
    (size_of::<&owned_persist::ReconstructedOwned>(), 8),
    (size_of::<&VerifiedOwnedTransitionExecution>(), 8),
    (size_of::<&mut OwnedPolicyCheck<'_>>(), 4),
    (size_of::<&mut ProofMeter>(), 4),
    (size_of::<Result<()>>(), 4),
]);
const LOCKED_ARCHIVE_SELECTOR_PAYMENT: u64 = comparison_payment(64 + 4 * 64, [
    (size_of::<Option<&owned_persist::ReconstructedOwned>>(), 8),
    (size_of::<&LockedArchivePolicy<'_>>(), 4),
    (size_of::<&VerifiedOwnedTransitionExecution>(), 4),
    (size_of::<&Digest>(), 8),
    (size_of::<bool>(), 4),
    (size_of::<OwnedPolicyCheck<'_>>(), 4),
    (size_of::<Result<()>>(), 4),
]);

// Completed immutable policy-predicate authority, not a future rerun promise.
// Runtime windows affect refusal, not successful projection values. The exact
// factory returns positive windows bounded by the default; clipped completed
// loops qualify too. Recipient live-window checks remain independent.
// Fixed provenance only. No source payload, surface, frame tape or budget credit.
pub(crate) struct PolicyFrameRecorder<'gate> {
    registry: &'gate owned_persist::ReconstructedOwned,
    execution: &'gate VerifiedOwnedTransitionExecution,
    completed: Option<CompletedPolicyFrames<'gate>>,
}
struct CompletedPolicyFrames<'gate> {
    registry: &'gate owned_persist::ReconstructedOwned,
    snapshot: &'gate ProjectSnapshot,
    recipe: &'gate owned_persist::OwnedExecutionRecipe,
    layer_id: &'gate Digest,
    layer: &'gate OwnedLayerV2,
}
// Named complete payloads make argument, result and error transport explicit.
pub(crate) struct SurfaceRecordCall<'a, 'gate> {
    pub(crate) check: &'a mut OwnedPolicyCheck<'gate>,
    pub(crate) recorder: &'a mut PolicyFrameRecorder<'gate>,
    pub(crate) source: &'a CapturedProgram,
}
pub(crate) struct SurfaceConsumeCall<'a, 'gate, 'record> {
    pub(crate) check: &'a mut OwnedPolicyCheck<'gate>,
    pub(crate) recorder: &'a PolicyFrameRecorder<'record>,
    pub(crate) source: &'a CapturedProgram,
}
// Direct chronological selection is paid before each actual epoch. No stored
// seed proof, selector activation, current-frame alias or success flag remains.
const POLICY_CHRONOLOGY_SEED_PAYMENT: u64 = comparison_payment(64, [
    (size_of::<usize>(), 8),
    (size_of::<bool>(), 16),
    (size_of::<&owned_persist::OwnedExecutionRecipe>(), 8),
    (size_of::<&PreparedOwnedPreview>(), 8),
    (size_of::<&DataSnapshot>(), 16),
    (size_of::<&owned_continuation::VerifiedOwnedPolicySurface>(), 8),
    (size_of::<&mut ProofMeter>(), 8),
    (size_of::<Result<()>>(), 8),
]);
// Fixed lexical inputs. No selected callback/helper owns these values.
struct PolicyFrameScope<'a> {
    snapshot: &'a ProjectSnapshot,
    meter: &'a mut ProofMeter,
}
struct EpochSeedInputs<'a> {
    seed: &'a DataSnapshot,
    day: i32,
}
const POLICY_FRAME_SCOPE_PAYMENT: u64 = {
    let actual = comparison_payment(64, [
    (size_of::<PolicyFrameScope<'_>>(), 4),
    (size_of::<Result<()>>(), 8),
]);
    // Preserve the admitted fusion-v3 floor despite the deleted profile owner.
    if actual < 1536 { 1536 } else { actual }
};
const POLICY_FRAME_EPOCH_PAYMENT: u64 = {
    let actual = comparison_payment(32, [
    (size_of::<EpochSeedInputs<'_>>(), 4),
    (size_of::<Result<()>>(), 4),
]);
    // Preserve the admitted fusion-v3 floor despite the deleted profile owner.
    if actual < 832 { 832 } else { actual }
};
pub(super) struct SelectedFrameCall<'a> {
    pub(super) registry: &'a owned_persist::ReconstructedOwned,
    pub(super) snapshot: &'a ProjectSnapshot,
    pub(super) recipe: &'a owned_persist::OwnedExecutionRecipe,
    pub(super) surface: &'a owned_continuation::VerifiedOwnedPolicySurface,
    pub(super) layer: &'a OwnedLayerV2,
    pub(super) meter: &'a mut ProofMeter,
}
pub(crate) struct RecordContinuationDispatch<'gate, C> {
    pub(crate) capture: C,
    pub(crate) run: for<'call, 'slot> fn(C, &'call mut OwnedPolicyCheck<'gate>, &'slot mut PolicyFrameRecorder<'gate>) -> Result<()>,
}
pub(crate) const fn policy_frame_open_payment<C>() -> u64 {
    let actual = comparison_payment(64, [
        (size_of::<PolicyFrameRecorder<'_>>(), 4),
        (size_of::<Option<CompletedPolicyFrames<'_>>>(), 4),
        (size_of::<&mut PolicyFrameRecorder<'_>>(), 8),
        (size_of::<&mut OwnedPolicyCheck<'_>>(), 8),
        (size_of::<&VerifiedOwnedTransitionExecution>(), 8),
        (size_of::<C>(), 4),
        (size_of::<RecordContinuationDispatch<'_, C>>(), 4),
        (size_of::<std::result::Result<(), crate::product_decisions::DecisionError>>(), 8),
        (size_of::<Result<()>>(), 8),
    ]);
    // The old fixed transport floor plus the actual concrete capture/dispatch
    // preserves every generic caller price, not just the unit capture case.
    let prior_floor = comparison_payment(3200, [
        (size_of::<C>(), 4),
        (size_of::<RecordContinuationDispatch<'_, C>>(), 4),
    ]);
    if actual < prior_floor { prior_floor } else { actual }
}
pub(crate) const POLICY_FRAME_RECORD_PAYMENT: u64 = {
    let actual = comparison_payment(64, [
    (size_of::<SurfaceRecordCall<'_, '_>>(), 4),
    (size_of::<&CapturedProgram>(), 8),
    (size_of::<&owned_persist::OwnedExecutionRecipe>(), 8),
    (size_of::<&OwnedLayerV2>(), 8),
    (size_of::<Result<owned_continuation::VerifiedOwnedPolicySurface>>(), 4),
    (size_of::<Result<()>>(), 4),
]);
    // Preserve the admitted fusion-v3 floor despite the deleted profile owner.
    if actual < 1984 { 1984 } else { actual }
};
const POLICY_FRAME_MINT_PAYMENT: u64 = {
    let actual = comparison_payment(64, [
    (size_of::<CompletedPolicyFrames<'_>>(), 4),
    (size_of::<Option<CompletedPolicyFrames<'_>>>(), 4),
    (size_of::<&owned_persist::ReconstructedOwned>(), 8),
    (size_of::<&ProjectSnapshot>(), 8),
    (size_of::<&owned_persist::OwnedExecutionRecipe>(), 8),
    (size_of::<&OwnedLayerV2>(), 8),
    (size_of::<&Digest>(), 8),
    (size_of::<Result<owned_continuation::VerifiedOwnedPolicySurface>>(), 4),
]);
    // Preserve the admitted fusion-v3 floor despite the deleted profile owner.
    if actual < 1856 { 1856 } else { actual }
};
pub(crate) struct RetainedPolicyDispatch<C> {
    pub(crate) capture: C,
    pub(crate) run: for<'gate, 'record> fn(&C, &mut OwnedPolicyCheck<'gate>, &PolicyFrameRecorder<'record>) -> Result<()>,
}
struct RetainedFrameReuseCapture<'call, 'gate, 'record, C> {
    recorder: &'call PolicyFrameRecorder<'record>,
    check: &'call mut OwnedPolicyCheck<'gate>,
    dispatch: &'call RetainedPolicyDispatch<C>,
}
pub(crate) const fn policy_frame_retained_payment<C>() -> u64 {
    comparison_payment(64, [
        (size_of::<Option<&CompletedPolicyFrames<'_>>>(), 12),
        (size_of::<RetainedFrameReuseCapture<'_, '_, '_, C>>(), 4),
        (size_of::<std::result::Result<(), crate::product_decisions::DecisionError>>(), 8),
        (size_of::<Result<()>>(), 8),
    ])
}
// Complete F7 caller/selector/live-window owners, including full lowered
// Result/ControlFlow paths. Preserve every earlier multiplicity and floor.
// The base pays seven fixed activations and four bounded comparisons; every
// actual transport/query slot pays construction, access, move and cleanup.
pub(super) const POLICY_FRAME_SELECT_PAYMENT: u64 = {
    let actual = comparison_payment(64 + 4 * 64 + 6 * 64, [
        (size_of::<Result<bool>>(), 16),
        (size_of::<&PolicyFrameRecorder<'_>>(), 8),
        (size_of::<SelectedFrameCall<'_>>(), 8),
        (size_of::<std::ops::ControlFlow<Result<std::convert::Infallible>, ()>>(), 20),
        (size_of::<Result<()>>(), 24),
        (size_of::<isize>(), 36),
        (size_of::<Result<std::convert::Infallible>>(), 28),
        (size_of::<bool>(), 44),
        (size_of::<Option<&CompletedPolicyFrames<'_>>>(), 8),
        (size_of::<&Option<CompletedPolicyFrames<'_>>>(), 4),
        (size_of::<&CompletedPolicyFrames<'_>>(), 4),
        (size_of::<*const owned_persist::ReconstructedOwned>(), 16),
        (size_of::<*const owned_persist::OwnedExecutionRecipe>(), 8),
        (size_of::<*const ProjectSnapshot>(), 8),
        (size_of::<&ProjectSnapshot>(), 8),
        (size_of::<&&Digest>(), 8),
        (size_of::<&Digest>(), 20),
        (size_of::<std::ops::ControlFlow<Result<std::convert::Infallible>, bool>>(), 8),
        (size_of::<&mut ProofMeter>(), 28),
        (size_of::<&owned_persist::ReconstructedOwned>(), 12),
        (size_of::<&owned_persist::OwnedExecutionRecipe>(), 8),
        (size_of::<&VerifiedOwnedTransitionExecution>(), 4),
        (size_of::<&owned_continuation::VerifiedOwnedPolicySurface>(), 4),
        (size_of::<&OwnedLayerV2>(), 12),
        (size_of::<Option<u128>>(), 4),
        (size_of::<u128>(), 16),
        (size_of::<u64>(), 28),
        (size_of::<&std::time::Duration>(), 4),
        (size_of::<std::time::Duration>(), 4),
        (size_of::<&std::time::Instant>(), 4),
        (size_of::<RuntimeLimits>(), 4),
    ]);
    if actual < 2984 { 2984 } else { actual }
};
pub(crate) const POLICY_FRAME_CONSUME_PAYMENT: u64 = comparison_payment(64, [
    (size_of::<SurfaceConsumeCall<'_, '_, '_>>(), 4),
    (size_of::<&mut OwnedPolicyCheck<'_>>(), 8),
    (size_of::<&PolicyFrameRecorder<'_>>(), 8),
    (size_of::<&CapturedProgram>(), 8),
    (size_of::<&owned_persist::ReconstructedOwned>(), 8),
    (size_of::<&owned_persist::OwnedExecutionRecipe>(), 8),
    (size_of::<&OwnedLayerV2>(), 8),
    (size_of::<Option<&CompletedPolicyFrames<'_>>>(), 8),
    (size_of::<Result<owned_continuation::VerifiedOwnedPolicySurface>>(), 4),
    (size_of::<std::result::Result<(), crate::product_decisions::DecisionError>>(), 4),
    (size_of::<Result<()>>(), 4),
]);
impl<'gate> LockedArchivePolicy<'gate> {
    pub(crate) fn with_policy_frame_recorder<C>(
        &self, check: &mut OwnedPolicyCheck<'gate>, dispatch: RecordContinuationDispatch<'gate, C>,
    ) -> Result<()> {
        // F0 including C and the zero-capture continuation was prepaid before
        // this call. The capsule cannot be obtained by preview/general callers.
        if !std::ptr::eq(check.execution, self.execution)
            || !matches!(check.registry, Some(registry) if std::ptr::eq(registry, self.registry)) {
            return Err(refuse("owned policy frames", "recorder is not the fresh locked check"));
        }
        let mut recorder = PolicyFrameRecorder {
            registry: self.registry, execution: check.execution, completed: None,
        };
        (dispatch.run)(dispatch.capture, check, &mut recorder)
    }
}
// The frame and epoch scopes remain lexical. Root rows call the exact original
// function, with its original arguments, local return and cleanup.
macro_rules! policy_epoch_body {
    ($recipe:ident, $seed:expr, $seed_day:expr, $endpoint:expr, $day:expr,
     $meter:expr, $surface:expr, $source_recipe:ident, $epoch_index:ident) => {{
        let epoch_inputs = EpochSeedInputs { seed: $seed, day: $seed_day };
        owned_replay::recipe_history_body!($recipe, epoch_inputs.seed, epoch_inputs.day,
            $endpoint, $day, $meter, [
                if $epoch_index != 0 || std::ptr::eq(&$source_recipe.initial().initialized, epoch_inputs.seed) {
                    $meter.charge(0)?;
                } else {
                    $meter.charge(canonical_bytes(epoch_inputs.seed)?.len() as u64)?;
                    $surface.validate_frame(epoch_inputs.seed)?;
                }
            ], [
                owned_replay::protected_row($recipe, &frame, row, old, event.day, $meter)?;
            ], [
                $meter.charge(canonical_bytes(&frame)?.len() as u64)?;
                $surface.validate_frame(&frame)?;
            ], [
                $meter.charge(0)?;
            ], frame, row, old, event);
    }};
}
impl SurfaceRecordCall<'_, '_> {
    pub(crate) fn run(self) -> Result<owned_continuation::VerifiedOwnedPolicySurface> {
        let Self { check, recorder, source } = self;
        if !std::ptr::eq(check.execution, recorder.execution)
            || !matches!(check.registry, Some(registry) if std::ptr::eq(registry, recorder.registry)) {
            return Err(refuse("owned policy frames", "recording check changed"));
        }
        if recorder.completed.is_some() { return check.surface(source); }
        check.meter.charge(5)?;
        let execution = check.execution;
        let layer_id = execution.continuation.original_layer();
        let layer = execution.current().owned_scope()?.layers.get(layer_id)
            .ok_or_else(|| refuse("owned policy", "original layer missing"))?;
        let registry = recorder.registry;
        registry.check_snapshot(execution.current())?;
        #[cfg(test)]
        adoption_diagnostic::phase(adoption_diagnostic::Phase::SurfaceDerive);
        let source_recipe = registry.resolve(source)?;
        let surface = match source_recipe {
            owned_persist::OwnedExecutionRecipe::Initial(original) =>
                owned_continuation::derive_policy_surface(original, layer, None, check.meter)?,
            owned_persist::OwnedExecutionRecipe::Transition(prior) =>
                owned_continuation::derive_policy_surface(&prior.original, layer,
                    Some((&prior.continuation, &prior.target)), check.meter)?,
        };
        #[cfg(test)]
        adoption_diagnostic::phase(adoption_diagnostic::Phase::SurfaceFrames);
        check.meter.charge(POLICY_FRAME_SCOPE_PAYMENT)?;
        {
            let scope = PolicyFrameScope { snapshot: execution.current(), meter: check.meter };
            owned_persist::policy_frames_body!(registry, scope.snapshot, surface, scope.meter, [], [
                scope.meter.charge(POLICY_FRAME_EPOCH_PAYMENT)?;
                scope.meter.charge(POLICY_CHRONOLOGY_SEED_PAYMENT)?;
            ], [
                policy_epoch_body!(recipe, recipe.seed(), recipe.basis().clock_day,
                    endpoint, day, scope.meter, &surface, source_recipe, epoch_index);
            ], epoch_index, recipe, endpoint, day, unused_visit, [()]);
            scope.meter.charge(POLICY_FRAME_MINT_PAYMENT)?;
        }
        recorder.completed = Some(CompletedPolicyFrames { registry, snapshot: execution.current(),
            recipe: source_recipe, layer_id, layer });
        #[cfg(test)]
        adoption_diagnostic::phase(adoption_diagnostic::Phase::SurfaceComplete);
        Ok(surface)
    }
}
impl SurfaceConsumeCall<'_, '_, '_> {
    pub(crate) fn run(self) -> Result<owned_continuation::VerifiedOwnedPolicySurface> {
        let Self { check, recorder, source } = self;
        check.meter.charge(5)?;
        let execution = check.execution;
        let layer_id = execution.continuation.original_layer();
        let layer = execution.current().owned_scope()?.layers.get(layer_id)
            .ok_or_else(|| refuse("owned policy", "original layer missing"))?;
        match check.registry {
            Some(registry) => {
                registry.check_snapshot(execution.current())?;
                #[cfg(test)]
                adoption_diagnostic::phase(adoption_diagnostic::Phase::SurfaceDerive);
                let recipe = registry.resolve(source)?;
                let surface = match recipe {
                    owned_persist::OwnedExecutionRecipe::Initial(original) =>
                        owned_continuation::derive_policy_surface(original, layer, None, check.meter)?,
                    owned_persist::OwnedExecutionRecipe::Transition(prior) =>
                        owned_continuation::derive_policy_surface(&prior.original, layer,
                            Some((&prior.continuation, &prior.target)), check.meter)?,
                };
                #[cfg(test)]
                adoption_diagnostic::phase(adoption_diagnostic::Phase::SurfaceFrames);
                registry.validate_policy_frames_reusing(execution.current(), &surface, layer, recipe, recorder, check.meter)?;
                #[cfg(test)]
                adoption_diagnostic::phase(adoption_diagnostic::Phase::SurfaceComplete);
                Ok(surface)
            }
            None => {
                #[cfg(test)]
                adoption_diagnostic::phase(adoption_diagnostic::Phase::SurfaceReconstruct);
                let registry = owned_persist::reconstruct(execution.current(), check.meter)?;
                policy_surface_from_registry(execution, &registry, source, layer, check.meter)
            }
        }
    }
}
impl PolicyFrameRecorder<'_> {
    pub(super) fn eligible(&self, call: SelectedFrameCall<'_>) -> Result<bool> {
        call.meter.charge(0)?;
        let eligible = match self.completed.as_ref() {
            Some(receipt) =>
            std::ptr::eq(self.registry, call.registry)
                && std::ptr::eq(receipt.registry, call.registry)
                && std::ptr::eq(receipt.recipe, call.recipe)
                && std::ptr::eq(receipt.snapshot, self.execution.current())
                && receipt.layer_id == call.surface.original_layer()
                && receipt.layer.candidate == call.layer.candidate
                && receipt.layer.basis.frozen.snapshot == call.layer.basis.frozen.snapshot,
            None => false,
        };
        // The complete original binding header already associated call.snapshot
        // with this registry. Never canonicalize it a second new time here.
        let _ = call.snapshot;
        let window = call.meter.policy_frame_default_window()?;
        call.meter.charge(0)?;
        if eligible && window { call.meter.charge(0)?; }
        Ok(eligible && window)
    }
}

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
                #[cfg(test)]
                adoption_diagnostic::phase(adoption_diagnostic::Phase::SurfaceReconstruct);
                let registry = owned_persist::reconstruct(execution.current(), self.meter)?;
                policy_surface_from_registry(execution, &registry, source, layer, self.meter)
            }
        }
    }
}

// The caller prepays the complete selected invocation before minting the loan.
pub(super) fn invoke_historical_initial_policy<'gate, C>(
    execution: &'gate VerifiedOwnedTransitionExecution,
    loan: &owned_persist::HistoricalInitialLoan<'_>, meter: &'gate mut ProofMeter,
    check: &mut C,
) -> Result<()>
where C: for<'g, 'l> FnMut(&mut OwnedPolicyCheck<'g>, &owned_persist::HistoricalInitialLoan<'l>) -> Result<()>,
{
    let mut context = OwnedPolicyCheck { execution, registry: None, meter };
    check(&mut context, loan)
}
impl OwnedPolicyCheck<'_> {
    pub(crate) fn surface_with_initial_loan(
        &mut self, loan: &owned_persist::HistoricalInitialLoan<'_>, source: &CapturedProgram,
    ) -> Result<owned_continuation::VerifiedOwnedPolicySurface> {
        self.meter.charge(5)?;
        loan.surface(self.execution, source, self.meter)
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
    meter.charge(owned_persist::TRANSITION_RECONSTRUCT_PAYMENT)?;
    let (registry, execution) = owned_persist::reconstruct_and_transition(
        current, before, after, sources, layer, operation, withdrawal, &mut meter,
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
            Option<&LockedArchivePolicy<'gate>>,
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
        meter.charge(owned_persist::TRANSITION_RECONSTRUCT_PAYMENT)?;
        let (registry, execution) = owned_persist::reconstruct_and_transition(
            &current,
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
        // Prepay both Some and None transports, errors and callback cleanup.
        meter.charge(LOCKED_ARCHIVE_TRANSPORT_PAYMENT)?;
        {
            let archive_policy = if fresh.is_withdrawal() {
                Some(LockedArchivePolicy { registry: &registry, execution: &fresh.execution })
            } else {
                None
            };
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
                archive_policy.as_ref(),
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
pub(super) fn policy_surface_from_registry(
    execution: &VerifiedOwnedTransitionExecution,
    registry: &owned_persist::ReconstructedOwned,
    source: &CapturedProgram,
    layer: &OwnedLayerV2,
    meter: &mut ProofMeter,
) -> Result<owned_continuation::VerifiedOwnedPolicySurface> {
    #[cfg(test)]
    adoption_diagnostic::phase(adoption_diagnostic::Phase::SurfaceDerive);
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
    #[cfg(test)]
    adoption_diagnostic::phase(adoption_diagnostic::Phase::SurfaceFrames);
    registry.validate_policy_frames(execution.current(), &surface, meter)?;
    #[cfg(test)]
    adoption_diagnostic::phase(adoption_diagnostic::Phase::SurfaceComplete);
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
// Lexical sharing keeps ordinary export's original frame and None path.
macro_rules! retained_transition_policy_body {
    ($snapshot:ident, $check:ident, $execution:ident, $meter:ident, [$($select:tt)*], $registry:expr, $context:ident, [$($invoke:tt)*]) => {{
    if !$snapshot.owned_scope().is_ok_and(|state| {
        state.compositions.values().any(|m| {
            m.lowering == OWNED_TRANSITION_LOWERING_VERSION
                && m.transition == ScopeTransition::Withdrawal
        })
    }) {
        return Ok(());
    }
    let mut $meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
    #[cfg(test)]
    adoption_diagnostic::phase(adoption_diagnostic::Phase::ArchiveReconstruct);
    let registry = owned_persist::reconstruct($snapshot, &mut $meter)?;
    #[cfg(test)]
    adoption_diagnostic::phase(adoption_diagnostic::Phase::ArchivePolicy);
    for $execution in registry.checked_withdrawals() {
        $meter.charge(1)?;
        $meter.charge(6)?;
        $($select)*
        let mut $context = OwnedPolicyCheck {
            execution: $execution,
            registry: $registry,
            meter: &mut $meter,
        };
        $($invoke)*
    }
    Ok(())
    }};
}
pub(crate) fn check_retained_transition_policies(
    snapshot: &ProjectSnapshot,
    mut check: impl for<'gate> FnMut(&mut OwnedPolicyCheck<'gate>) -> Result<()>,
) -> Result<()> {
    retained_transition_policy_body!(snapshot, check, execution, meter, [], None, context, [check(&mut context)?;])
}
pub(crate) fn check_retained_transition_policies_locked(
    snapshot: &ProjectSnapshot,
    capsule: &LockedArchivePolicy<'_>,
    mut check: impl for<'gate> FnMut(&mut OwnedPolicyCheck<'gate>) -> Result<()>,
) -> Result<()> {
    retained_transition_policy_body!(snapshot, check, execution, meter, [
        meter.charge(LOCKED_ARCHIVE_SELECTOR_PAYMENT)?;
        // Only a bounded eligibility hint. surface() must still associate the
        // complete actual historical snapshot and resolve this exact source.
        let selected_registry = if capsule.registry.snapshot == execution.manifest.basis.frozen.snapshot {
            Some(capsule.registry)
        } else {
            None
        };
    ], selected_registry, context, [check(&mut context)?;])
}

pub(crate) fn check_retained_transition_policies_with_frames<C>(
    snapshot: &ProjectSnapshot, capsule: &LockedArchivePolicy<'_>,
    recorder: &PolicyFrameRecorder<'_>, dispatch: RetainedPolicyDispatch<C>,
) -> Result<()> {
    retained_transition_policy_body!(snapshot, check, execution, meter, [
        meter.charge(LOCKED_ARCHIVE_SELECTOR_PAYMENT)?;
        meter.charge(const { policy_frame_retained_payment::<C>() })?;
        let selected_registry = if capsule.registry.snapshot == execution.manifest.basis.frozen.snapshot {
            Some(capsule.registry)
        } else { None };
    ], selected_registry, context, [
        let transport = RetainedFrameReuseCapture { recorder, check: &mut context, dispatch: &dispatch };
        (transport.dispatch.run)(&transport.dispatch.capture, transport.check, transport.recorder)?;
    ])
}

#[cfg(test)]
#[path = "owned_transition_registry_controls.rs"]
mod policy_reference_tests;

#[cfg(test)]
#[path = "owned_policy_context_tests.rs"]
mod policy_context_tests;

#[cfg(test)]
#[path = "owned_archive_policy_reuse_tests.rs"]
mod archive_policy_reuse_tests;

#[cfg(test)]
#[path = "owned_policy_frame_reuse_tests.rs"]
mod policy_frame_reuse_tests;
