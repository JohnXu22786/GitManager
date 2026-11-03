//! Production interpreter for the closed, typed local-application language.
//! Runs own copied state. Only the project store/controller can commit it.
mod compatibility;
mod capture_cost;
mod capture_native;
mod capture_eval;
mod capture_key;
mod fallible_lower;
mod query_order;
mod owned_env;
use owned_env::ManagedEnv;
pub(crate) use capture::{ProjectionHost, ProjectionReservation};
mod eval;
mod execute;
#[cfg(test)]
pub(crate) use execute::decoded_shared_transaction_kernel_parity_and_atomicity;
mod view;
use crate::product_alloc;

use crate::product_contract::*;
use crate::product_protocol::RuntimeView;
use eval::{Env, EnvAccess, Eval};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Instant;
use serde::Serialize;

pub const PROTECTED_FIELD_PREFIX: &str = "gm_scope_";
/// Host admission is separate from executable semantics. The interpreter still
/// executes one ordinary program; this hook validates retained scope provenance.
pub trait ReplayAdmission: Send + Sync {
    /// A family may allocate a private full-frame controller for this one run.
    /// The default preserves existing scalar admission behavior and identities.
    fn begin_replay(
        &self,
        _program: &CapturedProgram,
        _scenario: &ScenarioSpec,
        _decisions: &DecisionGraph,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<Option<Box<dyn ReplayAdmission>>> {
        Ok(None)
    }

    /// Keep synthetic record allocation tied to the authenticated original
    /// business input when host-only metadata is added to a replay seed.
    fn replay_operation_ids(
        &self,
        _program: &CapturedProgram,
        scenario: &ScenarioSpec,
    ) -> Result<Vec<Id>> {
        Ok(scenario.replay_operation_ids()?)
    }

    fn validate_seed(
        &self,
        program: &CapturedProgram,
        data: &DataSnapshot,
        day: i32,
    ) -> Result<()> {
        self.validate_state(program, data, day)
    }
    fn validate_state(
        &self,
        program: &CapturedProgram,
        data: &DataSnapshot,
        day: i32,
    ) -> Result<()>;
}
pub fn has_protected_fields(program: &CapturedProgram) -> bool {
    program.program.entities.iter().any(|e| {
        e.fields
            .iter()
            .any(|f| f.id.starts_with(PROTECTED_FIELD_PREFIX))
    })
}

#[cfg(test)]
#[path = "fallible_projection_tests.rs"]
mod fallible_projection_tests;
pub const RUNTIME_VERSION: &str = "local-interpreter/4";
pub const DRIVER_VERSION: &str = "semantic-input/3";
type Result<T> = std::result::Result<T, AdapterError>;

fn app_adapter_error(error: AppValidationError) -> AdapterError {
    match error {
        AppValidationError::Contract(error) => AdapterError::Invalid(error),
        AppValidationError::Allocation(failure) => AdapterError::Allocation(failure),
    }
}

fn codec_adapter_error(error: crate::product_backup::fallible_codec::CodecError) -> AdapterError {
    match error {
        crate::product_backup::fallible_codec::CodecError::Allocation(failure) => AdapterError::Allocation(failure),
        _ => AdapterError::Failed("decoded runtime evidence codec invariant".into()),
    }
}

/// Successful immutable checks retained only by one history-verification call.
/// Borrowing captures prevents mutation or pointer reuse during this lifetime.
/// No data frame, row, day, meter, cancellation state or evaluated value is kept.
#[derive(Default)]
pub(crate) struct ProjectionValidation<'a> {
    sources: Vec<&'a CapturedProgram>,
    types: std::collections::BTreeSet<Vec<u8>>,
    bytes: usize,
    #[cfg(test)]
    source_checks: usize,
    #[cfg(test)]
    type_checks: usize,
}
impl<'a> ProjectionValidation<'a> {
    fn source(&mut self, program: &'a CapturedProgram) -> Result<Option<usize>> {
        if let Some(index) = self
            .sources
            .iter()
            .position(|prior| std::ptr::eq(*prior, program) || *prior == program)
        {
            return Ok(Some(index));
        }
        #[cfg(test)]
        {
            self.source_checks += 1;
        }
        program.validate()?;
        if self.sources.len() == MAX_ITEMS {
            return Ok(None);
        }
        self.sources.push(program);
        Ok(Some(self.sources.len() - 1))
    }
    #[cfg(test)]
    pub(crate) fn counts(&self) -> (usize, usize) {
        (self.source_checks, self.type_checks)
    }
}

fn invalid(message: &str) -> AdapterError {
    AdapterError::Invalid(ContractError(message.into()))
}
fn failed(message: &str) -> AdapterError {
    AdapterError::Failed(message.into())
}
fn exhausted(message: &str) -> AdapterError {
    AdapterError::BudgetExhausted(message.into())
}

fn diagnostic(error: &AdapterError) -> String {
    let mut message = format!("{error:?}");
    if message.len() > MAX_TEXT_BYTES {
        let mut end = MAX_TEXT_BYTES - 3;
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
        message.push_str("...");
    }
    message
}

struct BoundedDiagnostic<'a> {
    value: String,
    truncated: bool,
    allocations: &'a mut product_alloc::AllocationContext,
    failure: Option<product_alloc::AllocationFailure>,
}
impl std::fmt::Write for BoundedDiagnostic<'_> {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        if self.truncated || value.is_empty() { return Ok(()); }
        let remaining = MAX_TEXT_BYTES.saturating_sub(self.value.len());
        let take = if value.len() <= remaining { value.len() } else { remaining };
        let mut end = take;
        while !value.is_char_boundary(end) { end -= 1; }
        if let Err(failure) = product_alloc::try_reserve_string(
            &mut self.value, end, self.allocations, product_alloc::AllocationSite::String,
        ) {
            self.failure = Some(failure);
            return Err(std::fmt::Error);
        }
        self.value.push_str(&value[..end]);
        if end < value.len() {
            self.truncated = true;
            let mut prefix = MAX_TEXT_BYTES.saturating_sub(3).min(self.value.len());
            while !self.value.is_char_boundary(prefix) { prefix -= 1; }
            self.value.truncate(prefix);
        }
        Ok(())
    }
}

fn diagnostic_decoded(
    error: &AdapterError,
    allocations: &mut product_alloc::AllocationContext,
) -> Result<String> {
    let mut output = BoundedDiagnostic {
        value: String::new(), truncated: false, allocations, failure: None,
    };
    let formatted = std::fmt::write(&mut output, format_args!("{error:?}"));
    if let Some(failure) = output.failure { return Err(AdapterError::Allocation(failure)); }
    if formatted.is_err() { return Err(failed("runtime diagnostic formatting failed")); }
    if output.truncated {
        product_alloc::try_append_str(
            &mut output.value, "...", output.allocations, product_alloc::AllocationSite::String,
        ).map_err(AdapterError::Allocation)?;
    }
    Ok(output.value)
}

/// The prepared branch owns its validation capability; it cannot be paired
/// with another caller-supplied AST or binding. Ordinary public entry points
/// keep the original full-validation path.
#[derive(Clone)]
pub(crate) struct ProgramSource<'a>(SourceKind<'a>);
#[derive(Clone)]
enum SourceKind<'a> {
    Ordinary(std::borrow::Cow<'a, CapturedProgram>),
    Prepared(PreparedProgram),
}
impl<'a> ProgramSource<'a> {
    pub(crate) fn ordinary(program: &'a CapturedProgram) -> Self {
        Self(SourceKind::Ordinary(std::borrow::Cow::Borrowed(program)))
    }
    pub(crate) fn prepare(program: &CapturedProgram) -> Result<Self> {
        Ok(Self(SourceKind::Prepared(PreparedProgram::new(program)?)))
    }
    fn owned(&self) -> ProgramSource<'static> {
        ProgramSource(match &self.0 {
            SourceKind::Ordinary(program) => {
                SourceKind::Ordinary(std::borrow::Cow::Owned(program.as_ref().clone()))
            }
            SourceKind::Prepared(program) => SourceKind::Prepared(program.clone()),
        })
    }
    pub(crate) fn capture(&self) -> &CapturedProgram {
        match &self.0 {
            SourceKind::Ordinary(program) => program,
            SourceKind::Prepared(program) => program.capture(),
        }
    }
    pub(crate) fn validate(&self) -> Result<()> {
        match &self.0 {
            SourceKind::Ordinary(program) => Ok(program.validate()?),
            SourceKind::Prepared(_) => Ok(()),
        }
    }
    fn validate_session(&self, session: &SessionState) -> Result<()> {
        match &self.0 {
            SourceKind::Ordinary(program) => Ok(session.validate(&program.program)?),
            SourceKind::Prepared(program) => Ok(program.validate_session(session)?),
        }
    }
    fn validate_scenario(&self, scenario: &ScenarioSpec) -> Result<()> {
        match &self.0 {
            SourceKind::Ordinary(program) => Ok(scenario.validate(&program.program)?),
            SourceKind::Prepared(program) => Ok(program.validate_scenario(scenario)?),
        }
    }
    fn initial_session(&self) -> Result<SessionState> {
        match &self.0 {
            SourceKind::Ordinary(program) => Ok(SessionState::initial(&program.program)?),
            SourceKind::Prepared(program) => Ok(program.initial_session()),
        }
    }
    fn observable_types(&self) -> Result<BTreeMap<Id, Type>> {
        match &self.0 {
            SourceKind::Ordinary(program) => Ok(program.program.observable_types()?),
            SourceKind::Prepared(program) => Ok(program.observable_types().clone()),
        }
    }
    fn view_schema(&self, view: &str) -> Result<Option<ViewSchema>> {
        match &self.0 {
            SourceKind::Ordinary(program) => Ok(program.program.view_schema(view)?),
            SourceKind::Prepared(program) => Ok(program.view_schema(view)?.clone()),
        }
    }
}
impl std::ops::Deref for ProgramSource<'_> {
    type Target = CapturedProgram;
    fn deref(&self) -> &Self::Target {
        self.capture()
    }
}

