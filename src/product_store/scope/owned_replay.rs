//! Immutable owned-family execution registry. Candidate recipes are indexed,
//! never unioned into one live state. Only first entry can initialize an ordinary
//! historical clone; transition and owned replay seeds retain protected history.
use super::common_fact_admission::{refuse, ProofMeter};
use super::owned_epoch::OwnedExecutionRecipe;
use super::*;
use std::sync::{atomic::AtomicBool, Arc};
pub(super) fn err(message: &str) -> StoreError {
    refuse("owned protected history", message)
}
pub(super) fn flag(row: &Record, key: &str) -> Result<bool> {
    match row.values.get(key) {
        Some(DataValue::Boolean { value }) => Ok(*value),
        _ => Err(err("missing protected Boolean cell")),
    }
}
pub(super) fn member(proof: &VerifiedOwnedPreviewPlan, row: &Record, existing: bool) -> bool {
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
macro_rules! protected_row_body {
    ($prepared:ident, $data:expr, $row:ident, $prior:ident, $day:expr, $meter:expr, $limits:expr, [$($early_success:tt)*], [$($finish:tt)*]) => {
        crate::product_store::scope::owned_replay::protected_row_body!($prepared, $data, $row, $prior, $day, $meter, $limits,
            [$($early_success)*], [$($finish)*], metadata, [
        let runtime = LocalRuntime::with_cancellation($meter.cancelled());
        let mut checks = crate::product_runtime::ProjectionValidation::default();
        let mut captured = BTreeSet::new();
        for (i, projection) in $prepared.projections().iter().enumerate() {
            if !captured.insert(&metadata.saved[i]) {
                continue;
            }
            let value = runtime.evaluate_record_projection_with(
                &mut checks,
                $limits,
                $prepared.target(),
                $data,
                &history::row_ref($row),
                &projection.binding,
                &$prepared.capture_expression(projection)?,
                &projection.host_value_type,
                $day,
            )?;
            if $row.values.get(&metadata.saved[i]) != Some(&value) {
                return Err(crate::product_store::scope::owned_replay::err(
                    "saved completion value does not regenerate from its historical frame",
                ));
            }
        }
            ])
    };
    ($prepared:ident, $data:expr, $row:ident, $prior:ident, $day:expr, $meter:expr, $limits:expr, [$($early_success:tt)*], [$($finish:tt)*], $metadata:ident, [$($projection:tt)*]) => {{
    let proof = &$prepared.initial().proof;
    let $metadata = $prepared.metadata();
    for field in $metadata.fields(proof) {
        validate_value(
            $row.values
                .get(&field.id)
                .ok_or_else(|| crate::product_store::scope::owned_replay::err("missing capture cell is not captured Null"))?,
            &field.value_type,
            0,
        )?;
    }
    if let Some(key) = &$metadata.revision {
        if $row.values.get(key)
            != Some(&DataValue::Integer {
                value: i64::try_from($row.revision)
                    .map_err(|_| crate::product_store::scope::owned_replay::err("root revision exceeds counter"))?,
            })
        {
            return Err(crate::product_store::scope::owned_replay::err("root revision differs from protected event counter"));
        }
    }
    let expected = $prior
        .map(|r| crate::product_store::scope::owned_replay::flag(r, &$metadata.member))
        .transpose()?
        .unwrap_or_else(|| $prepared.active() && crate::product_store::scope::owned_replay::member(proof, $row, false));
    if crate::product_store::scope::owned_replay::flag($row, &$metadata.member)? != expected {
        return Err(crate::product_store::scope::owned_replay::err("immutable owned population changed"));
    }
    if let Some($prior) = $prior {
        if $row.archived != $prior.archived || $row.created_program != $prior.created_program {
            return Err(crate::product_store::scope::owned_replay::err("owned identity/archive changed"));
        }
        if crate::product_store::scope::owned_replay::flag($prior, &$metadata.sealed)? {
            if $row != $prior {
                return Err(crate::product_store::scope::owned_replay::err("sealed business row or capture changed"));
            }
            $($early_success)*
        }
    } else if $row.archived || $row.created_program != $prepared.target().artifact.program_digest {
        return Err(crate::product_store::scope::owned_replay::err("future root lacks actual compiler provenance"));
    }
    if crate::product_store::scope::owned_replay::flag($row, &$metadata.sealed)? {
        if !$prepared.completed($row)?
            || $row.values.get(&$metadata.day) != Some(&DataValue::Date { days: $day })
            || $row.values.get(&$metadata.origin)
                != Some(&DataValue::Text {
                    value: $metadata.identity.as_str().into(),
                })
        {
            return Err(crate::product_store::scope::owned_replay::err("seal lacks exact completion provenance"));
        }
        $($projection)*
    } else {
        if $prepared.completed($row)?
            || $metadata
                .defaults(proof, expected)?
                .iter()
                .any(|(k, v)| Some(k) != $metadata.revision.as_ref() && $row.values.get(k) != Some(v))
        {
            return Err(crate::product_store::scope::owned_replay::err("unsealed root has completed or altered capture state"));
        }
    }
    $($finish)*
    }};
}

macro_rules! native_completed_projection {
    ($prepared:ident, $data:expr, $row:ident, $day:expr, $meter:expr, $limits:expr, $metadata:ident) => {{
        let runtime = LocalRuntime::with_cancellation($meter.cancelled());
        let mut checks = crate::product_runtime::ProjectionValidation::default();
        let mut captured = BTreeSet::new();
        for (i, projection) in $prepared.projections().iter().enumerate() {
            if !captured.insert(&$metadata.saved[i]) {
                continue;
            }
            let value = runtime.evaluate_record_projection_with(
                &mut checks,
                $limits,
                $prepared.target(),
                $data,
                &history::row_ref($row),
                &projection.binding,
                &$prepared.capture_expression(projection)?,
                &projection.host_value_type,
                $day,
            )?;
            if $row.values.get(&$metadata.saved[i]) != Some(&value) {
                return Err(crate::product_store::scope::owned_replay::err(
                    "saved completion value does not regenerate from its historical frame",
                ));
            }
        }
    }};
}
pub(super) use native_completed_projection;
pub(super) use protected_row_body;

pub(super) fn protected_row(
    prepared: &OwnedExecutionRecipe,
    data: &DataSnapshot,
    row: &Record,
    prior: Option<&Record>,
    day: i32,
    meter: &mut ProofMeter,
) -> Result<()> {
    protected_row_body!(prepared, data, row, prior, day, meter, meter.runtime()?, [return Ok(());], [Ok(())])
}

pub(super) fn owned_entity(prepared: &OwnedExecutionRecipe, entity: &str) -> bool {
    entity == prepared.metadata().root
        || prepared
            .request()
            .auxiliary
            .iter()
            .any(|a| a.entity == entity)
}
pub(super) fn event_revision(prepared: &OwnedExecutionRecipe, values: &Values, previous: u64) -> Result<u64> {
    let key = prepared
        .metadata()
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
    prepared: &OwnedExecutionRecipe,
    data: &DataSnapshot,
    day: i32,
    meter: &mut ProofMeter,
) -> Result<()> {
    validate_recipe_history_from(
        prepared,
        prepared.seed(),
        prepared.basis().clock_day,
        data,
        day,
        meter,
    )
}
pub(super) fn validate_recipe_history_from(
    prepared: &OwnedExecutionRecipe,
    seed: &DataSnapshot,
    seed_day: i32,
    data: &DataSnapshot,
    day: i32,
    meter: &mut ProofMeter,
) -> Result<()> {
    validate_recipe_history_with(prepared, seed, seed_day, data, day, meter, &mut |_, _| {
        Ok(())
    })
}
/// The visitor receives the same complete authenticated frames as capture and
/// owner validation. It cannot replace or skip any history proof.
macro_rules! recipe_history_body {
    ($prepared:ident, $seed:expr, $seed_day:expr, $data:expr, $day:expr, $meter:expr,
     [$($seed_call:tt)*], [$($row_call:tt)*], [$($event_call:tt)*], [$($final_call:tt)*], $frame:ident, $row:ident, $old:ident, $event:ident) => {
        crate::product_store::scope::owned_replay::recipe_history_body!($prepared, $seed, $seed_day, $data, $day, $meter,
            [$($seed_call)*], [$($row_call)*], [$($event_call)*], [$($final_call)*], $frame, $row, $old, $event,
            [$meter.charge(canonical_bytes($data)?.len() as u64)?;])
    };
    ($prepared:ident, $seed:expr, $seed_day:expr, $data:expr, $day:expr, $meter:expr,
     [$($seed_call:tt)*], [$($row_call:tt)*], [$($event_call:tt)*], [$($final_call:tt)*], $frame:ident, $row:ident, $old:ident, $event:ident,
     [$($serialization:tt)*]) => {{
    $($serialization)*
    $data.validate()?;
    validate_day($day)?;
    if $data.schema != $seed.schema
        || $data.project_id != $seed.project_id
        || !$data.events.starts_with(&$seed.events)
        || $data.generation < $seed.generation
        || $day < $seed_day
    {
        return Err(crate::product_store::scope::owned_replay::err(
            "source schema, original events or historical clock changed",
        ));
    }
    $($seed_call)*
    let mut $frame = $seed.clone();
    let mut revisions =
        revision_history::RevisionHistory::new($seed, |r| !crate::product_store::scope::owned_replay::owned_entity($prepared, &r.entity));
    let mut prior_day = $seed_day;
    for $event in &$data.events[$seed.events.len()..] {
        $meter.charge(1)?;
        let event_id = canonical_digest(
            IdentityDomain::Input,
            &(
                &$event.operation_id,
                &$prepared.target().artifact.program_digest,
            ),
        )?;
        if $frame.generation.checked_add(1) != Some($event.sequence)
            || $event.day < prior_day
            || $event.id != format!("event-{}", &event_id.as_str()[..32])
        {
            return Err(crate::product_store::scope::owned_replay::err(
                "event sequence/day/identity differs from the actual runtime chain",
            ));
        }
        prior_day = $event.day;
        if $event.program != $prepared.target().artifact.program_digest
            || $event.day > $day
            || !$prepared
                .target()
                .program
                .actions
                .iter()
                .any(|a| a.id == $event.action)
            || !$event.outputs.is_empty()
        {
            return Err(crate::product_store::scope::owned_replay::err(
                "event has unsupported producer, action, clock or output",
            ));
        }
        let prior = $frame.clone();
        let mut shared_writes = Vec::new();
        let mut exact_writes = 0u64;
        for change in &$event.changes {
            $meter.charge(1)?;
            if !$prepared.active()
                && $prepared
                    .request()
                    .auxiliary
                    .iter()
                    .any(|a| a.entity == change.entity)
            {
                return Err(crate::product_store::scope::owned_replay::err("inactive producer mutated dormant auxiliary history"));
            }
            if let Some($row) = $frame
                .records
                .iter_mut()
                .find(|r| r.entity == change.entity && r.id == change.record)
            {
                if change.before.as_ref() != Some(&$row.values) || $row.archived {
                    return Err(crate::product_store::scope::owned_replay::err("event before-values or archived facts changed"));
                }
                if $row.entity == $prepared.metadata().root && crate::product_store::scope::owned_replay::flag($row, &$prepared.metadata().sealed)?
                {
                    return Err(crate::product_store::scope::owned_replay::err("event attempted an Update on an already sealed root"));
                }
                $row.values = change.after.clone();
                $row.archived = change.archived;
                let old_revision = $row.revision;
                $row.revision = if crate::product_store::scope::owned_replay::owned_entity($prepared, &$row.entity) {
                    crate::product_store::scope::owned_replay::event_revision($prepared, &change.after, old_revision)?
                } else {
                    old_revision
                        .checked_add(1)
                        .ok_or_else(|| crate::product_store::scope::owned_replay::err("record revision overflow"))?
                };
                if crate::product_store::scope::owned_replay::owned_entity($prepared, &$row.entity) {
                    exact_writes = exact_writes
                        .checked_add($row.revision - old_revision)
                        .ok_or_else(|| crate::product_store::scope::owned_replay::err("owned write count overflow"))?;
                } else {
                    shared_writes.push(($row.entity.clone(), $row.id.clone()));
                }
            } else {
                if change.before.is_some() || change.archived {
                    return Err(crate::product_store::scope::owned_replay::err("record birth is missing or archived"));
                }
                let revision = if crate::product_store::scope::owned_replay::owned_entity($prepared, &change.entity) {
                    crate::product_store::scope::owned_replay::event_revision($prepared, &change.after, 0)?
                } else {
                    1
                };
                if crate::product_store::scope::owned_replay::owned_entity($prepared, &change.entity) {
                    exact_writes = exact_writes
                        .checked_add(revision)
                        .ok_or_else(|| crate::product_store::scope::owned_replay::err("owned birth write count overflow"))?;
                } else {
                    shared_writes.push((change.entity.clone(), change.record.clone()));
                }
                $frame.records.push(Record {
                    entity: change.entity.clone(),
                    id: change.record.clone(),
                    revision,
                    created_program: $event.program.clone(),
                    archived: false,
                    values: change.after.clone(),
                });
            }
        }
        revisions.event(shared_writes, exact_writes, $meter)?;
        $frame.generation = $event.sequence;
        $frame.events.push($event.clone());
        $frame.validate()?;
        for change in &$event.changes {
            let $row = $frame
                .records
                .iter()
                .find(|r| r.entity == change.entity && r.id == change.record)
                .ok_or_else(|| crate::product_store::scope::owned_replay::err("changed row missing"))?;
            let $old = prior
                .records
                .iter()
                .find(|r| r.entity == $row.entity && r.id == $row.id);
            if $row.entity == $prepared.metadata().root {
                $($row_call)*
            }
            for auxiliary in $prepared
                .request()
                .auxiliary
                .iter()
                .filter(|a| a.entity == $row.entity)
            {
                let Some(DataValue::Reference { entity, record }) =
                    $row.values.get(&auxiliary.owner_field)
                else {
                    return Err(crate::product_store::scope::owned_replay::err("auxiliary owner is not a required reference"));
                };
                let owner = $frame
                    .records
                    .iter()
                    .find(|r| {
                        &r.entity == entity
                            && &r.id == record
                            && r.entity == $prepared.metadata().root
                    })
                    .ok_or_else(|| crate::product_store::scope::owned_replay::err("auxiliary root missing or foreign"))?;
                if !crate::product_store::scope::owned_replay::flag(owner, &$prepared.metadata().member)? {
                    return Err(crate::product_store::scope::owned_replay::err("auxiliary escaped its frozen cohort"));
                }
                if let Some($old) = $old {
                    if $old.values.get(&auxiliary.owner_field)
                        != $row.values.get(&auxiliary.owner_field)
                        || $row.archived != $old.archived
                        || $row.created_program != $old.created_program
                    {
                        return Err(crate::product_store::scope::owned_replay::err("auxiliary immutable owner/identity changed"));
                    }
                    if prior.records.iter().any(|r| {
                        &r.entity == entity
                            && &r.id == record
                            && crate::product_store::scope::owned_replay::flag(r, &$prepared.metadata().sealed).unwrap_or(false)
                    }) {
                        return Err(crate::product_store::scope::owned_replay::err("sealed owner's auxiliary was updated"));
                    }
                } else if $row.archived
                    || crate::product_store::scope::owned_replay::flag(owner, &$prepared.metadata().sealed)?
                    || $row.created_program != $prepared.target().artifact.program_digest
                {
                    return Err(crate::product_store::scope::owned_replay::err("auxiliary birth lacks live owned provenance"));
                }
            }
        }
        $($event_call)*
    }
    if $frame.generation != $data.generation
        || $frame
            .records
            .iter()
            .map(|r| (&r.entity, &r.id))
            .collect::<Vec<_>>()
            != $data
                .records
                .iter()
                .map(|r| (&r.entity, &r.id))
                .collect::<Vec<_>>()
    {
        return Err(crate::product_store::scope::owned_replay::err(
            "current generation or record order differs from event history",
        ));
    }
    if $frame.records.len() != $data.records.len() {
        return Err(crate::product_store::scope::owned_replay::err("record disappeared or lacks event provenance"));
    }
    for expected in &$frame.records {
        let actual = $data
            .records
            .iter()
            .find(|r| r.entity == expected.entity && r.id == expected.id)
            .ok_or_else(|| crate::product_store::scope::owned_replay::err("retained record missing"))?;
        if actual.values != expected.values
            || actual.archived != expected.archived
            || actual.created_program != expected.created_program
            || actual.revision < expected.revision
        {
            return Err(crate::product_store::scope::owned_replay::err("current row differs from authenticated event history"));
        }
        // Owned revision stamps are emitted in the same actual business Update,
        // including the completion capture Update. They add no write or ordinal.
        if crate::product_store::scope::owned_replay::owned_entity($prepared, &expected.entity)
            && (actual.revision != expected.revision
                || actual.revision > (i64::MAX as u64).saturating_sub(10_000_000))
        {
            return Err(crate::product_store::scope::owned_replay::err(
                "current owned revision differs from its exact protected history",
            ));
        }
        // Initial sealed rows never receive any business write at all.
        if $seed.records.iter().any(|r| {
            r.entity == expected.entity
                && r.id == expected.id
                && r.entity == $prepared.metadata().root
                && crate::product_store::scope::owned_replay::flag(r, &$prepared.metadata().sealed).unwrap_or(false)
                && actual != r
        }) {
            return Err(crate::product_store::scope::owned_replay::err("initial completed record/revision changed"));
        }
    }
    revisions.validate($data, $meter)?;
    $($final_call)*
    }};
}
pub(super) use recipe_history_body;

pub(super) fn validate_recipe_history_with(
    prepared: &OwnedExecutionRecipe,
    seed: &DataSnapshot,
    seed_day: i32,
    data: &DataSnapshot,
    day: i32,
    meter: &mut ProofMeter,
    visit: &mut dyn FnMut(&DataSnapshot, &mut ProofMeter) -> Result<()>,
) -> Result<()> {
    recipe_history_body!(prepared, seed, seed_day, data, day, meter,
        [visit(seed, meter)?;], [protected_row(prepared, &frame, row, old, event.day, meter)?;],
        [visit(&frame, meter)?;], [visit(data, meter)], frame, row, old, event)
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
                    snapshot: canonical_digest(IdentityDomain::Data, snapshot)?,
                    proofs: BTreeMap::new(),
                    recipes: BTreeMap::new(),
                    epochs: vec![],
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
        registry.recipes.insert(
            id.clone(),
            OwnedExecutionRecipe::Initial(prepared.execution.clone()),
        );
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
    pub(super) fn for_transition(
        snapshot: &ProjectSnapshot,
        prepared: &PreparedOwnedTransitionV2,
        graph: Option<&DecisionGraph>,
    ) -> Result<Self> {
        prepared.check_basis(snapshot)?;
        let mut meter = ProofMeter::new(prepared.budget, prepared.cancelled.clone())?;
        let mut registry = owned_persist::reconstruct(snapshot, &mut meter)?;
        let execution = &prepared.execution;
        if execution.current() != snapshot {
            return Err(err("transition context changed its exact current basis"));
        }
        if let Some(next) = graph {
            owned_epoch::validate_transition_graph(snapshot, &execution.manifest, next, None)?;
        } else if execution.manifest.transition == ScopeTransition::Withdrawal {
            return Err(err("withdrawal replay requires its exact proposed graph"));
        }
        let id = revision(&execution.target)?;
        if registry
            .recipes
            .insert(
                id.clone(),
                OwnedExecutionRecipe::Transition(execution.clone()),
            )
            .is_some()
        {
            return Err(err("prepared transition aliases a retained producer"));
        }
        registry
            .manifests
            .insert(id.clone(), execution.manifest.clone());
        registry
            .initializations
            .insert(id.clone(), execution.initialization.clone());
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
    fn prepared(&self, target: &CapturedProgram) -> Result<Option<&OwnedExecutionRecipe>> {
        target.validate()?;
        if target.binding.project_id != self.snapshot.data.project_id {
            return Err(err("foreign replay source"));
        }
        if let Some(recipe) = self.registry.recipes.get(&revision(target)?) {
            if recipe.target() != target {
                return Err(err("owned source/provenance differs"));
            }
            Ok(Some(recipe))
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
        let Some(recipe) = self.prepared(source)? else {
            return Ok(if require_owned {
                ScopeApplicability::UnverifiedOwned
            } else {
                ScopeApplicability::Ordinary
            });
        };
        let same_lineage = self
            .prepared(target)?
            .is_some_and(|next| next.initial().metadata == recipe.initial().metadata);
        if !self.permits_target(target)? || !same_lineage || scope != &recipe.scope() {
            return Ok(ScopeApplicability::UnverifiedOwned);
        }
        Ok(ScopeApplicability::OwnedRoot(
            recipe.metadata().root.clone(),
        ))
    }
    pub(super) fn owns_decision(&self, decision: &ScopedDecision) -> bool {
        self.snapshot.decisions.decisions.contains(decision)
            && self.snapshot.owned_scope().is_ok_and(|scope| {
                scope
                    .rehearsals
                    .values()
                    .any(|receipt| receipt.witnesses.get(&decision.id) == Some(&decision.witness))
                    || scope
                        .adoptions
                        .iter()
                        .any(|receipt| receipt.decisions.contains(&decision.id))
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
        let Some(recipe) = self.prepared(target)? else {
            return Ok(OwnedBoundaryCoverage::default());
        };
        let mut meter = self.meter(cancelled)?;
        match recipe {
            OwnedExecutionRecipe::Initial(p) => {
                owned_coverage::derive(p, scope, contexts, &mut meter)
            }
            // A later compiler is not an original pure-create proof. Unsupported
            // boundary coverage stays Unknown rather than borrowing old syntax.
            OwnedExecutionRecipe::Transition(_) => Ok(OwnedBoundaryCoverage::default()),
        }
    }
    pub fn contains(&self, source: &CapturedProgram) -> bool {
        self.prepared(source).is_ok_and(|p| p.is_some())
    }
    fn verify_seed_with_meter(
        &self,
        target: &CapturedProgram,
        data: &DataSnapshot,
        day: i32,
        meter: &mut ProofMeter,
    ) -> Result<()> {
        let Some(recipe) = self.prepared(target)? else {
            data.validate()?;
            return Ok(());
        };
        let isolated = data == recipe.seed() && day == recipe.basis().clock_day;
        let current = target == self.snapshot.program()?
            && data == &self.snapshot.data
            && day == self.snapshot.clock_day;
        if !isolated && !current {
            return Err(err(
                "protected seed lacks its exact initializer or current transition frame",
            ));
        }
        if current {
            self.registry.validate_endpoint(
                &self.snapshot,
                data,
                day,
                self.snapshot.revision,
                meter,
            )
        } else if matches!(recipe, OwnedExecutionRecipe::Transition(_)) {
            self.registry.validate_historical_frame(
                &self.snapshot,
                recipe.basis().program()?,
                data,
                day,
                meter,
            )
        } else {
            validate_history(recipe, data, day, meter)
        }
    }
    pub fn verify_seed(
        &self,
        target: &CapturedProgram,
        data: &DataSnapshot,
        day: i32,
    ) -> Result<()> {
        let mut meter = self.meter(Arc::new(AtomicBool::new(false)))?;
        self.verify_seed_with_meter(target, data, day, &mut meter)
    }
    pub fn verify_state(
        &self,
        target: &CapturedProgram,
        data: &DataSnapshot,
        day: i32,
    ) -> Result<()> {
        let Some(recipe) = self.prepared(target)? else {
            data.validate()?;
            return Ok(());
        };
        let mut meter = self.meter(Arc::new(AtomicBool::new(false)))?;
        if target == self.snapshot.program()? && data.events.starts_with(&self.snapshot.data.events)
        {
            self.registry.validate_endpoint(
                &self.snapshot,
                &self.snapshot.data,
                self.snapshot.clock_day,
                self.snapshot.revision,
                &mut meter,
            )?;
            validate_recipe_history_from(
                recipe,
                &self.snapshot.data,
                self.snapshot.clock_day,
                data,
                day,
                &mut meter,
            )
        } else {
            if let OwnedExecutionRecipe::Transition(p) = recipe {
                self.registry.validate_historical_frame(
                    &self.snapshot,
                    p.current().program()?,
                    p.seed(),
                    p.current().clock_day,
                    &mut meter,
                )?;
            }
            validate_history(recipe, data, day, &mut meter)
        }
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
        let Some(recipe) = self.prepared(target)? else {
            if merged_data(target, original)? != *mapped {
                return Err(err("ordinary mapped seed differs"));
            }
            return Ok(original.clone());
        };
        match recipe {
            OwnedExecutionRecipe::Initial(p) => {
                if p.proof.snapshot.program()? != source
                    || p.proof.snapshot.data != *original
                    || p.proof.snapshot.clock_day != day
                    || original != mapped
                {
                    return Err(err(
                        "initial owned correspondence changed its original frame",
                    ));
                }
                Ok(p.initialized.clone())
            }
            OwnedExecutionRecipe::Transition(p) => {
                if p.current().program()? != source
                    || p.seed() != original
                    || original != mapped
                    || day != p.current().clock_day
                {
                    return Err(err(
                        "transition handoff changed its exact current identity frame",
                    ));
                }
                Ok(original.clone())
            }
        }
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
        let mut expected = original.clone();
        expected.seed = mapped.seed.clone();
        if expected != *mapped {
            return Err(err(
                "owned correspondence changed session/random/input/label frame",
            ));
        }
        if let Some(recipe) = self.prepared(target)? {
            if original.session != recipe.basis().session || original.random_seed != 0 {
                return Err(err(
                    "new owned experience changed its exact session/random frame",
                ));
            }
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
        let Some(recipe) = self.prepared(target)? else {
            return Ok(scenario.replay_operation_ids()?);
        };
        let mut original = scenario.clone();
        if scenario.seed == *recipe.seed()
            && scenario.session == recipe.basis().session
            && scenario.clock_day == recipe.basis().clock_day
            && scenario.random_seed == 0
        {
            if let OwnedExecutionRecipe::Initial(p) = recipe {
                original.seed = p.proof.snapshot.data.clone();
            }
        } else if target != self.snapshot.program()?
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
        let Some(current) = self.registry.recipes.get(&self.snapshot.active_revision) else {
            return Ok(result);
        };
        let original = current.initial();
        for row in self
            .snapshot
            .data
            .records
            .iter()
            .filter(|r| r.entity == current.metadata().root)
        {
            if !flag(row, &current.metadata().sealed)? {
                continue;
            }
            let initial = original.initialized.records.iter().any(|r| {
                r.entity == row.entity
                    && r.id == row.id
                    && flag(r, &original.metadata.sealed).unwrap_or(false)
            });
            let events: Vec<_> = self
                .snapshot
                .data
                .events
                .iter()
                .filter(|e| {
                    e.changes.iter().any(|c| {
                        c.entity == row.entity
                            && c.record == row.id
                            && c.before
                                .as_ref()
                                .and_then(|v| v.get(&current.metadata().sealed))
                                == Some(&DataValue::Boolean { value: false })
                            && c.after.get(&current.metadata().sealed)
                                == Some(&DataValue::Boolean { value: true })
                    })
                })
                .collect();
            if !initial && events.len() != 1 {
                return Err(err("completion has no unique genuine event"));
            }
            let event = if initial {
                None
            } else {
                events.first().copied()
            };
            let producer = if let Some(event) = event {
                let producers: Vec<_> = self
                    .registry
                    .epochs
                    .iter()
                    .filter_map(|epoch| self.registry.recipes.get(&epoch.output))
                    .filter(|recipe| {
                        recipe.target().artifact.program_digest == event.program
                            && row.values.get(&recipe.metadata().origin)
                                == Some(&DataValue::Text {
                                    value: recipe.metadata().identity.as_str().into(),
                                })
                    })
                    .collect();
                if producers.len() != 1 {
                    return Err(err("historical capture producer is missing or ambiguous"));
                }
                producers[0]
            } else {
                self.registry.recipes.values().find(|r| matches!(r, OwnedExecutionRecipe::Initial(p) if p.metadata == original.metadata))
                    .ok_or_else(|| err("original capture recipe missing"))?
            };
            let day = match row.values.get(&producer.metadata().day) {
                Some(DataValue::Date { days }) => *days,
                _ => return Err(err("sealed capture day missing")),
            };
            for (index, projection) in producer.projections().iter().enumerate() {
                if !producer.projection_present(projection) {
                    continue;
                }
                let key = producer
                    .metadata()
                    .saved
                    .get(index)
                    .ok_or_else(|| err("original capture slot missing"))?;
                let value = row
                    .values
                    .get(key)
                    .ok_or_else(|| err("saved value missing"))?;
                validate_value(value, &projection.host_value_type, 0)?;
                result.results.push(PreservedResult {
                    record: history::row_ref(row),
                    label: projection.location.clone(),
                    value: value.clone(),
                    day,
                    origin: if initial {
                        HistoryOrigin::CapturedAtAdoption
                    } else {
                        HistoryOrigin::ObservedAtCompletion
                    },
                    program: event.map(|e| e.program.clone()).unwrap_or(
                        original
                            .proof
                            .snapshot
                            .program()?
                            .artifact
                            .program_digest
                            .clone(),
                    ),
                    event: event.map(|e| e.id.clone()),
                });
            }
        }
        Ok(result)
    }
    pub fn begin_replay(
        &self,
        target: &CapturedProgram,
        scenario: &ScenarioSpec,
        graph: &DecisionGraph,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Box<dyn crate::product_runtime::ReplayAdmission>> {
        let historical = self.registry.recipes.values().any(|recipe| {
            let basis = recipe.basis();
            graph == &basis.decisions
                && scenario.session == basis.session
                && scenario.clock_day == basis.clock_day
                && scenario.random_seed == 0
                && ((target == recipe.target() && scenario.seed == *recipe.seed())
                    || (basis.program().is_ok_and(|p| p == target) && scenario.seed == basis.data))
        });
        if !self.permits_graph(graph) && !historical {
            return Err(err("replay graph differs from authenticated context"));
        }
        let mut meter = self.meter(cancelled)?;
        meter.charge(canonical_bytes(&(scenario, graph))?.len() as u64)?;
        self.verify_seed_with_meter(target, &scenario.seed, scenario.clock_day, &mut meter)?;
        let operations = self.operation_ids(target, scenario)?;
        if operations.len() != scenario.inputs.len() {
            return Err(err("incomplete replay operation IDs"));
        }
        Ok(Box::new(OwnedRunAdmission {
            context: self.clone(),
            target: target.clone(),
            scenario: scenario.clone(),
            graph: graph.clone(),
            initial_frame: (scenario.seed.clone(), scenario.clock_day),
            operations,
            meter: std::sync::Mutex::new(meter),
        }))
    }
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
            || !self.snapshot.decisions.decisions.contains(token.decision())
            || !graph.decisions.contains(token.decision())
        {
            return Err(err(
                "retained scene snapshot, graph, decision or target changed",
            ));
        }
        let recipe = self
            .prepared(target)?
            .ok_or_else(|| err("retained target recipe missing"))?;
        let original = token.scenario();
        original.validate(&token.program().program)?;
        meter.charge(canonical_bytes(&(original, graph))?.len() as u64)?;
        let (seed, operations) = if let Some(source_recipe) = self.prepared(token.program())? {
            if source_recipe.initial().metadata != recipe.initial().metadata {
                return Err(err(
                    "retained owned scene belongs to another original lineage",
                ));
            }
            self.registry.validate_historical_frame(
                &self.snapshot,
                token.program(),
                &original.seed,
                original.clock_day,
                &mut meter,
            )?;
            // Already-owned frames retain every protected cell and old producer.
            // Initial P4 scenarios derived operation IDs before initialization;
            // later transition scenarios use their exact protected identity seed.
            let mut operation_frame = original.clone();
            if let OwnedExecutionRecipe::Initial(initial) = source_recipe {
                if original.seed == initial.initialized
                    && original.session == initial.proof.snapshot.session
                    && original.clock_day == initial.proof.snapshot.clock_day
                    && original.random_seed == 0
                {
                    operation_frame.seed = initial.proof.snapshot.data.clone();
                }
            }
            (
                original.seed.clone(),
                operation_frame.replay_operation_ids()?,
            )
        } else {
            let initial = recipe.initial();
            if initial.proof.snapshot.program()? != token.program()
                || !initial
                    .proof
                    .snapshot
                    .decisions
                    .decisions
                    .contains(token.decision())
            {
                return Err(err(
                    "retained ordinary scene lacks exact original source/decision receipt",
                ));
            }
            meter.charge(
                (token.program().source_bytes.len() + initial.proof.candidate.source_bytes.len())
                    as u64,
            )?;
            let inventory = common_fact_inventory::derive(
                &initial.proof.snapshot,
                &initial.proof.candidate,
                &initial.proof.request.inventory,
            )?;
            let (certificates, projections, comparison) = common_fact_rules::verify_frame(
                &initial.proof.snapshot,
                &initial.proof.candidate,
                &initial.proof.request,
                &inventory,
                &original.seed,
                &mut meter,
            )?;
            if certificates != initial.proof.report.certificates
                || projections != initial.proof.report.projections
                || comparison != initial.proof.report.outcome_comparison
            {
                return Err(err(
                    "historical frame does not reproduce its original source proof",
                ));
            }
            let captures = common_fact_admission::capture_frame(
                token.program(),
                &original.seed,
                original.clock_day,
                &initial.proof.request,
                &projections,
                &mut meter,
            )?;
            // Only the original initializer may project an ordinary historical
            // clone. The actual target is never used as an initialization recipe.
            let seed = owned_preview::initialize_frame(
                &initial.proof,
                &initial.target,
                &initial.metadata,
                &original.seed,
                original.clock_day,
                &captures,
                &mut meter,
            )?;
            (seed, original.replay_operation_ids()?)
        };
        let mut actual = original.clone();
        actual.seed = seed.clone();
        actual.validate(&target.program)?;
        if operations.len() != actual.inputs.len() {
            return Err(err("historical operation vector changed"));
        }
        let admission = OwnedRunAdmission {
            context: self.clone(),
            target: target.clone(),
            scenario: actual.clone(),
            graph: graph.clone(),
            initial_frame: (seed, original.clock_day),
            operations,
            meter: std::sync::Mutex::new(meter),
        };
        Ok((actual, Box::new(admission)))
    }
}
/// This run owns the same live meter used for historical authentication. The
/// isolated suffix uses original replay operation IDs, never durable receipts.
struct OwnedRunAdmission {
    context: OwnedExecutionContext,
    target: CapturedProgram,
    scenario: ScenarioSpec,
    graph: DecisionGraph,
    initial_frame: (DataSnapshot, i32),
    operations: Vec<Id>,
    meter: std::sync::Mutex<ProofMeter>,
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
                "Owned replay retained frame changed".into(),
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
                "Owned replay input frame changed".into(),
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
                .map_err(|_| err("owned replay lock failed"))?;
            meter.charge(1)?;
            if let Some(recipe) = self.context.prepared(target)? {
                validate_recipe_history_from(
                    recipe,
                    &self.initial_frame.0,
                    self.initial_frame.1,
                    data,
                    day,
                    &mut meter,
                )?;
            } else {
                data.validate()?;
            }
            Ok(())
        })();
        checked.map_err(|e: StoreError| AdapterError::Unsupported(e.to_string()))
    }
}
