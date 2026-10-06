//! Production interpreter for the closed, typed local-application language.
//! Runs own copied state. Only the project store/controller can commit it.
mod compatibility;
mod eval;
mod execute;
mod view;

use crate::product_contract::*;
use crate::product_protocol::RuntimeView;
use eval::{Env, Eval};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Instant;

pub const RUNTIME_VERSION: &str = "local-interpreter/3";
pub const DRIVER_VERSION: &str = "semantic-input/3";
type Result<T> = std::result::Result<T, AdapterError>;
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

#[derive(Clone, Default)]
pub struct LocalRuntime {
    cancelled: Arc<AtomicBool>,
}
#[derive(Clone)]
struct State {
    data: DataSnapshot,
    session: SessionState,
    day: i32,
    outputs: Vec<LocalArtifact>,
}
/// An isolated execution copy. Accessors deliberately do not expose mutation.
pub struct ProductRun {
    program: CapturedProgram,
    state: State,
    seed: u64,
    limits: RuntimeLimits,
    fuel: Cell<u64>,
    trace: Vec<TraceStep>,
    observations: Vec<Observation>,
    receipts: BTreeMap<Id, (Digest, TraceStep)>,
}
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
struct Meter {
    limits: RuntimeLimits,
    fuel: u64,
    steps: usize,
    writes: usize,
    started: Instant,
    cancelled: Arc<AtomicBool>,
}
impl Meter {
    fn new(limits: RuntimeLimits, fuel: u64, cancelled: Arc<AtomicBool>) -> Self {
        Self {
            limits,
            fuel,
            steps: 0,
            writes: 0,
            started: Instant::now(),
            cancelled,
        }
    }
    fn tick(&mut self, cost: u64) -> Result<()> {
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
}
impl LocalRuntime {
    pub fn with_cancellation(cancelled: Arc<AtomicBool>) -> Self {
        Self { cancelled }
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
        program.validate()?;
        data.validate()?;
        validate_day(day)?;
        if data.project_id != program.binding.project_id || !valid_id(binding) {
            return Err(invalid("projection project or binding mismatch"));
        }
        let value = DataValue::Reference {
            entity: row.entity.clone(),
            record: row.record.clone(),
        };
        eval::record(data, &value)?;
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
        let session = SessionState::initial(&program.program)?;
        let limits = RuntimeLimits::default();
        let mut meter = Meter::new(limits.clone(), limits.fuel, self.cancelled.clone());
        let env = Env::from([(binding.to_owned(), (Type::reference(&row.entity), value))]);
        let mut evaluator = Eval {
            app: &program.program,
            data,
            session: &session,
            day,
            meter: &mut meter,
        };
        if evaluator.typ(expression, &env)? != *expected {
            return Err(invalid("projection result type mismatch"));
        }
        let result = evaluator.eval(expression, &env)?;
        validate_value(&result, expected, 0)?;
        Ok(result)
    }
    pub fn compatibility_at(
        &self,
        program: &CapturedProgram,
        current: &DataSnapshot,
        day: i32,
    ) -> Result<CompatibilityReport> {
        validate_day(day)?;
        compatibility::check(program, current, Some(day))
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
    pub fn view_model(&self, run: &ProductRun) -> Result<RuntimeView> {
        let observation = self.observe(run, "view")?;
        Ok(RuntimeView {
            program: run.program.program.clone(),
            observation: observation.view,
            artifacts: observation.outputs,
            retained_records: run.state.data.records.clone(),
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
        if !valid_id(id) {
            return Err(invalid("invalid run ID"));
        }
        scenario.validate(&program.program)?;
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
        let mut run = self.start(
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
        let operations = scenario.replay_operation_ids()?;
        for (input, operation) in scenario.inputs.iter().zip(operations) {
            if let Err(error) = self.apply(&mut run, input, &operation) {
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
}
impl RuntimeAdapter for LocalRuntime {
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
        self.validate(program)?;
        data.validate()?;
        session.validate(&program.program)?;
        validate_day(clock_day)?;
        limits.validate()?;
        if data.project_id != program.binding.project_id {
            return Err(invalid("program and data belong to different projects"));
        }
        let report = self.compatibility_at(program, data, clock_day)?;
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
            program: program.clone(),
            state: State {
                data: data.clone(),
                session: session.clone(),
                day: clock_day,
                outputs: vec![],
            },
            seed: random_seed,
            limits: limits.clone(),
            fuel: Cell::new(limits.fuel),
            trace: vec![],
            observations: vec![],
            receipts: BTreeMap::new(),
        })
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
                observation = Some(view::observe(
                    &run.program.program,
                    &next,
                    point,
                    &mut meter,
                )?);
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
        run.trace.push(step.clone());
        result?;
        run.receipts
            .insert(operation_id.into(), (request, step.clone()));
        Ok(step)
    }
    fn observe(&self, run: &ProductRun, point: &str) -> Result<Observation> {
        let mut meter = Meter::new(run.limits.clone(), run.fuel.get(), self.cancelled.clone());
        let result = view::observe(&run.program.program, &run.state, point, &mut meter);
        run.fuel.set(meter.fuel);
        result
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
        compatibility::check(program, current, None)
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
pub(crate) fn merged_data(
    program: &CapturedProgram,
    current: &DataSnapshot,
) -> Result<DataSnapshot> {
    compatibility::merge(program, current)
}