#[derive(Clone, Default)]
pub struct LocalRuntime {
    cancelled: Arc<AtomicBool>,
    admission: Option<Arc<dyn ReplayAdmission>>,
}
#[derive(Clone)]
struct State {
    data: DataSnapshot,
    session: SessionState,
    day: i32,
    outputs: Vec<LocalArtifact>,
    output_participants: Vec<EmittedRecordParticipants>,
    output_participant_count: usize,
}
/// An isolated execution copy. Accessors deliberately do not expose mutation.
pub struct ProductRun {
    program: ProgramSource<'static>,
    state: State,
    seed: u64,
    limits: RuntimeLimits,
    fuel: Cell<u64>,
    trace: Vec<TraceStep>,
    observations: Vec<Observation>,
    receipts: BTreeMap<Id, (Digest, TraceStep)>,
}

/// Transaction-local run owner for callers that already hold the fallibly
/// decoded program, data and session graphs. The legacy `ProductRun` remains
/// unchanged; ProductStore can migrate its apply path to this owner without
/// materializing the legacy runtime DTOs.
pub(crate) struct DecodedProductRun<'a> {
    program: &'a FallibleDecodedCapturedProgram,
    data: FallibleDecodedDataSnapshot,
    session: FallibleDecodedSessionState,
    day: i32,
    initial_day: i32,
    seed: u64,
    limits: RuntimeLimits,
    fuel: u64,
    trace: Vec<DecodedTraceStep>,
    observations: Vec<view::DecodedRuntimeObservation>,
    outputs: Vec<FallibleDecodedLocalArtifact>,
    receipts: Vec<DecodedOperationReceipt>,
}

#[derive(Debug)]
pub(crate) struct DecodedTraceStep {
    pub(crate) index: usize,
    pub(crate) operation_id: String,
    pub(crate) input_digest: Digest,
    /// Store-level idempotency digest for the original SemanticInput alone.
    pub(crate) store_request_digest: Digest,
    pub(crate) action: Option<String>,
    pub(crate) before_data: Digest,
    pub(crate) after_data: Digest,
    pub(crate) before_session: Digest,
    pub(crate) after_session: Digest,
    pub(crate) outputs: Vec<Digest>,
    pub(crate) outcome: StepOutcome,
    pub(crate) diagnostic: Option<String>,
}

pub(crate) struct DecodedOperationReceipt {
    pub(crate) operation_id: String,
    pub(crate) step: usize,
}

impl DecodedProductRun<'_> {
    pub(crate) fn data(&self) -> &FallibleDecodedDataSnapshot { &self.data }
    pub(crate) fn session(&self) -> &FallibleDecodedSessionState { &self.session }
    pub(crate) fn trace(&self) -> &[DecodedTraceStep] { &self.trace }
    pub(crate) fn observations(&self) -> &[view::DecodedRuntimeObservation] { &self.observations }
    pub(crate) fn artifacts(&self) -> &[FallibleDecodedLocalArtifact] { &self.outputs }
    pub(crate) fn clock_day(&self) -> i32 { self.day }
    pub(crate) fn initial_day(&self) -> i32 { self.initial_day }
    pub(crate) fn matches_program_revision_with(
        &self,
        revision: &Digest,
        allocations: &mut product_alloc::AllocationContext,
    ) -> std::result::Result<bool, AppValidationError> {
        Ok(self.program.revision_with(allocations)? == *revision)
    }
    pub(crate) fn operation_receipts(&self) -> &[DecodedOperationReceipt] { &self.receipts }
    pub(crate) fn trace_step_applied(&self, index: usize) -> bool {
        self.trace.get(index).is_some_and(|step| step.outcome == StepOutcome::Applied)
    }
    pub(crate) fn into_commit_parts(self) -> (
        FallibleDecodedDataSnapshot,
        FallibleDecodedSessionState,
        i32,
        Vec<FallibleDecodedLocalArtifact>,
    ) {
        (self.data, self.session, self.day, self.outputs)
    }
}
impl DecodedTraceStep {
    fn try_clone_with(
        &self,
        allocations: &mut product_alloc::AllocationContext,
    ) -> Result<Self> {
        let copy_string = |value: &str, allocations: &mut product_alloc::AllocationContext| {
            product_alloc::try_copy_str(value, allocations, product_alloc::AllocationSite::String)
                .map_err(AdapterError::Allocation)
        };
        let mut outputs = Vec::new();
        product_alloc::try_reserve(&mut outputs, self.outputs.len(), allocations, product_alloc::AllocationSite::Vector)
            .map_err(AdapterError::Allocation)?;
        for digest in &self.outputs {
            outputs.push(try_clone_digest(digest, allocations)?);
        }
        Ok(Self {
            index: self.index,
            operation_id: copy_string(&self.operation_id, allocations)?,
            input_digest: try_clone_digest(&self.input_digest, allocations)?,
            store_request_digest: try_clone_digest(&self.store_request_digest, allocations)?,
            action: self.action.as_deref().map(|value| copy_string(value, allocations)).transpose()?,
            before_data: try_clone_digest(&self.before_data, allocations)?,
            after_data: try_clone_digest(&self.after_data, allocations)?,
            before_session: try_clone_digest(&self.before_session, allocations)?,
            after_session: try_clone_digest(&self.after_session, allocations)?,
            outputs,
            outcome: self.outcome,
            diagnostic: self.diagnostic.as_deref().map(|value| copy_string(value, allocations)).transpose()?,
        })
    }
}

fn try_clone_digest(
    digest: &Digest,
    allocations: &mut product_alloc::AllocationContext,
) -> Result<Digest> {
    let value = product_alloc::try_copy_str(digest.as_str(), allocations, product_alloc::AllocationSite::String)
        .map_err(AdapterError::Allocation)?;
    Digest::from_owned_lower_hex(value).ok_or_else(|| failed("decoded digest copy invariant"))
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum DecodedObserveInput<'a> {
    Observe { point: &'a str },
}

#[derive(Serialize)]
struct DecodedObserveTrace<'a> {
    index: usize,
    input: DecodedObserveInput<'a>,
    before_data: &'a Digest,
    after_data: &'a Digest,
    before_session: &'a Digest,
    after_session: &'a Digest,
    outputs: &'a [Digest],
    outcome: StepOutcome,
    diagnostic: Option<&'a str>,
}

#[derive(Serialize)]
struct DecodedRunBindingWire<'a> {
    source: &'a SourceBinding,
    artifact: &'a ArtifactRef,
    data_digest: &'a Digest,
    input_digest: &'a Digest,
    scenario_digest: &'a Digest,
    decision_digest: &'a Digest,
    session_digest: &'a Digest,
    runtime_version: &'static str,
    driver_version: &'static str,
}

#[derive(Serialize)]
struct DecodedRunEvidenceWire<'a> {
    version: u32,
    id: &'a str,
    origin: ExecutionOrigin,
    binding: DecodedRunBindingWire<'a>,
    source_after: &'a SourceBinding,
    state: EvidenceState,
    trace: &'a [DecodedObserveTrace<'a>],
    observations: &'a [view::DecodedRuntimeObservation],
    limits: &'a RuntimeLimits,
    errors: &'a [String],
    uncovered: &'a [String],
}
impl crate::product_backup::fallible_codec::BackupWire for DecodedRunEvidenceWire<'_> {}

#[derive(Serialize)]
struct DecodedObserveRequest<'a>(DecodedObserveInput<'a>, &'a ArtifactRef);
impl crate::product_backup::fallible_codec::BackupWire for DecodedObserveRequest<'_> {}
#[derive(Serialize)]
struct DecodedApplyRequest<'a>(&'a SemanticInput, &'a ArtifactRef);
impl crate::product_backup::fallible_codec::BackupWire for DecodedApplyRequest<'_> {}
impl ProductRun {
    pub fn trace(&self) -> &[TraceStep] {
        &self.trace
    }
    pub fn observations(&self) -> &[Observation] {
        &self.observations
    }
    pub fn artifacts(&self) -> &[LocalArtifact] {
        &self.state.outputs
    }
    pub fn clock_day(&self) -> i32 {
        self.state.day
    }
}

