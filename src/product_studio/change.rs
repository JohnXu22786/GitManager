//! One frozen current/prospective experience. All trials are copied local work;
//! only DecisionEngine can issue an adoption accepted by the live store.
use super::*;
use crate::product_decisions::{
    accept_scene, AcceptedScene, Choice, DecisionEngine, IntentArchive, IntentionBinding,
    VerifiedChange,
};
use crate::product_runtime::ReplayAdmission;
use crate::product_store::scope::*;
use std::collections::BTreeSet;

#[derive(Clone)]
pub(super) struct LifecycleOption {
    pub record: RecordRef,
    pub field: Id,
    pub label: String,
}
#[derive(Clone)]
pub(super) struct ChangeView {
    pub basis: Basis,
    pub current: RuntimeView,
    pub trial_day: i32,
    pub target_artifact: Digest,
    pub alternative: Option<RuntimeView>,
    pub population: ScopePopulation,
    pub missing: Vec<(Id, String)>,
    pub lifecycle_options: Vec<LifecycleOption>,
    pub can_accept: bool,
    pub can_keep_current: bool,
    pub can_retain: bool,
    pub same_alternative: bool,
    pub readiness_notes: Vec<String>,
    pub whole_design: bool,
    pub wording_update: bool,
    pub needs_task: bool,
    pub partial_scope: bool,
    pub scope_rows: Vec<Record>,
    pub operations: Vec<String>,
    pub unavailable_tasks: Vec<String>,
    pub need: String,
    pub origin: String,
    pub lifecycle_note: String,
}
#[derive(Clone)]
pub(super) struct ChangeDraft {
    pub snapshot: ProjectSnapshot,
    pub candidate: CapturedProgram,
    pub request: Option<DevelopmentRequest>,
    pub result: Option<DevelopmentResult>,
    pub raw: Option<Vec<u8>>,
    pub receipt: Option<JobReceipt>,
    pub need: String,
    pub resolves: Vec<Id>,
    pub operation: Id,
    pub population: ScopePopulation,
    pub lifecycles: Vec<LifecycleBinding>,
    pub lifecycle_note: String,
    pub prepared: Option<PreparedScopedChange>,
    structural: bool,
    equivalent: bool,
    lifecycle_evidence: String,
    analysis: change_adapter::Analysis,
    scope_operations: BTreeSet<Id>,
    // Keep copied record allocation anchored to the committed business frame.
    // Prepared metadata belongs only to independently projected replay scenes.
    scenario: ScenarioSpec,
    current: RuntimeView,
    trial_day: i32,
    alternative: Option<RuntimeView>,
    scenes: Option<(AcceptedScene, AcceptedScene)>,
}
fn engine(store: &ProductStore, gate: &Gate) -> DecisionEngine<LocalRuntime> {
    DecisionEngine::new(
        LocalRuntime::with_cancellation(gate.cancelled.clone()),
        IntentArchive::new(store.clone()),
    )
}
fn operation(program: &AppDefinition, input: &SemanticInput) -> Option<Id> {
    match input {
        SemanticInput::Invoke { action, .. } => Some(action.clone()),
        SemanticInput::Submit { view, .. } => {
            program.views.iter().find(|v| &v.id == view).and_then(|v| {
                if let ViewKind::Form { action, .. } = &v.kind {
                    Some(action.clone())
                } else {
                    None
                }
            })
        }
        SemanticInput::Activate { view, binding, .. } => program
            .views
            .iter()
            .find(|v| &v.id == view)
            .and_then(|v| v.actions.iter().find(|a| &a.id == binding))
            .map(|a| a.action.clone()),
        SemanticInput::Control { view, control, .. } => program
            .views
            .iter()
            .find(|v| &v.id == view)
            .and_then(|v| match &v.kind {
                ViewKind::List {
                    controls,
                    selection,
                    ..
                } => controls
                    .iter()
                    .find(|c| &c.id == control)
                    .and_then(|c| c.on_change.clone())
                    .or_else(|| {
                        selection
                            .as_ref()
                            .filter(|s| &s.id == control)
                            .and_then(|s| s.on_change.clone())
                    }),
                _ => None,
            }),
        _ => None,
    }
}
fn exposed_operations(program: &AppDefinition) -> BTreeSet<Id> {
    let mut operations = BTreeSet::new();
    for view in &program.views {
        operations.extend(view.actions.iter().map(|binding| binding.action.clone()));
        match &view.kind {
            ViewKind::Form { action, .. } => {
                operations.insert(action.clone());
            }
            ViewKind::List {
                controls,
                selection,
                ..
            } => {
                operations.extend(
                    controls
                        .iter()
                        .filter_map(|control| control.on_change.clone()),
                );
                operations.extend(
                    selection
                        .iter()
                        .filter_map(|selection| selection.on_change.clone()),
                );
            }
            ViewKind::Detail { .. } => {}
        }
    }
    operations
}
fn replay_view(
    runtime: &LocalRuntime,
    program: &CapturedProgram,
    scenario: &ScenarioSpec,
    admission: &ScopedExecutionContext,
) -> Result<(RuntimeView, i32), String> {
    admission
        .validate_seed(program, &scenario.seed, scenario.clock_day)
        .map_err(error)?;
    let mut run = runtime
        .start(
            program,
            &scenario.seed,
            &scenario.session,
            scenario.clock_day,
            scenario.random_seed,
            RuntimeLimits::default(),
        )
        .map_err(error)?;
    let operations = admission
        .replay_operation_ids(program, scenario)
        .map_err(error)?;
    for (input, operation) in scenario.inputs.iter().zip(operations) {
        runtime.apply(&mut run, input, &operation).map_err(error)?;
        admission
            .validate_state(program, runtime.data(&run), run.clock_day())
            .map_err(error)?;
    }
    // This is the raw isolated execution. Saved facts are separately shown in
    // the daily ProductStore::runtime_view, never invented in a trial view.
    Ok((runtime.view_model(&run).map_err(error)?, run.clock_day()))
}

