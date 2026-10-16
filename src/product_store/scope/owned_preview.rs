//! Ephemeral protected execution. All mutable interpreter state and admission
//! continuity belong to one controller; exported data is never a resume token.
use super::common_fact_admission::{refuse, AdmissionResult, ProofMeter};
use super::owned_lowering::{self as lowering, Metadata};
use super::*;
use crate::product_runtime::{LocalRuntime, ProductRun, ProjectionValidation, ReplayAdmission};
use serde::Serialize;
use std::sync::{atomic::AtomicBool, Arc, Mutex};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OwnedPreviewReceipt {
    pub basis: Digest,
    pub compiler: Digest,
    pub target: Digest,
    pub original_data: Digest,
    pub initialized_data: Digest,
    pub session: Digest,
    pub decisions: Digest,
    pub day: i32,
    pub random_seed: u64,
    pub runtime: String,
    pub driver: String,
    pub availability: OwnedPreviewAvailability,
    pub outcome_comparison: Vec<String>,
}
/// Read-only diagnostic access does not confer durable admission. There is no
/// Deserialize, mutable frame access, scalar preparation or final-data resume.
pub struct PreparedOwnedPreview {
    proof: Arc<VerifiedOwnedPreviewPlan>,
    target: CapturedProgram,
    metadata: Metadata,
    initialized: DataSnapshot,
    receipt: OwnedPreviewReceipt,
}
fn member(proof: &VerifiedOwnedPreviewPlan, row: &Record, existing: bool) -> bool {
    let reference = RecordRef {
        entity: row.entity.clone(),
        record: row.id.clone(),
    };
    if proof.request.excluded_records.contains(&reference) {
        return false;
    }
    match &proof.request.population {
        ScopePopulation::All => true,
        ScopePopulation::FutureWork => !existing,
        ScopePopulation::SelectedUnfinished { records } => existing && records.contains(&reference),
    }
}
fn flag(row: &Record, key: &str) -> AdmissionResult<bool> {
    match row.values.get(key) {
        Some(DataValue::Boolean { value }) => Ok(*value),
        _ => Err(refuse(key, "missing or non-Boolean protected cell")),
    }
}
fn initialize(
    proof: &VerifiedOwnedPreviewPlan,
    target: &CapturedProgram,
    metadata: &Metadata,
    meter: &mut ProofMeter,
) -> AdmissionResult<DataSnapshot> {
    let mut data = proof.snapshot.data.clone();
    for entity in &target.program.entities {
        if let Some(prior) = data.schema.iter_mut().find(|e| e.id == entity.id) {
            // Proof has already authenticated compatible additive schemas. Do
            // not use merged_data before required protected cells are filled.
            for f in &entity.fields {
                if !prior.fields.iter().any(|p| p.id == f.id) {
                    prior.fields.push(f.clone());
                }
            }
        } else {
            data.schema.push(entity.clone());
        }
    }
    for row in data
        .records
        .iter_mut()
        .filter(|r| r.entity == metadata.root)
    {
        meter.charge(1)?;
        row.values
            .extend(metadata.defaults(proof, member(proof, row, true))?);
        if row.archived || lowering::completed(proof, row)? {
            let captured = proof
                .captured
                .get(&row.id)
                .ok_or_else(|| refuse(&row.id, "missing authenticated initial capture"))?;
            if captured.len() != proof.report.projections.len() {
                return Err(refuse(&row.id, "incomplete initial capture"));
            }
            for (key, p) in metadata.saved.iter().zip(&proof.report.projections) {
                row.values.insert(
                    key.clone(),
                    captured
                        .get(&p.location)
                        .ok_or_else(|| {
                            refuse(&p.location, "missing capture key, not captured Null")
                        })?
                        .clone(),
                );
            }
            row.values
                .insert(metadata.sealed.clone(), DataValue::Boolean { value: true });
            row.values.insert(
                metadata.day.clone(),
                DataValue::Date {
                    days: proof.snapshot.clock_day,
                },
            );
            row.values.insert(
                metadata.origin.clone(),
                DataValue::Text {
                    value: metadata.identity.as_str().into(),
                },
            );
        }
    }
    meter.charge(canonical_bytes(&data)?.len() as u64)?;
    data.validate()?;
    let mut stripped = data.clone();
    stripped.schema = proof.snapshot.data.schema.clone();
    let fields: BTreeSet<_> = metadata.fields(proof).into_iter().map(|f| f.id).collect();
    for row in &mut stripped.records {
        if row.entity == metadata.root {
            row.values.retain(|id, _| !fields.contains(id));
        }
    }
    if stripped != proof.snapshot.data {
        return Err(refuse("initialization", "business frame changed"));
    }
    Ok(data)
}
fn build(
    proof: VerifiedOwnedPreviewPlan,
    meter: &mut ProofMeter,
) -> AdmissionResult<PreparedOwnedPreview> {
    let (target, metadata) = lowering::compile(&proof, meter)?;
    let initialized = initialize(&proof, &target, &metadata, meter)?;
    let receipt = OwnedPreviewReceipt {
        basis: proof.report.snapshot.clone(),
        compiler: metadata.identity.clone(),
        target: revision(&target)?,
        original_data: proof.snapshot.data.identity()?,
        initialized_data: initialized.identity()?,
        session: proof.snapshot.session.identity()?,
        decisions: proof.snapshot.decisions.identity()?,
        day: proof.snapshot.clock_day,
        random_seed: 0,
        runtime: RUNTIME_VERSION.into(),
        driver: DRIVER_VERSION.into(),
        availability: proof.request.availability.clone(),
        outcome_comparison: proof.report.outcome_comparison.clone(),
    };
    meter.charge(1)?;
    Ok(PreparedOwnedPreview {
        proof: Arc::new(proof),
        target,
        metadata,
        initialized,
        receipt,
    })
}
fn regenerate(
    store: &ProductStore,
    proof: &VerifiedOwnedPreviewPlan,
    meter: &mut ProofMeter,
) -> AdmissionResult<VerifiedOwnedPreviewPlan> {
    meter.charge(1)?;
    let current = store.load()?;
    meter.charge(1)?;
    if current != proof.snapshot {
        return Err(refuse("basis", "prepared preview is stale"));
    }
    let regenerated = super::common_fact_admission::derive(
        &current,
        &proof.candidate,
        &proof.request,
        &proof.accepted,
        meter,
    )?;
    if regenerated.report != proof.report || regenerated.captured != proof.captured {
        return Err(refuse("proof", "admission regeneration differs"));
    }
    Ok(regenerated)
}
impl ProductStore {
    pub fn prepare_owned_preview(
        &self,
        proof: &VerifiedOwnedPreviewPlan,
        budget: ProofBudget,
        cancelled: Arc<AtomicBool>,
    ) -> AdmissionResult<PreparedOwnedPreview> {
        let mut meter = ProofMeter::new(budget, cancelled)?;
        let proof = regenerate(self, proof, &mut meter)?;
        build(proof, &mut meter)
    }
}
impl PreparedOwnedPreview {
    pub fn target(&self) -> &CapturedProgram {
        &self.target
    }
    pub fn initialized(&self) -> &DataSnapshot {
        &self.initialized
    }
    pub fn receipt(&self) -> &OwnedPreviewReceipt {
        &self.receipt
    }
    #[cfg(test)]
    pub(crate) fn validate_state_for_test(&self, data: &DataSnapshot) -> AdmissionResult<()> {
        let meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
        OwnedPreviewAdmission::new(self, None, meter).transition(
            &self.target,
            data,
            self.receipt.day,
        )
    }
    #[cfg(test)]
    pub(crate) fn validate_replay_frame_for_test(
        &self,
        original: &ScenarioSpec,
        changed: &ScenarioSpec,
        graph: &DecisionGraph,
        target: &CapturedProgram,
    ) -> AdmissionResult<()> {
        let mut projected = original.clone();
        projected.seed = self.initialized.clone();
        let meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
        OwnedPreviewAdmission::new(self, Some((original.clone(), projected)), meter)
            .authorize_frame(target, changed, graph)
    }
    fn fresh(&self, store: &ProductStore, meter: &mut ProofMeter) -> AdmissionResult<()> {
        let proof = regenerate(store, &self.proof, meter)?;
        let fresh = build(proof, meter)?;
        if fresh.target != self.target
            || fresh.metadata != self.metadata
            || fresh.initialized != self.initialized
            || fresh.receipt != self.receipt
        {
            return Err(refuse(
                "preparation",
                "complete compiler/initializer/frame regeneration differs",
            ));
        }
        meter.charge(1)?;
        Ok(())
    }
    pub fn start(
        &self,
        store: &ProductStore,
        limits: RuntimeLimits,
        budget: ProofBudget,
        cancelled: Arc<AtomicBool>,
    ) -> AdmissionResult<OwnedPreviewSession> {
        let mut meter = ProofMeter::new(budget, cancelled.clone())?;
        self.fresh(store, &mut meter)?;
        let admission = Arc::new(OwnedPreviewAdmission::new(self, None, meter));
        admission.validate_seed(&self.target, &self.initialized, self.receipt.day)?;
        let runtime = LocalRuntime::with_cancellation(cancelled).with_admission(admission.clone());
        let run = runtime.resume(
            &self.target,
            &self.initialized,
            &self.proof.snapshot.session,
            self.receipt.day,
            self.receipt.random_seed,
            limits,
            &self.proof.snapshot.artifacts,
        )?;
        admission.check_current(
            &self.target,
            runtime.data(&run),
            runtime.session(&run),
            run.clock_day(),
        )?;
        let namespace = canonical_digest(
            IdentityDomain::Input,
            &(
                "owned-preview-interactive/1",
                &self.proof.snapshot.data,
                &self.proof.snapshot.session,
                self.receipt.day,
                self.receipt.random_seed,
                &self.proof.snapshot.program()?.binding,
                &self.proof.candidate.binding,
                &self.proof.snapshot.decisions,
            ),
        )?;
        Ok(OwnedPreviewSession {
            runtime,
            run: Some(run),
            admission,
            basis: self.receipt.basis.clone(),
            namespace,
            next: 0,
            session_frame: self.proof.snapshot.session.clone(),
            invalid: None,
            #[cfg(test)]
            fail_next: false,
        })
    }
    /// Only current-basis original scenarios are supported. The controller binds
    /// the entire input frame before constructing its inaccessible admission.
    pub fn replay(
        &self,
        store: &ProductStore,
        original: &ScenarioSpec,
        limits: RuntimeLimits,
        budget: ProofBudget,
        cancelled: Arc<AtomicBool>,
        id: &str,
    ) -> AdmissionResult<RunEvidence> {
        let mut meter = ProofMeter::new(budget, cancelled.clone())?;
        self.fresh(store, &mut meter)?;
        original.validate(&self.proof.snapshot.program()?.program)?;
        if original.seed != self.proof.snapshot.data
            || original.session != self.proof.snapshot.session
            || original.clock_day != self.receipt.day
            || original.random_seed != self.receipt.random_seed
        {
            return Err(refuse(
                "replay",
                "original seed/session/day/random frame is not the authenticated basis",
            ));
        }
        let mut projected = original.clone();
        projected.seed = self.initialized.clone();
        let admission =
            OwnedPreviewAdmission::new(self, Some((original.clone(), projected.clone())), meter);
        admission.authorize_frame(&self.target, &projected, &self.proof.snapshot.decisions)?;
        Ok(LocalRuntime::with_cancellation(cancelled).replay_admitted(
            &self.target,
            &projected,
            &self.proof.snapshot.decisions,
            limits,
            id,
            Some(&admission),
        )?)
    }
}
struct Continuation {
    data: DataSnapshot,
    day: i32,
    meter: ProofMeter,
}
/// This capability is never exposed, shared across runs, serialized, or minted
/// from an exported seed. ReplayAdmission is necessary but the controller also
/// authenticates session, clock, graph, random seed, source and input frame.
struct OwnedPreviewAdmission {
    proof: Arc<VerifiedOwnedPreviewPlan>,
    target: CapturedProgram,
    metadata: Metadata,
    seed: DataSnapshot,
    initial_session: SessionState,
    initial_day: i32,
    frame: Option<(ScenarioSpec, ScenarioSpec)>,
    state: Mutex<Continuation>,
}
fn adapter(error: StoreError) -> AdapterError {
    AdapterError::Failed(format!("{error:?}"))
}
impl OwnedPreviewAdmission {
    fn new(
        prepared: &PreparedOwnedPreview,
        frame: Option<(ScenarioSpec, ScenarioSpec)>,
        meter: ProofMeter,
    ) -> Self {
        Self {
            proof: prepared.proof.clone(),
            target: prepared.target.clone(),
            metadata: prepared.metadata.clone(),
            seed: prepared.initialized.clone(),
            initial_session: prepared.proof.snapshot.session.clone(),
            initial_day: prepared.receipt.day,
            frame,
            state: Mutex::new(Continuation {
                data: prepared.initialized.clone(),
                day: prepared.receipt.day,
                meter,
            }),
        }
    }
    fn authorize_frame(
        &self,
        program: &CapturedProgram,
        scenario: &ScenarioSpec,
        graph: &DecisionGraph,
    ) -> AdmissionResult<()> {
        let Some((original, projected)) = &self.frame else {
            return Err(refuse("replay", "no private original frame"));
        };
        if program != &self.target
            || scenario != projected
            || graph != &self.proof.snapshot.decisions
            || original.seed != self.proof.snapshot.data
            || original.session != self.initial_session
            || original.clock_day != self.initial_day
            || original.random_seed != 0
            || scenario.inputs != original.inputs
        {
            return Err(refuse(
                "replay",
                "full source/graph/session/day/random/input binding mismatch",
            ));
        }
        Ok(())
    }
    fn check_current(
        &self,
        program: &CapturedProgram,
        data: &DataSnapshot,
        session: &SessionState,
        day: i32,
    ) -> AdmissionResult<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| refuse("session", "validation lock failed"))?;
        state.meter.charge(1)?;
        if program != &self.target || data != &state.data || day != state.day {
            return Err(refuse(
                "session",
                "current frame diverged from executed transition chain",
            ));
        }
        session.validate(&program.program)?;
        Ok(())
    }
    fn transition(
        &self,
        program: &CapturedProgram,
        data: &DataSnapshot,
        day: i32,
    ) -> AdmissionResult<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| refuse("session", "validation lock failed"))?;
        state.meter.charge(canonical_bytes(data)?.len() as u64)?;
        if program != &self.target || data.schema != self.seed.schema {
            return Err(refuse("session", "source or protected schema changed"));
        }
        data.validate()?;
        validate_day(day)?;
        if !data.events.starts_with(&state.data.events) {
            return Err(refuse("session", "existing factual event history changed"));
        }
        let runtime = LocalRuntime::with_cancellation(state.meter.cancelled());
        let mut checks = ProjectionValidation::default();
        let fields = self.metadata.fields(&self.proof);
        let mut present = BTreeSet::new();
        for row in data
            .records
            .iter()
            .filter(|r| r.entity == self.metadata.root)
        {
            state.meter.charge(1)?;
            present.insert(row.id.clone());
            for f in &fields {
                let value = row.values.get(&f.id).ok_or_else(|| {
                    refuse(&f.id, "protected cell missing, including captured Null")
                })?;
                validate_value(value, &f.value_type, 0)?;
            }
            let prior = state
                .data
                .records
                .iter()
                .find(|r| r.entity == row.entity && r.id == row.id);
            let sealed = flag(row, &self.metadata.sealed)?;
            let is_member = flag(row, &self.metadata.member)?;
            if is_member
                != prior
                    .map(|p| flag(p, &self.metadata.member))
                    .transpose()?
                    .unwrap_or_else(|| member(&self.proof, row, false))
            {
                return Err(refuse(&row.id, "immutable cohort changed"));
            }
            if let Some(prior) = prior {
                if flag(prior, &self.metadata.sealed)? {
                    if row != prior {
                        return Err(refuse(
                            &row.id,
                            "sealed business row or captured values changed",
                        ));
                    }
                    continue;
                }
                if row.archived != prior.archived || row.created_program != prior.created_program {
                    return Err(refuse(&row.id, "root identity or archive changed"));
                }
            } else if row.archived || row.created_program != self.target.artifact.program_digest {
                return Err(refuse(
                    &row.id,
                    "future root lacks actual target Create provenance",
                ));
            }
            if sealed {
                if !lowering::completed(&self.proof, row)?
                    || row.values.get(&self.metadata.day) != Some(&DataValue::Date { days: day })
                    || row.values.get(&self.metadata.origin)
                        != Some(&DataValue::Text {
                            value: self.metadata.identity.as_str().into(),
                        })
                {
                    return Err(refuse(&row.id, "seal lacks completion/capture provenance"));
                }
                let reference = RecordRef {
                    entity: row.entity.clone(),
                    record: row.id.clone(),
                };
                let mut captured = BTreeSet::new();
                for (i, p) in self.proof.report.projections.iter().enumerate() {
                    if !captured.insert(&self.metadata.saved[i]) {
                        continue;
                    }
                    let live = self.metadata.live(p, &p.binding)?;
                    let value = runtime.evaluate_record_projection_with(
                        &mut checks,
                        state.meter.runtime()?,
                        &self.target,
                        data,
                        &reference,
                        &p.binding,
                        &live,
                        &p.host_value_type,
                        day,
                    )?;
                    if row.values.get(&self.metadata.saved[i]) != Some(&value) {
                        return Err(refuse(
                            &p.location,
                            "completion capture differs from executed mixed expression",
                        ));
                    }
                }
            } else {
                if lowering::completed(&self.proof, row)? {
                    return Err(refuse(&row.id, "completed root has no seal"));
                }
                let defaults = self.metadata.defaults(&self.proof, is_member)?;
                for (key, value) in defaults {
                    if row.values.get(&key) != Some(&value) {
                        return Err(refuse(&key, "uncaptured metadata changed"));
                    }
                }
            }
        }
        if state
            .data
            .records
            .iter()
            .any(|r| r.entity == self.metadata.root && !present.contains(&r.id))
        {
            return Err(refuse("session", "retained root disappeared"));
        }
        for auxiliary in &self.proof.request.auxiliary {
            for row in data.records.iter().filter(|r| r.entity == auxiliary.entity) {
                state.meter.charge(1)?;
                let Some(DataValue::Reference { entity, record }) =
                    row.values.get(&auxiliary.owner_field)
                else {
                    return Err(refuse(
                        &row.id,
                        "auxiliary owner is not a required root reference",
                    ));
                };
                if entity != &self.metadata.root {
                    return Err(refuse(&row.id, "auxiliary owner has wrong root type"));
                }
                let owner = data
                    .records
                    .iter()
                    .find(|r| &r.entity == entity && &r.id == record)
                    .ok_or_else(|| refuse(&row.id, "auxiliary root missing"))?;
                if !flag(owner, &self.metadata.member)? {
                    return Err(refuse(&row.id, "auxiliary effects escaped selected cohort"));
                }
                let prior = state
                    .data
                    .records
                    .iter()
                    .find(|r| r.entity == row.entity && r.id == row.id);
                if let Some(prior) = prior {
                    if prior.values.get(&auxiliary.owner_field)
                        != row.values.get(&auxiliary.owner_field)
                        || prior.created_program != row.created_program
                        || prior.archived != row.archived
                    {
                        return Err(refuse(&row.id, "auxiliary immutable identity changed"));
                    }
                    if state.data.records.iter().any(|r| {
                        &r.entity == entity
                            && &r.id == record
                            && flag(r, &self.metadata.sealed).unwrap_or(false)
                    }) && prior != row
                    {
                        return Err(refuse(&row.id, "sealed owner auxiliary was modified"));
                    }
                } else if row.archived
                    || row.created_program != self.target.artifact.program_digest
                    || flag(owner, &self.metadata.sealed)?
                {
                    return Err(refuse(
                        &row.id,
                        "auxiliary Create lacks live owner/provenance",
                    ));
                }
            }
            if state.data.records.iter().any(|r| {
                r.entity == auxiliary.entity
                    && !data
                        .records
                        .iter()
                        .any(|n| n.entity == r.entity && n.id == r.id)
            }) {
                return Err(refuse(&auxiliary.entity, "retained auxiliary disappeared"));
            }
        }
        state.meter.charge(1)?;
        state.data = data.clone();
        state.day = day;
        Ok(())
    }
}
impl ReplayAdmission for OwnedPreviewAdmission {
    fn replay_operation_ids(
        &self,
        program: &CapturedProgram,
        scenario: &ScenarioSpec,
    ) -> std::result::Result<Vec<Id>, AdapterError> {
        self.authorize_frame(program, scenario, &self.proof.snapshot.decisions)
            .map_err(adapter)?;
        let original = &self
            .frame
            .as_ref()
            .ok_or_else(|| adapter(refuse("replay", "original frame absent")))?
            .0;
        let operations = original.replay_operation_ids()?;
        if operations.len() != scenario.inputs.len() {
            return Err(adapter(refuse(
                "replay",
                "operation count differs from authenticated input sequence",
            )));
        }
        Ok(operations)
    }
    fn validate_seed(
        &self,
        program: &CapturedProgram,
        data: &DataSnapshot,
        day: i32,
    ) -> std::result::Result<(), AdapterError> {
        if program != &self.target || data != &self.seed || day != self.initial_day {
            return Err(adapter(refuse(
                "seed",
                "exact initialized source/data/day required",
            )));
        }
        self.state
            .lock()
            .map_err(|_| adapter(refuse("seed", "validation lock failed")))?
            .meter
            .charge(1)
            .map_err(adapter)
    }
    fn validate_state(
        &self,
        program: &CapturedProgram,
        data: &DataSnapshot,
        day: i32,
    ) -> std::result::Result<(), AdapterError> {
        self.transition(program, data, day).map_err(adapter)
    }
}
/// Real LocalRuntime state. Every attempted input consumes its own ordinal;
/// ordinary failures retain honest fuel/trace, host failures dispose the run.
pub struct OwnedPreviewSession {
    runtime: LocalRuntime,
    run: Option<ProductRun>,
    admission: Arc<OwnedPreviewAdmission>,
    basis: Digest,
    namespace: Digest,
    next: u64,
    session_frame: SessionState,
    invalid: Option<String>,
    #[cfg(test)]
    fail_next: bool,
}
impl OwnedPreviewSession {
    fn current(&self) -> AdmissionResult<&ProductRun> {
        self.run
            .as_ref()
            .ok_or_else(|| refuse("session", self.invalid.as_deref().unwrap_or("disposed")))
    }
    pub fn data(&self) -> AdmissionResult<&DataSnapshot> {
        Ok(self.runtime.data(self.current()?))
    }
    pub fn session(&self) -> AdmissionResult<&SessionState> {
        Ok(self.runtime.session(self.current()?))
    }
    pub fn trace(&self) -> AdmissionResult<&[TraceStep]> {
        Ok(self.current()?.trace())
    }
    pub fn clock_day(&self) -> AdmissionResult<i32> {
        Ok(self.current()?.clock_day())
    }
    pub fn basis_is_current(&self, store: &ProductStore) -> AdmissionResult<bool> {
        Ok(canonical_digest(IdentityDomain::Data, &store.load()?)? == self.basis)
    }
    pub fn apply(&mut self, input: &SemanticInput) -> AdmissionResult<TraceStep> {
        let run = self.current()?;
        let checked = if self.runtime.session(run) != &self.session_frame {
            Err(refuse(
                "session",
                "session diverged from executed input chain",
            ))
        } else {
            self.admission.check_current(
                &self.admission.target,
                self.runtime.data(run),
                self.runtime.session(run),
                run.clock_day(),
            )
        };
        if let Err(error) = checked {
            self.invalid = Some(format!("terminal integrity failure: {error:?}"));
            self.run = None;
            return Err(error);
        }
        let operation = canonical_digest(
            IdentityDomain::Input,
            &("owned-preview-input/1", &self.namespace, self.next, input),
        )?;
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| refuse("session", "input ordinal exhausted"))?;
        let run = self.run.as_mut().expect("checked private run");
        let applied = self.runtime.apply(run, input, operation.as_str());
        let checked = self.admission.transition(
            &self.admission.target,
            self.runtime.data(run),
            run.clock_day(),
        );
        #[cfg(test)]
        let checked = if self.fail_next && applied.is_ok() {
            self.fail_next = false;
            Err(refuse("test postcondition", "injected integrity failure"))
        } else {
            checked
        };
        if let Err(error) = checked {
            self.invalid = Some(format!("terminal integrity failure: {error:?}"));
            self.run = None;
            return Err(error);
        }
        self.session_frame = self.runtime.session(self.current()?).clone();
        Ok(applied?)
    }
    pub fn observe(&mut self, point: &str) -> AdmissionResult<Observation> {
        self.apply(&SemanticInput::Observe {
            point: point.into(),
        })?;
        self.current()?
            .observations()
            .last()
            .cloned()
            .ok_or_else(|| refuse("observation", "runtime produced no observation"))
    }
    #[cfg(test)]
    pub(crate) fn fail_next_validation_for_test(&mut self) {
        self.fail_next = true;
    }
}