impl LocalRuntime {
    pub(crate) fn start_decoded<'a>(
        &self,
        program: &'a FallibleDecodedCapturedProgram,
        data: &FallibleDecodedDataSnapshot,
        session: &FallibleDecodedSessionState,
        clock_day: i32,
        random_seed: u64,
        limits: RuntimeLimits,
        allocations: &mut product_alloc::AllocationContext,
    ) -> Result<DecodedProductRun<'a>> {
        program.validate_with(allocations).map_err(app_adapter_error)?;
        data.validate_with(allocations).map_err(app_adapter_error)?;
        program.validate_session_with(session, allocations).map_err(app_adapter_error)?;
        validate_day(clock_day).map_err(AdapterError::Invalid)?;
        limits.validate().map_err(AdapterError::Invalid)?;
        if data.project_id() != program.project_id() {
            return Err(invalid("program and data belong to different projects"));
        }
        let (compatibility, merged) = self.decoded_compatibility_with(program, data, clock_day, allocations)?;
        if compatibility != CompatibilityState::Compatible {
            return Err(AdapterError::Unsupported(format!("decoded data is not compatible with the current app: {compatibility:?}")));
        }
        let mut initial = Meter::with_allocations(
            limits.clone(), limits.fuel, self.cancelled.clone(),
            std::mem::replace(allocations, product_alloc::AllocationContext::new()),
        );
        let constraints = execute::constraints_decoded(program, &merged, session, clock_day, &mut initial);
        *allocations = initial.allocations;
        constraints?;
        let data = data.try_clone_with(allocations).map_err(|error| match error {
            FallibleDataSnapshotCloneError::Allocation(failure) => AdapterError::Allocation(failure),
            FallibleDataSnapshotCloneError::Invariant => failed("decoded initial data copy invariant"),
        })?;
        let session = session.try_clone_with(allocations).map_err(app_adapter_error)?;
        Ok(DecodedProductRun {
            program,
            data,
            session,
            day: clock_day,
            initial_day: clock_day,
            seed: random_seed,
            fuel: limits.fuel,
            limits,
            trace: Vec::new(),
            observations: Vec::new(),
            outputs: Vec::new(),
            receipts: Vec::new(),
        })
    }

    /// Apply semantic inputs while keeping persisted data and session in their
    /// fallible decoded owners for the full transaction.
    pub(crate) fn apply_decoded(
        &self,
        run: &mut DecodedProductRun<'_>,
        input: &SemanticInput,
        operation_id: &str,
        allocations: &mut product_alloc::AllocationContext,
    ) -> Result<DecodedTraceStep> {
        if !valid_id(operation_id) { return Err(invalid("invalid operation ID")); }
        validate_input_shape(input)?;
        let request = crate::product_backup::fallible_codec::canonical_digest(
            IdentityDomain::Input,
            &DecodedApplyRequest(input, run.program.artifact()),
            allocations,
        ).map_err(codec_adapter_error)?;
        let store_request = crate::product_backup::fallible_codec::canonical_digest(
            IdentityDomain::Input,
            input,
            allocations,
        ).map_err(codec_adapter_error)?;
        if let Some(receipt) = run.receipts.iter().find(|receipt| receipt.operation_id == operation_id) {
            return if run.trace[receipt.step].input_digest == request {
                run.trace[receipt.step].try_clone_with(allocations)
            } else {
                Err(AdapterError::Stale("Operation ID belongs to another input".into()))
            };
        }
        if run.data.events().iter().any(|event| event.operation_id() == operation_id) {
            return Err(AdapterError::Stale(
                "Operation already committed; resolve its persisted receipt before retrying".into(),
            ));
        }
        if run.trace.len() >= run.limits.action_steps {
            return Err(exhausted("input action limit"));
        }

        let before_data = run.data.identity_with(allocations).map_err(app_adapter_error)?;
        let before_session = run.session.identity_with(allocations).map_err(codec_adapter_error)?;
        if let SemanticInput::Observe { point } = input {
            if run.observations.iter().any(|observation| observation.point() == point) {
                return Err(invalid("observation point already used"));
            }
        }
        let app = AppRef::from(run.program.app());
        let action = execute::input_action_decoded(input, app);
        let mut next_data = run.data.try_clone_with(allocations).map_err(|error| match error {
            FallibleDataSnapshotCloneError::Allocation(failure) => AdapterError::Allocation(failure),
            FallibleDataSnapshotCloneError::Invariant => failed("decoded input snapshot copy invariant"),
        })?;
        let mut next_session = run.session.try_clone_with(allocations).map_err(app_adapter_error)?;
        let mut next_day = run.day;
        let mut next_observation = None;
        let owned_allocations = std::mem::replace(allocations, product_alloc::AllocationContext::new());
        let mut meter = Meter::with_allocations(run.limits.clone(), run.fuel, self.cancelled.clone(), owned_allocations);
        meter.decoded_output_base_count = run.outputs.len();
        meter.decoded_output_base_bytes = run.outputs.iter()
            .map(FallibleDecodedLocalArtifact::byte_len).fold(0usize, usize::saturating_add);
        let result = (|| {
            meter.tick(1)?;
            match input {
                SemanticInput::Observe { point } => {
                    if !valid_id(point) { return Err(invalid("invalid observation point")); }
                    next_observation = Some(view::observe_decoded_with_meter(
                        run.program, &next_data, &next_session, run.day, point, &run.outputs, &mut meter,
                    )?);
                }
                SemanticInput::Invoke { .. }
                | SemanticInput::Control { .. }
                | SemanticInput::Activate { .. }
                | SemanticInput::Submit { .. }
                | SemanticInput::Navigate { .. }
                | SemanticInput::AdvanceClock { .. } => execute::input_decoded(
                    run.program, &mut next_data, &mut next_session, &mut next_day,
                    input, operation_id, run.seed, &mut meter,
                )?,
            }
            meter.tick(1)?;
            Ok(())
        })();
        *allocations = meter.allocations;
        if matches!(&result, Err(AdapterError::Allocation(_))) { return result.map(|_| unreachable!()); }
        let emitted = std::mem::take(&mut meter.decoded_outputs);
        let output_digests = if result.is_ok() {
            execute::decoded_output_digests(&emitted, allocations)?
        } else { Vec::new() };

        let (outcome, diagnostic) = match &result {
            Ok(()) => (StepOutcome::Applied, None),
            Err(AdapterError::Invalid(_)) => (StepOutcome::Rejected, Some(diagnostic_decoded(result.as_ref().unwrap_err(), allocations)?)),
            Err(AdapterError::BudgetExhausted(_)) => (StepOutcome::BudgetExhausted, Some(diagnostic_decoded(result.as_ref().unwrap_err(), allocations)?)),
            Err(error) => (StepOutcome::Failed, Some(diagnostic_decoded(error, allocations)?)),
        };
        let after_data = if result.is_ok() { next_data.identity_with(allocations).map_err(app_adapter_error)? } else { try_clone_digest(&before_data, allocations)? };
        let after_session = if result.is_ok() { next_session.identity_with(allocations).map_err(codec_adapter_error)? } else { try_clone_digest(&before_session, allocations)? };
        let step_action = action.map(|action| product_alloc::try_copy_str(action, allocations, product_alloc::AllocationSite::String).map_err(AdapterError::Allocation)).transpose()?;
        let step = DecodedTraceStep {
            index: run.trace.len(),
            operation_id: product_alloc::try_copy_str(operation_id, allocations, product_alloc::AllocationSite::String).map_err(AdapterError::Allocation)?,
            input_digest: request,
            store_request_digest: store_request,
            action: step_action,
            before_data,
            after_data,
            before_session,
            after_session,
            outputs: output_digests,
            outcome,
            diagnostic,
        };
        product_alloc::try_reserve(&mut run.trace, 1, allocations, product_alloc::AllocationSite::Vector).map_err(AdapterError::Allocation)?;
        let receipt_operation = if result.is_ok() {
            product_alloc::try_reserve(&mut run.receipts, 1, allocations, product_alloc::AllocationSite::Vector).map_err(AdapterError::Allocation)?;
            product_alloc::try_copy_str(operation_id, allocations, product_alloc::AllocationSite::String).map_err(AdapterError::Allocation)?
        } else { String::new() };
        let returned_step = if result.is_ok() { Some(step.try_clone_with(allocations)?) } else { None };
        if result.is_ok() && next_observation.is_some() {
            product_alloc::try_reserve(&mut run.observations, 1, allocations, product_alloc::AllocationSite::Vector).map_err(AdapterError::Allocation)?;
        }
        if result.is_ok() {
            product_alloc::try_reserve(&mut run.outputs, emitted.len(), allocations, product_alloc::AllocationSite::Vector).map_err(AdapterError::Allocation)?;
        }
        run.fuel = meter.fuel;
        if result.is_ok() {
            run.data = next_data;
            run.session = next_session;
            run.day = next_day;
            if let Some(observation) = next_observation {
                run.observations.push(observation);
            }
            run.outputs.extend(emitted);
            run.receipts.push(DecodedOperationReceipt { operation_id: receipt_operation, step: step.index });
        }
        run.trace.push(step);
        result?;
        Ok(returned_step.expect("successful decoded apply prepared its result"))
    }
}
struct Meter {
    limits: RuntimeLimits,
    fuel: u64,
    steps: usize,
    writes: usize,
    started: Instant,
    cancelled: Arc<AtomicBool>,
    allocations: product_alloc::AllocationContext,
    row: Option<capture::RowStop>,
    decoded_outputs: Vec<FallibleDecodedLocalArtifact>,
    decoded_output_base_count: usize,
    decoded_output_base_bytes: usize,
}
impl Meter {
    fn new(limits: RuntimeLimits, fuel: u64, cancelled: Arc<AtomicBool>) -> Self {
        Self::with_allocations(limits, fuel, cancelled, product_alloc::AllocationContext::new())
    }
    fn with_allocations(
        limits: RuntimeLimits,
        fuel: u64,
        cancelled: Arc<AtomicBool>,
        allocations: product_alloc::AllocationContext,
    ) -> Self {
        Self {
            limits,
            fuel,
            steps: 0,
            writes: 0,
            started: Instant::now(),
            cancelled,
            allocations,
            row: None,
            decoded_outputs: Vec::new(),
            decoded_output_base_count: 0,
            decoded_output_base_bytes: 0,
        }
    }
    fn tick(&mut self, cost: u64) -> Result<()> {
        if self.row.is_some() {
            return self.row_charge(cost).map_err(capture_cost::resource);
        }
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(AdapterError::Cancelled);
        }
        if self.started.elapsed().as_millis() >= u128::from(self.limits.elapsed_millis) {
            return Err(exhausted("elapsed time limit"));
        }
        if cost > self.fuel {
            self.fuel = 0;
            return Err(exhausted("evaluation fuel exhausted"));
        }
        self.fuel -= cost;
        Ok(())
    }
    fn collection(&mut self, n: usize) -> Result<()> {
        self.tick(1)?;
        if n > self.limits.collection_items {
            Err(exhausted("collection item limit"))
        } else {
            Ok(())
        }
    }
    fn step(&mut self) -> Result<()> {
        self.tick(1)?;
        self.steps += 1;
        if self.steps > self.limits.action_steps {
            Err(exhausted("statement limit"))
        } else {
            Ok(())
        }
    }
    fn write(&mut self) -> Result<()> {
        self.tick(1)?;
        self.writes += 1;
        if self.writes > self.limits.transaction_writes {
            Err(exhausted("transaction write limit"))
        } else {
            Ok(())
        }
    }
    fn value(&mut self, value: &DataValue) -> Result<()> {
        if self.row.is_some() { return self.row_value(value); }
        self.tick(1)?;
        match value {
            DataValue::List { items, .. } => {
                self.collection(items.len())?;
                for value in items {
                    self.value(value)?;
                }
            }
            DataValue::Text { value } => {
                self.tick(value.len() as u64)?;
                if value.len() > MAX_TEXT_BYTES {
                    return Err(exhausted("text byte limit"));
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn into_allocations(self) -> product_alloc::AllocationContext { self.allocations }
}
impl LocalRuntime {
    pub fn with_cancellation(cancelled: Arc<AtomicBool>) -> Self {
        Self {
            cancelled,
            admission: None,
        }
    }
    pub fn with_admission(mut self, admission: Arc<dyn ReplayAdmission>) -> Self {
        self.admission = Some(admission);
        self
    }
    #[cfg(test)]
    pub(crate) fn observe_with_allocations(
        &self,
        run: &ProductRun,
        point: &str,
        allocations: product_alloc::AllocationContext,
    ) -> (Result<Observation>, usize) {
        let mut meter = Meter::with_allocations(
            run.limits.clone(),
            run.fuel.get(),
            self.cancelled.clone(),
            allocations,
        );
        let result = view::observe(&run.program, &run.state, point, &mut meter);
        let requests = meter.allocations.growth_requests();
        run.fuel.set(meter.fuel);
        (result, requests)
    }
    /// Host-only bounded evaluation for metadata initialization. This uses the
    /// production evaluator, validates the expression in a typed action, and
    /// never creates a transaction or modifies a business record.
    pub(crate) fn evaluate_record_projection(
        &self,
        program: &CapturedProgram,
        data: &DataSnapshot,
        row: &RecordRef,
        binding: &str,
        expression: &Expr,
        expected: &Type,
        day: i32,
    ) -> Result<DataValue> {
        self.evaluate_record_projection_with(
            &mut ProjectionValidation::default(),
            RuntimeLimits::default(),
            program,
            data,
            row,
            binding,
            expression,
            expected,
            day,
        )
    }
    pub(crate) fn evaluate_record_projection_with<'a>(
        &self,
        checks: &mut ProjectionValidation<'a>,
        limits: RuntimeLimits,
        program: &'a CapturedProgram,
        data: &DataSnapshot,
        row: &RecordRef,
        binding: &str,
        expression: &Expr,
        expected: &Type,
        day: i32,
    ) -> Result<DataValue> {
        let source = checks.source(program)?;
        data.validate()?;
        self.evaluate_projection_inner(
            program, data, row, binding, expression, expected, day, limits, checks, source,
        )
    }
    /// Evaluate a scalar proof expression directly against bounded persisted
    /// owners. The legacy and decoded paths share `Eval`; no map conversion or
    /// cloned compatibility model is introduced.
    pub(crate) fn evaluate_decoded_record_projection_with(
        &self,
        program: &FallibleDecodedCapturedProgram,
        data: &FallibleDecodedDataSnapshot,
        row: &RecordRef,
        binding: &str,
        expression: &Expr,
        expected: &Type,
        day: i32,
        limits: RuntimeLimits,
        allocations: product_alloc::AllocationContext,
    ) -> Result<DataValue> {
        self.evaluate_decoded_record_projection_and_allocations(
            program, data, row, binding, expression, expected, day, limits, allocations,
        ).map(|(value, _)| value).map_err(|(error, _)| error)
    }
    /// Variant for callers that perform several projections in one fallible
    /// operation. Returning the charged context keeps all projection work on
    /// the caller's single refusal counter rather than resetting it per row.
    pub(crate) fn evaluate_decoded_record_projection_and_allocations(
        &self,
        program: &FallibleDecodedCapturedProgram,
        data: &FallibleDecodedDataSnapshot,
        row: &RecordRef,
        binding: &str,
        expression: &Expr,
        expected: &Type,
        day: i32,
        limits: RuntimeLimits,
        mut allocations: product_alloc::AllocationContext,
    ) -> std::result::Result<
        (DataValue, product_alloc::AllocationContext),
        (AdapterError, product_alloc::AllocationContext),
    > {
        fn app_error(error: AppValidationError) -> AdapterError {
            match error {
                AppValidationError::Contract(error) => AdapterError::Invalid(error),
                AppValidationError::Allocation(failure) => AdapterError::Allocation(failure),
            }
        }
        // Keep the caller's complete allocation accounting available on every
        // error path, including preflight failures before the evaluator meter
        // is constructed.
        let prepared = (|| -> Result<(FallibleDecodedSessionState, ManagedEnv)> {
            validate_day(day)?;
            limits.validate()?;
            program.validate_with(&mut allocations).map_err(app_error)?;
            data.validate_with(&mut allocations).map_err(app_error)?;
            if program.project_id() != data.project_id() || !valid_id(binding) {
                return Err(invalid("projection project or binding mismatch"));
            }
            let view = DataSnapshotView::from(data);
            if view.record(&row.entity, &row.record).is_none() {
                return Err(invalid("record reference does not exist"));
            }
            let actual = crate::product_contract::decoded_expression_type_with(
                program.app(), expression, binding, &row.entity, &mut allocations,
            ).map_err(app_error)?;
            if actual != *expected { return Err(invalid("projection result type mismatch")); }
            let session = program.initial_session_with(&mut allocations).map_err(AdapterError::Allocation)?;
            program.validate_session_with(&session, &mut allocations).map_err(app_error)?;
            let value = DataValue::Reference {
                entity: product_alloc::try_copy_str(&row.entity, &mut allocations, product_alloc::AllocationSite::String).map_err(AdapterError::Allocation)?,
                record: product_alloc::try_copy_str(&row.record, &mut allocations, product_alloc::AllocationSite::String).map_err(AdapterError::Allocation)?,
            };
            let mut managed = ManagedEnv::new();
            let env_entity = product_alloc::try_copy_str(&row.entity, &mut allocations, product_alloc::AllocationSite::String)
                .map_err(AdapterError::Allocation)?;
            let typ = Type::Reference { entity: env_entity };
            let binding = product_alloc::try_copy_str(binding, &mut allocations, product_alloc::AllocationSite::String)
                .map_err(AdapterError::Allocation)?;
            managed.try_bind(&binding, &typ, &value, &mut allocations).map_err(AdapterError::Allocation)?;
            Ok((session, managed))
        })();
        let (session, managed) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => return Err((error, allocations)),
        };
        let mut meter = Meter::with_allocations(limits.clone(), limits.fuel, self.cancelled.clone(), allocations);
        let result = {
            let mut evaluator = Eval::fallible(program.app(), data, &session, day, &mut meter);
            evaluator.eval(expression, &managed).and_then(|result| {
                validate_value(&result, expected, 0)?;
                Ok(result)
            })
        };
        let allocations = meter.into_allocations();
        match result {
            Ok(value) => Ok((value, allocations)),
            Err(error) => Err((error, allocations)),
        }
    }
    pub(crate) fn evaluate_decoded_historical_record_projection_with(
        &self,
        program: &FallibleDecodedCapturedProgram,
        data: &FallibleDecodedDataSnapshot,
        row: &RecordRef,
        values: &FallibleValues,
        archived: bool,
        binding: &str,
        expression: &Expr,
        expected: &Type,
        day: i32,
        limits: RuntimeLimits,
        allocations: product_alloc::AllocationContext,
    ) -> Result<DataValue> {
        self.evaluate_decoded_historical_record_projection_and_allocations(
            program, data, row, values, archived, binding, expression, expected, day, limits,
            allocations,
        ).map(|(value, _)| value).map_err(|(error, _)| error)
    }
    pub(crate) fn evaluate_decoded_historical_record_projection_and_allocations(
        &self,
        program: &FallibleDecodedCapturedProgram,
        data: &FallibleDecodedDataSnapshot,
        row: &RecordRef,
        values: &FallibleValues,
        archived: bool,
        binding: &str,
        expression: &Expr,
        expected: &Type,
        day: i32,
        limits: RuntimeLimits,
        mut allocations: product_alloc::AllocationContext,
    ) -> std::result::Result<(DataValue, product_alloc::AllocationContext), (AdapterError, product_alloc::AllocationContext)> {
        fn app_error(error: AppValidationError) -> AdapterError {
            match error {
                AppValidationError::Contract(error) => AdapterError::Invalid(error),
                AppValidationError::Allocation(failure) => AdapterError::Allocation(failure),
            }
        }
        let preflight = (|| -> std::result::Result<(), AdapterError> {
            validate_day(day)?;
            limits.validate()?;
            program.validate_with(&mut allocations).map_err(app_error)?;
            data.validate_with(&mut allocations).map_err(app_error)?;
            if values.len() > MAX_ITEMS { return Err(invalid("historical projection row exceeds bounds")); }
            let base = DataSnapshotView::from(data);
            let storage = base.schema().iter().find(|entity| entity.id == row.entity)
                .ok_or_else(|| invalid("historical projection storage entity missing"))?;
            for (id, value) in values.iter() {
                let field = storage.fields.iter().find(|field| field.id == *id)
                    .ok_or_else(|| invalid("historical projection field missing"))?;
                validate_value(value, &field.value_type, 0)?;
            }
            let producer = AppRef::from(program.app()).entities().iter().find(|entity| entity.id == row.entity)
                .ok_or_else(|| invalid("historical producer entity missing"))?;
            for field in &producer.fields {
                validate_value(values.get(&field.id).unwrap_or(&DataValue::Null), &field.value_type, 0)?;
            }
            if program.project_id() != data.project_id() || !valid_id(binding) {
                return Err(invalid("projection project or binding mismatch"));
            }
            Ok(())
        })();
        if let Err(error) = preflight { return Err((error, allocations)); }
        let base = DataSnapshotView::from(data);
        let mut meter = Meter::with_allocations(limits.clone(), limits.fuel, self.cancelled.clone(), allocations);
        let result = (|| -> Result<DataValue> {
            let mut historical_record = None;
            for candidate in base.records() {
                let comparison_bytes = candidate.entity().len()
                    .saturating_add(row.entity.len())
                    .saturating_add(candidate.id().len())
                    .saturating_add(row.record.len());
                meter.tick(u64::try_from(comparison_bytes).unwrap_or(u64::MAX))?;
                if candidate.entity() == row.entity && candidate.id() == row.record {
                    historical_record = Some(candidate);
                    break;
                }
            }
            let historical_record = historical_record
                .ok_or_else(|| invalid("historical projection record missing"))?;
            let actual = crate::product_contract::decoded_expression_type_with(
                program.app(), expression, binding, &row.entity, &mut meter.allocations,
            ).map_err(app_error)?;
            if actual != *expected { return Err(invalid("projection result type mismatch")); }
            let session = program.initial_session_with(&mut meter.allocations).map_err(AdapterError::Allocation)?;
            program.validate_session_with(&session, &mut meter.allocations).map_err(app_error)?;
            let value = DataValue::Reference {
                entity: product_alloc::try_copy_str(&row.entity, &mut meter.allocations, product_alloc::AllocationSite::String).map_err(AdapterError::Allocation)?,
                record: product_alloc::try_copy_str(&row.record, &mut meter.allocations, product_alloc::AllocationSite::String).map_err(AdapterError::Allocation)?,
            };
            let env_entity = product_alloc::try_copy_str(&row.entity, &mut meter.allocations, product_alloc::AllocationSite::String)
                .map_err(AdapterError::Allocation)?;
            let typ = Type::Reference { entity: env_entity };
            let env_binding = product_alloc::try_copy_str(binding, &mut meter.allocations, product_alloc::AllocationSite::String)
                .map_err(AdapterError::Allocation)?;
            let mut managed = ManagedEnv::new();
            managed.try_bind(&env_binding, &typ, &value, &mut meter.allocations).map_err(AdapterError::Allocation)?;
            let data_view = DataSnapshotView::fallible_historical(data, historical_record, values, archived);
            let mut evaluator = Eval::fallible_view(program.app(), data_view, &session, day, &mut meter);
            let result = evaluator.eval(expression, &managed)?;
            validate_value(&result, expected, 0)?;
            Ok(result)
        })();
        let allocations = meter.into_allocations();
        match result {
            Ok(value) => Ok((value, allocations)),
            Err(error) => Err((error, allocations)),
        }
    }
    /// Evaluate an independently proven own-row historical expression. Validate
    /// the coherent current data and every producer-row value first; global
    /// constraints must not be applied to a mixture of historical/current rows.
    pub(crate) fn evaluate_historical_record_projection(
        &self,
        program: &CapturedProgram,
        data: &DataSnapshot,
        row: &RecordRef,
        values: &Values,
        archived: bool,
        binding: &str,
        expression: &Expr,
        expected: &Type,
        day: i32,
    ) -> Result<DataValue> {
        self.evaluate_historical_record_projection_with(
            &mut ProjectionValidation::default(),
            RuntimeLimits::default(),
            program,
            data,
            row,
            values,
            archived,
            binding,
            expression,
            expected,
            day,
        )
    }
    pub(crate) fn evaluate_historical_record_projection_with<'a>(
        &self,
        checks: &mut ProjectionValidation<'a>,
        limits: RuntimeLimits,
        program: &'a CapturedProgram,
        data: &DataSnapshot,
        row: &RecordRef,
        values: &Values,
        archived: bool,
        binding: &str,
        expression: &Expr,
        expected: &Type,
        day: i32,
    ) -> Result<DataValue> {
        let source = checks.source(program)?;
        data.validate()?;
        if values.len() > MAX_ITEMS {
            return Err(invalid("historical projection row exceeds bounds"));
        }
        let storage = data
            .schema
            .iter()
            .find(|e| e.id == row.entity)
            .ok_or_else(|| invalid("historical projection storage entity missing"))?;
        for (id, value) in values {
            let field = storage
                .fields
                .iter()
                .find(|f| &f.id == id)
                .ok_or_else(|| invalid("historical projection field missing"))?;
            validate_value(value, &field.value_type, 0)?;
        }
        let producer = program
            .program
            .entities
            .iter()
            .find(|e| e.id == row.entity)
            .ok_or_else(|| invalid("historical producer entity missing"))?;
        for field in &producer.fields {
            validate_value(
                values.get(&field.id).unwrap_or(&DataValue::Null),
                &field.value_type,
                0,
            )?;
        }
        let mut historical = data.clone();
        let record = historical
            .records
            .iter_mut()
            .find(|r| r.entity == row.entity && r.id == row.record)
            .ok_or_else(|| invalid("historical projection record missing"))?;
        record.values = values.clone();
        record.archived = archived;
        self.evaluate_projection_inner(
            program,
            &historical,
            row,
            binding,
            expression,
            expected,
            day,
            limits,
            checks,
            source,
        )
    }
    fn evaluate_projection_inner(
        &self,
        program: &CapturedProgram,
        data: &DataSnapshot,
        row: &RecordRef,
        binding: &str,
        expression: &Expr,
        expected: &Type,
        day: i32,
        limits: RuntimeLimits,
        checks: &mut ProjectionValidation<'_>,
        source: Option<usize>,
    ) -> Result<DataValue> {
        validate_day(day)?;
        limits.validate()?;
        if data.project_id != program.binding.project_id || !valid_id(binding) {
            return Err(invalid("projection project or binding mismatch"));
        }
        let value = DataValue::Reference {
            entity: row.entity.clone(),
            record: row.record.clone(),
        };
        eval::record(data, &value)?;
        let key = source
            .map(|source| {
                canonical_bytes(&(source, &row.entity, binding, expression, expected, &limits))
            })
            .transpose()?;
        let cached = key.as_ref().is_some_and(|key| checks.types.contains(key));
        if !cached {
            #[cfg(test)]
            {
                checks.type_checks += 1;
            }
            let mut app = program.program.clone();
            let mut id = "gm_scope_projection".to_owned();
            while app.actions.iter().any(|a| a.id == id) {
                id.push('_');
            }
            app.actions.push(ActionDefinition {
                id,
                label: "Host projection type check".into(),
                parameters: BTreeMap::from([(binding.to_owned(), Type::reference(&row.entity))]),
                guards: vec![],
                ensures: vec![],
                steps: vec![Statement::Assert {
                    condition: Expr::Equal {
                        left: Box::new(expression.clone()),
                        right: Box::new(expression.clone()),
                    },
                    message: "Projection type check".into(),
                }],
            });
            app.validate()?;
        }
        let session = SessionState::initial(&program.program)?;
        let mut meter = Meter::new(limits.clone(), limits.fuel, self.cancelled.clone());
        let env = Env::from([(binding.to_owned(), (Type::reference(&row.entity), value))]);
        let mut evaluator = Eval::legacy(&program.program, data, &session, day, &mut meter);
        if evaluator.typ(expression, &env)? != *expected {
            return Err(invalid("projection result type mismatch"));
        }
        let result = evaluator.eval(expression, &env)?;
        validate_value(&result, expected, 0)?;
        if !cached {
            if let Some(key) = key {
                // Saturation only disables reuse. It never changes which valid
                // projection can execute or relaxes the original AST budgets.
                if checks.types.len() < MAX_ITEMS && key.len() <= MAX_WIRE_BYTES - checks.bytes {
                    checks.bytes += key.len();
                    checks.types.insert(key);
                }
            }
        }
        Ok(result)
    }
    pub fn compatibility_at(
        &self,
        program: &CapturedProgram,
        current: &DataSnapshot,
        day: i32,
    ) -> Result<CompatibilityReport> {
        validate_day(day)?;
        compatibility::check(&ProgramSource::ordinary(program), current, Some(day))
    }
    fn decoded_compatibility_with(
        &self,
        program: &FallibleDecodedCapturedProgram,
        current: &FallibleDecodedDataSnapshot,
        day: i32,
        allocations: &mut product_alloc::AllocationContext,
    ) -> Result<(CompatibilityState, FallibleDecodedDataSnapshot)> {
        fn app_error(error: AppValidationError) -> AdapterError {
            match error {
                AppValidationError::Contract(error) => AdapterError::Invalid(error),
                AppValidationError::Allocation(failure) => AdapterError::Allocation(failure),
            }
        }
        validate_day(day)?;
        program.validate_with(allocations).map_err(app_error)?;
        current.validate_with(allocations).map_err(app_error)?;
        if program.project_id() != current.project_id() { return Err(invalid("compatibility project mismatch")); }

        let app = AppRef::from(program.app());
        let mut state = CompatibilityState::Compatible;
        for stored in current.schema() {
            if let Some(target) = app.entities().iter().find(|entity| entity.id == stored.id) {
                for field in &stored.fields {
                    if !target.fields.iter().any(|candidate| candidate.id == field.id)
                        && !matches!(field.value_type, Type::Optional { .. })
                    { state = CompatibilityState::Incompatible; }
                }
            } else if DataSnapshotView::from(current).records().any(|record| record.entity() == stored.id) {
                state = CompatibilityState::RetainedButUnavailable;
            }
        }
        let merged = match current.merge_app_with(program.app(), allocations) {
            Ok(merged) => merged,
            Err(AppValidationError::Allocation(failure)) => return Err(AdapterError::Allocation(failure)),
            Err(AppValidationError::Contract(_)) => return Ok((CompatibilityState::Incompatible, current.try_clone_with(allocations).map_err(|error| match error {
                FallibleDataSnapshotCloneError::Allocation(failure) => AdapterError::Allocation(failure),
                FallibleDataSnapshotCloneError::Invariant => failed("decoded compatibility data copy invariant"),
            })?)),
        };
        let session = program.initial_session_with(allocations).map_err(AdapterError::Allocation)?;
        let limits = RuntimeLimits { fuel: 10_000_000, ..RuntimeLimits::default() };
        let owned_allocations = std::mem::replace(allocations, product_alloc::AllocationContext::new());
        let mut meter = Meter::with_allocations(limits.clone(), limits.fuel, Arc::new(AtomicBool::new(false)), owned_allocations);
        match execute::constraints_decoded(program, &merged, &session, day, &mut meter) {
            Ok(()) => { *allocations = meter.allocations; Ok((state, merged)) },
            Err(AdapterError::Allocation(failure)) => { *allocations = meter.allocations; Err(AdapterError::Allocation(failure)) },
            Err(_) => { *allocations = meter.allocations; Ok((CompatibilityState::Incompatible, merged)) },
        }
    }
    pub(crate) fn compatibility_decoded_state_at(
        &self,
        program: &FallibleDecodedCapturedProgram,
        current: &FallibleDecodedDataSnapshot,
        day: i32,
        mut allocations: product_alloc::AllocationContext,
    ) -> Result<CompatibilityState> {
        self.decoded_compatibility_with(program, current, day, &mut allocations)
            .map(|(state, _)| state)
    }
    /// Resume daily work with its actual committed artifact receipts. Scenario
    /// replay intentionally starts with no new emissions; it uses `start`.
    pub fn resume(
        &self,
        program: &CapturedProgram,
        data: &DataSnapshot,
        session: &SessionState,
        clock_day: i32,
        random_seed: u64,
        limits: RuntimeLimits,
        artifacts: &[LocalArtifact],
    ) -> Result<ProductRun> {
        if artifacts.len() > MAX_ITEMS
            || artifacts.iter().map(|a| a.bytes.len()).sum::<usize>() > limits.output_bytes
        {
            return Err(exhausted("retained output limit"));
        }
        for artifact in artifacts {
            artifact.validate()?;
            if artifact.rows.len() > limits.collection_items {
                return Err(exhausted("retained output row limit"));
            }
            if !data
                .events
                .iter()
                .any(|e| e.outputs.contains(&artifact.digest))
            {
                return Err(invalid("artifact has no committed business-event receipt"));
            }
        }
        let mut run = self.start(program, data, session, clock_day, random_seed, limits)?;
        run.state.outputs = artifacts.to_vec();
        Ok(run)
    }
    #[cfg(test)]
    pub(crate) fn test_fresh_output_participants<'a>(
        &self,
        run: &'a ProductRun,
    ) -> &'a [EmittedRecordParticipants] {
        &run.state.output_participants
    }
    pub fn view_model(&self, run: &ProductRun) -> Result<RuntimeView> {
        let observation = self.observe(run, "view")?;
        Ok(RuntimeView {
            program: run.program.program.clone(),
            observation: observation.view,
            artifacts: observation.outputs,
            retained_records: run.state.data.records.clone(),
            history: crate::product_protocol::PreservedHistory {
                results: vec![],
                events: run.state.data.events.clone(),
            },
            read_only: false,
            issues: vec![],
        })
    }
    /// Executes the declared input prefix and keeps failures and exhausted runs
    /// inconclusive/failed. It never fills missing points with predicted values.
    pub fn replay(
        &self,
        program: &CapturedProgram,
        scenario: &ScenarioSpec,
        decisions: &DecisionGraph,
        limits: RuntimeLimits,
        id: &str,
    ) -> Result<RunEvidence> {
        self.replay_admitted(
            program,
            scenario,
            decisions,
            limits,
            id,
            self.admission.as_deref(),
        )
    }
    pub fn replay_admitted(
        &self,
        program: &CapturedProgram,
        scenario: &ScenarioSpec,
        decisions: &DecisionGraph,
        limits: RuntimeLimits,
        id: &str,
        admission: Option<&dyn ReplayAdmission>,
    ) -> Result<RunEvidence> {
        self.replay_source(
            &ProgramSource::ordinary(program),
            scenario,
            decisions,
            limits,
            id,
            admission,
        )
    }
    #[cfg(test)]
    pub(crate) fn replay_prepared(
        &self,
        program: &PreparedProgram,
        scenario: &ScenarioSpec,
        decisions: &DecisionGraph,
        limits: RuntimeLimits,
        id: &str,
        admission: Option<&dyn ReplayAdmission>,
    ) -> Result<RunEvidence> {
        self.replay_source(
            &ProgramSource(SourceKind::Prepared(program.clone())),
            scenario,
            decisions,
            limits,
            id,
            admission,
        )
    }
    pub(crate) fn replay_source(
        &self,
        program: &ProgramSource,
        scenario: &ScenarioSpec,
        decisions: &DecisionGraph,
        limits: RuntimeLimits,
        id: &str,
        admission: Option<&dyn ReplayAdmission>,
    ) -> Result<RunEvidence> {
        let per_run = admission
            .map(|context| {
                context.begin_replay(program, scenario, decisions, self.cancelled.clone())
            })
            .transpose()?
            .flatten();
        let admission = per_run.as_deref().or(admission);
        if has_protected_fields(program) && admission.is_none() {
            return Err(AdapterError::Unsupported(
                "Protected scope history needs a verified host admission context".into(),
            ));
        }
        if let Some(admission) = admission {
            admission.validate_seed(program, &scenario.seed, scenario.clock_day)?;
        }
        if !valid_id(id) {
            return Err(invalid("invalid run ID"));
        }
        program.validate_scenario(scenario)?;
        decisions.validate()?;
        let binding = RunBinding {
            source: program.binding.clone(),
            artifact: program.artifact.clone(),
            data_digest: scenario.seed.identity()?,
            input_digest: scenario.input_identity()?,
            scenario_digest: scenario.identity()?,
            decision_digest: decisions.identity()?,
            session_digest: scenario.session.identity()?,
            runtime_version: RUNTIME_VERSION.into(),
            driver_version: DRIVER_VERSION.into(),
        };
        let mut run = self.start_source(
            program,
            &scenario.seed,
            &scenario.session,
            scenario.clock_day,
            scenario.random_seed,
            limits.clone(),
        )?;
        let mut state = EvidenceState::Observed;
        let mut errors = Vec::new();
        let mut uncovered = Vec::new();
        // The shared replay driver excludes observation instrumentation from
        // mutation identity. Live apply/store IDs are supplied by their caller.
        let operations = if let Some(admission) = admission {
            admission.replay_operation_ids(program, scenario)?
        } else {
            scenario.replay_operation_ids()?
        };
        for (input, operation) in scenario.inputs.iter().zip(operations) {
            let applied = self.apply(&mut run, input, &operation).and_then(|step| {
                if let Some(admission) = admission {
                    admission.validate_state(program, self.data(&run), run.clock_day())?;
                }
                Ok(step)
            });
            if let Err(error) = applied {
                state = if matches!(
                    error,
                    AdapterError::Cancelled | AdapterError::BudgetExhausted(_)
                ) {
                    EvidenceState::Inconclusive
                } else {
                    EvidenceState::Failed
                };
                errors.push(diagnostic(&error));
                break;
            }
        }
        if state == EvidenceState::Observed {
            if run.observations.is_empty() {
                state = EvidenceState::Inconclusive;
                uncovered.push("No observation point was requested".into());
            }
            for property in &scenario.validity {
                match property.evaluate(&run.observations) {
                    Some(true) => {}
                    Some(false) => {
                        state = EvidenceState::Failed;
                        errors.push(format!("Workflow validity failed: {}", property.id));
                    }
                    None => {
                        state = EvidenceState::Inconclusive;
                        uncovered.push(format!("Workflow validity is unverified: {}", property.id));
                    }
                }
            }
        }
        let result = RunEvidence {
            version: CONTRACT_VERSION,
            id: id.into(),
            origin: ExecutionOrigin::ProductionRuntime,
            binding,
            source_after: program.binding.clone(),
            state,
            trace: run.trace,
            observations: run.observations,
            limits,
            errors,
            uncovered,
        };
        result.validate()?;
        Ok(result)
    }

    /// Replay the lifecycle runtime's strict one-Observe scenario directly over
    /// fallible decoded owners. The returned RunEvidence DTO can only be minted
    /// here after the shared-meter observation and the single-step evidence
    /// invariants have both been checked.
    pub(crate) fn replay_decoded_observe_with(
        &self,
        program: &FallibleDecodedCapturedProgram,
        scenario: &FallibleDecodedScenarioSpec,
        decisions: &FallibleDecodedDecisionGraph,
        limits: RuntimeLimits,
        id: &str,
        allocations: &mut product_alloc::AllocationContext,
    ) -> Result<FallibleDecodedRunEvidence> {
        if program.has_protected_fields() {
            return Err(AdapterError::Unsupported(
                "Protected scope history needs a verified host admission context".into(),
            ));
        }
        if !valid_id(id) { return Err(invalid("invalid run ID")); }
        scenario.validate_with(program, allocations).map_err(|error| match error {
            AppValidationError::Contract(error) => AdapterError::Invalid(error),
            AppValidationError::Allocation(failure) => AdapterError::Allocation(failure),
        })?;
        decisions.validate_with(allocations).map_err(|error| match error {
            DecisionValidationError::Contract(error) => AdapterError::Invalid(error),
            DecisionValidationError::Allocation(failure) => AdapterError::Allocation(failure),
        })?;
        let Some(point) = scenario.single_observe_point() else {
            return Err(invalid("decoded replay requires exactly one Observe input and no validity properties"));
        };
        if scenario.has_validity() {
            return Err(invalid("decoded replay requires exactly one Observe input and no validity properties"));
        }

        // Keep the legacy binding construction order: seed, input, scenario,
        // decisions, and session identities are computed before runtime start.
        let data_digest = scenario.seed_identity_with(allocations).map_err(codec_adapter_error)?;
        let input_digest = scenario.input_identity_with(allocations).map_err(codec_adapter_error)?;
        let scenario_digest = scenario.identity_with(allocations).map_err(codec_adapter_error)?;
        let decision_digest = decisions.identity_with(allocations).map_err(|error| match error {
            DecisionValidationError::Contract(error) => AdapterError::Invalid(error),
            DecisionValidationError::Allocation(failure) => AdapterError::Allocation(failure),
        })?;
        let session_digest = scenario.session_identity_with(allocations).map_err(codec_adapter_error)?;

        // This is the decoded counterpart of start_source's complete preflight
        // and initial compatibility/resource checks. It does not materialize
        // or mutate a ProductRun copy.
        program.validate_with(allocations).map_err(app_adapter_error)?;
        scenario.validate_seed_with(allocations).map_err(app_adapter_error)?;
        scenario.validate_session_with(program).map_err(app_adapter_error)?;
        validate_day(scenario.clock_day()).map_err(AdapterError::Invalid)?;
        limits.validate().map_err(AdapterError::Invalid)?;
        let start_data = scenario.seed_with(allocations).map_err(app_adapter_error)?;
        let start_session = scenario.session_with(allocations).map_err(app_adapter_error)?;
        crate::product_store::scope::validate_initial_runtime_fallible(
            program, &start_data, &start_session,
            scenario.clock_day(), limits.clone(), allocations,
        ).map_err(|error| match error {
            crate::product_store::scope::CompatibilityFailure::Allocation(failure) => AdapterError::Allocation(failure),
            crate::product_store::scope::CompatibilityFailure::Store(crate::product_store::StoreError::Runtime(error)) => error,
            crate::product_store::scope::CompatibilityFailure::Store(crate::product_store::StoreError::Invalid(message)) => {
                AdapterError::Invalid(ContractError(message))
            }
            crate::product_store::scope::CompatibilityFailure::Store(error) => {
                AdapterError::Failed(format!("decoded runtime start failed: {error:?}"))
            }
        })?;

        // ScenarioSpec::replay_operation_ids performs this after start. The
        // single Observe path has a stable index-zero replay operation ID.
        let operation_id = scenario.observe_operation_id_with(allocations).map_err(app_adapter_error)?;
        if !valid_id(&operation_id) { return Err(invalid("invalid operation ID")); }
        let observe_input = DecodedObserveInput::Observe { point };
        // apply computes this request identity before checking persisted event
        // collisions, even though Observe has no mutation receipt of its own.
        crate::product_backup::fallible_codec::canonical_digest(
            IdentityDomain::Input,
            &DecodedObserveRequest(observe_input, program.artifact()),
            allocations,
        ).map_err(codec_adapter_error)?;

        let apply_error = if start_data.events().iter()
            .any(|event| event.operation_id() == operation_id)
        {
            Some(AdapterError::Stale(
                "Operation already committed; resolve its persisted receipt before retrying".into(),
            ))
        } else {
            None
        };
        let had_early_apply_error = apply_error.is_some();
        let before_data = if had_early_apply_error { None } else {
            Some(start_data.identity_with(allocations).map_err(app_adapter_error)?)
        };
        let before_session = if had_early_apply_error { None } else {
            Some(start_session.identity_with(allocations).map_err(codec_adapter_error)?)
        };
        let before_data = before_data.as_ref().unwrap_or(&data_digest);
        let before_session = before_session.as_ref().unwrap_or(&session_digest);
        let mut observation = None;
        let mut step_error = apply_error;
        if step_error.is_none() {
            // Prefix tick, input-shape/input validation, view observation, and
            // suffix tick use one meter and one allocation context, matching
            // LocalRuntime::apply's ordering.
            let owned_allocations = std::mem::replace(allocations, product_alloc::AllocationContext::new());
            let mut meter = Meter::with_allocations(
                limits.clone(), limits.fuel, self.cancelled.clone(), owned_allocations,
            );
            let result = (|| {
                meter.tick(1)?;
                if !valid_id(point) { return Err(invalid("invalid observation point")); }
                scenario.validate_replay_input(program).map_err(app_adapter_error)?;
                if scenario.single_observe_point().is_none() {
                    return Err(invalid("invalid observation input"));
                }
                let result = view::observe_decoded_with_meter(
                    program, &start_data, &start_session, scenario.clock_day(), point, &[], &mut meter,
                )?;
                meter.tick(1)?;
                Ok(result)
            })();
            *allocations = meter.allocations;
            match result {
                Ok(value) => observation = Some(value),
                Err(error @ AdapterError::Allocation(_)) => return Err(error),
                Err(error) => step_error = Some(error),
            }
        }

        let (outcome, diagnostic_text, state) = match (&observation, &step_error) {
            (Some(_), None) => (StepOutcome::Applied, None, EvidenceState::Observed),
            (None, Some(error)) => {
                let text = diagnostic_decoded(error, allocations)?;
                let outcome = match error {
                    AdapterError::Invalid(_) => StepOutcome::Rejected,
                    AdapterError::BudgetExhausted(_) => StepOutcome::BudgetExhausted,
                    _ => StepOutcome::Failed,
                };
                let state = if matches!(error, AdapterError::Cancelled | AdapterError::BudgetExhausted(_)) {
                    EvidenceState::Inconclusive
                } else {
                    EvidenceState::Failed
                };
                (outcome, Some(text), state)
            }
            _ => return Err(failed("decoded Observe replay state invariant")),
        };
        let after_data = if had_early_apply_error { None } else {
            Some(start_data.identity_with(allocations).map_err(app_adapter_error)?)
        };
        let after_session = if had_early_apply_error { None } else {
            Some(start_session.identity_with(allocations).map_err(codec_adapter_error)?)
        };
        let after_data = after_data.as_ref().unwrap_or(before_data);
        let after_session = after_session.as_ref().unwrap_or(before_session);
        let output_digests: [Digest; 0] = [];
        let trace_step = DecodedObserveTrace {
            index: 0,
            input: DecodedObserveInput::Observe { point },
            before_data,
            after_data,
            before_session,
            after_session,
            outputs: &output_digests,
            outcome,
            diagnostic: diagnostic_text.as_deref(),
        };
        let trace: &[DecodedObserveTrace<'_>] = if step_error.is_some() && observation.is_none() && had_early_apply_error {
            &[]
        } else {
            std::slice::from_ref(&trace_step)
        };
        let observations: &[view::DecodedRuntimeObservation] = match observation.as_ref() {
            Some(value) => std::slice::from_ref(value),
            None => &[],
        };
        let errors: &[String] = match diagnostic_text.as_ref() {
            Some(value) => std::slice::from_ref(value),
            None => &[],
        };
        let wire = DecodedRunEvidenceWire {
            version: CONTRACT_VERSION,
            id,
            origin: ExecutionOrigin::ProductionRuntime,
            binding: DecodedRunBindingWire {
                source: program.binding(),
                artifact: program.artifact(),
                data_digest: &data_digest,
                input_digest: &input_digest,
                scenario_digest: &scenario_digest,
                decision_digest: &decision_digest,
                session_digest: &session_digest,
                runtime_version: RUNTIME_VERSION,
                driver_version: DRIVER_VERSION,
            },
            source_after: program.binding(),
            state,
            trace,
            observations,
            limits: &limits,
            errors,
            uncovered: &[],
        };
        let value = crate::product_backup::fallible_codec::try_serialize_value(&wire, allocations)
            .map_err(codec_adapter_error)?;
        let evidence = decode_run_evidence_runtime(value, allocations).map_err(app_adapter_error)?;
        evidence.validate_single_observe(allocations).map_err(|error| match error {
            RuntimeEvidenceError::Contract(error) => AdapterError::Invalid(error),
            RuntimeEvidenceError::Allocation(failure) => AdapterError::Allocation(failure),
        })?;
        Ok(evidence)
    }
}
impl LocalRuntime {
    /// Evaluate the ordinary constraint pass used by compatibility checking
    /// over the bounded decoded model. Allocation refusal is returned as the
    /// outer error so callers can keep it distinct from a semantic
    /// incompatibility, matching the legacy compatibility report contract.
    pub(crate) fn check_decoded_compatibility_constraints(
        &self,
        program: &FallibleDecodedCapturedProgram,
        merged: &FallibleDecodedDataSnapshot,
        day: i32,
        allocations: &mut product_alloc::AllocationContext,
    ) -> std::result::Result<std::result::Result<(), AdapterError>, AdapterError> {
        let session = program.initial_session_with(allocations).map_err(AdapterError::Allocation)?;
        let limits = RuntimeLimits { fuel: 10_000_000, ..RuntimeLimits::default() };
        let owned_allocations = std::mem::replace(allocations, product_alloc::AllocationContext::new());
        let mut meter = Meter::with_allocations(
            limits.clone(), limits.fuel, Arc::new(AtomicBool::new(false)), owned_allocations,
        );
        let result = execute::constraints_decoded(program, merged, &session, day, &mut meter);
        *allocations = meter.allocations;
        match result {
            Err(error @ AdapterError::Allocation(_)) => Err(error),
            Ok(()) => Ok(Ok(())),
            Err(error) => Ok(Err(error)),
        }
    }

