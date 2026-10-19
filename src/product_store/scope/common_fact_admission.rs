//! Exact-source, ephemeral admission. Serializable diagnostics never carry authority.
use super::*;
use crate::product_runtime::{LocalRuntime, ProjectionValidation};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};
pub(super) type AdmissionResult<T> = std::result::Result<T, StoreError>;
pub(super) fn refuse(at: &str, rule: &str) -> StoreError {
    StoreError::Invalid(format!("Common factual admission: {at}: {rule}"))
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnedPreviewAvailability {
    NullableAddedBoolean,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedPreviewRequest {
    pub inventory: CommonFactInventoryRequest,
    pub auxiliary: Vec<OwnedAuxiliary>,
    pub population: ScopePopulation,
    pub excluded_records: Vec<RecordRef>,
    pub operations: BTreeSet<Id>,
    pub availability: OwnedPreviewAvailability,
    pub purpose: String,
}
/// One cumulative host budget, separate from ordinary interpreter limits.
#[derive(Clone, Copy, Debug)]
pub struct ProofBudget {
    pub units: u64,
    pub elapsed_millis: u64,
}
impl Default for ProofBudget {
    fn default() -> Self {
        Self {
            units: 10_000_000,
            elapsed_millis: 60_000,
        }
    }
}
pub(crate) struct ProofMeter {
    left: u64,
    started: Instant,
    millis: u64,
    cancelled: Arc<AtomicBool>,
    additional_cancelled: Option<Arc<AtomicBool>>,
    #[cfg(test)]
    adoption_diagnostic: (u64, u8, u32, u8),
}
impl ProofMeter {
    #[cfg_attr(test, track_caller)]
    pub(super) fn new(budget: ProofBudget, cancelled: Arc<AtomicBool>) -> AdmissionResult<Self> {
        if budget.units == 0
            || budget.units > 10_000_000
            || budget.elapsed_millis == 0
            || budget.elapsed_millis > 60_000
        {
            return Err(refuse("budget", "host proof budget outside bounds"));
        }
        let mut result = Self {
            left: budget.units,
            started: Instant::now(),
            millis: budget.elapsed_millis,
            cancelled,
            additional_cancelled: None,
            #[cfg(test)]
            adoption_diagnostic: adoption_diagnostic::meter(std::panic::Location::caller()),
        };
        result.charge(1)?;
        Ok(result)
    }
    pub(super) fn watch_cancellation(&mut self, token: &Arc<AtomicBool>) {
        self.additional_cancelled = Some(token.clone());
    }
    pub(crate) fn charge(&mut self, units: u64) -> AdmissionResult<()> {
        if self.cancelled.load(Ordering::Relaxed)
            || self
                .additional_cancelled
                .as_ref()
                .is_some_and(|c| c.load(Ordering::Relaxed))
            || self.started.elapsed().as_millis() >= u128::from(self.millis)
        {
            #[cfg(test)]
            adoption_diagnostic::refusal(self.adoption_diagnostic, self.left, units, 2);
            return Err(refuse(
                "budget",
                "cancelled or cumulative elapsed budget exhausted",
            ));
        }
        self.left = self
            .left
            .checked_sub(units)
            .ok_or_else(|| {
                #[cfg(test)]
                adoption_diagnostic::refusal(self.adoption_diagnostic, self.left, units, 1);
                refuse("budget", "cumulative proof/initialization budget exhausted")
            })?;
        Ok(())
    }
    /// Reserve the entire allowed evaluation fuel before each execution. A new
    /// interpreter meter never replenishes this enclosing preparation budget.
    pub(super) fn runtime(&mut self) -> AdmissionResult<RuntimeLimits> {
        let mut limits = RuntimeLimits::default();
        self.charge(limits.fuel)?;
        let remaining = self
            .millis
            .saturating_sub(self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64);
        if remaining == 0 {
            return Err(refuse("budget", "cumulative elapsed budget exhausted"));
        }
        limits.elapsed_millis = limits.elapsed_millis.min(remaining);
        Ok(limits)
    }
    pub(super) fn cancelled(&self) -> Arc<AtomicBool> {
        self.cancelled.clone()
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommonFactCertificate {
    pub side: OwnedSourceSide,
    pub site: String,
    pub rule: String,
    pub identity: Digest,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OwnedProjectionDestination {
    ViewColumn {
        view: Id,
        column: Id,
    },
    RootCount {
        observable: Id,
        include_archived: bool,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedProjectionRecipe {
    pub location: String,
    pub destination: OwnedProjectionDestination,
    pub binding: Id,
    pub value_type: Type,
    /// Type in the host presentation, distinct from the authored candidate type.
    pub host_value_type: Type,
    /// Explicit baseline absence only, never a missing capture-cell fallback.
    pub absent_value: Option<DataValue>,
    pub baseline: Option<Expr>,
    pub candidate: Expr,
    /// The compiler must preserve exact cell presence and receipt provenance, even
    /// when the saved value is Null. Missing storage is never captured absence.
    pub capture_required: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommonFactAdmissionReport {
    pub version: u32,
    pub snapshot: Digest,
    pub before: Digest,
    pub after: Digest,
    pub decisions: Digest,
    pub acceptance: Digest,
    pub certificates: Vec<CommonFactCertificate>,
    pub projections: Vec<OwnedProjectionRecipe>,
    pub phases: Vec<CommonFactAction>,
    pub outcome_comparison: Vec<String>,
    pub capture_presence_required: bool,
    pub durable_adoption_supported: bool,
}
/// No Deserialize, public constructor, or conversion to a scalar-v1 preparation.
/// Full captures retain authored bytes and producer identity without rewriting.
pub struct VerifiedOwnedPreviewPlan {
    pub(super) snapshot: ProjectSnapshot,
    pub(super) candidate: CapturedProgram,
    pub(super) request: OwnedPreviewRequest,
    pub(super) accepted: super::owned_lifecycle::AcceptedPreviewLifecycle,
    pub(super) report: CommonFactAdmissionReport,
    pub(super) captured: BTreeMap<String, Values>,
}
impl VerifiedOwnedPreviewPlan {
    pub fn report(&self) -> &CommonFactAdmissionReport {
        &self.report
    }
}
pub(super) fn request_identity(request: &OwnedPreviewRequest) -> AdmissionResult<Digest> {
    Ok(canonical_digest(
        IdentityDomain::Evidence,
        &("owned-preview-request/1", request),
    )?)
}
fn authenticate_inner(
    snapshot: &ProjectSnapshot,
    candidate: &CapturedProgram,
    request: &OwnedPreviewRequest,
    meter: &mut ProofMeter,
    verify_snapshot: bool,
) -> AdmissionResult<CommonFactInventory> {
    meter.charge((candidate.source_bytes.len() + snapshot.program()?.source_bytes.len()) as u64)?;
    if verify_snapshot {
        snapshot.validate()?;
    }
    candidate.validate()?;
    if !matches!(snapshot.scope,ScopeEnvelope::ScalarV1(ref s) if *s==ScopeState::default())
        && !matches!(snapshot.scope, ScopeEnvelope::OwnedV2(ref s) if s.layers.is_empty() && !s.rehearsals.is_empty())
    {
        return Err(refuse("basis", "existing scoped lineage is unsupported"));
    }
    super::compiler::reject_reserved(&candidate.program)?;
    if request.purpose.trim().is_empty()
        || request.purpose.len() > MAX_TEXT_BYTES
        || request.operations.is_empty()
        || request.operations.len() > MAX_ITEMS
    {
        return Err(refuse(
            "request",
            "bounded explicit preview purpose and operations required",
        ));
    }
    let all: BTreeSet<_> = snapshot
        .program()?
        .program
        .actions
        .iter()
        .map(|a| a.id.clone())
        .collect();
    // This first closed grammar requires a complete writer surface. No silently
    // expanded operation set or unverified Invoke-only route can survive.
    if request.operations != all {
        return Err(refuse(
            "operations",
            "all source writers must be explicitly covered",
        ));
    }
    let mut seen = BTreeSet::new();
    for row in &request.excluded_records {
        if row.entity != request.inventory.sources.root_entity
            || !seen.insert((&row.entity, &row.record))
            || !snapshot
                .data
                .records
                .iter()
                .any(|r| r.entity == row.entity && r.id == row.record)
        {
            return Err(refuse("scope", "missing, duplicate or foreign exclusion"));
        }
    }
    if request.excluded_records.len() > MAX_COLLECTION {
        return Err(refuse("scope", "exclusion bound exceeded"));
    }
    if let ScopePopulation::SelectedUnfinished { records } = &request.population {
        if records.is_empty() || records.len() > MAX_COLLECTION {
            return Err(refuse("scope", "invalid selected population"));
        }
        let mut ids = BTreeSet::new();
        for row in records {
            if row.entity != request.inventory.sources.root_entity
                || !ids.insert((&row.entity, &row.record))
                || request.excluded_records.contains(row)
                || !snapshot
                    .data
                    .records
                    .iter()
                    .any(|r| r.entity == row.entity && r.id == row.record && !r.archived)
            {
                return Err(refuse(
                    "scope",
                    "selected root is missing, archived, repeated or excluded",
                ));
            }
        }
    }
    super::common_fact_inventory::derive(snapshot, candidate, &request.inventory)
}
pub(super) fn authenticate(
    snapshot: &ProjectSnapshot,
    candidate: &CapturedProgram,
    request: &OwnedPreviewRequest,
    meter: &mut ProofMeter,
) -> AdmissionResult<CommonFactInventory> {
    authenticate_inner(snapshot, candidate, request, meter, true)
}
// This internal decision route authenticates the live snapshot, but does not
// settle intentions. DecisionEngine independently checks the selected target.
pub(super) fn derive_for_decision(
    snapshot: &ProjectSnapshot,
    candidate: &CapturedProgram,
    request: &OwnedPreviewRequest,
    accepted: &super::owned_lifecycle::AcceptedPreviewLifecycle,
    meter: &mut ProofMeter,
) -> AdmissionResult<VerifiedOwnedPreviewPlan> {
    snapshot.validate()?;
    derive_inner(snapshot, candidate, request, accepted, meter, false)
}
pub(super) fn derive_retained(
    snapshot: &ProjectSnapshot,
    candidate: &CapturedProgram,
    request: &OwnedPreviewRequest,
    accepted: &super::owned_lifecycle::AcceptedPreviewLifecycle,
    meter: &mut ProofMeter,
) -> AdmissionResult<VerifiedOwnedPreviewPlan> {
    derive_inner(snapshot, candidate, request, accepted, meter, false)
}
pub(super) fn derive(
    snapshot: &ProjectSnapshot,
    candidate: &CapturedProgram,
    request: &OwnedPreviewRequest,
    accepted: &super::owned_lifecycle::AcceptedPreviewLifecycle,
    meter: &mut ProofMeter,
) -> AdmissionResult<VerifiedOwnedPreviewPlan> {
    derive_inner(snapshot, candidate, request, accepted, meter, true)
}
fn derive_inner(
    snapshot: &ProjectSnapshot,
    candidate: &CapturedProgram,
    request: &OwnedPreviewRequest,
    accepted: &super::owned_lifecycle::AcceptedPreviewLifecycle,
    meter: &mut ProofMeter,
    verify_snapshot: bool,
) -> AdmissionResult<VerifiedOwnedPreviewPlan> {
    let inventory = authenticate_inner(snapshot, candidate, request, meter, verify_snapshot)?;
    accepted.verify(snapshot, candidate, request, meter)?;
    // Structural regeneration of retained recipes never settles independent
    // intentions. Their archive scenes are checked by the decision replay path.
    // Public preview still cannot make that comparison or issue write authority.
    if verify_snapshot
        && snapshot
            .decisions
            .decisions
            .iter()
            .any(|d| d.status == DecisionStatus::Active)
    {
        return Err(refuse("accepted decisions", "active retained outcome/property comparison is unsupported by this preview proof; no obligation is silently passed"));
    }
    let (certificates, projections, comparison) =
        super::common_fact_rules::verify(snapshot, candidate, request, &inventory, meter)?;
    let captured = capture_frame(
        snapshot.program()?,
        &snapshot.data,
        snapshot.clock_day,
        request,
        &projections,
        meter,
    )?;
    let report = CommonFactAdmissionReport {
        version: 1,
        snapshot: request.inventory.snapshot.clone(),
        before: inventory.structure.before.clone(),
        after: inventory.structure.after.clone(),
        decisions: snapshot.decisions.identity()?,
        acceptance: accepted.identity().clone(),
        certificates,
        projections,
        phases: inventory.actions,
        outcome_comparison: comparison,
        capture_presence_required: true,
        durable_adoption_supported: false,
    };
    meter.charge(1)?;
    Ok(VerifiedOwnedPreviewPlan {
        snapshot: snapshot.clone(),
        candidate: candidate.clone(),
        request: request.clone(),
        accepted: accepted.copy_internal(),
        report,
        captured,
    })
}
impl ProductStore {
    pub fn verify_owned_preview(
        &self,
        candidate: &CapturedProgram,
        request: &OwnedPreviewRequest,
        accepted: &super::owned_lifecycle::AcceptedPreviewLifecycle,
        budget: ProofBudget,
        cancelled: Arc<AtomicBool>,
    ) -> AdmissionResult<VerifiedOwnedPreviewPlan> {
        let mut meter = ProofMeter::new(budget, cancelled)?;
        let snapshot = self.load()?;
        meter.charge(1)?;
        derive(&snapshot, candidate, request, accepted, &mut meter)
    }
    /// Re-derive all authority against the live store; a diagnostic digest or old
    /// in-memory plan is insufficient when any source/frame/decision has changed.
    pub fn revalidate_owned_preview(
        &self,
        proof: &VerifiedOwnedPreviewPlan,
        budget: ProofBudget,
        cancelled: Arc<AtomicBool>,
    ) -> AdmissionResult<()> {
        let mut meter = ProofMeter::new(budget, cancelled)?;
        let snapshot = self.load()?;
        meter.charge(1)?;
        let regenerated = derive(
            &snapshot,
            &proof.candidate,
            &proof.request,
            &proof.accepted,
            &mut meter,
        )?;
        if regenerated.snapshot != proof.snapshot
            || regenerated.report != proof.report
            || regenerated.captured != proof.captured
        {
            return Err(refuse(
                "proof",
                "complete proof/capture regeneration differs",
            ));
        }
        meter.charge(1)?;
        Ok(())
    }
}

#[cfg(test)]
mod budget_tests {
    use super::*;
    #[test]
    fn child_runtime_budget_inherits_remaining_host_deadline() {
        let mut meter = ProofMeter::new(
            ProofBudget {
                units: 1_000_000,
                elapsed_millis: 100,
            },
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert!(meter.runtime().unwrap().elapsed_millis <= 100);
    }
}

/// Evaluate sealed-row captures against an explicit authenticated scene frame.
pub(super) fn capture_frame(
    baseline: &CapturedProgram,
    data: &DataSnapshot,
    day: i32,
    request: &OwnedPreviewRequest,
    projections: &[OwnedProjectionRecipe],
    meter: &mut ProofMeter,
) -> AdmissionResult<BTreeMap<String, Values>> {
    let mut captured = BTreeMap::new();
    let runtime = LocalRuntime::with_cancellation(meter.cancelled());
    let mut checks = ProjectionValidation::default();
    for row in data
        .records
        .iter()
        .filter(|r| r.entity == request.inventory.sources.root_entity)
    {
        meter.charge(1)?;
        let reference = RecordRef {
            entity: row.entity.clone(),
            record: row.id.clone(),
        };
        let completed = runtime.evaluate_record_projection_with(
            &mut checks,
            meter.runtime()?,
            baseline,
            data,
            &reference,
            "record",
            &request.inventory.lifecycle.before,
            &Type::Boolean,
            day,
        )?;
        let is_completed = completed == DataValue::Boolean { value: true };
        if matches!(&request.population,ScopePopulation::SelectedUnfinished{records} if records.contains(&reference))
            && is_completed
        {
            return Err(refuse("scope", "selected root is already completed"));
        }
        if !row.archived && !is_completed {
            continue;
        }
        let mut values = Values::new();
        for projection in projections {
            let value = if let Some(expr) = &projection.baseline {
                runtime.evaluate_record_projection_with(
                    &mut checks,
                    meter.runtime()?,
                    baseline,
                    data,
                    &reference,
                    &projection.binding,
                    expr,
                    &projection.value_type,
                    day,
                )?
            } else {
                projection.absent_value.clone().ok_or_else(|| {
                    refuse(
                        &projection.location,
                        "absent baseline has no explicit availability rule",
                    )
                })?
            };
            // Inserting even Null is intentional; presence is part of the capture receipt.
            values.insert(projection.location.clone(), value);
            meter.charge(1)?;
        }
        captured.insert(row.id.clone(), values);
    }
    Ok(captured)
}

#[cfg(test)]
impl ProofMeter {
    pub(super) fn remaining_units_for_test(&self) -> u64 {
        self.left
    }
}

// Numeric, test-only diagnosis of one strict withdrawal-adoption call. This
// cannot reserve, refund, mutate, expose or construct a proof capability.
#[cfg(test)]
pub(crate) mod adoption_diagnostic {
    use std::cell::Cell;
    thread_local! {
        static STATE: Cell<(bool, u64, u8, bool)> = const { Cell::new((false, 0, 0, false)) };
    }
    #[derive(Clone, Copy)]
    #[repr(u8)]
    pub(crate) enum Phase {
        Entry = 1, LockedLoad = 2, Request = 3, Retry = 4,
        Regenerate = 5, Plan = 6, Graph = 7, CallbackBinding = 8,
        WithdrawalGraph = 9, Policy = 10, Packages = 11,
        RemainingReplay = 12, Bundle = 13, NextValidate = 14,
        Resume = 15, Save = 16, Pointer = 17, ErrorLoad = 18,
        ErrorConfirm = 19,
        ArchiveReconstruct = 20, ArchivePolicy = 21,
    }
    pub(crate) struct Guard((bool, u64, u8, bool));
    pub(crate) fn start() -> Guard {
        Guard(STATE.with(|s| s.replace((true, 0, Phase::Entry as u8, false))))
    }
    impl Drop for Guard {
        fn drop(&mut self) { STATE.with(|s| s.set(self.0)); }
    }
    pub(crate) fn phase(phase: Phase) {
        STATE.with(|s| { let (active, next, _, failed) = s.get();
            if active { s.set((active, next, phase as u8, failed)); }
        });
    }
    pub(super) fn meter(at: &std::panic::Location<'_>) -> (u64, u8, u32, u8) {
        STATE.with(|s| {
            let (active, next, phase, failed) = s.get();
            if !active { return (0, 0, 0, 0); }
            let next = next.saturating_add(1);
            s.set((active, next, phase, failed));
            let file = if at.file().ends_with("common_fact_admission.rs") { 1 }
                else if at.file().ends_with("owned_transition.rs") { 2 }
                else if at.file().ends_with("owned_persist.rs") { 3 }
                else if at.file().ends_with("owned_replay.rs") { 4 }
                else if at.file().ends_with("owned_lifecycle.rs") { 5 }
                else if at.file().ends_with("owned_preview.rs") { 6 }
                else if at.file().ends_with("owned_choice.rs") { 7 }
                else { 0 };
            (next, file, at.line(), phase)
        })
    }
    pub(super) fn refusal(meter: (u64, u8, u32, u8), left: u64, requested: u64, reason: u8) {
        STATE.with(|s| {
            let (active, next, phase, failed) = s.get();
            if active && !failed {
                s.set((active, next, phase, true));
                eprintln!("P5_ADOPTION_FIRST_REFUSAL meter={} file={} line={} origin_phase={} phase={} left={} requested={} reason={}",
                    meter.0, meter.1, meter.2, meter.3, phase, left, requested, reason);
            }
        });
    }
}
