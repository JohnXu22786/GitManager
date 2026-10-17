//! Immutable owned-family execution registry. Candidate recipes are indexed,
//! never unioned into one live state. Each replay uses its own exact initializer.
use super::common_fact_admission::{refuse, ProofMeter};
use super::*;
use crate::product_runtime::ProjectionValidation;
use std::sync::{atomic::AtomicBool, Arc};
fn err(message: &str) -> StoreError {
    refuse("owned protected history", message)
}
fn flag(row: &Record, key: &str) -> Result<bool> {
    match row.values.get(key) {
        Some(DataValue::Boolean { value }) => Ok(*value),
        _ => Err(err("missing protected Boolean cell")),
    }
}
fn member(proof: &VerifiedOwnedPreviewPlan, row: &Record, existing: bool) -> bool {
    let reference = history::row_ref(row);
    !proof.request.excluded_records.contains(&reference)
        && match &proof.request.population {
            ScopePopulation::All => true,
            ScopePopulation::FutureWork => !existing,
            ScopePopulation::SelectedUnfinished { records } => {
                existing && records.contains(&reference)
            }
        }
}
fn protected_row(
    prepared: &PreparedOwnedPreview,
    data: &DataSnapshot,
    row: &Record,
    prior: Option<&Record>,
    day: i32,
    meter: &mut ProofMeter,
) -> Result<()> {
    let proof = &prepared.proof;
    let metadata = &prepared.metadata;
    for field in metadata.fields(proof) {
        validate_value(
            row.values
                .get(&field.id)
                .ok_or_else(|| err("missing capture cell is not captured Null"))?,
            &field.value_type,
            0,
        )?;
    }
    if let Some(key) = &metadata.revision {
        if row.values.get(key)
            != Some(&DataValue::Integer {
                value: i64::try_from(row.revision)
                    .map_err(|_| err("root revision exceeds counter"))?,
            })
        {
            return Err(err("root revision differs from protected event counter"));
        }
    }
    let expected = prior
        .map(|r| flag(r, &metadata.member))
        .transpose()?
        .unwrap_or_else(|| member(proof, row, false));
    if flag(row, &metadata.member)? != expected {
        return Err(err("immutable owned population changed"));
    }
    if let Some(prior) = prior {
        if row.archived != prior.archived || row.created_program != prior.created_program {
            return Err(err("owned identity/archive changed"));
        }
        if flag(prior, &metadata.sealed)? {
            if row != prior {
                return Err(err("sealed business row or capture changed"));
            }
            return Ok(());
        }
    } else if row.archived || row.created_program != prepared.target.artifact.program_digest {
        return Err(err("future root lacks actual compiler provenance"));
    }
    if flag(row, &metadata.sealed)? {
        if !owned_lowering::completed(proof, row)?
            || row.values.get(&metadata.day) != Some(&DataValue::Date { days: day })
            || row.values.get(&metadata.origin)
                != Some(&DataValue::Text {
                    value: metadata.identity.as_str().into(),
                })
        {
            return Err(err("seal lacks exact completion provenance"));
        }
        let runtime = LocalRuntime::with_cancellation(meter.cancelled());
        let mut checks = ProjectionValidation::default();
        let mut captured = BTreeSet::new();
        for (i, projection) in proof.report.projections.iter().enumerate() {
            if !captured.insert(&metadata.saved[i]) {
                continue;
            }
            let value = runtime.evaluate_record_projection_with(
                &mut checks,
                meter.runtime()?,
                &prepared.target,
                data,
                &history::row_ref(row),
                &projection.binding,
                &metadata.live(projection, &projection.binding)?,
                &projection.host_value_type,
                day,
            )?;
            if row.values.get(&metadata.saved[i]) != Some(&value) {
                return Err(err(
                    "saved completion value does not regenerate from its historical frame",
                ));
            }
        }
    } else {
        if owned_lowering::completed(proof, row)?
            || metadata
                .defaults(proof, expected)?
                .iter()
                .any(|(k, v)| Some(k) != metadata.revision.as_ref() && row.values.get(k) != Some(v))
        {
            return Err(err("unsealed root has completed or altered capture state"));
        }
    }
    Ok(())
}
fn owned_entity(prepared: &PreparedOwnedPreview, entity: &str) -> bool {
    entity == prepared.metadata.root
        || prepared
            .proof
            .request
            .auxiliary
            .iter()
            .any(|a| a.entity == entity)
}
fn event_revision(prepared: &PreparedOwnedPreview, values: &Values, previous: u64) -> Result<u64> {
    let key = prepared
        .metadata
        .revision
        .as_ref()
        .ok_or_else(|| err("owned revision counter missing"))?;
    let Some(DataValue::Integer { value }) = values.get(key) else {
        return Err(err("owned event revision stamp missing"));
    };
    let actual = u64::try_from(*value).map_err(|_| err("negative owned revision stamp"))?;
    if actual <= previous || actual - previous > RuntimeLimits::default().transaction_writes as u64
    {
        return Err(err(
            "owned event revision stamp exceeds real transaction-write bounds",
        ));
    }
    Ok(actual)
}
/// Rebuild every event's exact value frame before checking new seals. Evaluating
/// against the final shared facts would silently rewrite completed history.
pub(super) fn validate_history(
    prepared: &PreparedOwnedPreview,
    data: &DataSnapshot,
    day: i32,
    meter: &mut ProofMeter,
) -> Result<()> {
    validate_history_from(
        prepared,
        &prepared.initialized,
        prepared.proof.snapshot.clock_day,
        data,
        day,
        meter,
    )
}
fn validate_history_from(
    prepared: &PreparedOwnedPreview,
    seed: &DataSnapshot,
    seed_day: i32,
    data: &DataSnapshot,
    day: i32,
    meter: &mut ProofMeter,
) -> Result<()> {
    meter.charge(canonical_bytes(data)?.len() as u64)?;
    data.validate()?;
    validate_day(day)?;
    if data.schema != seed.schema
        || data.project_id != seed.project_id
        || !data.events.starts_with(&seed.events)
        || data.generation < seed.generation
        || day < seed_day
    {
        return Err(err(
            "source schema, original events or historical clock changed",
        ));
    }
    let mut frame = seed.clone();
    let mut revisions =
        revision_history::RevisionHistory::new(seed, |r| !owned_entity(prepared, &r.entity));
    let mut prior_day = seed_day;
    for event in &data.events[seed.events.len()..] {
        meter.charge(1)?;
        let event_id = canonical_digest(
            IdentityDomain::Input,
            &(
                &event.operation_id,
                &prepared.target.artifact.program_digest,
            ),
        )?;
        if frame.generation.checked_add(1) != Some(event.sequence)
            || event.day < prior_day
            || event.id != format!("event-{}", &event_id.as_str()[..32])
        {
            return Err(err(
                "event sequence/day/identity differs from the actual runtime chain",
            ));
        }
        prior_day = event.day;
        if event.program != prepared.target.artifact.program_digest
            || event.day > day
            || !prepared
                .target
                .program
                .actions
                .iter()
                .any(|a| a.id == event.action)
            || !event.outputs.is_empty()
        {
            return Err(err(
                "event has unsupported producer, action, clock or output",
            ));
        }
        let prior = frame.clone();
        let mut shared_writes = Vec::new();
        let mut exact_writes = 0u64;
        for change in &event.changes {
            meter.charge(1)?;
            if let Some(row) = frame
                .records
                .iter_mut()
                .find(|r| r.entity == change.entity && r.id == change.record)
            {
                if change.before.as_ref() != Some(&row.values) || row.archived {
                    return Err(err("event before-values or archived facts changed"));
                }
                if row.entity == prepared.metadata.root && flag(row, &prepared.metadata.sealed)? {
                    return Err(err("event attempted an Update on an already sealed root"));
                }
                row.values = change.after.clone();
                row.archived = change.archived;
                let old_revision = row.revision;
                row.revision = if owned_entity(prepared, &row.entity) {
                    event_revision(prepared, &change.after, old_revision)?
                } else {
                    old_revision
                        .checked_add(1)
                        .ok_or_else(|| err("record revision overflow"))?
                };
                if owned_entity(prepared, &row.entity) {
                    exact_writes = exact_writes
                        .checked_add(row.revision - old_revision)
                        .ok_or_else(|| err("owned write count overflow"))?;
                } else {
                    shared_writes.push((row.entity.clone(), row.id.clone()));
                }
            } else {
                if change.before.is_some() || change.archived {
                    return Err(err("record birth is missing or archived"));
                }
                let revision = if owned_entity(prepared, &change.entity) {
                    event_revision(prepared, &change.after, 0)?
                } else {
                    1
                };
                if owned_entity(prepared, &change.entity) {
                    exact_writes = exact_writes
                        .checked_add(revision)
                        .ok_or_else(|| err("owned birth write count overflow"))?;
                } else {
                    shared_writes.push((change.entity.clone(), change.record.clone()));
                }
                frame.records.push(Record {
                    entity: change.entity.clone(),
                    id: change.record.clone(),
                    revision,
                    created_program: event.program.clone(),
                    archived: false,
                    values: change.after.clone(),
                });
            }
        }
        revisions.event(shared_writes, exact_writes, meter)?;
        frame.generation = event.sequence;
        frame.events.push(event.clone());
        frame.validate()?;
        for change in &event.changes {
            let row = frame
                .records
                .iter()
                .find(|r| r.entity == change.entity && r.id == change.record)
                .ok_or_else(|| err("changed row missing"))?;
            let old = prior
                .records
                .iter()
                .find(|r| r.entity == row.entity && r.id == row.id);
            if row.entity == prepared.metadata.root {
                protected_row(prepared, &frame, row, old, event.day, meter)?;
            }
            for auxiliary in prepared
                .proof
                .request
                .auxiliary
                .iter()
                .filter(|a| a.entity == row.entity)
            {
                let Some(DataValue::Reference { entity, record }) =
                    row.values.get(&auxiliary.owner_field)
                else {
                    return Err(err("auxiliary owner is not a required reference"));
                };
                let owner = frame
                    .records
                    .iter()
                    .find(|r| {
                        &r.entity == entity && &r.id == record && r.entity == prepared.metadata.root
                    })
                    .ok_or_else(|| err("auxiliary root missing or foreign"))?;
                if !flag(owner, &prepared.metadata.member)? {
                    return Err(err("auxiliary escaped its frozen cohort"));
                }
                if let Some(old) = old {
                    if old.values.get(&auxiliary.owner_field)
                        != row.values.get(&auxiliary.owner_field)
                        || row.archived != old.archived
                        || row.created_program != old.created_program
                    {
                        return Err(err("auxiliary immutable owner/identity changed"));
                    }
                    if prior.records.iter().any(|r| {
                        &r.entity == entity
                            && &r.id == record
                            && flag(r, &prepared.metadata.sealed).unwrap_or(false)
                    }) {
                        return Err(err("sealed owner's auxiliary was updated"));
                    }
                } else if row.archived
                    || flag(owner, &prepared.metadata.sealed)?
                    || row.created_program != prepared.target.artifact.program_digest
                {
                    return Err(err("auxiliary birth lacks live owned provenance"));
                }
            }
        }
    }
    if frame.generation != data.generation
        || frame
            .records
            .iter()
            .map(|r| (&r.entity, &r.id))
            .collect::<Vec<_>>()
            != data
                .records
                .iter()
                .map(|r| (&r.entity, &r.id))
                .collect::<Vec<_>>()
    {
        return Err(err(
            "current generation or record order differs from event history",
        ));
    }
    if frame.records.len() != data.records.len() {
        return Err(err("record disappeared or lacks event provenance"));
    }
    for expected in &frame.records {
        let actual = data
            .records
            .iter()
            .find(|r| r.entity == expected.entity && r.id == expected.id)
            .ok_or_else(|| err("retained record missing"))?;
        if actual.values != expected.values
            || actual.archived != expected.archived
            || actual.created_program != expected.created_program
            || actual.revision < expected.revision
        {
            return Err(err("current row differs from authenticated event history"));
        }
        // Owned revision stamps are emitted in the same actual business Update,
        // including the completion capture Update. They add no write or ordinal.
        if owned_entity(prepared, &expected.entity)
            && (actual.revision != expected.revision
                || actual.revision > (i64::MAX as u64).saturating_sub(10_000_000))
        {
            return Err(err(
                "current owned revision differs from its exact protected history",
            ));
        }
        // Initial sealed rows never receive any business write at all.
        if seed.records.iter().any(|r| {
            r.entity == expected.entity
                && r.id == expected.id
                && r.entity == prepared.metadata.root
                && flag(r, &prepared.metadata.sealed).unwrap_or(false)
                && actual != r
        }) {
            return Err(err("initial completed record/revision changed"));
        }
    }
    revisions.validate(data, meter)
}
#[derive(Clone)]
pub(super) struct OwnedExecutionContext {
    pub snapshot: ProjectSnapshot,
    pub registry: Arc<owned_persist::ReconstructedOwned>,
    pub budget: ProofBudget,
    prepared_target: Option<Digest>,
    proposed_graph: Option<DecisionGraph>,
    preparation_cancelled: Option<Arc<AtomicBool>>,
}
impl OwnedExecutionContext {
    pub fn new(snapshot: &ProjectSnapshot) -> Result<Self> {
        let mut meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
        Ok(Self {
            snapshot: snapshot.clone(),
            registry: Arc::new(owned_persist::reconstruct(snapshot, &mut meter)?),
            budget: ProofBudget::default(),
            prepared_target: None,
            proposed_graph: None,
            preparation_cancelled: None,
        })
    }
    pub(super) fn for_prepared(
        snapshot: &ProjectSnapshot,
        prepared: &PreparedOwnedScopedChangeV2,
        graph: Option<&DecisionGraph>,
    ) -> Result<Self> {
        snapshot.validate()?;
        prepared.check_basis(snapshot)?;
        let mut registry = match &snapshot.scope {
            ScopeEnvelope::ScalarV1(s) if *s == ScopeState::default() => {
                owned_persist::ReconstructedOwned {
                    proofs: BTreeMap::new(),
                    manifests: BTreeMap::new(),
                    initializations: BTreeMap::new(),
                }
            }
            ScopeEnvelope::OwnedV2(_) => {
                let mut meter = ProofMeter::new(prepared.budget, prepared.cancelled.clone())?;
                owned_persist::reconstruct(snapshot, &mut meter)?
            }
            _ => return Err(err("owned preparation cannot enter scalar history")),
        };
        let id = revision(prepared.target())?;
        registry
            .proofs
            .insert(id.clone(), prepared.execution.clone());
        registry
            .manifests
            .insert(id.clone(), prepared.manifest.clone());
        registry
            .initializations
            .insert(id.clone(), prepared.initialization.clone());
        if let Some(next) = graph {
            next.validate()?;
            if snapshot
                .decisions
                .decisions
                .iter()
                .filter(|d| d.status == DecisionStatus::Active)
                .any(|d| !next.decisions.contains(d))
            {
                return Err(err("owned choice changed an independent active intention"));
            }
        }
        Ok(Self {
            snapshot: snapshot.clone(),
            registry: Arc::new(registry),
            budget: prepared.budget,
            prepared_target: Some(id),
            proposed_graph: graph.cloned(),
            preparation_cancelled: Some(prepared.cancelled.clone()),
        })
    }
    fn meter(&self, cancelled: Arc<AtomicBool>) -> Result<ProofMeter> {
        let mut meter = ProofMeter::new(self.budget, cancelled)?;
        if let Some(token) = &self.preparation_cancelled {
            meter.watch_cancellation(token);
        }
        meter.charge(1)?;
        Ok(meter)
    }
    fn permits_graph(&self, graph: &DecisionGraph) -> bool {
        graph == &self.snapshot.decisions || self.proposed_graph.as_ref() == Some(graph)
    }
    pub(super) fn permits_target(&self, target: &CapturedProgram) -> Result<bool> {
        Ok(target == self.snapshot.program()?
            || self.prepared_target.as_ref() == Some(&revision(target)?)
                && self.prepared(target)?.is_some())
    }
    fn prepared(&self, target: &CapturedProgram) -> Result<Option<&PreparedOwnedPreview>> {
        target.validate()?;
        if target.binding.project_id != self.snapshot.data.project_id {
            return Err(err("foreign replay source"));
        }
        if let Some(p) = self.registry.proofs.get(&revision(target)?) {
            if p.target != *target {
                return Err(err("owned source/provenance differs"));
            }
            Ok(Some(p))
        } else {
            compiler::reject_reserved(&target.program)?;
            Ok(None)
        }
    }
    pub(super) fn applicability(
        &self,
        source: &CapturedProgram,
        target: &CapturedProgram,
        scope: &DecisionScope,
        require_owned: bool,
    ) -> Result<ScopeApplicability> {
        let Some(prepared) = self.prepared(source)? else {
            return Ok(if require_owned {
                ScopeApplicability::UnverifiedOwned
            } else {
                ScopeApplicability::Ordinary
            });
        };
        if source != target
            || !self.permits_target(target)?
            || scope != &owned_coverage::expected_scope(prepared)
        {
            return Ok(ScopeApplicability::UnverifiedOwned);
        }
        Ok(ScopeApplicability::OwnedRoot(
            prepared.proof.request.inventory.sources.root_entity.clone(),
        ))
    }
    pub(super) fn owns_decision(&self, decision: &ScopedDecision) -> bool {
        self.snapshot.decisions.decisions.contains(decision)
            && self.snapshot.owned_scope().is_ok_and(|scope| {
                scope
                    .rehearsals
                    .values()
                    .any(|receipt| receipt.witnesses.get(&decision.id) == Some(&decision.witness))
            })
    }
    pub(super) fn boundary_coverage(
        &self,
        target: &CapturedProgram,
        scope: &DecisionScope,
        contexts: &[ScopeContext],
        cancelled: Arc<AtomicBool>,
    ) -> Result<OwnedBoundaryCoverage> {
        if !self.permits_target(target)? {
            return Ok(OwnedBoundaryCoverage::default());
        }
        let Some(prepared) = self.prepared(target)? else {
            return Ok(OwnedBoundaryCoverage::default());
        };
        // One live-cancellable host meter covers the entire additional source
        // proof. It does not replace or replenish any replay/runtime meter.
        let mut meter = self.meter(cancelled)?;
        owned_coverage::derive(prepared, scope, contexts, &mut meter)
    }
    pub fn contains(&self, source: &CapturedProgram) -> bool {
        self.prepared(source).is_ok_and(|p| p.is_some())
    }
    pub fn verify_seed(
        &self,
        target: &CapturedProgram,
        data: &DataSnapshot,
        day: i32,
    ) -> Result<()> {
        let Some(p) = self.prepared(target)? else {
            data.validate()?;
            return Ok(());
        };
        if (data != &p.initialized || day != p.proof.snapshot.clock_day)
            && (revision(target)? != self.snapshot.active_revision
                || data != &self.snapshot.data
                || day != self.snapshot.clock_day)
        {
            return Err(err(
                "protected seed lacks its exact isolated initializer/current frame",
            ));
        }
        self.verify_state(target, data, day)
    }
    pub fn verify_state(
        &self,
        target: &CapturedProgram,
        data: &DataSnapshot,
        day: i32,
    ) -> Result<()> {
        let Some(p) = self.prepared(target)? else {
            data.validate()?;
            return Ok(());
        };
        let mut meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
        validate_history(p, data, day, &mut meter)
    }
    pub fn project_seed(
        &self,
        source: &CapturedProgram,
        original: &DataSnapshot,
        target: &CapturedProgram,
        mapped: &DataSnapshot,
        day: i32,
    ) -> Result<DataSnapshot> {
        self.verify_seed(source, original, day)?;
        let Some(p) = self.prepared(target)? else {
            if merged_data(target, original)? != *mapped {
                return Err(err("ordinary mapped seed differs"));
            }
            return Ok(original.clone());
        };
        if p.proof.snapshot.program()? != source
            || p.proof.snapshot.data != *original
            || p.proof.snapshot.clock_day != day
            || original != mapped
        {
            return Err(err(
                "owned correspondence has a different original source/data frame",
            ));
        }
        Ok(p.initialized.clone())
    }
    pub fn project_scenario(
        &self,
        source: &CapturedProgram,
        original: &ScenarioSpec,
        target: &CapturedProgram,
        mapped: &ScenarioSpec,
    ) -> Result<ScenarioSpec> {
        original.validate(&source.program)?;
        if source == target {
            if original != mapped {
                return Err(err("identity owned replay changed its original frame"));
            }
            self.verify_seed(source, &original.seed, original.clock_day)?;
            self.operation_ids(target, original)?;
            return Ok(original.clone());
        }
        let Some(p) = self.prepared(target)? else {
            if self.prepared(source)?.is_some() {
                return Err(err(
                    "owned-to-ordinary replay requires the later withdrawal proof",
                ));
            }
            mapped.validate(&target.program)?;
            if original.clock_day != mapped.clock_day || original.random_seed != mapped.random_seed
            {
                return Err(err("ordinary replay changed clock/random context"));
            }
            self.project_seed(
                source,
                &original.seed,
                target,
                &mapped.seed,
                mapped.clock_day,
            )?;
            return Ok(mapped.clone());
        };
        let mut expected = original.clone();
        expected.seed = mapped.seed.clone();
        if expected != *mapped
            || original.session != p.proof.snapshot.session
            || original.random_seed != 0
        {
            return Err(err(
                "owned correspondence changed session/random/input frame",
            ));
        }
        expected.seed = self.project_seed(
            source,
            &original.seed,
            target,
            &mapped.seed,
            original.clock_day,
        )?;
        expected.validate(&target.program)?;
        Ok(expected)
    }
    pub fn operation_ids(
        &self,
        target: &CapturedProgram,
        scenario: &ScenarioSpec,
    ) -> Result<Vec<Id>> {
        let Some(p) = self.prepared(target)? else {
            return Ok(scenario.replay_operation_ids()?);
        };
        let mut original = scenario.clone();
        if scenario.seed == p.initialized
            && scenario.session == p.proof.snapshot.session
            && scenario.clock_day == p.proof.snapshot.clock_day
            && scenario.random_seed == 0
        {
            original.seed = p.proof.snapshot.data.clone();
        } else if revision(target)? != self.snapshot.active_revision
            || scenario.seed != self.snapshot.data
            || scenario.session != self.snapshot.session
            || scenario.clock_day != self.snapshot.clock_day
            || scenario.random_seed != 0
        {
            return Err(err("owned replay session/day/random frame changed"));
        }
        let target_identity = revision(target)?;
        for receipt in self
            .snapshot
            .owned_scope()
            .ok()
            .into_iter()
            .flat_map(|s| s.correspondences.values())
            .filter(|r| r.proof.target == target_identity)
        {
            if let Some(pair) = &receipt.proof.scenario {
                if pair.projected.id == scenario.id && pair.projected != *scenario {
                    return Err(err("retained original/projected scenario binding changed"));
                }
            }
        }
        Ok(original.replay_operation_ids()?)
    }
    pub fn presentation(&self) -> Result<crate::product_protocol::PreservedHistory> {
        use crate::product_protocol::{HistoryOrigin, PreservedHistory, PreservedResult};
        let mut result = PreservedHistory {
            results: vec![],
            events: self.snapshot.data.events.clone(),
        };
        let Some(p) = self.registry.proofs.get(&self.snapshot.active_revision) else {
            return Ok(result);
        };
        for row in self
            .snapshot
            .data
            .records
            .iter()
            .filter(|r| r.entity == p.metadata.root)
        {
            if !flag(row, &p.metadata.sealed)? {
                continue;
            }
            let initial = p.initialized.records.iter().any(|r| {
                r.entity == row.entity
                    && r.id == row.id
                    && flag(r, &p.metadata.sealed).unwrap_or(false)
            });
            let event = if initial {
                None
            } else {
                self.snapshot.data.events.iter().find(|e| {
                    e.changes.iter().any(|c| {
                        c.entity == row.entity
                            && c.record == row.id
                            && c.before.as_ref().and_then(|v| v.get(&p.metadata.sealed))
                                == Some(&DataValue::Boolean { value: false })
                            && c.after.get(&p.metadata.sealed)
                                == Some(&DataValue::Boolean { value: true })
                    })
                })
            };
            let day = match row.values.get(&p.metadata.day) {
                Some(DataValue::Date { days }) => *days,
                _ => return Err(err("sealed capture day missing")),
            };
            if !initial && event.is_none() {
                return Err(err("completion event missing"));
            }
            for (i, recipe) in p.proof.report.projections.iter().enumerate() {
                result.results.push(PreservedResult {
                    record: history::row_ref(row),
                    label: recipe.location.clone(),
                    value: row
                        .values
                        .get(&p.metadata.saved[i])
                        .ok_or_else(|| err("saved value missing"))?
                        .clone(),
                    day,
                    origin: if initial {
                        HistoryOrigin::CapturedAtAdoption
                    } else {
                        HistoryOrigin::ObservedAtCompletion
                    },
                    program: match event {
                        Some(e) => e.program.clone(),
                        None => p.proof.snapshot.program()?.artifact.program_digest.clone(),
                    },
                    event: event.map(|e| e.id.clone()),
                });
            }
        }
        Ok(result)
    }
}

