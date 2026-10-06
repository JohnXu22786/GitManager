//! Independent, copied production-runtime comparisons and structural reduction.
//! Provider completion and predicted outputs are never execution evidence.
use crate::product_contract::*;
use crate::product_runtime::LocalRuntime;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ObservationTarget {
    Observable {
        point: Id,
        observable: Id,
    },
    OutputCount {
        point: Id,
        output: Id,
    },
    OutputColumn {
        point: Id,
        output: Id,
        column: Id,
    },
    ViewRows {
        point: Id,
    },
    ViewColumn {
        point: Id,
        column: Id,
    },
    /// Host-derived conjunction; all terms must be present before evaluation.
    Property {
        property: AcceptedProperty,
    },
}
impl ObservationTarget {
    pub fn point(&self) -> &str {
        match self {
            Self::Observable { point, .. }
            | Self::OutputCount { point, .. }
            | Self::OutputColumn { point, .. }
            | Self::ViewRows { point }
            | Self::ViewColumn { point, .. } => point,
            Self::Property { .. } => "",
        }
    }
    pub(crate) fn differs(&self, before: &[Observation], after: &[Observation]) -> Option<bool> {
        let (_, a_type, a) = self.sample(before)?;
        let (_, b_type, b) = self.sample(after)?;
        (a_type == b_type).then_some(a != b)
    }
    /// Presence is independent of whether a bounded typed sample can be built.
    /// In particular, several valid exports can exceed one aggregated list's
    /// size limit; that is unavailable sampling, not an absent output channel.
    pub(crate) fn channel_presence(&self, observations: &[Observation]) -> Option<bool> {
        if let Self::Property { property } = self {
            let PropertyPredicate::And { values } = &property.predicate else {
                return None;
            };
            // Each host coordinate is independently required by sample(). Do
            // not infer required channels through conditional Boolean trees.
            let mut unknown = false;
            for value in values {
                let (PropertyPredicate::Equal { left, right }
                | PropertyPredicate::Less { left, right }) = value
                else {
                    return None;
                };
                for term in [left, right] {
                    match term_channel_presence(term, observations) {
                        Some(false) => return Some(false),
                        Some(true) => {}
                        None => unknown = true,
                    }
                }
            }
            return (!unknown).then_some(true);
        }
        let Some(observation) = observations.iter().find(|o| o.point == self.point()) else {
            return Some(false);
        };
        observation.validate().ok()?;
        Some(match self {
            Self::Observable { observable, .. } => {
                observation.values.contains_key(observable)
                    || observation.value_types.contains_key(observable)
            }
            Self::OutputCount { output, .. } => {
                observation.outputs.iter().any(|a| &a.output == output)
            }
            Self::OutputColumn { output, column, .. } => observation
                .outputs
                .iter()
                .any(|a| &a.output == output && a.columns.iter().any(|c| &c.id == column)),
            Self::ViewRows { .. } => {
                observation.view_schema.is_some() || !observation.view.rows.is_empty()
            }
            Self::ViewColumn { column, .. } => {
                observation
                    .view_schema
                    .as_ref()
                    .is_some_and(|s| s.columns.contains_key(column))
                    || observation
                        .view
                        .rows
                        .iter()
                        .any(|r| r.cells.contains_key(column))
            }
            Self::Property { .. } => unreachable!(),
        })
    }
    /// Read actual typed observations, including declared types for empty/null cells.
    pub(crate) fn sample(
        &self,
        observations: &[Observation],
    ) -> Option<(PropertyPredicate, Type, DataValue)> {
        if let Self::Property { property } = self {
            // Do not let And's short-circuit hide a deleted/missing coordinate.
            let PropertyPredicate::And { values } = &property.predicate else {
                return None;
            };
            for predicate in values {
                AcceptedProperty {
                    id: "coordinate".into(),
                    description: "Required measured coordinate".into(),
                    predicate: predicate.clone(),
                }
                .evaluate(observations)?;
            }
            return Some((
                property.predicate.clone(),
                Type::Boolean,
                DataValue::Boolean {
                    value: property.evaluate(observations)?,
                },
            ));
        }
        let (term, typ, value) = self.sample_term(observations)?;
        Some((
            PropertyPredicate::Equal {
                left: term,
                right: PropertyTerm::Literal {
                    value_type: typ.clone(),
                    value: value.clone(),
                },
            },
            typ,
            value,
        ))
    }
    pub(crate) fn sample_term(
        &self,
        observations: &[Observation],
    ) -> Option<(PropertyTerm, Type, DataValue)> {
        let o = observations.iter().find(|o| o.point == self.point())?;
        o.validate().ok()?;
        Some(match self {
            Self::Property { .. } => return None,
            Self::Observable { point, observable } => {
                let typ = o.value_types.get(observable)?.clone();
                (
                    PropertyTerm::Observed {
                        point: point.clone(),
                        observable: observable.clone(),
                        value_type: typ.clone(),
                    },
                    typ,
                    o.values.get(observable)?.clone(),
                )
            }
            Self::OutputCount { point, output } => {
                let outputs: Vec<_> = o.outputs.iter().filter(|a| &a.output == output).collect();
                if outputs.is_empty() {
                    return None;
                }
                let count = outputs.iter().try_fold(0i64, |n, a| {
                    n.checked_add(i64::try_from(a.rows.len()).ok()?)
                })?;
                (
                    PropertyTerm::OutputCount {
                        point: point.clone(),
                        output: output.clone(),
                    },
                    Type::Integer,
                    DataValue::Integer { value: count },
                )
            }
            Self::OutputColumn {
                point,
                output,
                column,
            } => {
                let outputs: Vec<_> = o.outputs.iter().filter(|a| &a.output == output).collect();
                let first = outputs.first()?;
                let typ = first
                    .columns
                    .iter()
                    .find(|c| &c.id == column)?
                    .value_type
                    .clone();
                let mut items = vec![];
                for a in outputs {
                    if !a
                        .columns
                        .iter()
                        .any(|c| &c.id == column && c.value_type == typ)
                    {
                        return None;
                    }
                    for row in &a.rows {
                        items.push(row.get(column)?.clone());
                        if items.len() > MAX_COLLECTION {
                            return None;
                        }
                    }
                }
                (
                    PropertyTerm::OutputColumn {
                        point: point.clone(),
                        output: output.clone(),
                        column: column.clone(),
                        value_type: typ.clone(),
                    },
                    Type::list(typ.clone()),
                    DataValue::List {
                        item_type: typ,
                        items,
                    },
                )
            }
            Self::ViewRows { point } => {
                let entity = o.view_schema.as_ref()?.entity.clone();
                let typ = Type::reference(&entity);
                let items = o
                    .view
                    .rows
                    .iter()
                    .map(|r| DataValue::Reference {
                        entity: r.record.entity.clone(),
                        record: r.record.record.clone(),
                    })
                    .collect();
                (
                    PropertyTerm::ViewRows {
                        point: point.clone(),
                        entity,
                    },
                    Type::list(typ.clone()),
                    DataValue::List {
                        item_type: typ,
                        items,
                    },
                )
            }
            Self::ViewColumn { point, column } => {
                let typ = o.view_schema.as_ref()?.columns.get(column)?.clone();
                let items = o
                    .view
                    .rows
                    .iter()
                    .map(|r| r.cells.get(column).cloned())
                    .collect::<Option<Vec<_>>>()?;
                (
                    PropertyTerm::ViewColumn {
                        point: point.clone(),
                        column: column.clone(),
                        value_type: typ.clone(),
                    },
                    Type::list(typ.clone()),
                    DataValue::List {
                        item_type: typ,
                        items,
                    },
                )
            }
        })
    }
}