    /// Decoded counterpart of the validation work performed before an
    /// ordinary runtime run is published. `compatibility` and `issues` are
    /// the freshly recomputed report values, not persisted claims.
    pub(crate) fn validate_decoded_initial_runtime(
        &self,
        program: &FallibleDecodedCapturedProgram,
        data: &FallibleDecodedDataSnapshot,
        session: &FallibleDecodedSessionState,
        clock_day: i32,
        limits: RuntimeLimits,
        compatibility: CompatibilityState,
        issues: &[String],
        allocations: &mut product_alloc::AllocationContext,
    ) -> Result<()> {
        program.validate_with(allocations).map_err(|error| match error {
            AppValidationError::Contract(error) => AdapterError::Invalid(error),
            AppValidationError::Allocation(failure) => AdapterError::Allocation(failure),
        })?;
        data.validate_with(allocations).map_err(|error| match error {
            AppValidationError::Contract(error) => AdapterError::Invalid(error),
            AppValidationError::Allocation(failure) => AdapterError::Allocation(failure),
        })?;
        program.validate_session_with(session, allocations).map_err(|error| match error {
            AppValidationError::Contract(error) => AdapterError::Invalid(error),
            AppValidationError::Allocation(failure) => AdapterError::Allocation(failure),
        })?;
        validate_day(clock_day)?;
        limits.validate()?;
        if data.project_id() != program.project_id() {
            return Err(invalid("program and data belong to different projects"));
        }
        self.validate_decoded_initial_runtime_after_report(
            program, data, session, clock_day, limits, compatibility, issues, allocations,
        )
    }

