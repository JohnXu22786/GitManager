//! Exact-source, ephemeral admission. Serializable diagnostics never carry authority.
use super::*;
use crate::product_contract::{capture_payment, ContractWork, ContractWorkError, WorkFailure};
use crate::product_runtime::{LocalRuntime, ProjectionHost, ProjectionReservation};
#[cfg(test)]
use crate::product_runtime::ProjectionValidation;
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
    #[cfg_attr(test, track_caller)]
    fn charge_cause(&mut self, units: u64) -> std::result::Result<(), ProofChargeFailure> {
        #[cfg(test)]
        let caller = std::panic::Location::caller();
        let stopped = if self.cancelled.load(Ordering::Relaxed) {
            Some(ProofChargeFailure::Cancelled)
        } else if self
            .additional_cancelled
            .as_ref()
            .is_some_and(|c| c.load(Ordering::Relaxed))
        {
            Some(ProofChargeFailure::Cancelled)
        } else if self.started.elapsed().as_millis() >= u128::from(self.millis) {
            Some(ProofChargeFailure::Elapsed)
        } else {
            None
        };
        if let Some(cause) = stopped {
            #[cfg(test)]
            adoption_diagnostic::refusal(self.adoption_diagnostic, self.left, units, 2, caller);
            return Err(cause);
        }
        self.left = self.left.checked_sub(units).ok_or_else(|| {
            #[cfg(test)]
            adoption_diagnostic::refusal(self.adoption_diagnostic, self.left, units, 1, caller);
            ProofChargeFailure::Fuel
        })?;
        Ok(())
    }
    #[cfg_attr(test, track_caller)]
    pub(crate) fn charge(&mut self, units: u64) -> AdmissionResult<()> {
        self.charge_cause(units).map_err(|cause| match cause {
            ProofChargeFailure::Cancelled | ProofChargeFailure::Elapsed => refuse(
                "budget", "cancelled or cumulative elapsed budget exhausted",
            ),
            ProofChargeFailure::Fuel => {
                refuse("budget", "cumulative proof/initialization budget exhausted")
            }
        })
    }
    /// Reserve the entire allowed evaluation fuel before each execution. A new
    /// interpreter meter never replenishes this enclosing preparation budget.
    #[cfg_attr(test, track_caller)]
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
// The ordinary wrapper and capture adapter consume the actual selected cause.
// No later token reread or diagnostic text classification determines it.
enum ProofChargeFailure {
    Cancelled,
    Elapsed,
    Fuel,
}
impl ProofChargeFailure {
    fn work(self) -> WorkFailure {
        match self {
            Self::Cancelled => WorkFailure::Cancelled,
            Self::Elapsed => WorkFailure::BudgetExhausted("cumulative elapsed budget exhausted"),
            Self::Fuel => WorkFailure::BudgetExhausted("cumulative proof/initialization budget exhausted"),
        }
    }
}
impl ContractWork for ProofMeter {
    fn charge(&mut self, units: u64) -> std::result::Result<(), WorkFailure> {
        self.charge_cause(units).map_err(ProofChargeFailure::work)
    }
}
impl ProjectionHost for ProofMeter {
    fn reserve_projection_row(&mut self) -> std::result::Result<ProjectionReservation, WorkFailure> {
        // Compile-time pricing cannot overflow on any supported pointer width.
        const _: () = assert!(usize::BITS <= u64::BITS);
        const HEADER: u64 = 40 + std::mem::size_of::<ProjectionReservation>() as u64;
        self.charge_cause(HEADER).map_err(ProofChargeFailure::work)?;
        let mut limits = RuntimeLimits::default();
        let host_deadline = self.started.checked_add(std::time::Duration::from_millis(self.millis))
            .ok_or(WorkFailure::BudgetExhausted("cumulative elapsed budget exhausted"))?;
        let reservation_now = Instant::now();
        let remaining = self.millis.saturating_sub(
            self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        );
        if remaining == 0 {
            return Err(WorkFailure::BudgetExhausted("cumulative elapsed budget exhausted"));
        }
        let row_window = limits.elapsed_millis;
        limits.elapsed_millis = row_window.min(remaining);
        let row_deadline = reservation_now.checked_add(std::time::Duration::from_millis(row_window))
            .ok_or(WorkFailure::BudgetExhausted("cumulative elapsed budget exhausted"))?
            .min(host_deadline);
        self.charge_cause(limits.fuel).map_err(ProofChargeFailure::work)?;
        Ok(ProjectionReservation::from_actual_host(
            limits, host_deadline, row_deadline,
            self.cancelled.clone(), self.additional_cancelled.clone(),
        ))
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
macro_rules! authenticate_body {
    ($snapshot:ident, $candidate:ident, $request:ident, $meter:ident, $verify_snapshot:ident, $tail:expr) => {{
    $meter.charge(($candidate.source_bytes.len() + $snapshot.program()?.source_bytes.len()) as u64)?;
    if $verify_snapshot {
        $snapshot.validate()?;
    }
    $candidate.validate()?;
    if !matches!($snapshot.scope,ScopeEnvelope::ScalarV1(ref s) if *s==ScopeState::default())
        && !matches!($snapshot.scope, ScopeEnvelope::OwnedV2(ref s) if s.layers.is_empty() && !s.rehearsals.is_empty())
    {
        return Err(refuse("basis", "existing scoped lineage is unsupported"));
    }
    super::compiler::reject_reserved(&$candidate.program)?;
    if $request.purpose.trim().is_empty()
        || $request.purpose.len() > MAX_TEXT_BYTES
        || $request.operations.is_empty()
        || $request.operations.len() > MAX_ITEMS
    {
        return Err(refuse(
            "request",
            "bounded explicit preview purpose and operations required",
        ));
    }
    let all: BTreeSet<_> = $snapshot
        .program()?
        .program
        .actions
        .iter()
        .map(|a| a.id.clone())
        .collect();
    // This first closed grammar requires a complete writer surface. No silently
    // expanded operation set or unverified Invoke-only route can survive.
    if $request.operations != all {
        return Err(refuse(
            "operations",
            "all source writers must be explicitly covered",
        ));
    }
    let mut seen = BTreeSet::new();
    for row in &$request.excluded_records {
        if row.entity != $request.inventory.sources.root_entity
            || !seen.insert((&row.entity, &row.record))
            || !$snapshot
                .data
                .records
                .iter()
                .any(|r| r.entity == row.entity && r.id == row.record)
        {
            return Err(refuse("scope", "missing, duplicate or foreign exclusion"));
        }
    }
    if $request.excluded_records.len() > MAX_COLLECTION {
        return Err(refuse("scope", "exclusion bound exceeded"));
    }
    if let ScopePopulation::SelectedUnfinished { records } = &$request.population {
        if records.is_empty() || records.len() > MAX_COLLECTION {
            return Err(refuse("scope", "invalid selected population"));
        }
        let mut ids = BTreeSet::new();
        for row in records {
            if row.entity != $request.inventory.sources.root_entity
                || !ids.insert((&row.entity, &row.record))
                || $request.excluded_records.contains(row)
                || !$snapshot
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

    $tail
    }};
}

fn authenticate_inner(
    snapshot: &ProjectSnapshot,
    candidate: &CapturedProgram,
    request: &OwnedPreviewRequest,
    meter: &mut ProofMeter,
    verify_snapshot: bool,
) -> AdmissionResult<CommonFactInventory> {
    authenticate_body!(snapshot, candidate, request, meter, verify_snapshot,
        super::common_fact_inventory::derive(snapshot, candidate, &request.inventory))
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
// A finite D1-only record, never a proof or reusable runtime admission. The
// closed producer owns it through completion; failed work cannot export facts.
pub(super) struct InitialLimitsProfile {
    lifecycle: u64,
    rows: u64,
    full: bool,
}
impl InitialLimitsProfile {
    const ROOT: usize = 4 * (std::mem::size_of::<Self>()
        + 2 * std::mem::size_of::<RuntimeLimits>() + 8 * std::mem::size_of::<usize>()
        + 4 * std::mem::size_of::<AdmissionResult<()>>());
    pub(super) fn new(meter: &mut ProofMeter) -> AdmissionResult<Self> {
        meter.charge(0)?;
        meter.charge(Self::ROOT as u64 + 64)?;
        Ok(Self { lifecycle: 0, rows: 0, full: true })
    }
    pub(super) fn observe(&mut self, limits: &RuntimeLimits, row: bool, meter: &mut ProofMeter) -> AdmissionResult<()> {
        meter.charge(256)?;
        self.observe_paid(limits, row).map_err(capture_resource_error)?;
        Ok(())
    }
    // Caller prepays256 before the immutable tuple read and this finite update.
    pub(super) fn observe_paid(&mut self, limits: &RuntimeLimits, row: bool) -> std::result::Result<(), WorkFailure> {
        let RuntimeLimits { fuel, collection_items, action_steps, transaction_writes, output_bytes, elapsed_millis } = RuntimeLimits::default();
        let RuntimeLimits { fuel: f, collection_items: c, action_steps: a, transaction_writes: w, output_bytes: o, elapsed_millis: e } = limits;
        let full = *f == fuel && *c == collection_items && *a == action_steps
            && *w == transaction_writes && *o == output_bytes && *e == elapsed_millis;
        let count = if row { &mut self.rows } else { &mut self.lifecycle };
        *count = count.checked_add(1).ok_or(WorkFailure::BudgetExhausted("initial limits profile overflow"))?;
        self.full &= full;
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn deny_reuse_for_test(&mut self) { self.full = false; }
    #[cfg(test)]
    pub(super) fn counts_for_test(&self) -> (u64, u64, bool) { (self.lifecycle, self.rows, self.full) }
    pub(super) fn eligible(&self, meter: &mut ProofMeter) -> AdmissionResult<bool> {
        meter.charge(0)?;
        meter.charge(64)?;
        let elapsed = meter.started.elapsed().as_millis().min(u64::MAX as u128) as u64;
        let eligible = self.full && self.lifecycle == 6 && self.rows >= 2
            && meter.millis.saturating_sub(elapsed) >= RuntimeLimits::default().elapsed_millis;
        meter.charge(0)?;
        Ok(eligible)
    }
}
// Sole caller: the closed initial-owned composition. This returns real full D1
// facts, not authority to attach a plan supplied by a caller or another meter.
pub(super) fn derive_initial_for_decision(
    snapshot: &ProjectSnapshot,
    candidate: &CapturedProgram,
    request: &OwnedPreviewRequest,
    accepted: &super::owned_lifecycle::AcceptedPreviewLifecycle,
    meter: &mut ProofMeter,
) -> AdmissionResult<(VerifiedOwnedPreviewPlan, InitialLimitsProfile)> {
    snapshot.validate()?;
    let inventory = authenticate_inner(snapshot, candidate, request, meter, false)?;
    let mut profile = InitialLimitsProfile::new(meter)?;
    accepted.verify_profiled(snapshot, candidate, request, meter, Some(&mut profile))?;
    meter.charge(2)?;
    meter.charge(FINISH_PROFILE_FRAME)?;
    let proof = finish_derived_plan_profiled(snapshot, candidate, request, accepted, meter, false, inventory, Some(&mut profile))?;
    meter.charge(0)?;
    meter.charge(64)?;
    Ok((proof, profile))
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
// This closed composition alone may reuse the verification it just performed.
// A live default-sized window is an explicit resource-sensitive eligibility
// rule; it does not assert equivalence to later counterfactual clock clamps.
macro_rules! finish_admission_body {
    ($snapshot:ident, $candidate:ident, $request:ident, $accepted:ident, $meter:ident, $inventory:ident, $certificates:ident, $projections:ident, $comparison:ident, $captured:ident) => {{
    let report = CommonFactAdmissionReport {
        version: 1,
        snapshot: $request.inventory.snapshot.clone(),
        before: $inventory.structure.before.clone(),
        after: $inventory.structure.after.clone(),
        decisions: $snapshot.decisions.identity()?,
        acceptance: $accepted.identity().clone(),
        certificates: $certificates,
        projections: $projections,
        phases: $inventory.actions,
        outcome_comparison: $comparison,
        capture_presence_required: true,
        durable_adoption_supported: false,
    };
    $meter.charge(1)?;
    Ok::<_, StoreError>(VerifiedOwnedPreviewPlan {
        snapshot: $snapshot.clone(),
        candidate: $candidate.clone(),
        request: $request.clone(),
        accepted: $accepted.copy_internal(),
        report,
        captured: $captured,
    })
    }};
}

pub(super) fn reconstruct_and_derive_retained(
    snapshot: &ProjectSnapshot,
    candidate: &CapturedProgram,
    request: &OwnedPreviewRequest,
    lifecycle: &OwnedLifecycleV2,
    meter: &mut ProofMeter,
) -> AdmissionResult<(super::owned_lifecycle::AcceptedPreviewLifecycle, VerifiedOwnedPreviewPlan)> {
    let accepted = super::owned_lifecycle::AcceptedPreviewLifecycle::reconstruct(
        snapshot, candidate, request, lifecycle, meter,
    )?;
    meter.charge(RETAINED_SOURCE_HANDOFF_G)?;
    let CapturedRetainedCapture { inventory, certificates, projections, comparison, captured } =
        authenticate_retained(snapshot, candidate, request, meter,
            RetainedFinish { snapshot, candidate, request, accepted: &accepted, lifecycle },
            CaptureTail { snapshot, request })?;
    let proof = finish_admission_body!(snapshot, candidate, request, accepted, meter, inventory,
        certificates, projections, comparison, captured)?;
    Ok((accepted, proof))
}

macro_rules! retained_guard_body {
    ($snapshot:ident, $candidate:ident, $request:ident, $lifecycle:ident, $accepted:ident, $meter:ident) => {{
    let RuntimeLimits {
        fuel, collection_items, action_steps, transaction_writes, output_bytes,
        elapsed_millis,
    } = RuntimeLimits::default();
    // Exhaustive fields deliberately make a future runtime limit an explicit
    // eligibility decision rather than silently reusing incomplete evidence.
    let default_limits = |limits: &RuntimeLimits| {
        let RuntimeLimits {
            fuel: actual_fuel,
            collection_items: actual_collection_items,
            action_steps: actual_action_steps,
            transaction_writes: actual_transaction_writes,
            output_bytes: actual_output_bytes,
            elapsed_millis: actual_elapsed_millis,
        } = limits;
        *actual_fuel == fuel
            && *actual_collection_items == collection_items
            && *actual_action_steps == action_steps
            && *actual_transaction_writes == transaction_writes
            && *actual_output_bytes == output_bytes
            && *actual_elapsed_millis == elapsed_millis
    };
    let eligible = match $lifecycle.evidence.as_slice() {
        [before, after] if default_limits(&before.limits) && default_limits(&after.limits) => {
            let elapsed = $meter.started.elapsed().as_millis().min(u64::MAX as u128) as u64;
            $meter.millis.saturating_sub(elapsed) >= elapsed_millis
        }
        _ => false,
    };
    if !eligible {
        $accepted.verify($snapshot, $candidate, $request, $meter)?;
    }
    // Includes the suffix boundary and the factory's returned pair/headers.
    // This live check retains both tokens and the original elapsed deadline.
    $meter.charge(5)?;

    }};
}

// Test-only frozen selected suffix retains the previous exact accounting oracle.
// Production uses RetainedFinish and the same lexical guard/body below.
#[cfg(test)]
fn finish_reconstructed_plan(
    snapshot: &ProjectSnapshot,
    candidate: &CapturedProgram,
    request: &OwnedPreviewRequest,
    lifecycle: &OwnedLifecycleV2,
    accepted: &super::owned_lifecycle::AcceptedPreviewLifecycle,
    inventory: CommonFactInventory,
    meter: &mut ProofMeter,
) -> AdmissionResult<VerifiedOwnedPreviewPlan> {
    retained_guard_body!(snapshot, candidate, request, lifecycle, accepted, meter);
    finish_derived_plan(snapshot, candidate, request, accepted, meter, false, inventory)
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
    meter.charge(2)?;
    finish_derived_plan(
        snapshot, candidate, request, accepted, meter, verify_snapshot, inventory,
    )
}
// Added wrapper arguments include the owned inventory itself, not merely its
// pointer. Four extra words cover return-place/status and alignment/cleanup.
const FINISH_PROFILE_FRAME: u64 = 4 * (6 * std::mem::size_of::<&()>()
    + std::mem::size_of::<bool>() + std::mem::size_of::<CommonFactInventory>()
    + 4 * std::mem::size_of::<usize>()) as u64 + 64;
fn finish_derived_plan(
    snapshot: &ProjectSnapshot,
    candidate: &CapturedProgram,
    request: &OwnedPreviewRequest,
    accepted: &super::owned_lifecycle::AcceptedPreviewLifecycle,
    meter: &mut ProofMeter,
    verify_snapshot: bool,
    inventory: CommonFactInventory,
) -> AdmissionResult<VerifiedOwnedPreviewPlan> {
    meter.charge(FINISH_PROFILE_FRAME)?;
    finish_derived_plan_profiled(snapshot, candidate, request, accepted, meter, verify_snapshot, inventory, None)
}

fn finish_derived_plan_profiled(
    snapshot: &ProjectSnapshot,
    candidate: &CapturedProgram,
    request: &OwnedPreviewRequest,
    accepted: &super::owned_lifecycle::AcceptedPreviewLifecycle,
    meter: &mut ProofMeter,
    verify_snapshot: bool,
    inventory: CommonFactInventory,
    profile: Option<&mut InitialLimitsProfile>,
) -> AdmissionResult<VerifiedOwnedPreviewPlan> {
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
    meter.charge(INITIAL_CAPTURE_PROFILE_TRANSPORT)?;
    let captured = capture_frame_batched_profiled(
        snapshot.program()?,
        &snapshot.data,
        snapshot.clock_day,
        request,
        &projections,
        meter,
        profile,
    )?;
    finish_admission_body!(snapshot, candidate, request, accepted, meter, inventory, certificates, projections, comparison, captured)
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
    capture_frame_batched(baseline, data, day, request, projections, meter)
}

fn capture_resource_error(cause: WorkFailure) -> AdapterError {
    match cause {
        WorkFailure::Cancelled => AdapterError::Cancelled,
        WorkFailure::BudgetExhausted(reason) => AdapterError::BudgetExhausted(reason.into()),
    }
}
fn capture_formula_error(error: ContractWorkError) -> AdapterError {
    match error {
        ContractWorkError::Contract(error) => AdapterError::Invalid(error),
        ContractWorkError::Resource(cause) => capture_resource_error(cause),
    }
}
fn capture_arithmetic_error() -> AdapterError {
    capture_resource_error(WorkFailure::BudgetExhausted("capture work arithmetic overflow"))
}

// These private helpers receive only the current capture's charge method. They
// neither construct a gate nor accept a replacement source/frame/reservation.
fn capture_string_equal(
    left: &str, right: &str,
    charge: &mut impl FnMut(u64) -> std::result::Result<(), AdapterError>,
) -> std::result::Result<bool, AdapterError> {
    charge(2)?; // Actual length reads, including uncapped no-root input.
    let a = left.len();
    let b = right.len();
    charge(2)?; // Length comparison and its branch.
    let native = if a == b {
        charge(6)?; // Conversion, multiply, add and their failure decisions.
        u64::try_from(a).ok().and_then(|n| n.checked_mul(2))
            .and_then(|n| n.checked_add(3)).ok_or_else(capture_arithmetic_error)?
    } else {
        3
    };
    charge(native)?;
    Ok(left == right)
}
fn capture_string_copy(
    value: &str,
    charge: &mut impl FnMut(u64) -> std::result::Result<(), AdapterError>,
) -> std::result::Result<String, AdapterError> {
    charge(8)?;
    let native = capture_payment::string_copy_bytes(value.len()).map_err(capture_formula_error)?;
    charge(native)?;
    Ok(value.to_owned())
}
fn capture_reference_equal(
    left: &RecordRef, right: &RecordRef,
    charge: &mut impl FnMut(u64) -> std::result::Result<(), AdapterError>,
) -> std::result::Result<bool, AdapterError> {
    charge(2)?;
    let entity = capture_string_equal(&left.entity, &right.entity, charge)?;
    charge(1)?;
    if !entity { return Ok(false); }
    charge(2)?;
    capture_string_equal(&left.record, &right.record, charge)
}
fn capture_insert<V>(
    map: &mut BTreeMap<String, V>, key: String, value: V,
    charge: &mut impl FnMut(u64) -> std::result::Result<(), AdapterError>,
) -> std::result::Result<(), AdapterError> {
    charge(8)?; // Two lengths, conversion, comparator multiply/add and decisions.
    let len = map.len();
    let comparator = u64::try_from(key.len()).ok().and_then(|n| n.checked_mul(2))
        .and_then(|n| n.checked_add(3)).ok_or_else(capture_arithmetic_error)?;
    const INSERT_DESCRIPTOR: u64 = 104 + 4 * usize::BITS as u64;
    charge(INSERT_DESCRIPTOR)?; // D+96, including checked native formula.
    let native = capture_payment::btree_insert::<String, V>(len, comparator)
        .map_err(capture_formula_error)?;
    charge(native)?;
    map.insert(key, value);
    Ok(())
}
fn capture_paid_refusal(
    at: &str, rule: &str,
    charge: &mut impl FnMut(u64) -> std::result::Result<(), AdapterError>,
) -> AdmissionResult<StoreError> {
    charge(8)?; // Actual two lengths, three additions and failure decisions.
    let bytes = 26usize.checked_add(at.len()).and_then(|n| n.checked_add(2))
        .and_then(|n| n.checked_add(rule.len())).ok_or_else(capture_arithmetic_error)?;
    const FORMAT_DESCRIPTOR: u64 = 64 + 4 * usize::BITS as u64;
    charge(FORMAT_DESCRIPTOR)?;
    let native = capture_payment::format_bytes(bytes).map_err(capture_formula_error)?;
    charge(native)?;
    charge(1)?; // StoreError wrapper, before constructing the formatted error.
    Ok(refuse(at, rule))
}

// The added helper owns copies of all ordinary arguments as well as three
// simultaneous optional-profile roles (parameter, outer and inner closures).
// Two return-place/status words and two alignment/cleanup words are explicit.
// Prepay before the helper/closures, including None and first-row refusal.
const INITIAL_CAPTURE_PROFILE_TRANSPORT: u64 = 4 * (
    std::mem::size_of::<&CapturedProgram>() + std::mem::size_of::<&DataSnapshot>()
    + std::mem::size_of::<i32>() + std::mem::size_of::<&OwnedPreviewRequest>()
    + std::mem::size_of::<&[OwnedProjectionRecipe]>() + std::mem::size_of::<&mut ProofMeter>()
    + 3 * std::mem::size_of::<Option<&'static mut InitialLimitsProfile>>()
    + 4 * std::mem::size_of::<usize>()) as u64 + 64;
fn capture_frame_batched(
    baseline: &CapturedProgram,
    data: &DataSnapshot,
    day: i32,
    request: &OwnedPreviewRequest,
    projections: &[OwnedProjectionRecipe],
    meter: &mut ProofMeter,
) -> AdmissionResult<BTreeMap<String, Values>> {
    meter.charge(INITIAL_CAPTURE_PROFILE_TRANSPORT)?;
    capture_frame_batched_profiled(baseline, data, day, request, projections, meter, None)
}
macro_rules! capture_frame_body {
    ($capture:ident, $data:ident, $request:ident, $projections:ident, $profile:ident, $captured:ident) => {{
        $capture.charge_host(1)?;
        let mut records = $data.records.iter();
        loop {
            $capture.charge_host(1)?;
            let Some(record) = records.next() else { break; };
            $capture.charge_host(5)?; // Child edge2, record1, two inspected entity fields2.
            let matches = capture_string_equal(
                &record.entity, &$request.inventory.sources.root_entity,
                &mut |n| $capture.charge_host(n),
            )?;
            $capture.charge_host(1)?;
            if !matches { continue; }
            $capture.charge_host(1)?; // Preserve the original matching-row debit.
            $capture.charge_host(2)?;
            let entity = capture_string_copy(&record.entity, &mut |n| $capture.charge_host(n))?;
            let id = capture_string_copy(&record.id, &mut |n| $capture.charge_host(n))?;
            $capture.charge_host(4)?; // Field moves, RecordRef constructor and cleanup.
            let reference = RecordRef { entity, record: id };
            let values = $capture.with_row(
                &reference, "record", &$request.inventory.lifecycle.before, &Type::Boolean,
                |row, completed| -> AdmissionResult<Option<Values>> {
                    row.charge_host(2)?;
                    if let Some($profile) = $profile.as_deref_mut() {
                        row.charge_host(256)?;
                        $profile.observe_paid(row.actual_limits(), true).map_err(capture_resource_error)?;
                    }
                    row.charge_host(3)?; // Boolean/tag inspection and comparison.
                    let is_completed = completed == DataValue::Boolean { value: true };
                    row.charge_host(2)?; // Population field/tag inspection.
                    let selected = if let ScopePopulation::SelectedUnfinished { records } = &$request.population {
                        row.charge_host(1)?;
                        let mut selected = records.iter();
                        let mut found = false;
                        loop {
                            row.charge_host(1)?;
                            let Some(candidate) = selected.next() else { break; };
                            row.charge_host(2)?;
                            let equal = capture_reference_equal(candidate, &reference, &mut |n| row.charge_host(n))?;
                            row.charge_host(1)?;
                            if equal { found = true; break; }
                        }
                        found
                    } else { false };
                    // Membership deliberately precedes the completed short circuit.
                    row.charge_host(1)?;
                    if selected {
                        row.charge_host(1)?;
                        if is_completed {
                            return Err(capture_paid_refusal(
                                "scope", "selected root is already completed", &mut |n| row.charge_host(n),
                            )?);
                        }
                    }
                    row.charge_host(2)?; // Archived field and its Boolean branch.
                    if !record.archived {
                        row.charge_host(1)?;
                        if !is_completed { return Ok(None); }
                    }
                    row.charge_host(8)?;
                    let mut values = Values::new();
                    row.charge_host(1)?;
                    let mut recipes = $projections.iter();
                    loop {
                        row.charge_host(1)?;
                        let Some(projection) = recipes.next() else { break; };
                        row.charge_host(4)?; // Child edge2, record and baseline tag.
                        let value = if let Some(expr) = &projection.baseline {
                            row.evaluate(&projection.binding, expr, &projection.value_type)?
                        } else {
                            row.charge_host(1)?;
                            let Some(value) = projection.absent_value.as_ref() else {
                                return Err(capture_paid_refusal(
                                    &projection.location, "absent baseline has no explicit availability rule",
                                    &mut |n| row.charge_host(n),
                                )?);
                            };
                            row.account_value(value, &projection.host_value_type)?
                        };
                        row.charge_host(1)?;
                        let key = capture_string_copy(&projection.location, &mut |n| row.charge_host(n))?;
                        capture_insert(&mut values, key, value, &mut |n| row.charge_host(n))?;
                        row.charge_host(1)?; // Preserve original successful insertion debit.
                    }
                    row.charge_host(1)?;
                    Ok(Some(values))
                },
            )?;
            $capture.charge_host(1)?;
            if let Some(values) = values {
                $capture.charge_host(1)?;
                let key = capture_string_copy(&record.id, &mut |n| $capture.charge_host(n))?;
                capture_insert(&mut $captured, key, values, &mut |n| $capture.charge_host(n))?;
            }
        }
        $capture.charge_host(1)?;
        Ok($captured)
    }};
}

fn capture_frame_batched_profiled(
    baseline: &CapturedProgram,
    data: &DataSnapshot,
    day: i32,
    request: &OwnedPreviewRequest,
    projections: &[OwnedProjectionRecipe],
    meter: &mut ProofMeter,
    mut profile: Option<&mut InitialLimitsProfile>,
) -> AdmissionResult<BTreeMap<String, Values>> {
    // Empty capture-map cleanup8 and actual LocalRuntime token/header lifetime5.
    ContractWork::charge(meter, 13).map_err(capture_resource_error)?;
    let mut captured = BTreeMap::new();
    let runtime = LocalRuntime::with_cancellation(meter.cancelled());
    runtime.with_projection_capture(baseline, data, day, meter, |capture| {
        capture_frame_body!(capture, data, request, projections, profile, captured)
    })
}

// All arithmetic is evaluated only in constants. Overflow or an unsupported
// pointer width is a compilation error, never a discounted runtime payment.
const fn retained_sum<const N: usize>(terms: [usize; N]) -> usize {
    let mut total = 0usize;
    let mut i = 0;
    while i < N { total = match total.checked_add(terms[i]) { Some(n) => n, None => panic!("retained source sum overflow") }; i += 1; }
    total
}
const fn retained_mul(a: usize, b: usize) -> usize {
    match a.checked_mul(b) { Some(n) => n, None => panic!("retained source product overflow") }
}
const fn retained_units(bytes: usize) -> u64 {
    assert!(usize::BITS <= u64::BITS, "retained source size conversion overflow");
    bytes as u64
}
const RETAINED_SOURCE_REGIONS: [usize; 14] = {
    use std::mem::size_of;
    type Ready = ReadyCaptureSource<'static, 'static, ProofMeter>;
    type Map = BTreeMap<String, Values>;
    const R: usize = size_of::<&()>();
    const PWORD: usize = size_of::<usize>();
    const WORDS: usize = retained_mul(8, PWORD);
    const F: usize = Ready::FACT_BYTES;
    const O: usize = size_of::<Ready>();
    const CB: usize = retained_sum([size_of::<RetainedCaptureBody>(), WORDS]);
    const WB: usize = retained_sum([CB, retained_mul(2, R), size_of::<i32>(), WORDS]);
    const T: usize = size_of::<RetainedFinish>();
    const IX: usize = size_of::<super::common_fact_inventory::InventoryTail>();
    const SX: usize = size_of::<super::owned_resolve::StructuralTail>();
    const CX: usize = size_of::<CaptureTail>();
    const P: usize = size_of::<PreparedRetainedCapture>();
    const K: usize = size_of::<CapturedRetainedCapture>();
    const L: usize = size_of::<LocalRuntime>();
    const A: usize = size_of::<CommonFactInventory>();
    const S: usize = size_of::<OwnedStructuralReport>();
    const D: usize = size_of::<Digest>();
    const V: usize = size_of::<VerifiedOwnedPreviewPlan>();
    const B: usize = size_of::<CommonFactAdmissionReport>();
    const M: usize = size_of::<Map>();
    const Q: usize = Ready::FACT_RESULT_BYTES;
    const Z: usize = size_of::<AdmissionResult<PreparedRetainedCapture>>();
    const RR: usize = size_of::<AdmissionResult<CapturedRetainedCapture>>();
    const J: usize = size_of::<AdmissionResult<VerifiedOwnedPreviewPlan>>();
    const Y: usize = size_of::<AdmissionResult<(Vec<CommonFactCertificate>, Vec<OwnedProjectionRecipe>, Vec<String>)>>();
    const AS: usize = size_of::<AdmissionResult<OwnedStructuralReport>>();
    const AA: usize = size_of::<AdmissionResult<CommonFactInventory>>();
    const AM: usize = size_of::<AdmissionResult<Map>>();
    const X: usize = LocalRuntime::capture_result_bytes::<Map, StoreError>();
    [
        retained_sum([T, CX, WORDS]),
        retained_sum([retained_mul(4,R), T, CX, RR, WORDS]),
        retained_sum([retained_mul(4,R), T, CX, IX, D, RR, WORDS]),
        retained_sum([retained_mul(4,R), IX, CX, SX, RR, WORDS]),
        retained_sum([retained_mul(2,R), SX, CX, F, Q, P, Z, retained_mul(2,O), RR, WORDS]),
        retained_sum([SX, R, S, AS, Z, WORDS]),
        retained_sum([IX, S, R, A, AA, Z, WORDS]),
        retained_sum([T, A, R, P, Y, Z, L, M, WORDS]),
        retained_sum([CX, O, P, K, L, retained_mul(2,M), AM, RR, WORDS]),
        retained_sum([O, retained_mul(3,R), size_of::<&str>(), size_of::<i32>(), retained_mul(2,M), AM, retained_mul(2,CB), WORDS]),
        retained_sum([retained_mul(2,R), CB, O, size_of::<i32>(), M, X, AM, WORDS]),
        retained_sum([O, retained_mul(2,R), WB, size_of::<u64>(), F, Ready::OPTIONAL_FACT_BYTES, X, WORDS]),
        retained_sum([retained_mul(3,R), retained_mul(4,PWORD)]),
        retained_sum([retained_mul(2,RR), retained_mul(2,Z), retained_mul(2,AM), retained_mul(2,Q), retained_mul(2,X), J, V, B, K, WORDS]),
    ]
};
const RETAINED_SOURCE_HANDOFF_G: u64 = retained_units(retained_sum([
    retained_mul(4, retained_sum(RETAINED_SOURCE_REGIONS)), retained_mul(14, 128),
]));

// Fixed, private transport records. Every complete owner/result is in G below.
pub(super) struct RetainedFinish<'s> {
    snapshot: &'s ProjectSnapshot,
    candidate: &'s CapturedProgram,
    request: &'s OwnedPreviewRequest,
    accepted: &'s super::owned_lifecycle::AcceptedPreviewLifecycle,
    lifecycle: &'s OwnedLifecycleV2,
}
pub(super) struct PreparedRetainedCapture {
    inventory: CommonFactInventory,
    certificates: Vec<CommonFactCertificate>,
    projections: Vec<OwnedProjectionRecipe>,
    comparison: Vec<String>,
    captured: BTreeMap<String, Values>,
    runtime: LocalRuntime,
}
pub(super) struct CapturedRetainedCapture {
    inventory: CommonFactInventory,
    certificates: Vec<CommonFactCertificate>,
    projections: Vec<OwnedProjectionRecipe>,
    comparison: Vec<String>,
    captured: BTreeMap<String, Values>,
}
pub(super) struct CaptureTail<'s> {
    snapshot: &'s ProjectSnapshot,
    request: &'s OwnedPreviewRequest,
}
impl RetainedFinish<'_> {
    pub(super) fn prepare(self, inventory: CommonFactInventory, meter: &mut ProofMeter)
        -> AdmissionResult<PreparedRetainedCapture>
    {
        let Self { snapshot, candidate, request, accepted, lifecycle } = self;
        meter.charge(32)?;
        retained_guard_body!(snapshot, candidate, request, lifecycle, accepted, meter);
        meter.charge(FINISH_PROFILE_FRAME)?;
        let (certificates, projections, comparison) =
            super::common_fact_rules::verify(snapshot, candidate, request, &inventory, meter)?;
        meter.charge(INITIAL_CAPTURE_PROFILE_TRANSPORT)?;
        // Preserve the original capture prelude's lookup and error position.
        snapshot.program()?;
        ContractWork::charge(meter, 13).map_err(capture_resource_error)?;
        let captured = BTreeMap::new();
        let runtime = LocalRuntime::with_cancellation(meter.cancelled());
        Ok(PreparedRetainedCapture { inventory, certificates, projections, comparison, captured, runtime })
    }
}
impl<'s> SourceConsumer<'s, ProofMeter, PreparedRetainedCapture> for CaptureTail<'s> {
    type Output = CapturedRetainedCapture;
    type Error = StoreError;
    fn consume<'g>(self, ready: ReadyCaptureSource<'g, 's, ProofMeter>, prepared: PreparedRetainedCapture)
        -> AdmissionResult<CapturedRetainedCapture>
    {
        let PreparedRetainedCapture { inventory, certificates, projections, comparison, captured, runtime } = prepared;
        let captured = capture_frame_retained(ready, &runtime, &self.snapshot.data,
            self.snapshot.clock_day, self.request, &projections, captured)?;
        Ok(CapturedRetainedCapture { inventory, certificates, projections, comparison, captured })
    }
}
fn authenticate_retained<'s>(snapshot: &'s ProjectSnapshot, candidate: &'s CapturedProgram,
    request: &'s OwnedPreviewRequest, meter: &mut ProofMeter,
    finish: RetainedFinish<'s>, consume: CaptureTail<'s>,
) -> AdmissionResult<CapturedRetainedCapture> {
    let verify_snapshot = false;
    authenticate_body!(snapshot, candidate, request, meter, verify_snapshot,
        super::common_fact_inventory::derive_retained(snapshot, candidate, &request.inventory, meter, finish, consume))
}
struct RetainedCaptureBody<'b, 'p> {
    captured: BTreeMap<String, Values>,
    data: &'b DataSnapshot,
    request: &'b OwnedPreviewRequest,
    projections: &'b [OwnedProjectionRecipe],
    profile: Option<&'p mut InitialLimitsProfile>,
}
fn capture_frame_retained<'g, 's>(ready: ReadyCaptureSource<'g, 's, ProofMeter>, runtime: &LocalRuntime,
    data: &'s DataSnapshot, day: i32, request: &OwnedPreviewRequest,
    projections: &[OwnedProjectionRecipe], captured: BTreeMap<String, Values>,
) -> AdmissionResult<BTreeMap<String, Values>> {
    let body = RetainedCaptureBody { captured, data, request, projections, profile: None };
    runtime.with_retained_projection_capture(ready, data, day, move |capture| {
        let RetainedCaptureBody { mut captured, data, request, projections, mut profile } = body;
        capture_frame_body!(capture, data, request, projections, profile, captured)
    })
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
        SurfaceReconstruct = 22, SurfaceDerive = 23,
        SurfaceFrames = 24, SurfaceComplete = 25,
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
    pub(super) fn refusal(meter: (u64, u8, u32, u8), left: u64, requested: u64, reason: u8, caller: &std::panic::Location<'_>) {
        STATE.with(|s| {
            let (active, next, phase, failed) = s.get();
            if active && !failed {
                s.set((active, next, phase, true));
                let charge_file = if caller.file().ends_with("common_fact_admission.rs") { 1 }
                    else if caller.file().ends_with("owned_transition.rs") { 2 }
                    else if caller.file().ends_with("owned_persist.rs") { 3 }
                    else if caller.file().ends_with("owned_replay.rs") { 4 }
                    else if caller.file().ends_with("owned_lifecycle.rs") { 5 }
                    else if caller.file().ends_with("owned_preview.rs") { 6 }
                    else if caller.file().ends_with("owned_choice.rs") { 7 }
                    else { 0 };
                eprintln!("P5_ADOPTION_FIRST_REFUSAL meter={} file={} line={} origin_phase={} phase={} left={} requested={} reason={} charge_file={} charge_line={}",
                    meter.0, meter.1, meter.2, meter.3, phase, left, requested, reason, charge_file, caller.line());
            }
        });
    }
}

#[cfg(test)]
mod retained_lifecycle_controls;

#[cfg(test)]
mod capture_row_batch_controls;

#[cfg(test)]
mod retained_source_controls;