// Presence does not resolve a value or exceed the sampler's aggregate bounds.
fn term_channel_presence(term: &PropertyTerm, observations: &[Observation]) -> Option<bool> {
    let target = match term {
        PropertyTerm::Literal { .. } => return Some(true),
        PropertyTerm::Count { value } => return term_channel_presence(value, observations),
        PropertyTerm::Observed {
            point, observable, ..
        } => ObservationTarget::Observable {
            point: point.clone(),
            observable: observable.clone(),
        },
        PropertyTerm::OutputCount { point, output } => ObservationTarget::OutputCount {
            point: point.clone(),
            output: output.clone(),
        },
        PropertyTerm::OutputColumn {
            point,
            output,
            column,
            ..
        } => ObservationTarget::OutputColumn {
            point: point.clone(),
            output: output.clone(),
            column: column.clone(),
        },
        PropertyTerm::ViewRows { point, .. } => ObservationTarget::ViewRows {
            point: point.clone(),
        },
        PropertyTerm::ViewColumn { point, column, .. } => ObservationTarget::ViewColumn {
            point: point.clone(),
            column: column.clone(),
        },
    };
    target.channel_presence(observations)
}

/// Only this module can mint the wrapper; consumers get immutable artifacts.
/// Restored serialized evidence is a claim and must be replayed again.
#[derive(Clone, Debug)]
pub struct VerifiedWitness {
    witness: DifferentialWitness,
    before: CapturedProgram,
    after: CapturedProgram,
    initial: ScenarioSpec,
    initial_before: RunEvidence,
    initial_after: RunEvidence,
    reductions: Vec<ReductionAudit>,
}
#[derive(Clone, Debug)]
pub struct ReductionAudit {
    pub trial: ReductionTrial,
    pub comparison_state: EvidenceState,
    /// A single bounded overflow receipt is kept when full traces do not fit.
    pub evidence_omitted: bool,
    pub runs: Vec<RunEvidence>,
    pub diagnostics: Vec<String>,
}
impl VerifiedWitness {
    pub fn witness(&self) -> &DifferentialWitness {
        &self.witness
    }
    pub fn initial_scenario(&self) -> &ScenarioSpec {
        &self.initial
    }
    pub fn initial_runs(&self) -> (&RunEvidence, &RunEvidence) {
        (&self.initial_before, &self.initial_after)
    }
    pub fn reduction_audit(&self) -> &[ReductionAudit] {
        &self.reductions
    }
    pub fn before_program(&self) -> &CapturedProgram {
        &self.before
    }
    pub fn after_program(&self) -> &CapturedProgram {
        &self.after
    }
    pub fn matches_sources(&self, before: &CapturedProgram, after: &CapturedProgram) -> bool {
        before.validate().is_ok()
            && after.validate().is_ok()
            && before == &self.before
            && after == &self.after
    }
}
#[derive(Clone, Debug)]
pub struct ComparisonReport {
    pub state: EvidenceState,
    pub witness: Option<VerifiedWitness>,
    /// All available real runs, including failed and inconclusive prefixes.
    pub runs: Vec<RunEvidence>,
    pub diagnostics: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct SearchBudget {
    pub max_trials: usize,
    /// Full reduction trace/diagnostic bytes; initial/final runs use RuntimeLimits.
    /// One overflow receipt (at most 8 KiB) is reserved outside this trace budget.
    pub max_evidence_bytes: usize,
    /// Retain exact measured outcomes when distinguishing a new result from a
    /// saved pair; a common smaller counterexample does not prove equivalence.
    pub retain_initial_outcomes: bool,
    pub runtime: RuntimeLimits,
}
impl Default for SearchBudget {
    fn default() -> Self {
        Self {
            max_trials: MAX_ITEMS,
            max_evidence_bytes: 16 * 1024 * 1024,
            retain_initial_outcomes: false,
            runtime: RuntimeLimits::default(),
        }
    }
}
#[derive(Clone)]
pub struct ComparisonEngine {
    cancelled: Arc<AtomicBool>,
    admission: Option<Arc<dyn crate::product_runtime::ReplayAdmission>>,
}
/// Count through a capped writer rather than allocating another full run copy.
fn bounded_evidence_size(value: &impl Serialize, limit: usize) -> Option<usize> {
    struct Counter {
        bytes: usize,
        limit: usize,
    }
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.bytes) {
                return Err(std::io::Error::other("evidence byte limit"));
            }
            self.bytes += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut count = Counter { bytes: 0, limit };
    serde_json::to_writer(&mut count, value).ok()?;
    Some(count.bytes)
}
fn bounded_summary(message: &str) -> String {
    if message.len() <= 128 {
        return message.into();
    }
    let mut end = 114;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...[truncated]", &message[..end])
}
fn invalid(s: &str) -> AdapterError {
    AdapterError::Invalid(ContractError(s.into()))
}
fn classify(error: &AdapterError) -> EvidenceState {
    match error {
        AdapterError::Cancelled | AdapterError::BudgetExhausted(_) => EvidenceState::Inconclusive,
        AdapterError::Unsupported(_) => EvidenceState::Unsupported,
        AdapterError::Stale(_) => EvidenceState::Stale,
        _ => EvidenceState::Failed,
    }
}
/// Structural shape of the same material distinction, not one scalar sign for
/// every inequality. In particular, lost membership cannot become mere order.
#[derive(Clone, Debug, PartialEq, Eq)]
enum DifferenceShape {
    Less,
    Greater,
    Order,
    Removed,
    Added,
    Replaced,
    Scalar,
}
fn direction(a: &DataValue, b: &DataValue) -> Result<DifferenceShape, AdapterError> {
    use std::cmp::Ordering::*;
    Ok(match (a, b) {
        (DataValue::Integer { value: a }, DataValue::Integer { value: b }) => {
            if a < b {
                DifferenceShape::Less
            } else {
                DifferenceShape::Greater
            }
        }
        (DataValue::Date { days: a }, DataValue::Date { days: b }) => {
            if a < b {
                DifferenceShape::Less
            } else {
                DifferenceShape::Greater
            }
        }
        (DataValue::List { items: a, .. }, DataValue::List { items: b, .. }) => {
            let counts=|items:&[DataValue]|->Result<std::collections::BTreeMap<Vec<u8>,usize>,AdapterError>{let mut counts=std::collections::BTreeMap::new();for value in items{*counts.entry(canonical_bytes(value)?).or_insert(0)+=1;}Ok(counts)};
            let (a, b) = (counts(a)?, counts(b)?);
            let sub = |a: &std::collections::BTreeMap<Vec<u8>, usize>,
                       b: &std::collections::BTreeMap<Vec<u8>, usize>| {
                a.iter().all(|(v, n)| b.get(v).unwrap_or(&0) >= n)
            };
            match (sub(&a, &b), sub(&b, &a)) {
                (true, true) => DifferenceShape::Order,
                (false, true) => DifferenceShape::Removed,
                (true, false) => DifferenceShape::Added,
                _ => DifferenceShape::Replaced,
            }
        }
        (DataValue::Boolean { value: a }, DataValue::Boolean { value: b }) => match a.cmp(b) {
            Less => DifferenceShape::Less,
            _ => DifferenceShape::Greater,
        },
        _ => DifferenceShape::Scalar,
    })
}
impl ComparisonEngine {
    pub fn new(cancelled: Arc<AtomicBool>) -> Self {
        Self {
            cancelled,
            admission: None,
        }
    }
    pub fn with_admission(
        mut self,
        admission: Arc<dyn crate::product_runtime::ReplayAdmission>,
    ) -> Self {
        self.admission = Some(admission);
        self
    }
    pub fn compare(
        &self,
        before: &CapturedProgram,
        after: &CapturedProgram,
        scenario: &ScenarioSpec,
        decisions: &DecisionGraph,
        target: ObservationTarget,
        limits: RuntimeLimits,
    ) -> Result<ComparisonReport, AdapterError> {
        before.validate()?;
        after.validate()?;
        decisions.validate()?;
        limits.validate()?;
        if before.binding.project_id != after.binding.project_id
            || before.program.id != after.program.id
        {
            return Err(invalid(
                "comparison programs belong to different applications",
            ));
        }
        let runtime = LocalRuntime::with_cancellation(self.cancelled.clone());
        let mut report = ComparisonReport {
            state: EvidenceState::Inconclusive,
            witness: None,
            runs: vec![],
            diagnostics: vec![],
        };
        if self.cancelled.load(Ordering::Acquire) {
            report
                .diagnostics
                .push("Comparison cancelled before execution".into());
            return Ok(report);
        }
        for (source, id) in [(before, "before-run"), (after, "after-run")] {
            match runtime.replay_admitted(
                source,
                scenario,
                decisions,
                limits.clone(),
                id,
                self.admission.as_deref(),
            ) {
                Ok(run) => report.runs.push(run),
                Err(error) => {
                    report.state = classify(&error);
                    report.diagnostics.push(format!("{id}: {error:?}"));
                }
            }
        }
        if report.runs.len() != 2 {
            return Ok(report);
        }
        if let Some(run) = report
            .runs
            .iter()
            .find(|r| r.state != EvidenceState::Observed)
        {
            report.state = run.state;
            report
                .diagnostics
                .extend(run.errors.iter().chain(&run.uncovered).cloned());
            return Ok(report);
        }
        let (a, b) = (&report.runs[0], &report.runs[1]);
        let (Some((predicate, typ, left)), Some((_, right_type, right))) = (
            target.sample(&a.observations),
            target.sample(&b.observations),
        ) else {
            report
                .diagnostics
                .push("Requested material observation is unavailable in this scenario".into());
            return Ok(report);
        };
        if typ != right_type {
            report.state = EvidenceState::Unsupported;
            report
                .diagnostics
                .push("Compared observations have incompatible types".into());
            return Ok(report);
        }
        if left == right {
            report.state = EvidenceState::NoDifferenceFound;
            report.diagnostics.push("No difference in this target and executed scenario; other behavior remains unchecked".into());
            return Ok(report);
        }
        let property = AcceptedProperty {
            id: "actual-distinction".into(),
            description: "Equality to the independently observed first outcome".into(),
            predicate,
        };
        if let Err(error) = property.validate() {
            report.state = EvidenceState::Unsupported;
            report.diagnostics.push(format!(
                "Target cannot be represented by a bounded property: {error}"
            ));
            return Ok(report);
        }
        let witness = DifferentialWitness {
            version: CONTRACT_VERSION,
            id: "observed-contrast".into(),
            scenario: scenario.clone(),
            before: a.clone(),
            after: b.clone(),
            state: EvidenceState::Observed,
            distinguishing_properties: vec![property],
            minimization: None,
        };
        witness.validate()?;
        report.state = EvidenceState::Observed;
        report.witness = Some(VerifiedWitness {
            witness,
            before: before.clone(),
            after: after.clone(),
            initial: scenario.clone(),
            initial_before: a.clone(),
            initial_after: b.clone(),
            reductions: vec![],
        });
        Ok(report)
    }
    /// Enumerate at most the contract's certificate limit. A larger search is
    /// explicitly incomplete; production reduction materializes one copy at a time.
    pub fn single_reductions(
        &self,
        before: &CapturedProgram,
        after: &CapturedProgram,
        scenario: &ScenarioSpec,
    ) -> Result<Vec<(String, ScenarioSpec)>, AdapterError> {
        Ok(deletions(before, after, scenario)?
            .into_iter()
            .take(MAX_ITEMS)
            .map(|d| (d.name(scenario), d.apply(scenario)))
            .collect())
    }
    pub fn minimize(
        &self,
        before: &CapturedProgram,
        after: &CapturedProgram,
        scenario: &ScenarioSpec,
        decisions: &DecisionGraph,
        target: ObservationTarget,
        budget: SearchBudget,
    ) -> Result<ComparisonReport, AdapterError> {
        if budget.max_evidence_bytes == 0 || budget.max_evidence_bytes > 64 * 1024 * 1024 {
            return Err(invalid(
                "reduction evidence byte budget outside host limits",
            ));
        }
        if budget.max_trials > MAX_ITEMS {
            return Err(invalid("reduction trial budget exceeds evidence contract"));
        }
        let mut best = self.compare(
            before,
            after,
            scenario,
            decisions,
            target.clone(),
            budget.runtime.clone(),
        )?;
        if best.witness.is_none() {
            return Ok(best);
        }
        let a = target
            .sample(&best.runs[0].observations)
            .ok_or_else(|| invalid("initial target absent"))?
            .2;
        let b = target
            .sample(&best.runs[1].observations)
            .ok_or_else(|| invalid("initial target absent"))?
            .2;
        let initial_before = best.runs[0].clone();
        let initial_after = best.runs[1].clone();
        let initial_values = (a.clone(), b.clone());
        let material_direction = direction(&a, &b)?;
        let mut certificate = MinimalityCertificate {
            initial: scenario.identity()?,
            final_scenario: scenario.identity()?,
            deletion_operations: vec![
                "delete_seed_record".into(),
                "delete_input".into(),
                "delete_optional_field".into(),
                "reset_session_condition".into(),
            ],
            trials: vec![],
            final_single_deletions: vec![],
            complete: false,
        };
        if budget.retain_initial_outcomes {
            for operation in &mut certificate.deletion_operations {
                operation.push_str(" while retaining both initial observed target values");
            }
        }
        let mut audit = vec![];
        let mut audit_bytes = 0usize;
        'search: loop {
            let current = best.witness.as_ref().unwrap().witness.scenario.clone();
            let from = current.identity()?;
            let reductions = deletions(before, after, &current)?;
            let all_enumerated = reductions.len() <= MAX_ITEMS;
            let mut final_trials = vec![];
            for deletion in reductions.into_iter().take(MAX_ITEMS) {
                let operation = deletion.name(&current);
                let reduced = deletion.apply(&current);
                if certificate.trials.len() >= budget.max_trials
                    || self.cancelled.load(Ordering::Acquire)
                {
                    break 'search;
                }
                let trial = self.compare(
                    before,
                    after,
                    &reduced,
                    decisions,
                    target.clone(),
                    budget.runtime.clone(),
                )?;
                let Some(trial_bytes) = bounded_evidence_size(
                    &(&trial.runs, &trial.diagnostics),
                    budget.max_evidence_bytes.saturating_sub(audit_bytes),
                ) else {
                    let message=format!("Executed comparison {:?}; full run evidence omitted after the reduction evidence byte budget was exhausted",trial.state);
                    let mut diagnostics = vec![bounded_summary(&message)];
                    diagnostics
                        .extend(trial.diagnostics.iter().take(2).map(|s| bounded_summary(s)));
                    for run in trial.runs.iter().take(2) {
                        diagnostics.push(bounded_summary(&format!(
                            "{}: {:?}, {} trace steps, {} observations; {}",
                            run.id,
                            run.state,
                            run.trace.len(),
                            run.observations.len(),
                            run.errors
                                .first()
                                .or(run.uncovered.first())
                                .map(String::as_str)
                                .unwrap_or("no execution diagnostic")
                        )));
                    }
                    if trial.diagnostics.len() > 2 {
                        diagnostics.push(format!(
                            "{} additional diagnostics omitted",
                            trial.diagnostics.len() - 2
                        ));
                    }
                    let record = ReductionTrial {
                        from: from.clone(),
                        reduced: reduced.identity()?,
                        operation,
                        outcome: ReductionOutcome::Inconclusive,
                        before_run: None,
                        after_run: None,
                    };
                    audit.push(ReductionAudit {
                        trial: record.clone(),
                        comparison_state: trial.state,
                        evidence_omitted: true,
                        runs: vec![],
                        diagnostics,
                    });
                    certificate.trials.push(record);
                    best.diagnostics.push(message);
                    break 'search;
                };
                audit_bytes += trial_bytes;
                let outcome = if trial.state == EvidenceState::Observed {
                    let a = target.sample(&trial.runs[0].observations).unwrap().2;
                    let b = target.sample(&trial.runs[1].observations).unwrap().2;
                    if direction(&a, &b)? == material_direction
                        && (!budget.retain_initial_outcomes
                            || (a == initial_values.0 && b == initial_values.1))
                    {
                        ReductionOutcome::DifferencePreserved
                    } else {
                        ReductionOutcome::DifferenceLost
                    }
                } else if trial.runs.len() == 2
                    && trial
                        .runs
                        .iter()
                        .all(|run| run.state == EvidenceState::Observed)
                    && trial
                        .runs
                        .iter()
                        .any(|run| target.sample(&run.observations).is_none())
                {
                    if trial
                        .runs
                        .iter()
                        .any(|run| target.channel_presence(&run.observations) == Some(false))
                    {
                        ReductionOutcome::InvalidScenario
                    } else {
                        ReductionOutcome::Inconclusive
                    }
                } else if !reduced
                    .inputs
                    .iter()
                    .any(|i| matches!(i, SemanticInput::Observe { .. }))
                {
                    ReductionOutcome::DifferenceLost
                } else {
                    match trial.state {
                        EvidenceState::NoDifferenceFound => ReductionOutcome::DifferenceLost,
                        EvidenceState::Failed => ReductionOutcome::InvalidScenario,
                        _ => ReductionOutcome::Inconclusive,
                    }
                };
                let record = ReductionTrial {
                    from: from.clone(),
                    reduced: reduced.identity()?,
                    operation,
                    outcome,
                    before_run: trial
                        .runs
                        .iter()
                        .find(|r| r.id == "before-run")
                        .map(RunEvidence::identity)
                        .transpose()?,
                    after_run: trial
                        .runs
                        .iter()
                        .find(|r| r.id == "after-run")
                        .map(RunEvidence::identity)
                        .transpose()?,
                };
                audit.push(ReductionAudit {
                    trial: record.clone(),
                    comparison_state: trial.state,
                    evidence_omitted: false,
                    runs: trial.runs.clone(),
                    diagnostics: trial.diagnostics.clone(),
                });
                certificate.trials.push(record.clone());
                final_trials.push(record);
                if outcome == ReductionOutcome::DifferencePreserved {
                    best = trial;
                    continue 'search;
                }
            }
            certificate.complete = all_enumerated
                && final_trials.iter().all(|t| {
                    matches!(
                        t.outcome,
                        ReductionOutcome::InvalidScenario | ReductionOutcome::DifferenceLost
                    )
                });
            certificate.final_single_deletions = final_trials;
            break;
        }
        certificate.final_scenario = best.witness.as_ref().unwrap().witness.scenario.identity()?;
        if !certificate.complete {
            best.diagnostics.push(
                "Smallest found; minimization incomplete under the selected budget or cancellation"
                    .into(),
            );
        } else {
            best.diagnostics.push(
                "1-minimal under the declared unprotected structural reductions; protected cohort/history data are retained; not a global optimum".into(),
            );
        }
        let verified = best.witness.as_mut().unwrap();
        verified.initial = scenario.clone();
        verified.initial_before = initial_before;
        verified.initial_after = initial_after;
        verified.reductions = audit;
        verified.witness.minimization = Some(certificate);
        verified.witness.validate()?;
        Ok(best)
    }
}