    pub(crate) fn validate_decoded_initial_runtime_after_report(
        &self,
        _program: &FallibleDecodedCapturedProgram,
        data: &FallibleDecodedDataSnapshot,
        session: &FallibleDecodedSessionState,
        _clock_day: i32,
        limits: RuntimeLimits,
        compatibility: CompatibilityState,
        issues: &[String],
        allocations: &mut product_alloc::AllocationContext,
    ) -> Result<()> {
        if compatibility != CompatibilityState::Compatible {
            let mut message = String::new();
            for (index, issue) in issues.iter().enumerate() {
                if index != 0 {
                    product_alloc::try_append_str(&mut message, "; ", allocations, product_alloc::AllocationSite::String)
                        .map_err(AdapterError::Allocation)?;
                }
                product_alloc::try_append_str(&mut message, issue, allocations, product_alloc::AllocationSite::String)
                    .map_err(AdapterError::Allocation)?;
            }
            return Err(AdapterError::Unsupported(message));
        }

        let owned_allocations = std::mem::replace(allocations, product_alloc::AllocationContext::new());
        let mut meter = Meter::with_allocations(
            limits.clone(), 10_000_000, Arc::new(AtomicBool::new(false)), owned_allocations,
        );
        let data_view = DataSnapshotView::from(data);
        let result = (|| {
            meter.collection(data_view.record_count())?;
            for row in data_view.records() {
                for (_, value) in row.values().iter() { meter.value(value)?; }
            }
            for (_, value) in session.values().iter() {
                meter.value(value)?;
                check_references_decoded(value, data_view)?;
            }
            if let Some(focus) = session.focused_record() {
                if data_view.record(&focus.entity, &focus.record).is_none() {
                    return Err(invalid("record reference does not exist"));
                }
            }
            Ok(())
        })();
        *allocations = meter.allocations;
        result
    }