/// Per-run state is never shared between comparisons. Full-frame admission
/// precedes interpreter startup; all subsequent checks share this host meter.
struct OwnedRunAdmission {
    context: OwnedExecutionContext,
    target: CapturedProgram,
    scenario: ScenarioSpec,
    graph: DecisionGraph,
    initial_frame: Option<(DataSnapshot, i32)>,
    operations: Vec<Id>,
    meter: std::sync::Mutex<ProofMeter>,
}
impl OwnedExecutionContext {
    pub fn begin_replay(
        &self,
        target: &CapturedProgram,
        scenario: &ScenarioSpec,
        graph: &DecisionGraph,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Box<dyn crate::product_runtime::ReplayAdmission>> {
        let historical_frame = self.registry.proofs.values().any(|p| {
            let basis = &p.proof.snapshot;
            graph == &basis.decisions
                && scenario.session == basis.session
                && scenario.clock_day == basis.clock_day
                && scenario.random_seed == 0
                && ((target == &p.target && scenario.seed == p.initialized)
                    || (basis.program().is_ok_and(|before| before == target)
                        && scenario.seed == basis.data))
        });
        if !self.permits_graph(graph) && !historical_frame {
            return Err(err(
                "replay decision graph differs from authenticated context",
            ));
        }
        let mut meter = self.meter(cancelled)?;
        meter.charge(canonical_bytes(&(scenario, graph))?.len() as u64)?;
        let operations = self.operation_ids(target, scenario)?;
        if operations.len() != scenario.inputs.len() {
            return Err(err("incomplete replay operation identity vector"));
        }
        Ok(Box::new(OwnedRunAdmission {
            context: self.clone(),
            target: target.clone(),
            scenario: scenario.clone(),
            graph: graph.clone(),
            initial_frame: None,
            operations,
            meter: std::sync::Mutex::new(meter),
        }))
    }
}
impl crate::product_runtime::ReplayAdmission for OwnedRunAdmission {
    fn begin_replay(
        &self,
        target: &CapturedProgram,
        scenario: &ScenarioSpec,
        graph: &DecisionGraph,
        _cancelled: Arc<AtomicBool>,
    ) -> std::result::Result<Option<Box<dyn crate::product_runtime::ReplayAdmission>>, AdapterError>
    {
        if target != &self.target || scenario != &self.scenario || graph != &self.graph {
            return Err(AdapterError::Unsupported(
                "Owned replay full retained frame changed".into(),
            ));
        }
        self.meter
            .lock()
            .map_err(|_| AdapterError::Unsupported("Owned replay lock failed".into()))?
            .charge(1)
            .map_err(|e| AdapterError::Unsupported(e.to_string()))?;
        Ok(None)
    }
    fn replay_operation_ids(
        &self,
        target: &CapturedProgram,
        scenario: &ScenarioSpec,
    ) -> std::result::Result<Vec<Id>, AdapterError> {
        if target != &self.target || scenario != &self.scenario {
            return Err(AdapterError::Unsupported(
                "Owned replay full input frame changed".into(),
            ));
        }
        Ok(self.operations.clone())
    }
    fn validate_seed(
        &self,
        target: &CapturedProgram,
        data: &DataSnapshot,
        day: i32,
    ) -> std::result::Result<(), AdapterError> {
        if target != &self.target || data != &self.scenario.seed || day != self.scenario.clock_day {
            return Err(AdapterError::Unsupported(
                "Owned replay seed frame changed".into(),
            ));
        }
        self.validate_state(target, data, day)
    }
    fn validate_state(
        &self,
        target: &CapturedProgram,
        data: &DataSnapshot,
        day: i32,
    ) -> std::result::Result<(), AdapterError> {
        let checked = (|| {
            if target != &self.target {
                return Err(err("owned replay source changed"));
            }
            let mut meter = self
                .meter
                .lock()
                .map_err(|_| err("owned replay validation lock failed"))?;
            meter.charge(1)?;
            if let Some(prepared) = self.context.prepared(target)? {
                if let Some((seed, seed_day)) = &self.initial_frame {
                    validate_history_from(prepared, seed, *seed_day, data, day, &mut meter)?;
                } else {
                    validate_history(prepared, data, day, &mut meter)?;
                }
            } else {
                data.validate()?;
            }
            Ok(())
        })();
        checked.map_err(|e: StoreError| AdapterError::Unsupported(e.to_string()))
    }
}

impl OwnedExecutionContext {
    pub(super) fn project_retained_scene(
        &self,
        token: &super::replay::RetainedScene,
        target: &CapturedProgram,
        graph: &DecisionGraph,
        cancelled: Arc<AtomicBool>,
    ) -> Result<(
        ScenarioSpec,
        Box<dyn crate::product_runtime::ReplayAdmission>,
    )> {
        let mut meter = self.meter(cancelled)?;
        if token.snapshot() != &canonical_digest(IdentityDomain::Data, &self.snapshot)?
            || !self.permits_graph(graph)
            || !self.permits_target(target)?
        {
            return Err(err("retained scene snapshot/graph/current target changed"));
        }
        let p = self
            .prepared(target)?
            .ok_or_else(|| err("retained owned target missing"))?;
        let original = token.scenario();
        if p.proof.snapshot.program()? != token.program()
            || !p
                .proof
                .snapshot
                .decisions
                .decisions
                .contains(token.decision())
            || !graph.decisions.contains(token.decision())
            || self.prepared(token.program())?.is_some()
        {
            return Err(err(
                "retained scene lacks the exact ordinary source/decision receipt",
            ));
        }
        original.validate(&token.program().program)?;
        meter.charge(canonical_bytes(&(original, graph))?.len() as u64)?;
        meter.charge(
            (token.program().source_bytes.len() + p.proof.candidate.source_bytes.len()) as u64,
        )?;
        let inventory = super::common_fact_inventory::derive(
            &p.proof.snapshot,
            &p.proof.candidate,
            &p.proof.request.inventory,
        )?;
        let (certificates, projections, comparison) = super::common_fact_rules::verify_frame(
            &p.proof.snapshot,
            &p.proof.candidate,
            &p.proof.request,
            &inventory,
            &original.seed,
            &mut meter,
        )?;
        if certificates != p.proof.report.certificates
            || projections != p.proof.report.projections
            || comparison != p.proof.report.outcome_comparison
        {
            return Err(err(
                "historical frame does not reproduce the verified rule/projection recipe",
            ));
        }
        let captures = super::common_fact_admission::capture_frame(
            token.program(),
            &original.seed,
            original.clock_day,
            &p.proof.request,
            &projections,
            &mut meter,
        )?;
        let seed = super::owned_preview::initialize_frame(
            &p.proof,
            target,
            &p.metadata,
            &original.seed,
            original.clock_day,
            &captures,
            &mut meter,
        )?;
        let mut actual = original.clone();
        actual.seed = seed.clone();
        actual.validate(&target.program)?;
        let operations = original.replay_operation_ids()?;
        let admission = OwnedRunAdmission {
            context: self.clone(),
            target: target.clone(),
            scenario: actual.clone(),
            graph: graph.clone(),
            initial_frame: Some((seed, original.clock_day)),
            operations,
            meter: std::sync::Mutex::new(meter),
        };
        Ok((actual, Box::new(admission)))
    }
}