#[derive(Clone)]
enum Deletion {
    Record(usize),
    Input(usize),
    Field(usize, Id),
    Condition(Id, DataValue),
}
impl Deletion {
    fn name(&self, s: &ScenarioSpec) -> String {
        match self {
            Self::Record(i) => format!(
                "delete record {}/{}",
                s.seed.records[*i].entity, s.seed.records[*i].id
            ),
            Self::Input(i) => format!("delete input {i}"),
            Self::Field(i, f) => format!(
                "delete optional field {}/{}/{}",
                s.seed.records[*i].entity, s.seed.records[*i].id, f
            ),
            Self::Condition(id, _) => format!("reset session condition {id}"),
        }
    }
    fn apply(&self, s: &ScenarioSpec) -> ScenarioSpec {
        let mut s = s.clone();
        match self {
            Self::Record(i) => {
                s.seed.records.remove(*i);
            }
            Self::Input(i) => {
                s.inputs.remove(*i);
            }
            Self::Field(i, f) => {
                s.seed.records[*i].values.remove(f);
            }
            Self::Condition(id, value) => {
                s.session.values.insert(id.clone(), value.clone());
            }
        }
        s
    }
}
fn deletions(
    before: &CapturedProgram,
    after: &CapturedProgram,
    scenario: &ScenarioSpec,
) -> Result<Vec<Deletion>, AdapterError> {
    before.validate()?;
    after.validate()?;
    scenario.validate_structure()?;
    let mut result = vec![];
    for i in 0..scenario.seed.records.len() {
        if scenario.seed.records[i]
            .values
            .keys()
            .any(|key| key.starts_with(crate::product_runtime::PROTECTED_FIELD_PREFIX))
        {
            continue;
        }
        result.push(Deletion::Record(i));
        if result.len() > MAX_ITEMS {
            return Ok(result);
        }
    }
    for i in 0..scenario.inputs.len() {
        result.push(Deletion::Input(i));
        if result.len() > MAX_ITEMS {
            return Ok(result);
        }
    }
    for (index, record) in scenario.seed.records.iter().enumerate() {
        for field in record.values.keys() {
            if field.starts_with(crate::product_runtime::PROTECTED_FIELD_PREFIX) {
                continue;
            }
            if [before, after].iter().all(|p| {
                p.program
                    .entities
                    .iter()
                    .find(|e| e.id == record.entity)
                    .is_some_and(|e| {
                        e.fields.iter().any(|f| {
                            &f.id == field && matches!(f.value_type, Type::Optional { .. })
                        })
                    })
            }) {
                result.push(Deletion::Field(index, field.clone()));
                if result.len() > MAX_ITEMS {
                    return Ok(result);
                }
            }
        }
    }
    for state in &before.program.state {
        if after.program.state.iter().any(|other| {
            other.id == state.id
                && other.value_type == state.value_type
                && other.initial == state.initial
        }) && scenario
            .session
            .values
            .get(&state.id)
            .is_some_and(|value| value != &state.initial)
        {
            result.push(Deletion::Condition(state.id.clone(), state.initial.clone()));
            if result.len() > MAX_ITEMS {
                return Ok(result);
            }
        }
    }
    Ok(result)
}