    fn start_source(
        &self,
        program: &ProgramSource,
        data: &DataSnapshot,
        session: &SessionState,
        clock_day: i32,
        random_seed: u64,
        limits: RuntimeLimits,
    ) -> Result<ProductRun> {
        program.validate()?;
        data.validate()?;
        program.validate_session(session)?;
        validate_day(clock_day)?;
        limits.validate()?;
        if data.project_id != program.binding.project_id {
            return Err(invalid("program and data belong to different projects"));
        }
        let report = compatibility::check(program, data, Some(clock_day))?;
        if report.state != CompatibilityState::Compatible {
            return Err(AdapterError::Unsupported(report.issues.join("; ")));
        }
        let mut meter = Meter::new(limits.clone(), 10_000_000, Arc::new(AtomicBool::new(false)));
        meter.collection(data.records.len())?;
        for row in &data.records {
            for value in row.values.values() {
                meter.value(value)?;
            }
        }
        for value in session.values.values() {
            meter.value(value)?;
            check_references(value, data)?;
        }
        if let Some(focus) = &session.focused_record {
            eval::record(
                data,
                &DataValue::Reference {
                    entity: focus.entity.clone(),
                    record: focus.record.clone(),
                },
            )?;
        }
        // Validation is bounded separately, preserving the requested execution
        // fuel for input evaluation (including deliberately tiny test budgets).
        Ok(ProductRun {
            program: program.owned(),
            state: State {
                data: data.clone(),
                session: session.clone(),
                day: clock_day,
                outputs: vec![],
                output_participants: vec![],
                output_participant_count: 0,
            },
            seed: random_seed,
            limits: limits.clone(),
            fuel: Cell::new(limits.fuel),
            trace: vec![],
            observations: vec![],
            receipts: BTreeMap::new(),
        })
    }
}
impl RuntimeAdapter for LocalRuntime {
    fn cancellation_token(&self) -> Option<Arc<AtomicBool>> {
        Some(self.cancelled.clone())
    }
    type Run = ProductRun;
    fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities {adapter:"local-app".into(),version:RUNTIME_VERSION.into(),artifact_kinds:vec![ArtifactKind::GeneratedApp],features:["relations","queries","transactions","session-state","typed-local-output","generated-views","current-data-recovery","data-inspector"].into_iter().map(String::from).collect(),unavailable:vec!["External effects, arbitrary native code and network access are not language operations".into()]}
    }
    fn validate(&self, program: &CapturedProgram) -> Result<()> {
        program.validate()?;
        Ok(())
    }
    fn start(
        &self,
        program: &CapturedProgram,
        data: &DataSnapshot,
        session: &SessionState,
        clock_day: i32,
        random_seed: u64,
        limits: RuntimeLimits,
    ) -> Result<ProductRun> {
        self.start_source(
            &ProgramSource::ordinary(program),
            data,
            session,
            clock_day,
            random_seed,
            limits,
        )
    }
    fn apply(
        &self,
        run: &mut ProductRun,
        input: &SemanticInput,
        operation_id: &str,
    ) -> Result<TraceStep> {
        if !valid_id(operation_id) {
            return Err(invalid("invalid operation ID"));
        }
        validate_input_shape(input)?;
        let request = canonical_digest(IdentityDomain::Input, &(input, &run.program.artifact))?;
        if let Some((prior, step)) = run.receipts.get(operation_id) {
            return if prior == &request {
                Ok(step.clone())
            } else {
                Err(AdapterError::Stale(
                    "Operation ID belongs to another input".into(),
                ))
            };
        }
        if run
            .state
            .data
            .events
            .iter()
            .any(|event| event.operation_id == operation_id)
        {
            return Err(AdapterError::Stale(
                "Operation already committed; resolve its persisted receipt before retrying".into(),
            ));
        }
        if run.trace.len() >= run.limits.action_steps {
            return Err(exhausted("input action limit"));
        }
        let before_data = run.state.data.identity()?;
        let before_session = run.state.session.identity()?;
        let before_day = run.state.day;
        let mut next = run.state.clone();
        let mut meter = Meter::new(run.limits.clone(), run.fuel.get(), self.cancelled.clone());
        let mut observation = None;
        let result = (|| {
            meter.tick(1)?;
            validate_input(input, &run.program.program)?;
            if let SemanticInput::Observe { point } = input {
                if run.observations.iter().any(|o| &o.point == point) {
                    return Err(invalid("observation point already used"));
                }
                observation = Some(view::observe(&run.program, &next, point, &mut meter)?);
            } else {
                execute::input(
                    &run.program,
                    &mut next,
                    input,
                    operation_id,
                    run.seed,
                    &mut meter,
                )?;
            }
            meter.tick(1)?;
            Ok(())
        })();
        run.fuel.set(meter.fuel);
        let output_start = run.state.outputs.len();
        // Restored artifacts have no freshly executed sidecars. Their count
        // must never index the independent vector of this run's receipts.
        let participant_start = run.state.output_participants.len();
        let (outcome, diagnostic) = match &result {
            Ok(()) => (StepOutcome::Applied, None),
            Err(e) => (
                match e {
                    AdapterError::Invalid(_) => StepOutcome::Rejected,
                    AdapterError::BudgetExhausted(_) => StepOutcome::BudgetExhausted,
                    _ => StepOutcome::Failed,
                },
                Some(diagnostic(e)),
            ),
        };
        if result.is_ok() {
            run.state = next;
            if let Some(observation) = observation {
                run.observations.push(observation);
            }
        }
        let step = TraceStep {
            index: run.trace.len(),
            input: input.clone(),
            before_data,
            after_data: run.state.data.identity()?,
            before_session,
            after_session: run.state.session.identity()?,
            outputs: run.state.outputs[output_start..]
                .iter()
                .map(|o| o.digest.clone())
                .collect(),
            outcome,
            diagnostic,
        };
        for receipt in &mut run.state.output_participants[participant_start..] {
            receipt.bind_step(
                &request,
                &step.before_data,
                &step.before_session,
                before_day,
            );
        }
        run.trace.push(step.clone());
        result?;
        run.receipts
            .insert(operation_id.into(), (request, step.clone()));
        Ok(step)
    }
    fn observe(&self, run: &ProductRun, point: &str) -> Result<Observation> {
        let mut meter = Meter::new(run.limits.clone(), run.fuel.get(), self.cancelled.clone());
        let result = view::observe(&run.program, &run.state, point, &mut meter);
        run.fuel.set(meter.fuel);
        result
    }
    fn emitted_artifacts<'a>(&self, run: &'a ProductRun) -> Result<&'a [LocalArtifact]> {
        Ok(run.artifacts())
    }
    fn emitted_record_participants<'a>(
        &self,
        run: &'a ProductRun,
    ) -> Result<Option<&'a [EmittedRecordParticipants]>> {
        // Resume preserves historical bytes without inventing execution
        // receipts for them. A fresh suffix is not a complete inventory.
        Ok(
            (run.state.output_participants.len() == run.state.outputs.len())
                .then_some(run.state.output_participants.as_slice()),
        )
    }
    fn data<'a>(&self, run: &'a ProductRun) -> &'a DataSnapshot {
        &run.state.data
    }
    fn session<'a>(&self, run: &'a ProductRun) -> &'a SessionState {
        &run.state.session
    }
    fn compatibility(
        &self,
        program: &CapturedProgram,
        current: &DataSnapshot,
    ) -> Result<CompatibilityReport> {
        compatibility::check(&ProgramSource::ordinary(program), current, None)
    }
    fn prepare_adoption(&self, plan: AdoptionPlan, current: &DataSnapshot) -> Result<AdoptionPlan> {
        plan.validate()?;
        current.validate()?;
        if plan.expected_data != current.identity()?
            || plan.expected_generation != current.generation
            || plan.project_id != current.project_id
        {
            return Err(AdapterError::Stale(
                "Current data changed; rehearse adoption again".into(),
            ));
        }
        if plan.compatibility.state != CompatibilityState::Compatible {
            return Err(AdapterError::Unsupported(
                "Adoption compatibility has not been established".into(),
            ));
        }
        // This checks only structural freshness. The controller/store must
        // recompute target compatibility and decision obligations before commit.
        Ok(plan)
    }
}
fn check_references(value: &DataValue, data: &DataSnapshot) -> Result<()> {
    match value {
        DataValue::Reference { .. } => {
            eval::record(data, value)?;
        }
        DataValue::List { items, .. } => {
            for item in items {
                check_references(item, data)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn check_references_decoded(value: &DataValue, data: DataSnapshotView<'_>) -> Result<()> {
    match value {
        DataValue::Reference { entity, record } => {
            if data.record(entity, record).is_none() { return Err(invalid("record reference does not exist")); }
        }
        DataValue::List { items, .. } => {
            for item in items { check_references_decoded(item, data)?; }
        }
        _ => {}
    }
    Ok(())
}
pub(crate) fn merged_data(
    program: &CapturedProgram,
    current: &DataSnapshot,
) -> Result<DataSnapshot> {
    merged_definition_data(&program.program, current)
}
pub(crate) fn merged_definition_data(
    program: &AppDefinition,
    current: &DataSnapshot,
) -> Result<DataSnapshot> {
    compatibility::merge(program, current)
}

// Capture-row API/payment controls are staged before the APIs exist.
mod capture;
#[cfg(test)]
mod capture_row_controls;
#[cfg(test)]
mod capture_native_controls;