impl ChangeDraft {
    pub fn new(
        snapshot: ProjectSnapshot,
        candidate: CapturedProgram,
        need: String,
    ) -> Result<Self, String> {
        let analysis = change_adapter::analyze(&snapshot, &candidate)?;
        let structural = analysis.structural || analysis.patches.is_empty();
        let equivalent = change_adapter::baseline(&snapshot)?
            .artifact
            .semantic_digest
            == candidate.artifact.semantic_digest;
        let runtime = LocalRuntime::default();
        let scenario = ScenarioSpec {
            version: CONTRACT_VERSION,
            id: id("trial"),
            label: "Try the requested change on copied work".into(),
            seed: snapshot.data.clone(),
            session: snapshot.session.clone(),
            clock_day: snapshot.clock_day,
            random_seed: 0,
            inputs: vec![],
            validity: vec![],
        };
        let (current, trial_day) = replay_view(
            &runtime,
            snapshot.program().map_err(error)?,
            &scenario,
            &ScopedExecutionContext::committed(&snapshot).map_err(error)?,
        )?;
        let lifecycles = change_adapter::retained_lifecycles(&snapshot)?;
        Ok(Self {
            snapshot,
            candidate,
            request: None,
            result: None,
            raw: None,
            receipt: None,
            need,
            resolves: vec![],
            operation: id("change"),
            population: ScopePopulation::All,
            lifecycles,
            lifecycle_note: String::new(),
            prepared: None,
            structural,
            equivalent,
            lifecycle_evidence: String::new(),
            analysis,
            scope_operations: BTreeSet::new(),
            scenario,
            current,
            trial_day,
            alternative: None,
            scenes: None,
        })
    }
    pub fn missing(&self) -> Vec<(Id, String)> {
        if self.structural {
            return vec![];
        }
        self.analysis
            .entities
            .iter()
            .filter(|e| !self.lifecycles.iter().any(|l| &l.entity == *e))
            .map(|e| {
                (
                    e.clone(),
                    self.current
                        .program
                        .entities
                        .iter()
                        .find(|entity| &entity.id == e)
                        .map(|e| e.label.clone())
                        .unwrap_or_else(|| e.clone()),
                )
            })
            .collect()
    }
    pub fn prepare(&mut self, store: &ProductStore, gate: &Gate) -> Result<(), String> {
        gate.check()?;
        self.check(store)?;
        if !self.missing().is_empty()
            || (!self.structural
                && self.analysis.operations.is_empty()
                && self.scope_operations.is_empty())
        {
            return Ok(());
        }
        let mut prepared = if self.structural {
            if self.population != ScopePopulation::All {
                return Err("This changes the tool's design. A partial rule scope cannot describe this design change".into());
            }
            if self
                .snapshot
                .editable_scope_context()
                .map_err(error)?
                .is_some()
            {
                Some(
                    store
                        .prepare_managed_evolution(
                            &self.candidate,
                            &change_adapter::slot_mappings(&self.snapshot, &self.candidate)?,
                            &self.operation,
                        )
                        .map_err(error)?,
                )
            } else {
                None
            }
        } else {
            let request = change_adapter::request(
                &self.snapshot,
                &self.candidate,
                self.population.clone(),
                self.lifecycles.clone(),
                self.scope_operations.clone(),
            )?;
            Some(
                store
                    .prepare_scoped_change(&self.candidate, &request, &self.operation)
                    .map_err(error)?,
            )
        };
        let mut checker = engine(store, gate);
        let report = if let Some(p) = &prepared {
            checker.check_prepared_discovery_candidate(
                &self.snapshot,
                p.target(),
                p,
                &[],
                RuntimeLimits::default(),
            )
        } else {
            checker.check_discovery_candidate(
                &self.snapshot,
                &self.candidate,
                &[],
                RuntimeLimits::default(),
            )
        }
        .map_err(error)?;
        if report.disposition != crate::product_decisions::CheckDisposition::Ready {
            return Err("This proposed design does not yet preserve the saved intentions. It needs repair before it can be offered as an acceptable alternative".into());
        }
        let target = prepared
            .as_ref()
            .map(|p| p.target())
            .unwrap_or(&self.candidate);
        // A structurally different session needs a separate supported common
        // scenario mapping. Never drop selection/state silently to make it fit.
        self.snapshot.session.validate(&target.program).map_err(|_|"The new design needs a compatible shared working state before these two versions can be tried together".to_string())?;
        let mut scenario = self.scenario.clone();
        scenario.inputs.clear();
        scenario.id = id("trial");
        scenario.seed = self.snapshot.data.clone();
        let (admission, replay) = if let Some(p) = &prepared {
            let (bound, context, actual) = prepared_replay(&self.snapshot, p, &scenario)?;
            prepared = Some(bound);
            (context, actual)
        } else {
            (
                ScopedExecutionContext::committed(&self.snapshot).map_err(error)?,
                scenario.clone(),
            )
        };
        let target = prepared
            .as_ref()
            .map(|p| p.target())
            .unwrap_or(&self.candidate);
        let runtime = LocalRuntime::with_cancellation(gate.cancelled.clone());
        let (current, trial_day) = replay_view(
            &runtime,
            self.snapshot.program().map_err(error)?,
            &replay,
            &admission,
        )?;
        let (alternative, alternative_day) = replay_view(&runtime, target, &replay, &admission)?;
        if trial_day != alternative_day {
            return Err("The copied versions did not keep the same simulated date. Start a fresh comparison".into());
        }
        gate.check()?;
        self.prepared = prepared;
        self.scenario = scenario;
        self.current = current;
        self.trial_day = trial_day;
        self.alternative = Some(alternative);
        self.scenes = None;
        Ok(())
    }
    pub fn check(&self, store: &ProductStore) -> Result<(), String> {
        if store.load().map_err(error)? != self.snapshot {
            return Err("Saved work changed after this comparison began. Return to current work and rehearse a fresh comparison".into());
        }
        Ok(())
    }
    pub fn set_scope(
        &mut self,
        store: &ProductStore,
        population: ScopePopulation,
        gate: &Gate,
    ) -> Result<(), String> {
        // Reconstruct on a temporary draft so a failed preparation never leaves
        // old evidence paired with a newly selected scope.
        let old = self.population.clone();
        let operation = self.operation.clone();
        self.population = population;
        self.operation = id("change");
        match self.prepare(store, gate) {
            Ok(()) => Ok(()),
            Err(e) => {
                self.population = old;
                self.operation = operation;
                Err(e)
            }
        }
    }
    pub fn clarify(
        &mut self,
        store: &ProductStore,
        record: &RecordRef,
        field: &str,
        gate: &Gate,
    ) -> Result<(), String> {
        self.check(store)?;
        if !self.missing().iter().any(|(e, _)| e == &record.entity) {
            return Err("This collection already has an established finished-work meaning".into());
        }
        let row = self
            .current
            .retained_records
            .iter()
            .find(|r| r.entity == record.entity && r.id == record.record)
            .ok_or("That copied example is no longer visible")?;
        let definition = self
            .current
            .program
            .entities
            .iter()
            .find(|e| e.id == record.entity)
            .and_then(|e| e.fields.iter().find(|f| f.id == field))
            .ok_or("That business outcome is not in the current tool")?;
        if !matches!(
            definition.value_type,
            Type::Boolean | Type::Integer | Type::Text | Type::Date
        ) {
            return Err("Choose a simple recorded outcome to define finished work".into());
        }
        let value = row
            .values
            .get(field)
            .ok_or("That example has no recorded outcome")?
            .clone();
        let completed = Expr::Equal {
            left: Box::new(Expr::Field {
                record: Box::new(Expr::Variable {
                    name: "record".into(),
                }),
                field: field.into(),
            }),
            right: Box::new(Expr::Literal {
                value_type: definition.value_type.clone(),
                value: value.clone(),
            }),
        };
        let trace = serde_json::to_string(&(record, field, &value, &self.scenario.inputs))
            .map_err(error)?;
        if self.lifecycle_evidence.len() + trace.len() + 512 > MAX_TEXT_BYTES {
            return Err("The completion demonstration is too long to retain. Restart with a shorter copied example".into());
        }
        self.lifecycle_evidence
            .push_str(&format!("Completion demonstration: {trace}. "));
        self.lifecycles.push(LifecycleBinding {
            entity: record.entity.clone(),
            completed,
            source: self.snapshot.active_revision.clone(),
        });
        self.lifecycle_note.push_str(&format!(
            "Finished when {} is {} (chosen from a copied record). ",
            definition.label,
            crate::ui::product_runtime_view::value_text(&value)
        ));
        self.prepare(store, gate)
    }
    pub fn no_finished_state(
        &mut self,
        store: &ProductStore,
        entity: &str,
        gate: &Gate,
    ) -> Result<(), String> {
        self.check(store)?;
        if !self.missing().iter().any(|(id, _)| id == entity) {
            return Err("That collection already has a finished-work meaning".into());
        }
        self.lifecycles.push(LifecycleBinding {
            entity: entity.into(),
            completed: Expr::Literal {
                value_type: Type::Boolean,
                value: DataValue::Boolean { value: false },
            },
            source: self.snapshot.active_revision.clone(),
        });
        self.lifecycle_note
            .push_str("You explicitly said this collection has no finished state. ");
        self.prepare(store, gate)
    }
    pub fn trial(
        &mut self,
        store: &ProductStore,
        input: SemanticInput,
        gate: &Gate,
    ) -> Result<(), String> {
        self.check(store)?;
        gate.check()?;
        if self.scenario.inputs.len() >= MAX_ITEMS - 1 {
            return Err(
                "This comparison has reached its action limit. Restart the copied trial".into(),
            );
        }
        let mut scenario = self.scenario.clone();
        scenario.inputs.push(input);
        let runtime = LocalRuntime::with_cancellation(gate.cancelled.clone());
        if !self.missing().is_empty() {
            let admission = ScopedExecutionContext::committed(&self.snapshot).map_err(error)?;
            let (current, trial_day) = replay_view(
                &runtime,
                self.snapshot.program().map_err(error)?,
                &scenario,
                &admission,
            )?;
            gate.check()?;
            self.scenario = scenario;
            self.current = current;
            self.trial_day = trial_day;
            return Ok(());
        }
        if !self.structural && self.analysis.operations.is_empty() {
            let operations: BTreeSet<_> = scenario
                .inputs
                .iter()
                .filter_map(|input| operation(&self.candidate.program, input))
                .collect();
            if operations.is_empty() {
                let admission = ScopedExecutionContext::committed(&self.snapshot).map_err(error)?;
                let (current, trial_day) = replay_view(
                    &runtime,
                    self.snapshot.program().map_err(error)?,
                    &scenario,
                    &admission,
                )?;
                gate.check()?;
                self.scenario = scenario;
                self.current = current;
                self.trial_day = trial_day;
                return Ok(());
            }
            if self.scope_operations != operations || self.alternative.is_none() {
                let inputs = scenario.inputs.clone();
                self.scope_operations = operations;
                self.operation = id("change");
                self.prepare(store, gate)?;
                scenario = self.scenario.clone();
                scenario.inputs = inputs;
            }
        }
        if self.alternative.is_none() {
            return Err("Prepare a compatible comparison before trying this change".into());
        }
        // Observation is explicitly part of the exact independently replayed
        // scene; keep the editable trace without accumulating hidden observes.
        let mut proof = scenario.clone();
        proof.inputs.push(SemanticInput::Observe {
            point: "result".into(),
        });
        let (prepared, admission, proof) = if let Some(p) = &self.prepared {
            let (bound, context, actual) = prepared_replay(&self.snapshot, p, &proof)?;
            (Some(bound), context, actual)
        } else {
            (
                None,
                ScopedExecutionContext::committed(&self.snapshot).map_err(error)?,
                proof,
            )
        };
        let target = prepared
            .as_ref()
            .map(|p| p.target())
            .unwrap_or(&self.candidate);
        let engine = engine(store, gate);
        let current_scene = if let Some(p) = &prepared {
            engine.accept_prepared_current_scene(store, p, &proof, Disclosure::ExplicitlySelected)
        } else {
            engine.accept_current_scene(&self.snapshot, &proof, Disclosure::ExplicitlySelected)
        }
        .map_err(error)?;
        let alternative_scene = if let Some(p) = &prepared {
            engine.accept_scoped_scene(store, p, &proof, Disclosure::ExplicitlySelected)
        } else {
            accept_scene(
                &runtime,
                target,
                &proof,
                Disclosure::ExplicitlySelected,
                RuntimeLimits::default(),
            )
        }
        .map_err(error)?;
        let (current, trial_day) = replay_view(
            &runtime,
            self.snapshot.program().map_err(error)?,
            current_scene.scenario(),
            &admission,
        )?;
        let (alternative, alternative_day) =
            replay_view(&runtime, target, alternative_scene.scenario(), &admission)?;
        if trial_day != alternative_day {
            return Err("The copied versions did not keep the same simulated date. Start a fresh comparison".into());
        }
        gate.check()?;
        self.prepared = prepared;
        self.scenario = scenario;
        self.current = current;
        self.trial_day = trial_day;
        self.alternative = Some(alternative);
        self.scenes = Some((current_scene, alternative_scene));
        Ok(())
    }
    pub fn view(&self) -> Result<ChangeView, String> {
        let missing = self.missing();
        let mut options = vec![];
        for record in &self.current.retained_records {
            if !missing.iter().any(|(entity, _)| entity == &record.entity) {
                continue;
            }
            if let Some(entity) = self
                .current
                .program
                .entities
                .iter()
                .find(|e| e.id == record.entity)
            {
                let name = entity
                    .fields
                    .iter()
                    .find_map(|f| match record.values.get(&f.id) {
                        Some(DataValue::Text { value }) => Some(value.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| entity.label.clone());
                for field in &entity.fields {
                    if matches!(
                        field.value_type,
                        Type::Boolean | Type::Integer | Type::Text | Type::Date
                    ) {
                        if let Some(value) = record.values.get(&field.id) {
                            options.push(LifecycleOption {
                                record: RecordRef {
                                    entity: record.entity.clone(),
                                    record: record.id.clone(),
                                },
                                field: field.id.clone(),
                                label: format!(
                                    "{name}: finished when {} is {}",
                                    field.label,
                                    crate::ui::product_runtime_view::value_text(value)
                                ),
                            });
                        }
                    }
                }
            }
        }
        let required = self
            .prepared
            .as_ref()
            .map(|p| p.scope().operations.clone())
            .unwrap_or_else(|| {
                self.scenario
                    .inputs
                    .iter()
                    .filter_map(|i| operation(&self.candidate.program, i))
                    .collect()
            });
        let experienced: BTreeSet<_> = self
            .scenario
            .inputs
            .iter()
            .filter_map(|i| operation(&self.candidate.program, i))
            .collect();
        let operations = required
            .iter()
            .map(|id| {
                self.candidate
                    .program
                    .actions
                    .iter()
                    .find(|a| &a.id == id)
                    .map(|a| a.label.clone())
                    .unwrap_or_else(|| id.clone())
            })
            .collect();
        let mut scope_rows = vec![];
        if !self.structural && missing.is_empty() {
            let runtime = LocalRuntime::default();
            for row in &self.snapshot.data.records {
                if row.archived || !self.analysis.entities.contains(&row.entity) {
                    continue;
                }
                let Some(lifecycle) = self.lifecycles.iter().find(|l| l.entity == row.entity)
                else {
                    continue;
                };
                let reference = RecordRef {
                    entity: row.entity.clone(),
                    record: row.id.clone(),
                };
                let completed = runtime
                    .evaluate_record_projection(
                        self.snapshot.program().map_err(error)?,
                        &self.snapshot.data,
                        &reference,
                        "record",
                        &lifecycle.completed,
                        &Type::Boolean,
                        self.snapshot.clock_day,
                    )
                    .map_err(error)?;
                if completed == (DataValue::Boolean { value: false }) {
                    scope_rows.push(row.clone());
                }
            }
        }
        let origin = match &self.candidate.binding.producer {
            Producer::Fixture {name}=>format!("Synthetic provider fixture: {name}. Copied work is executed locally."),
            Producer::LiveAgent {provider,invocation_id,..}=>format!("Actual returned source from {provider}, request {invocation_id}. The scoped executable is a separate host-compiled source."),
            _=>"Exact retained authored source; this is a fresh rehearsal of the saved choice.".into(),
        };
        // Revisiting a pending choice is an explicit resolution, even when a
        // separate adoption has since made its alternative match current work.
        let wording_update = self.equivalent && self.resolves.is_empty();
        // The language permits internal actions without generated controls. Do
        // not ask a person to complete an impossible acceptance checklist. This
        // detects absent routes, not a claim that dynamic guards will succeed;
        // the actual checked rehearsal remains required for every scoped task.
        let unavailable_tasks = self
            .alternative
            .as_ref()
            .filter(|_| !wording_update)
            .map(|alternative| {
                let current = exposed_operations(&self.current.program);
                let prospective = exposed_operations(&alternative.program);
                required
                    .iter()
                    .filter(|task| !current.contains(*task) || !prospective.contains(*task))
                    .map(|task| {
                        self.candidate
                            .program
                            .actions
                            .iter()
                            .find(|action| &action.id == task)
                            .map(|action| action.label.clone())
                            .unwrap_or_else(|| "A required task".into())
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let experienced_scope = self.scenes.is_some()
            && !required.is_empty()
            && required.is_subset(&experienced)
            && unavailable_tasks.is_empty();
        let target = self
            .prepared
            .as_ref()
            .map(|p| p.target())
            .unwrap_or(&self.candidate);
        let same_alternative = self.alternative.is_some()
            && target.artifact == self.snapshot.program().map_err(error)?.artifact;
        Ok(ChangeView {
            basis: Basis::capture(&self.snapshot)?,
            current: self.current.clone(),
            trial_day: self.trial_day,
            target_artifact: self
                .prepared
                .as_ref()
                .map(|p| p.target())
                .unwrap_or(&self.candidate)
                .artifact
                .program_digest
                .clone(),
            alternative: self.alternative.clone(),
            population: self.population.clone(),
            missing,
            lifecycle_options: options,
            can_accept: (wording_update && self.alternative.is_some()) || experienced_scope,
            can_keep_current: experienced_scope && !wording_update,
            can_retain: self.scenes.is_some()
                && !required.is_empty()
                && !same_alternative
                && !wording_update,
            same_alternative,
            readiness_notes: vec![],
            whole_design: self.structural,
            wording_update,
            needs_task: !wording_update && required.is_empty(),
            partial_scope: !self.structural
                && !self.analysis.patches.iter().any(|p| {
                    matches!(
                        p.destination,
                        EffectDestination::Observable { .. } | EffectDestination::EmitItems { .. }
                    )
                }),
            scope_rows,
            operations,
            unavailable_tasks,
            need: self.need.clone(),
            origin,
            lifecycle_note: self.lifecycle_note.clone(),
        })
    }
    /// Active promises must pass the same preparation used at the actual click.
    /// This runs only on the worker, after copied tasks have been experienced.
    /// Staged proof objects are not adoption receipts; the final commit still
    /// performs its own fresh preparation and source/data/day fencing.
    pub fn checked_view(&self, store: &ProductStore, gate: &Gate) -> Result<ChangeView, String> {
        let mut view = self.view()?;
        for (outcome, ready, note) in [
            (DecisionOutcome::Accept { artifact: view.target_artifact.clone() }, &mut view.can_accept,
             "This copied example has not verified the full chosen scope for accepting the change. Try the affected work or return to saved work."),
            (DecisionOutcome::KeepCurrent, &mut view.can_keep_current,
             "This copied example has not verified the full chosen scope for recording Keep current. Returning to saved work leaves the existing tool and earlier choices unchanged."),
        ] {
            if *ready {
                if let Err(error) = self.decision(store, outcome, &id("readiness"), gate) {
                    *ready = false;
                    view.readiness_notes.push(note.into());
                    #[cfg(test)]
                    eprintln!("Checked choice preparation: {error}");
                    #[cfg(not(test))]
                    let _ = error;
                }
            }
        }
        gate.check()?;
        self.check(store)?;
        Ok(view)
    }
    pub fn decision(
        &self,
        store: &ProductStore,
        outcome: DecisionOutcome,
        recording: &str,
        gate: &Gate,
    ) -> Result<VerifiedChange, String> {
        self.check(store)?;
        gate.check()?;
        if let (Some(request), Some(result), Some(raw), Some(receipt)) =
            (&self.request, &self.result, &self.raw, &self.receipt)
        {
            result.validate_for(request).map_err(error)?;
            if DevelopmentResponse::parse(raw).map_err(error)? != result.response
                || receipt.state != JobState::TransportValidated
            {
                return Err("The returned source receipt changed".into());
            }
            if !result.response.candidates.iter().any(|c| {
                CapturedProgram::capture(
                    c.source_json.as_bytes(),
                    &request.project_id,
                    result.producer.clone(),
                    None,
                )
                .ok()
                .as_ref()
                    == Some(&self.candidate)
            }) {
                return Err("The experienced source is not the actual provider result".into());
            }
        }
        if let DecisionOutcome::Accept { artifact } = &outcome {
            let target = self
                .prepared
                .as_ref()
                .map(|p| p.target())
                .unwrap_or(&self.candidate);
            if artifact != &target.artifact.program_digest {
                return Err("The accepted artifact is not the exact displayed alternative. Rehearse the current comparison".into());
            }
        }
        if self.equivalent
            && matches!(outcome, DecisionOutcome::Accept { .. })
            && self.resolves.is_empty()
        {
            let engine = engine(store, gate);
            return if let Some(prepared) = &self.prepared {
                engine.prepare_managed_change(store, prepared.clone(), &[], &self.operation)
            } else {
                engine.prepare_change(store, &self.candidate, &[], &self.operation)
            }
            .map_err(error);
        }
        let (mut current, alternative) = self
            .scenes
            .clone()
            .ok_or("Try the actual copied alternatives before recording a choice")?;
        let view = self.view()?;
        if outcome == DecisionOutcome::KeepCurrent && !view.can_keep_current {
            return Err("Try every affected task through the available controls before recording Keep current. Returning to saved work leaves earlier choices unchanged".into());
        }
        if !matches!(
            outcome,
            DecisionOutcome::Accept { .. } | DecisionOutcome::KeepCurrent
        ) && !view.can_retain
        {
            return Err("Try an actual business task with two distinct tool versions before recording a new pair. Returning to saved work keeps earlier choices".into());
        }
        let engine = engine(store, gate);
        if outcome == DecisionOutcome::KeepCurrent && self.prepared.is_some() {
            // Keep the original evidence intact. A current-only retained promise
            // gets its own fresh replay on the exact committed starting frame.
            let mut original = current.scenario().clone();
            original.seed = self.snapshot.data.clone();
            let replay = engine
                .accept_current_scene(&self.snapshot, &original, Disclosure::ExplicitlySelected)
                .map_err(error)?;
            let same = current.observations().len() == replay.observations().len()
                && current
                    .observations()
                    .iter()
                    .zip(replay.observations())
                    .all(|(a, b)| {
                        a.point == b.point
                            && a.session_digest == b.session_digest
                            && a.values == b.values
                            && a.value_types == b.value_types
                            && a.view == b.view
                            && a.view_schema == b.view_schema
                            && a.outputs == b.outputs
                    });
            if !same {
                return Err("The committed-current replay differs from the current result you experienced. Start a fresh comparison before keeping it".into());
            }
            current = replay;
        }
        let accepting = matches!(outcome, DecisionOutcome::Accept { .. });
        let scope = if let Some(p) = &self.prepared {
            p.scope().clone()
        } else {
            DecisionScope {
                operations: self
                    .scenario
                    .inputs
                    .iter()
                    .filter_map(|i| operation(&self.candidate.program, i))
                    .collect(),
                population: Population::All,
                conditions: Values::new(),
                excluded_records: vec![],
                unknowns: vec![],
            }
        };
        let choice = Choice {
            id: id("intention"),
            request: self.need.clone(),
            rationale: (!self.lifecycle_note.is_empty())
                .then(|| format!("{} {}", self.lifecycle_note, self.lifecycle_evidence)),
            scope,
            outcome: outcome.clone(),
            obligations: vec![],
            binding: IntentionBinding::ObservedOutcome,
        };
        if accepting {
            if !self.view()?.can_accept {
                return Err("Try each affected task before accepting its result".into());
            }
            if let Some(prepared) = &self.prepared {
                if self.resolves.is_empty() {
                    engine.prepare_scoped_choice(
                        store,
                        prepared.clone(),
                        choice,
                        vec![alternative],
                        &self.operation,
                    )
                } else {
                    engine.prepare_scoped_resolution(
                        store,
                        prepared.clone(),
                        choice,
                        vec![alternative],
                        &self.resolves,
                        &self.operation,
                    )
                }
                .map_err(error)
            } else if self.resolves.is_empty() {
                engine
                    .prepare_choice(
                        store,
                        &self.candidate,
                        choice,
                        vec![alternative],
                        &self.operation,
                    )
                    .map_err(error)
            } else {
                engine
                    .prepare_resolution(
                        store,
                        &self.candidate,
                        choice,
                        vec![alternative],
                        &self.resolves,
                        &self.operation,
                    )
                    .map_err(error)
            }
        } else if outcome == DecisionOutcome::KeepCurrent {
            if self.resolves.is_empty() {
                engine.prepare_choice(
                    store,
                    self.snapshot.program().map_err(error)?,
                    choice,
                    vec![current],
                    recording,
                )
            } else {
                engine.prepare_resolution(
                    store,
                    self.snapshot.program().map_err(error)?,
                    choice,
                    vec![current],
                    &self.resolves,
                    recording,
                )
            }
            .map_err(error)
        } else if let Some(prepared) = &self.prepared {
            engine
                .prepare_rehearsed_choice(
                    store,
                    prepared.clone(),
                    choice,
                    vec![current, alternative],
                    &self.resolves,
                    recording,
                )
                .map_err(error)
        } else if self.resolves.is_empty() {
            engine
                .prepare_choice(
                    store,
                    self.snapshot.program().map_err(error)?,
                    choice,
                    vec![current, alternative],
                    recording,
                )
                .map_err(error)
        } else {
            engine
                .prepare_resolution(
                    store,
                    self.snapshot.program().map_err(error)?,
                    choice,
                    vec![current, alternative],
                    &self.resolves,
                    recording,
                )
                .map_err(error)
        }
    }
}
