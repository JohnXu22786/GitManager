//! Source-bound hypotheses become questions only after independent execution.
//! This module cannot adopt programs or mutate a daily-work store.
mod history;
mod prepared;
mod provider;
mod request_codec;
mod source;
use crate::product_contract::*;
use crate::product_decisions::{CheckDisposition, IntentionBinding};
use crate::product_runtime::LocalRuntime;
use crate::product_scenarios::*;
use history::RetainedRun;
pub use history::{CheckedWitnessReplay, VerifiedRetainedHistory};
pub use prepared::PreparedDiscoveryCandidate;
pub use provider::*;
pub use request_codec::ExactUtf8DevelopmentRequest;
pub use source::{analyze_delta, SourceDelta};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

#[derive(Clone, Debug)]
pub struct DiscoveryPolicy {
    /// Explicit controller-owned feature requirements, never model suggestions.
    pub required_actions: BTreeSet<Id>,
    pub requirements: Vec<RequirementCase>,
    /// Context resolved by the trusted controller. Missing facts remain unknown.
    pub scope_contexts: BTreeMap<Id, ScopeContext>,
    pub search: SearchBudget,
    pub max_comparisons: usize,
    pub max_precheck_replays: usize,
    /// Host-approved workflow constraints replace untrusted provider oracles.
    pub workflow_validity: BTreeMap<Id, Vec<AcceptedProperty>>,
    /// Genuine V07 scene-package history, loaded by the host from its store.
    /// The package identity is never interpreted as a differential-witness hash.
    pub retained_history: Option<VerifiedRetainedHistory>,
    /// Genuine differential witnesses in their own identity domain.
    pub retained_witnesses: Vec<DifferentialWitness>,
    /// Explicit host promise kind for genuine legacy witnesses. Absence keeps
    /// concrete choices concrete; predicates alone never imply PropertiesOnly.
    pub witness_bindings: BTreeMap<Id, IntentionBinding>,
    /// Exact selected artifact from the controller's verified adoption/history receipt.
    /// KeepCurrent alone does not identify which generated witness side was current.
    pub chosen_artifacts: BTreeMap<Id, Digest>,
}
impl Default for DiscoveryPolicy {
    fn default() -> Self {
        Self {
            required_actions: BTreeSet::new(),
            requirements: vec![],
            scope_contexts: BTreeMap::new(),
            search: SearchBudget::default(),
            max_comparisons: 64,
            max_precheck_replays: 256,
            workflow_validity: BTreeMap::new(),
            retained_history: None,
            retained_witnesses: vec![],
            witness_bindings: BTreeMap::new(),
            chosen_artifacts: BTreeMap::new(),
        }
    }
}
#[derive(Clone, Debug)]
pub struct RequirementCase {
    pub id: Id,
    pub scenario: ScenarioSpec,
    pub properties: Vec<AcceptedProperty>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    Question,
    Grouped,
    Settled,
    Defect,
    PossibleDefect,
    RequestedChange,
    Irrelevant,
    NoWitness,
    Unverified,
}
#[derive(Clone, Debug)]
pub struct HypothesisLog {
    pub hypothesis: Id,
    /// Provider prose is a suggestion, never the measured consequence.
    pub suggestion: String,
    pub disposition: Disposition,
    pub state: EvidenceState,
    pub explanation: String,
}
#[derive(Clone, Debug)]
pub struct ChoiceQuestion {
    pub id: Id,
    /// Hypothesis grouping/context label, not adoption scope authority. Retained
    /// witnesses can exercise its producer; inspect their actual source scenes.
    pub action: Id,
    pub observable: Id,
    pub statement: String,
    pub hypotheses: Vec<Id>,
    pub witnesses: Vec<VerifiedWitness>,
    pub producer: Producer,
    pub unknowns: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct DiscoveryReport {
    /// Exact original development results and their checked host lowering links.
    pub lowerings: Vec<PreparedDiscoveryCandidate>,
    pub coverage: Vec<String>,
    pub delta: SourceDelta,
    pub questions: Vec<ChoiceQuestion>,
    pub defects: Vec<String>,
    pub unverified: Vec<String>,
    pub checks: Vec<DecisionCheck>,
    pub runs: Vec<RunEvidence>,
    /// Reduction-backed equivalence checks used to suppress retained scenes.
    pub normalizations: Vec<VerifiedWitness>,
    pub log: Vec<HypothesisLog>,
}
fn invalid(s: &str) -> AdapterError {
    AdapterError::Invalid(ContractError(s.into()))
}
fn scope_context(policy: &DiscoveryPolicy, operation: &str) -> ScopeContext {
    let mut context = policy
        .scope_contexts
        .get(operation)
        .cloned()
        .unwrap_or_else(|| ScopeContext {
            operation: operation.into(),
            record: None,
            is_new: None,
            created_generation: None,
            attributes: Values::new(),
            predicate_result: None,
        });
    context.operation = operation.into();
    context
}
fn scenario_operations(s: &ScenarioSpec, p: &AppDefinition) -> BTreeSet<Id> {
    input_operations(s.inputs.iter(), p)
}
fn input_operations<'a>(
    inputs: impl Iterator<Item = &'a SemanticInput>,
    p: &AppDefinition,
) -> BTreeSet<Id> {
    inputs
        .filter_map(|i| match i {
            SemanticInput::Invoke { action, .. } => Some(action.clone()),
            SemanticInput::Activate { view, binding, .. } => p
                .views
                .iter()
                .find(|v| &v.id == view)?
                .actions
                .iter()
                .find(|b| &b.id == binding)
                .map(|b| b.action.clone()),
            SemanticInput::Submit { view, .. } => {
                p.views
                    .iter()
                    .find(|v| &v.id == view)
                    .and_then(|v| match &v.kind {
                        ViewKind::Form { action, .. } => Some(action.clone()),
                        _ => None,
                    })
            }
            SemanticInput::Control { view, control, .. } => p
                .views
                .iter()
                .find(|v| &v.id == view)
                .and_then(|v| match &v.kind {
                    ViewKind::List { controls, .. } => controls
                        .iter()
                        .find(|c| &c.id == control)
                        .and_then(|c| c.on_change.clone()),
                    _ => None,
                }),
            _ => None,
        })
        .collect()
}
/// Add host constraints for the operations actually exercised by either side.
/// Provider validity is discarded; verified historical constraints are retained.
fn checked_scene(
    scene: &ScenarioSpec,
    preserve_history: bool,
    programs: &[&CapturedProgram],
    policy: &DiscoveryPolicy,
) -> ScenarioSpec {
    let mut checked = scene.clone();
    if !preserve_history {
        checked.validity.clear();
    }
    let operations: BTreeSet<_> = programs
        .iter()
        .flat_map(|program| scenario_operations(scene, &program.program))
        .collect();
    for operation in operations {
        if let Some(properties) = policy.workflow_validity.get(&operation) {
            for property in properties {
                if !checked.validity.contains(property) {
                    checked.validity.push(property.clone());
                }
            }
        }
    }
    checked
}

/// Identify only a type-level absence of an independently required new
/// action and its added session state. The projected value is used solely to
/// classify the validation error; it is never executed or called current proof.
fn required_feature_absence(
    before: &CapturedProgram,
    candidate: &CapturedProgram,
    scene: &ScenarioSpec,
    policy: &DiscoveryPolicy,
) -> Option<AdapterError> {
    let error = scene.validate(&before.program).err()?;
    scene.validate(&candidate.program).ok()?;
    let missing: BTreeSet<_> = policy
        .required_actions
        .iter()
        .filter(|id| {
            !before
                .program
                .actions
                .iter()
                .any(|action| &action.id == *id)
                && candidate
                    .program
                    .actions
                    .iter()
                    .any(|action| &action.id == *id)
        })
        .collect();
    if missing.is_empty() {
        return None;
    }
    let mut projected = scene.clone();
    let mut removed = false;
    projected.inputs.retain(|input| {
        let absent =
            matches!(input, SemanticInput::Invoke { action, .. } if missing.contains(action));
        removed |= absent;
        !absent
    });
    if !removed {
        return None;
    }
    projected
        .session
        .values
        .retain(|id, _| before.program.state.iter().any(|state| &state.id == id));
    // Reject any other invalid view, input, argument, field or session value.
    projected.validate(&before.program).ok()?;
    Some(AdapterError::Invalid(error))
}

/// Declared output channels can be absent in a real new-feature scene. This
/// applies only to two complete, exact prospective runs with an actual matching
/// observation and no artifact of that output on either side. Missing points,
/// missing columns in an emitted artifact and failed/partial runs stay unknown.
fn completed_output_absence(
    comparison: &ComparisonReport,
    before: &CapturedProgram,
    after: &CapturedProgram,
    scene: &ScenarioSpec,
    target: &ObservationTarget,
) -> bool {
    let (point, output) = match target {
        ObservationTarget::OutputCount { point, output }
        | ObservationTarget::OutputColumn { point, output, .. } => (point, output),
        _ => return false,
    };
    if comparison.state != EvidenceState::Inconclusive
        || comparison.witness.is_some()
        || comparison.runs.len() != 2
    {
        return false;
    }
    comparison
        .runs
        .iter()
        .zip([(before, "before-run"), (after, "after-run")])
        .all(|(run, (source, id))| {
            run.id == id
                && run.state == EvidenceState::Observed
                && run.errors.is_empty()
                && run.uncovered.is_empty()
                && run.validate().is_ok()
                && run.binding.source == source.binding
                && run.binding.artifact == source.artifact
                && scene.identity().ok().as_ref() == Some(&run.binding.scenario_digest)
                && scene.input_identity().ok().as_ref() == Some(&run.binding.input_digest)
                && run
                    .observations
                    .iter()
                    .find(|observation| &observation.point == point)
                    .is_some_and(|observation| {
                        !observation
                            .outputs
                            .iter()
                            .any(|artifact| &artifact.output == output)
                    })
        })
}

struct ReplayBudget {
    remaining: Cell<usize>,
    cancelled: Arc<AtomicBool>,
}
impl ReplayBudget {
    fn reserve(&self, count: usize) -> Result<(), AdapterError> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(AdapterError::Cancelled);
        }
        if count > self.remaining.get() {
            return Err(AdapterError::BudgetExhausted(
                "Discovery precheck replay budget exhausted".into(),
            ));
        }
        self.remaining.set(self.remaining.get() - count);
        Ok(())
    }
    fn check(&self) -> Result<(), AdapterError> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(AdapterError::Cancelled)
        } else if self.remaining.get() == 0 {
            Err(AdapterError::BudgetExhausted(
                "Discovery precheck replay budget exhausted".into(),
            ))
        } else {
            Ok(())
        }
    }
    fn replay(
        &self,
        runtime: &LocalRuntime,
        program: &CapturedProgram,
        scenario: &ScenarioSpec,
        decisions: &DecisionGraph,
        limits: RuntimeLimits,
        id: &str,
    ) -> Result<RunEvidence, AdapterError> {
        self.check()?;
        self.remaining.set(self.remaining.get() - 1);
        runtime.replay(program, scenario, decisions, limits, id)
    }
}
fn check_requirements(
    program: &CapturedProgram,
    request: &DevelopmentRequest,
    policy: &DiscoveryPolicy,
    runtime: &LocalRuntime,
    budget: &ReplayBudget,
) -> Result<(Vec<String>, Vec<String>, Vec<RunEvidence>), AdapterError> {
    let mut defects = vec![];
    let mut unknown = vec![];
    let mut runs = vec![];
    for action in &policy.required_actions {
        if !program.program.actions.iter().any(|a| &a.id == action) {
            defects.push(format!("Required feature action {action} is absent"));
        }
    }
    for requirement in &policy.requirements {
        if let Err(error) = budget.check() {
            unknown.push(format!("Required checks are incomplete: {error:?}"));
            break;
        }
        if !valid_id(&requirement.id) || requirement.properties.is_empty() {
            return Err(invalid(
                "explicit requirement must have an ID and independent properties",
            ));
        }
        for p in &requirement.properties {
            p.validate()?;
        }
        match budget.replay(
            runtime,
            program,
            &checked_scene(&requirement.scenario, true, &[program], policy),
            &request.decisions,
            policy.search.runtime.clone(),
            "requirement-run",
        ) {
            Ok(run) => {
                if run.state != EvidenceState::Observed {
                    unknown.push(format!(
                        "Requirement {} could not execute: {:?}",
                        requirement.id, run.state
                    ));
                } else {
                    for p in &requirement.properties {
                        match p.evaluate(&run.observations) {
                            Some(true) => {}
                            Some(false) => defects.push(format!(
                                "Requirement {} / {} is violated",
                                requirement.id, p.id
                            )),
                            None => unknown.push(format!(
                                "Requirement {} / {} is unverified",
                                requirement.id, p.id
                            )),
                        }
                    }
                }
                runs.push(run);
            }
            Err(e) => unknown.push(format!(
                "Requirement {} could not execute: {e:?}",
                requirement.id
            )),
        }
    }
    Ok((defects, unknown, runs))
}

fn is_chosen(decision: &ScopedDecision) -> bool {
    decision.status == DecisionStatus::Active
        && matches!(
            decision.outcome,
            DecisionOutcome::Accept { .. } | DecisionOutcome::KeepCurrent
        )
}
fn binds_chosen_outcome(decision: &ScopedDecision, policy: &DiscoveryPolicy) -> bool {
    is_chosen(decision)
        && policy.witness_bindings.get(&decision.id) != Some(&IntentionBinding::PropertiesOnly)
}
fn chosen_evidence<'a>(
    decision: &ScopedDecision,
    policy: &'a DiscoveryPolicy,
) -> Result<&'a RunEvidence, AdapterError> {
    let unavailable =
        |message: &str| AdapterError::Unsupported(format!("Decision {}: {message}", decision.id));
    let artifact = match &decision.outcome {
        DecisionOutcome::Accept { artifact } => artifact,
        DecisionOutcome::KeepCurrent => {
            policy.chosen_artifacts.get(&decision.id).ok_or_else(|| {
                unavailable("KeepCurrent needs its verified historical selected-artifact mapping")
            })?
        }
        _ => return Err(unavailable("decision has no chosen outcome")),
    };
    let witness = policy
        .retained_witnesses
        .iter()
        .find(|w| w.identity().ok().as_ref() == Some(&decision.witness))
        .ok_or_else(|| unavailable("accepted witness is missing or invalid"))?;
    let sides: Vec<_> = [&witness.before, &witness.after]
        .into_iter()
        .filter(|run| &run.binding.artifact.program_digest == artifact)
        .collect();
    if sides.len() != 1
        || sides[0].state != EvidenceState::Observed
        || sides[0].origin != ExecutionOrigin::ProductionRuntime
    {
        return Err(unavailable(
            "selected witness side is ambiguous or unverified",
        ));
    }
    Ok(sides[0])
}
#[allow(clippy::too_many_arguments)]
fn check_chosen(
    program: &CapturedProgram,
    scene: &ScenarioSpec,
    decision: &ScopedDecision,
    request: &DevelopmentRequest,
    policy: &DiscoveryPolicy,
    runtime: &LocalRuntime,
    budget: &ReplayBudget,
    id: &str,
    history_runs: &mut Vec<RunEvidence>,
) -> Result<(RunEvidence, CheckState, Digest), AdapterError> {
    let primary = chosen_evidence(decision, policy)?;
    let expected = if scene.input_identity()? == primary.binding.input_digest {
        let source = request
            .sources
            .iter()
            .find(|source| source.artifact == primary.binding.artifact)
            .ok_or_else(|| {
                AdapterError::Unsupported("Selected historical source is unavailable".into())
            })?;
        let replay = budget.replay(
            runtime,
            source,
            &checked_scene(scene, true, &[source], policy),
            &request.decisions,
            policy.search.runtime.clone(),
            "accepted-selected-witness",
        )?;
        history_runs.push(replay.clone());
        if replay.state != EvidenceState::Observed || replay.observations != primary.observations {
            return Err(AdapterError::Unsupported(
                "Selected historical witness cannot be independently reproduced".into(),
            ));
        }
        replay
    } else {
        replay_accepted_outcome(
            request,
            policy,
            runtime,
            budget,
            decision,
            scene,
            &primary.binding.artifact,
            "accepted-history-selected",
            history_runs,
        )?
    };
    if decision
        .obligations
        .iter()
        .any(|property| property.evaluate(&expected.observations) != Some(true))
    {
        return Err(AdapterError::Unsupported(
            "Saved predicates were not demonstrated by the selected historical outcome".into(),
        ));
    }
    let run = budget.replay(
        runtime,
        program,
        &checked_scene(scene, true, &[program], policy),
        &request.decisions,
        policy.search.runtime.clone(),
        id,
    )?;
    let comparable = comparable_outcomes(&run, &expected);
    let mut state = if run.state != EvidenceState::Observed {
        CheckState::Failed
    } else if !binds_chosen_outcome(decision, policy) {
        CheckState::Satisfied
    } else if !comparable {
        CheckState::Unknown
    } else if same_outcomes(&run, &expected) {
        CheckState::Satisfied
    } else {
        CheckState::Violated
    };
    if run.state == EvidenceState::Observed {
        for property in &decision.obligations {
            match property.evaluate(&run.observations) {
                Some(false) => state = CheckState::Violated,
                None if state == CheckState::Satisfied => state = CheckState::Unknown,
                _ => {}
            }
        }
    }
    let obligation = canonical_digest(
        IdentityDomain::Decision,
        &(
            &decision.witness,
            binds_chosen_outcome(decision, policy),
            &decision.obligations,
            &expected.binding.artifact.program_digest,
            expected.identity()?,
        ),
    )?;
    Ok((run, state, obligation))
}

/// Additional accepted scenes need their own original source-qualified outcomes.
/// Context is portable input, not proof: replay both archived sources and match
/// every observation before using it to settle or establish a novel result.
#[allow(clippy::too_many_arguments)]
fn retained_history(
    request: &DevelopmentRequest,
    policy: &DiscoveryPolicy,
    runtime: &LocalRuntime,
    budget: &ReplayBudget,
    decision: &ScopedDecision,
    prior: Option<&DifferentialWitness>,
    current_scene: &ScenarioSpec,
    runs: &mut Vec<RunEvidence>,
) -> Result<(ScenarioSpec, RetainedRun, RetainedRun), AdapterError> {
    if let Some(history) = &policy.retained_history {
        return history.pair(
            decision,
            current_scene,
            request,
            policy,
            runtime,
            budget,
            runs,
        );
    }
    let prior = prior.ok_or_else(|| invalid("accepted differential witness is missing"))?;
    let input = current_scene.input_identity()?;
    let primary_input = &prior.before.binding.input_digest;
    let accepted: Vec<_> = request
        .examples
        .iter()
        .filter(|example| {
            example
                .scenario
                .identity()
                .ok()
                .is_some_and(|id| decision.scenarios.contains(&id))
        })
        .map(|example| &example.scenario)
        .collect();
    let selected = if &input == primary_input {
        &prior.scenario
    } else if let Some(scene) = accepted
        .iter()
        .find(|scene| scene.input_identity().ok().as_ref() == Some(&input))
    {
        *scene
    } else {
        // This selects possible history only. Actual point correspondence is
        // established later from replayed traces, never from this key alone.
        let key = history_execution_key(current_scene)?;
        let mut matching = BTreeMap::new();
        for scene in std::iter::once(&prior.scenario).chain(accepted.iter().copied()) {
            if history_execution_key(scene)? == key {
                matching.entry(scene.input_identity()?).or_insert(scene);
            }
        }
        if matching.len() == 1 {
            *matching.values().next().unwrap()
        } else if matching.len() > 1
            || accepted
                .iter()
                .any(|scene| scene.input_identity().ok().as_ref() != Some(primary_input))
        {
            return Err(AdapterError::Unsupported(format!(
                "Decision {} additional accepted history correspondence is unresolved",
                decision.id
            )));
        } else {
            &prior.scenario
        }
    };
    if selected.input_identity()? == *primary_input {
        return Ok((
            prior.scenario.clone(),
            RetainedRun::direct(prior.before.clone()),
            RetainedRun::direct(prior.after.clone()),
        ));
    }
    let mut expected = vec![];
    for (artifact, id) in [
        (&prior.before.binding.artifact, "accepted-history-before"),
        (&prior.after.binding.artifact, "accepted-history-after"),
    ] {
        expected.push(replay_accepted_outcome(
            request, policy, runtime, budget, decision, selected, artifact, id, runs,
        )?);
    }
    let after = expected.pop().unwrap();
    let before = expected.pop().unwrap();
    Ok((
        selected.clone(),
        RetainedRun::direct(before),
        RetainedRun::direct(after),
    ))
}

#[allow(clippy::too_many_arguments)]
fn replay_accepted_outcome(
    request: &DevelopmentRequest,
    policy: &DiscoveryPolicy,
    runtime: &LocalRuntime,
    budget: &ReplayBudget,
    decision: &ScopedDecision,
    scene: &ScenarioSpec,
    artifact: &ArtifactRef,
    id: &str,
    runs: &mut Vec<RunEvidence>,
) -> Result<RunEvidence, AdapterError> {
    let digest = scene.identity()?;
    let context = request.accepted_scenes.iter().find(|context| context.decision == decision.id && context.scenario == digest && &context.source == artifact)
        .ok_or_else(|| AdapterError::Unsupported(format!("Decision {} has no source-qualified historical outcome for accepted scene {digest:?}", decision.id)))?;
    let source = request
        .sources
        .iter()
        .find(|source| &source.artifact == artifact)
        .ok_or_else(|| {
            AdapterError::Unsupported("Accepted historical source is unavailable".into())
        })?;
    let run = budget.replay(
        runtime,
        source,
        &checked_scene(scene, true, &[source], policy),
        &request.decisions,
        policy.search.runtime.clone(),
        id,
    )?;
    let verified = run.state == EvidenceState::Observed && run.observations == context.observations;
    runs.push(run.clone());
    if !verified {
        return Err(AdapterError::Unsupported(format!("Decision {} historical scene {digest:?} observations could not be independently verified", decision.id)));
    }
    Ok(run)
}

/// Input sources are ordered baseline then current captured candidate. Neither
/// result candidates nor model labels can change which edit is being analyzed.
pub fn discover(
    request: &DevelopmentRequest,
    result: &DevelopmentResult,
    policy: &DiscoveryPolicy,
    cancelled: Arc<AtomicBool>,
) -> Result<DiscoveryReport, AdapterError> {
    request.validate()?;
    result.validate_for(request)?;
    if policy.max_precheck_replays > 4096
        || policy.max_comparisons > MAX_ITEMS
        || policy.requirements.len() > MAX_ITEMS
        || policy.retained_witnesses.len() > MAX_ITEMS
    {
        return Err(invalid("discovery budget exceeds host limits"));
    }
    for properties in policy.workflow_validity.values() {
        for property in properties {
            property.validate()?;
        }
    }
    if request.operation != DevelopmentOperation::Discover || request.sources.len() < 2 {
        return Err(invalid(
            "discovery requires an ordered captured baseline/candidate pair",
        ));
    }
    let before = &request.sources[0];
    let candidate = &request.sources[1];
    if before.program.id != candidate.program.id {
        return Err(invalid("source revision changes the application identity"));
    }
    let lowerings = policy
        .retained_history
        .as_ref()
        .map(|history| history.result_lowerings(request, result))
        .transpose()?
        .unwrap_or_default();
    let delta = analyze_delta(before, candidate)?;
    let mut report = DiscoveryReport {
        lowerings,
        coverage: vec!["Finite source-guided hypotheses and executed scenarios; untested behavior is not proven equivalent".into()],
        delta,
        questions: vec![],
        defects: vec![],
        unverified: result.response.unsupported.clone(),
        checks: vec![],
        runs: vec![],
        normalizations: vec![],
        log: vec![],
    };
    let admission = policy
        .retained_history
        .as_ref()
        .map(VerifiedRetainedHistory::replay_admission)
        .transpose()?;
    let runtime = LocalRuntime::with_cancellation(cancelled.clone());
    let runtime = if let Some(admission) = &admission {
        runtime.with_admission(admission.clone())
    } else {
        runtime
    };
    let replay_budget = ReplayBudget {
        remaining: Cell::new(policy.max_precheck_replays),
        cancelled: cancelled.clone(),
    };
    let mut history_unverified = false;
    if let Some(history) = &policy.retained_history {
        match history
            .verify(request, cancelled.clone(), &replay_budget)
            .and_then(|()| history.check(candidate, cancelled.clone(), policy, &replay_budget))
        {
            Ok(checked) => {
                history_unverified = history::append_check(&mut report, checked, true)
                    == CheckDisposition::Unverified;
            }
            Err(error) => {
                history_unverified = true;
                report
                    .unverified
                    .push(format!("Saved intention gate is unavailable: {error:?}"));
            }
        }
    }
    let (defects, unknown, runs) =
        check_requirements(candidate, request, policy, &runtime, &replay_budget)?;
    let missing_feature_coverage: Vec<_> = policy
        .required_actions
        .iter()
        .filter(|action| {
            !before.program.actions.iter().any(|a| &a.id == *action)
                && !policy.requirements.iter().any(|case| {
                    !case.properties.is_empty()
                        && scenario_operations(&case.scenario, &candidate.program).contains(*action)
                })
        })
        .cloned()
        .collect();
    let required_unknown = !missing_feature_coverage.is_empty()
        || !unknown.is_empty()
        || !request
            .required_capabilities
            .is_subset(&runtime.capabilities().features);
    for action in missing_feature_coverage {
        report.unverified.push(format!("New feature {action} needs an independently checked behavioral requirement, not only an action declaration"));
    }
    report.defects.extend(defects);
    report.unverified.extend(unknown);
    report.runs.extend(runs);
    for capability in request
        .required_capabilities
        .difference(&runtime.capabilities().features)
    {
        report.unverified.push(format!(
            "Required runtime capability is unsupported: {capability}"
        ));
    }
    // Recheck every active decision for the actual candidate, independent of
    // whether the provider mentioned it. Scope never defaults to universal.
    let relevant_actions: BTreeSet<_> = request
        .decisions
        .decisions
        .iter()
        .filter(|d| matches!(d.status, DecisionStatus::Active | DecisionStatus::Pending))
        .flat_map(|d| d.scope.operations.iter().cloned())
        .chain(result.response.hypotheses.iter().map(|h| h.action.clone()))
        .collect();
    let mut hypothesis_operations = BTreeMap::new();
    let mut hypothesis_groups = BTreeMap::new();
    let mut hypothesis_positions = BTreeMap::new();
    for (index, hypothesis) in result.response.hypotheses.iter().enumerate() {
        hypothesis_groups.insert(
            hypothesis.id.clone(),
            canonical_digest(
                IdentityDomain::Decision,
                &(&hypothesis.action, &hypothesis.observable),
            )?,
        );
        hypothesis_positions.insert(hypothesis.id.clone(), index);
        let scene = hypothesis.scenario()?;
        let mut operations = scenario_operations(&scene, &before.program);
        operations.extend(scenario_operations(&scene, &candidate.program));
        hypothesis_operations.insert(hypothesis.id.clone(), operations);
    }
    let mut blocked = BTreeSet::new();
    if history_unverified {
        blocked.extend(relevant_actions.iter().cloned());
    }
    let mut pending = BTreeMap::<Id, Vec<&ScopedDecision>>::new();
    'decision_checks: for decision in request
        .decisions
        .decisions
        .iter()
        .filter(|d| matches!(d.status, DecisionStatus::Active | DecisionStatus::Pending))
    {
        if decision.status == DecisionStatus::Active
            && policy
                .retained_history
                .as_ref()
                .is_some_and(|h| h.contains(decision))
        {
            // The authoritative V07 gate above checks every active concrete or
            // property-only intention with its own source-qualified mappings.
            continue;
        }
        if decision.status == DecisionStatus::Pending {
            if let Some(history) = &policy.retained_history {
                let operations = history.pending_operations(decision);
                for hypothesis in &result.response.hypotheses {
                    if operations.contains(&hypothesis.action)
                        || !operations.is_disjoint(&hypothesis_operations[&hypothesis.id])
                    {
                        pending
                            .entry(hypothesis.id.clone())
                            .or_default()
                            .push(decision);
                    }
                }
                continue;
            }
        }
        if is_chosen(decision)
            && (!policy
                .retained_witnesses
                .iter()
                .any(|w| w.identity().ok().as_ref() == Some(&decision.witness))
                || (policy.witness_bindings.get(&decision.id)
                    == Some(&IntentionBinding::PropertiesOnly)
                    && decision.obligations.is_empty()))
        {
            blocked.extend(decision.scope.operations.iter().cloned());
            report.unverified.push(format!(
                "Decision {} has no verified retained history or explicit nonempty binding",
                decision.id
            ));
            continue;
        }
        for action in &relevant_actions {
            if let Err(error) = replay_budget.check() {
                blocked.extend(relevant_actions.iter().cloned());
                report
                    .unverified
                    .push(format!("Saved intention checks incomplete: {error:?}"));
                break 'decision_checks;
            }
            let scope = decision.scope.matches(&scope_context(policy, action));
            if scope == ScopeMatch::Outside {
                continue;
            }
            if scope == ScopeMatch::Unknown {
                blocked.insert(action.clone());
                report.unverified.push(format!(
                    "Decision {} applicability is unknown for {action}",
                    decision.id
                ));
                continue;
            }
            let scenes: Vec<_> = decision
                .scenarios
                .iter()
                .map(|digest| {
                    request
                        .examples
                        .iter()
                        .find(|e| e.scenario.identity().ok().as_ref() == Some(digest))
                })
                .collect();
            if scenes.iter().any(|s| s.is_none()) {
                blocked.insert(action.clone());
                report.unverified.push(format!(
                    "Accepted scenario for decision {} is missing",
                    decision.id
                ));
                continue;
            }
            if is_chosen(decision) {
                for scene in scenes.iter().flatten() {
                    match check_chosen(
                        candidate,
                        &scene.scenario,
                        decision,
                        request,
                        policy,
                        &runtime,
                        &replay_budget,
                        "chosen-outcome",
                        &mut report.runs,
                    ) {
                        Ok((run, state, property_digest)) => {
                            if state == CheckState::Violated {
                                report.defects.push(format!(
                                    "Active decision {} violates its accepted outcome or predicates",
                                    decision.id
                                ));
                                blocked.insert(action.clone());
                            }
                            if matches!(state, CheckState::Unknown | CheckState::Failed) {
                                report.unverified.push(format!(
                                    "Chosen outcome for {} could not be verified",
                                    decision.id
                                ));
                                blocked.insert(action.clone());
                            }
                            report.checks.push(DecisionCheck {
                                decision: decision.id.clone(),
                                decision_digest: decision.identity()?,
                                property_digest,
                                scope,
                                state,
                                binding: run.binding.clone(),
                                evidence: Some(run.identity()?),
                                explanation: "Independently replayed the selected outcome with its explicit binding and predicates"
                                    .into(),
                            });
                            report.runs.push(run);
                        }
                        Err(error) => {
                            report
                                .unverified
                                .push(format!("Chosen outcome unavailable: {error:?}"));
                            blocked.insert(action.clone());
                        }
                    }
                }
                continue;
            }
            if decision.status == DecisionStatus::Pending
                || matches!(decision.outcome, DecisionOutcome::EitherAcceptable)
            {
                for h in &result.response.hypotheses {
                    if &h.action == action || hypothesis_operations[&h.id].contains(action) {
                        pending.entry(h.id.clone()).or_default().push(decision);
                    }
                }
            }
            if decision.status != DecisionStatus::Active || decision.obligations.is_empty() {
                continue;
            }
            for scene in scenes.into_iter().flatten() {
                match replay_budget.replay(
                    &runtime,
                    candidate,
                    &checked_scene(&scene.scenario, true, &[candidate], policy),
                    &request.decisions,
                    policy.search.runtime.clone(),
                    "decision-run",
                ) {
                    Ok(run) => {
                        for property in &decision.obligations {
                            let state = if run.state != EvidenceState::Observed {
                                CheckState::Failed
                            } else {
                                match property.evaluate(&run.observations) {
                                    Some(true) => CheckState::Satisfied,
                                    Some(false) => CheckState::Violated,
                                    None => CheckState::Unknown,
                                }
                            };
                            if state == CheckState::Violated {
                                report.defects.push(format!(
                                    "Active decision {} / {} is violated",
                                    decision.id, property.id
                                ));
                                blocked.insert(action.clone());
                            }
                            if matches!(state, CheckState::Unknown | CheckState::Failed) {
                                report.unverified.push(format!(
                                    "Active decision {} / {} was not verified",
                                    decision.id, property.id
                                ));
                                blocked.insert(action.clone());
                            }
                            report.checks.push(DecisionCheck{decision:decision.id.clone(),decision_digest:decision.identity()?,property_digest:property.identity()?,scope,state,binding:run.binding.clone(),evidence:Some(run.identity()?),explanation:"Replayed the saved obligation against the exact captured candidate".into()});
                        }
                        report.runs.push(run);
                    }
                    Err(error) => {
                        blocked.insert(action.clone());
                        report.unverified.push(format!(
                            "Decision {} replay unavailable: {error:?}",
                            decision.id
                        ));
                    }
                }
            }
        }
    }
    let mut candidates = BTreeMap::new();
    for c in &result.response.candidates {
        let captured = if let Some(lowering) = report
            .lowerings
            .iter()
            .find(|link| link.candidate_id() == c.id)
        {
            lowering.target().clone()
        } else if let Some(source) = request
            .sources
            .iter()
            .find(|source| source.source_bytes == c.source_json.as_bytes())
        {
            source.clone()
        } else {
            CapturedProgram::capture(
                c.source_json.as_bytes(),
                &request.project_id,
                result.producer.clone(),
                None,
            )?
        };
        if captured.program.id != candidate.program.id {
            return Err(invalid("alternative belongs to another application"));
        }
        candidates.insert(c.id.clone(), captured);
    }
    let engine = ComparisonEngine::new(cancelled.clone());
    let engine = if let Some(admission) = &admission {
        engine.with_admission(admission.clone())
    } else {
        engine
    };
    let mut comparisons = 0usize;
    let mut witness_scene_cache = BTreeMap::new();
    for hypothesis in &result.response.hypotheses {
        let unverified_before = report.unverified.len();
        let mut material_unknowns = BTreeMap::new();
        let mut entry = HypothesisLog {
            hypothesis: hypothesis.id.clone(),
            suggestion: hypothesis.statement.clone(),
            disposition: Disposition::Unverified,
            state: EvidenceState::Inconclusive,
            explanation: String::new(),
        };
        if cancelled.load(Ordering::Acquire) {
            entry.explanation = "Discovery cancelled; no additional question was executed".into();
            report.log.push(entry);
            continue;
        }
        if !report.defects.is_empty() {
            entry.disposition = Disposition::Defect;
            entry.state = EvidenceState::Failed;
            entry.explanation =
                "Repair explicit requirement violations before presenting alternatives".into();
            report.log.push(entry);
            continue;
        }
        if required_unknown || !blocked.is_empty() {
            entry.explanation =
                "The captured implementation has unverified applicable saved requirements".into();
            report.log.push(entry);
            continue;
        }
        let exact = hypothesis
            .alternatives
            .iter()
            .find(|id| candidates.get(*id).is_some_and(|p| p == candidate));
        let Some(exact) = exact else {
            entry.explanation =
                "No alternative binds the exact captured candidate; provider substitution rejected"
                    .into();
            report.log.push(entry);
            continue;
        };
        let scene = checked_scene(&hypothesis.scenario()?, false, &[before, candidate], policy);
        let mut hypothesis_search = policy.search.clone();
        hypothesis_search.retain_initial_outcomes |= pending.contains_key(&hypothesis.id);
        // Retained scenes are independent search inputs. A provider can omit
        // every revealing observation, so they cannot depend on its witness.
        let mut source_scenes = vec![(scene, false)];
        let mut scene_indices = BTreeMap::from([(source_scenes[0].0.identity()?, 0usize)]);
        let mut capture_unavailable = false;
        let mut retain_scene = |scene: &ScenarioSpec| -> Result<(), AdapterError> {
            let retained = checked_scene(scene, true, &[before, candidate], policy);
            let identity = retained.identity()?;
            if let Some(index) = scene_indices.get(&identity) {
                source_scenes[*index].1 = true;
            } else {
                scene_indices.insert(identity, source_scenes.len());
                source_scenes.push((retained, true));
            }
            Ok(())
        };
        for decision in pending.get(&hypothesis.id).into_iter().flatten() {
            if policy
                .retained_history
                .as_ref()
                .is_some_and(|h| h.contains(decision))
            {
                let history = policy.retained_history.as_ref().unwrap();
                for scene in history.pending_scenes(decision) {
                    retain_scene(scene.mapped())?;
                }
                continue;
            } else if let Some(prior) = policy
                .retained_witnesses
                .iter()
                .find(|w| w.identity().ok().as_ref() == Some(&decision.witness))
            {
                retain_scene(&prior.scenario)?;
            } else {
                capture_unavailable = true;
                report.unverified.push(format!(
                    "Accepted witness for {} is unavailable",
                    decision.id
                ));
            }
            // A decision can accept more scenes than its primary witness.
            // Every selected, hash-bound scene is an independent search input.
            for digest in &decision.scenarios {
                if let Some(example) = request
                    .examples
                    .iter()
                    .find(|example| example.scenario.identity().ok().as_ref() == Some(digest))
                {
                    retain_scene(&example.scenario)?;
                } else {
                    capture_unavailable = true;
                    report
                        .unverified
                        .push(format!("Accepted scene for {} is unavailable", decision.id));
                }
            }
        }
        let mut relevant_scenes = vec![];
        for (scene, retained) in source_scenes {
            // The provider's consumer label cannot exclude an associated,
            // hash-bound producer scene (including a minimized observation).
            if (retained
                || scenario_operations(&scene, &candidate.program).contains(&hypothesis.action))
                && source::relevant(&report.delta, before, candidate, hypothesis, &scene)?
            {
                relevant_scenes.push(scene);
            }
        }
        if relevant_scenes.is_empty() {
            if !capture_unavailable {
                entry.disposition = Disposition::Irrelevant;
                entry.explanation = "No proposed or retained scene performs this action with a changed reachable source locus".into();
            } else {
                entry.explanation = "Accepted scene source relevance is unavailable".into();
            }
            report.log.push(entry);
            continue;
        }
        let mut captured_scenes = vec![];
        for (index, scene) in relevant_scenes.into_iter().enumerate() {
            let scene = if let Some(history) = &policy.retained_history {
                match history.project_comparison_scene(before, candidate, &scene) {
                    Ok(scene) => scene,
                    Err(error) => {
                        capture_unavailable = true;
                        report.unverified.push(format!(
                            "Accepted scene initialization is unavailable: {error:?}"
                        ));
                        continue;
                    }
                }
            } else {
                scene
            };
            let new_feature = policy.required_actions.iter().any(|id| {
                !before.program.actions.iter().any(|a| &a.id == id)
                    && scenario_operations(&scene, &candidate.program).contains(id)
            });
            let baseline_run = replay_budget.replay(
                &runtime,
                before,
                &scene,
                &request.decisions,
                policy.search.runtime.clone(),
                if index == 0 {
                    "captured-baseline"
                } else {
                    "captured-retained-baseline"
                },
            );
            let candidate_run = replay_budget.replay(
                &runtime,
                candidate,
                &scene,
                &request.decisions,
                policy.search.runtime.clone(),
                if index == 0 {
                    "captured-candidate"
                } else {
                    "captured-retained-candidate"
                },
            );
            let anchored = matches!((&baseline_run,&candidate_run),(Ok(a),Ok(b)) if a.state==EvidenceState::Observed&&b.state==EvidenceState::Observed&&!same_outcomes(a,b));
            let unchanged =
                matches!((&baseline_run,&candidate_run),(Ok(a),Ok(b)) if same_outcomes(a,b));
            let current_ran = matches!(&candidate_run,Ok(b) if b.state==EvidenceState::Observed);
            let captured_observations = match (&baseline_run, &candidate_run) {
                (Ok(a), Ok(b))
                    if a.state == EvidenceState::Observed && b.state == EvidenceState::Observed =>
                {
                    Some((
                        history::comparison_observations(policy, before, a)?,
                        history::comparison_observations(policy, candidate, b)?,
                    ))
                }
                _ => None,
            };
            if let Some((a, b)) = &captured_observations {
                let channels = unrepresented_differences(a, b);
                if !channels.is_empty() {
                    material_unknowns.insert((before.binding.identity()?, candidate.binding.identity()?, scene_equivalence_key(&scene)?), format!("Captured source executions differ in material channels without executable property terms: {}", channels.join(", ")));
                }
            }
            let expected_absence = if new_feature && current_ran {
                required_feature_absence(before, candidate, &scene, policy)
            } else {
                None
            };
            for (side, result) in [baseline_run, candidate_run].into_iter().enumerate() {
                match result {
                    Ok(run) => report.runs.push(run),
                    Err(error) if side == 0 && expected_absence.as_ref() == Some(&error) => {
                        report.coverage.push("Current cannot execute this independently required new action/state; compare the two verified prospective implementations. No current-side experience is claimed".into());
                    }
                    Err(error) => report
                        .unverified
                        .push(format!("Captured-source replay unavailable: {error:?}")),
                }
            }
            if unchanged {
                continue;
            }
            if anchored || (new_feature && current_ran) {
                captured_scenes.push((scene, new_feature, captured_observations));
            } else {
                capture_unavailable = true;
                report.unverified.push("Actual edited behavior could not be linked to a valid source comparison or independently required new feature".into());
            }
        }
        if capture_unavailable {
            report.unverified.extend(material_unknowns.into_values());
            entry.explanation =
                "Applicable captured scenes could not all be independently checked".into();
            report.log.push(entry);
            continue;
        }
        if captured_scenes.is_empty() {
            entry.disposition = Disposition::NoWitness;
            entry.state = EvidenceState::NoDifferenceFound;
            entry.explanation = "Actual captured edit has identical outcomes in the checked scenes; generated alternatives cannot invent a choice".into();
            report.log.push(entry);
            continue;
        }
        let comparisons_before_hypothesis = comparisons;
        let mut witnesses = vec![];
        let mut exhausted = false;
        for id in hypothesis.alternatives.iter().filter(|id| *id != exact) {
            if let Err(error) = replay_budget.check() {
                entry.explanation = format!("Alternative validation incomplete: {error:?}");
                report.unverified.push(entry.explanation.clone());
                break;
            }
            let alternative = &candidates[id];
            let alternative_unverified_before = report.unverified.len();
            if let Some(history) = &policy.retained_history {
                let state =
                    match history.check(alternative, cancelled.clone(), policy, &replay_budget) {
                        Ok(checked) => history::append_check(&mut report, checked, false),
                        Err(error) => {
                            report.unverified.push(format!(
                                "Alternative {id} intention gate unavailable: {error:?}"
                            ));
                            CheckDisposition::Unverified
                        }
                    };
                if state != CheckDisposition::Ready {
                    entry.disposition = if state == CheckDisposition::RepairRequired {
                        Disposition::Settled
                    } else {
                        Disposition::Unverified
                    };
                    entry.state = if state == CheckDisposition::RepairRequired {
                        EvidenceState::Observed
                    } else {
                        EvidenceState::Inconclusive
                    };
                    entry.explanation = if state == CheckDisposition::RepairRequired {
                        format!("Alternative {id} requires repair of an approved intention; it is not a preference option")
                    } else {
                        format!("Alternative {id} has unverified saved intentions")
                    };
                    continue;
                }
            }
            let (defects, unknown, runs) =
                check_requirements(alternative, request, policy, &runtime, &replay_budget)?;
            report.runs.extend(runs);
            if !defects.is_empty() || !unknown.is_empty() {
                report.unverified.extend(defects.into_iter().chain(unknown));
                continue;
            }
            // A known obligation failing on an alternative is not an equally
            // valid product choice, even if the model labels it unresolved.
            let mut permitted = true;
            for decision in request.decisions.decisions.iter().filter(|d| {
                d.status == DecisionStatus::Active
                    && (!d.obligations.is_empty() || is_chosen(d))
                    && !policy
                        .retained_history
                        .as_ref()
                        .is_some_and(|h| h.contains(d))
            }) {
                if let Err(error) = replay_budget.check() {
                    permitted = false;
                    report
                        .unverified
                        .push(format!("Alternative checks incomplete: {error:?}"));
                    break;
                }
                let scopes: Vec<_> = decision
                    .scope
                    .operations
                    .iter()
                    .map(|operation| decision.scope.matches(&scope_context(policy, operation)))
                    .collect();
                if scopes.iter().all(|scope| *scope == ScopeMatch::Outside) {
                    continue;
                }
                if scopes.contains(&ScopeMatch::Unknown) {
                    permitted = false;
                    report.unverified.push(format!(
                        "Alternative {} has unknown applicability for decision {}",
                        id, decision.id
                    ));
                    continue;
                }
                for digest in &decision.scenarios {
                    if let Err(error) = replay_budget.check() {
                        permitted = false;
                        report
                            .unverified
                            .push(format!("Alternative checks incomplete: {error:?}"));
                        break;
                    }
                    if let Some(example) = request
                        .examples
                        .iter()
                        .find(|s| s.scenario.identity().ok().as_ref() == Some(digest))
                    {
                        if is_chosen(decision) {
                            match check_chosen(
                                alternative,
                                &example.scenario,
                                decision,
                                request,
                                policy,
                                &runtime,
                                &replay_budget,
                                "alternative-chosen-outcome",
                                &mut report.runs,
                            ) {
                                Ok((run, state, _)) => {
                                    if state != CheckState::Satisfied {
                                        permitted = false;
                                    }
                                    if !matches!(
                                        state,
                                        CheckState::Satisfied | CheckState::Violated
                                    ) {
                                        report.unverified.push(format!("Alternative {id} chosen outcome for {} is unverified: {state:?}, runtime {:?}",decision.id,run.state));
                                        report.unverified.extend(
                                            run.errors.iter().chain(&run.uncovered).map(
                                                |message| format!("Alternative {id}: {message}"),
                                            ),
                                        );
                                    }
                                    report.runs.push(run);
                                }
                                Err(error) => {
                                    permitted = false;
                                    report.unverified.push(format!(
                                        "Alternative chosen outcome unavailable: {error:?}"
                                    ));
                                }
                            }
                            continue;
                        }
                        match replay_budget.replay(
                            &runtime,
                            alternative,
                            &checked_scene(&example.scenario, true, &[alternative], policy),
                            &request.decisions,
                            policy.search.runtime.clone(),
                            "alternative-obligation",
                        ) {
                            Ok(run) => {
                                if run.state != EvidenceState::Observed {
                                    permitted = false;
                                    report.unverified.push(format!("Alternative {id} obligation check for {} is unverified: {:?}",decision.id,run.state));
                                    report.unverified.extend(
                                        run.errors
                                            .iter()
                                            .chain(&run.uncovered)
                                            .map(|message| format!("Alternative {id}: {message}")),
                                    );
                                } else {
                                    for property in &decision.obligations {
                                        match property.evaluate(&run.observations) {
                                            Some(true) => {}
                                            Some(false) => permitted = false,
                                            None => {
                                                permitted = false;
                                                report.unverified.push(format!("Alternative {id} obligation {} for {} is unknown",property.id,decision.id));
                                            }
                                        }
                                    }
                                }
                                report.runs.push(run);
                            }
                            Err(error) => {
                                permitted = false;
                                report.unverified.push(format!(
                                    "Alternative obligation replay failed: {error:?}"
                                ));
                            }
                        }
                    } else {
                        permitted = false;
                        report.unverified.push(format!(
                            "Alternative {id} accepted scene for {} is unavailable",
                            decision.id
                        ));
                    }
                }
            }
            if !permitted {
                if report.unverified.len() > alternative_unverified_before {
                    entry.disposition = Disposition::Unverified;
                    entry.state = EvidenceState::Inconclusive;
                    entry.explanation =
                        "Competing implementation has incomplete approved-intention checks".into();
                } else {
                    entry.disposition = Disposition::Settled;
                    entry.state = EvidenceState::Observed;
                    entry.explanation="Competing implementation was excluded by an observed approved-intention violation".into();
                }
                continue;
            }
            'scenes: for (scene, new_feature, captured_observations) in &captured_scenes {
                let scene = checked_scene(scene, true, &[alternative, candidate], policy);
                let mut channels_checked = false;
                let points: Vec<_> = scene
                    .inputs
                    .iter()
                    .filter_map(|input| match input {
                        SemanticInput::Observe { point } => Some(point.clone()),
                        _ => None,
                    })
                    .collect();
                for point in &points {
                    // Only actual captured changes may generate new choice witnesses.
                    for target in comparison_targets(&[before, candidate], point, policy)? {
                        if !*new_feature {
                            match captured_observations
                                .as_ref()
                                .and_then(|(a, b)| target.differs(a, b))
                            {
                                Some(true) => {}
                                Some(false) => continue,
                                None => {
                                    // Both absent means no channel was emitted at this
                                    // point. Failed/over-limit samples or one-sided and
                                    // incompatible evidence are unavailable, never equal.
                                    if !captured_observations.as_ref().is_some_and(|(a, b)| {
                                        target.channel_presence(a) == Some(false)
                                            && target.channel_presence(b) == Some(false)
                                    }) {
                                        report.unverified.push(format!("Captured target {target:?} is unavailable or has incompatible types"));
                                    }
                                    continue;
                                }
                            }
                        }
                        if comparisons >= policy.max_comparisons {
                            exhausted = true;
                            break 'scenes;
                        }
                        comparisons += 1;
                        let comparison = engine.minimize(
                            alternative,
                            candidate,
                            &scene,
                            &request.decisions,
                            target.clone(),
                            hypothesis_search.clone(),
                        )?;
                        if !channels_checked {
                            let original = comparison
                                .witness
                                .as_ref()
                                .map(|w| w.initial_runs())
                                .or_else(|| {
                                    Some((
                                        comparison.runs.iter().find(|r| r.id == "before-run")?,
                                        comparison.runs.iter().find(|r| r.id == "after-run")?,
                                    ))
                                });
                            if let Some((a, b)) = original.filter(|(a, b)| {
                                a.state == EvidenceState::Observed
                                    && b.state == EvidenceState::Observed
                            }) {
                                channels_checked = true;
                                for point in &points {
                                    for channel in comparison_targets(
                                        &[before, alternative, candidate],
                                        point,
                                        policy,
                                    )? {
                                        if channel
                                            .differs(&a.observations, &b.observations)
                                            .is_none()
                                            && !(channel.channel_presence(&a.observations)
                                                == Some(false)
                                                && channel.channel_presence(&b.observations)
                                                    == Some(false))
                                        {
                                            report.unverified.push(format!("Alternative {id} target {channel:?} is unavailable or has incompatible types"));
                                        }
                                    }
                                }
                                let channels = unrepresented_differences(
                                    &history::comparison_observations(policy, alternative, a)?,
                                    &history::comparison_observations(policy, candidate, b)?,
                                );
                                if !channels.is_empty() {
                                    material_unknowns.insert((alternative.binding.identity()?, candidate.binding.identity()?, scene_equivalence_key(&scene)?), format!("Alternative {id} executions differ in material channels without executable property terms: {}", channels.join(", ")));
                                }
                            }
                        }
                        if *new_feature
                            && completed_output_absence(
                                &comparison,
                                alternative,
                                candidate,
                                &scene,
                                &target,
                            )
                        {
                            let output = match &target {
                                ObservationTarget::OutputCount { output, .. }
                                | ObservationTarget::OutputColumn { output, .. } => output,
                                _ => unreachable!("checked output target"),
                            };
                            report.coverage.push(format!("Output {output} was absent on both completed prospective runs at {}; no output-value comparison or witness is claimed", target.point()));
                            report.runs.extend(comparison.runs);
                            continue;
                        }
                        entry.state = comparison.state;
                        if !matches!(
                            comparison.state,
                            EvidenceState::Observed | EvidenceState::NoDifferenceFound
                        ) {
                            report.unverified.push(format!(
                                "Alternative {id} comparison is unverified: {:?}",
                                comparison.state
                            ));
                            report
                                .unverified
                                .extend(comparison.diagnostics.iter().cloned());
                        }
                        report.runs.extend(comparison.runs);
                        if let Some(witness) = comparison.witness {
                            if witness
                                .witness()
                                .minimization
                                .as_ref()
                                .is_some_and(|m| !m.complete)
                            {
                                report.unverified.extend(
                                    comparison
                                        .diagnostics
                                        .iter()
                                        .filter(|s| s.contains("incomplete"))
                                        .cloned(),
                                );
                            }
                            witnesses.push(witness);
                        }
                        entry.explanation = comparison.diagnostics.join("; ");
                    }
                }
            }
        }
        if exhausted {
            entry.state = EvidenceState::Inconclusive;
            entry.explanation =
                "Discovery comparison budget exhausted; remaining behavior is unverified".into();
            report.unverified.push(entry.explanation.clone());
            report.coverage.push(entry.explanation.clone());
        }
        if let Some(decisions) = pending.get(&hypothesis.id) {
            let mut matched = BTreeSet::new();
            let mut replacements = BTreeMap::new();
            let mut settled_pairs = BTreeSet::new();
            let mut unavailable = false;
            for decision in decisions {
                if let Err(error) = replay_budget.check() {
                    unavailable = true;
                    report
                        .unverified
                        .push(format!("Retained checks incomplete: {error:?}"));
                    break;
                }
                let prior = policy
                    .retained_witnesses
                    .iter()
                    .find(|w| w.identity().ok().as_ref() == Some(&decision.witness));
                if prior.is_some()
                    || policy
                        .retained_history
                        .as_ref()
                        .is_some_and(|h| h.contains(decision))
                {
                    for (index, witness) in witnesses.iter().enumerate() {
                        let (historical_scene, expected_before, expected_after) =
                            match retained_history(
                                request,
                                policy,
                                &runtime,
                                &replay_budget,
                                decision,
                                prior,
                                witness.initial_scenario(),
                                &mut report.runs,
                            ) {
                                Ok(history) => history,
                                Err(error) => {
                                    unavailable = true;
                                    report.unverified.push(format!(
                                        "Accepted scene history is unverified: {error:?}"
                                    ));
                                    // Missing additional history cannot erase the
                                    // primary accepted workflow's actual failures.
                                    // These are diagnostics only, not correspondence
                                    // evidence for the scene whose history is missing.
                                    if let Some(prior) = prior {
                                        let primary = checked_scene(
                                            &prior.scenario,
                                            true,
                                            &[witness.before_program(), witness.after_program()],
                                            policy,
                                        );
                                        for (source, id) in [
                                            (witness.before_program(), "retained-before"),
                                            (witness.after_program(), "retained-after"),
                                        ] {
                                            match replay_budget.replay(&runtime, source, &primary, &request.decisions, policy.search.runtime.clone(), id) {
                                            Ok(run) => report.runs.push(run),
                                            Err(error) => report.unverified.push(format!("Primary accepted workflow replay unavailable: {error:?}")),
                                        }
                                        }
                                    }
                                    continue;
                                }
                            };
                        let retained_scene = checked_scene(
                            &historical_scene,
                            true,
                            &[witness.before_program(), witness.after_program()],
                            policy,
                        );
                        let a = replay_budget.replay(
                            &runtime,
                            witness.before_program(),
                            &retained_scene,
                            &request.decisions,
                            policy.search.runtime.clone(),
                            "retained-before",
                        );
                        let b = replay_budget.replay(
                            &runtime,
                            witness.after_program(),
                            &retained_scene,
                            &request.decisions,
                            policy.search.runtime.clone(),
                            "retained-after",
                        );
                        match (&a, &b) {
                            (Ok(a), Ok(b))
                                if expected_before.comparable(a)
                                    && expected_after.comparable(b) =>
                            {
                                let pair_key = (
                                    witness.before_program().binding.identity()?,
                                    witness.after_program().binding.identity()?,
                                    scene_equivalence_key(witness.initial_scenario())?,
                                );
                                let retained_key = (
                                    pair_key.0.clone(),
                                    pair_key.1.clone(),
                                    scene_equivalence_key(&retained_scene)?,
                                );
                                let (original_before, original_after) = witness.initial_runs();
                                let profile = retained_profile(
                                    original_before,
                                    original_after,
                                    &expected_before,
                                    &expected_after,
                                    &history::comparison_observations(
                                        policy,
                                        witness.before_program(),
                                        original_before,
                                    )?,
                                    &history::comparison_observations(
                                        policy,
                                        witness.after_program(),
                                        original_after,
                                    )?,
                                );
                                // The provider's observation subset cannot hide a changed
                                // point already observed by our complete retained replay.
                                let replay_profile = retained_profile(
                                    a,
                                    b,
                                    &expected_before,
                                    &expected_after,
                                    &history::comparison_observations(
                                        policy,
                                        witness.before_program(),
                                        a,
                                    )?,
                                    &history::comparison_observations(
                                        policy,
                                        witness.after_program(),
                                        b,
                                    )?,
                                );
                                if profile.is_none() || replay_profile.is_none() {
                                    unavailable = true;
                                    report.unverified.push("Retained material observation correspondence is unresolved".into());
                                }
                                if replay_profile.as_ref().is_some_and(|p| p.matches) {
                                    settled_pairs.insert(retained_key.clone());
                                }
                                for (key, initial_scene, profile) in [
                                    (&pair_key, witness.initial_scenario(), profile.as_ref()),
                                    (&retained_key, &retained_scene, replay_profile.as_ref()),
                                ] {
                                    if let Some(property) = profile.and_then(|p| p.novel.clone()) {
                                        if !replacements.contains_key(key) {
                                            if comparisons >= policy.max_comparisons {
                                                unavailable = true;
                                                report.unverified.push("Novel outcome combination remains unverified: comparison budget exhausted".into());
                                            } else {
                                                comparisons += 1;
                                                let combined = engine.minimize(
                                                    witness.before_program(),
                                                    witness.after_program(),
                                                    initial_scene,
                                                    &request.decisions,
                                                    ObservationTarget::Property { property },
                                                    hypothesis_search.clone(),
                                                )?;
                                                report.runs.extend(combined.runs);
                                                report.unverified.extend(
                                                    combined
                                                        .diagnostics
                                                        .iter()
                                                        .filter(|s| s.contains("incomplete"))
                                                        .cloned(),
                                                );
                                                if let Some(combined) = combined.witness {
                                                    replacements.insert(key.clone(), combined);
                                                } else {
                                                    unavailable = true;
                                                    report.unverified.extend(combined.diagnostics);
                                                }
                                            }
                                        }
                                    }
                                }
                                let original_replaced = replacements.contains_key(&pair_key);
                                let retained_replaced = replacements.contains_key(&retained_key);
                                if original_replaced
                                    || (retained_replaced
                                        && profile.as_ref().is_some_and(|p| p.matches))
                                {
                                    matched.insert(index);
                                }
                                if !original_replaced && !retained_replaced {
                                    if let Some(current_target) = witness_target(witness.witness())
                                    {
                                        if let (
                                            Some(prior_target),
                                            Some(expected_first),
                                            Some(expected_second),
                                        ) = (
                                            retained_target(&current_target, original_before, a),
                                            expected_before
                                                .target(&current_target, original_before),
                                            expected_after.target(&current_target, original_before),
                                        ) {
                                            let current_pair = (
                                                current_target
                                                    .sample(&original_before.observations),
                                                current_target.sample(&original_after.observations),
                                            );
                                            let expected_pair = (
                                                expected_first
                                                    .sample(&expected_before.original.observations),
                                                expected_second
                                                    .sample(&expected_after.original.observations),
                                            );
                                            let replay_pair = (
                                                prior_target.sample(&a.observations),
                                                prior_target.sample(&b.observations),
                                            );
                                            if current_pair.0.is_none() || current_pair.1.is_none()
                                            {
                                                unavailable = true;
                                                report.unverified.push(
                                                    "Original material contrast is unavailable"
                                                        .into(),
                                                );
                                            } else if expected_pair.0.is_none()
                                                || expected_pair.1.is_none()
                                            {
                                                // A newly observed channel is not an old settled outcome.
                                                // Missing old typed view provenance remains unverified.
                                                if matches!(
                                                    prior_target,
                                                    ObservationTarget::ViewRows { .. }
                                                        | ObservationTarget::ViewColumn { .. }
                                                ) && expected_before
                                                    .original
                                                    .observations
                                                    .iter()
                                                    .chain(&expected_after.original.observations)
                                                    .any(|o| {
                                                        o.point == prior_target.point()
                                                            && o.view_schema.is_none()
                                                    })
                                                {
                                                    unavailable = true;
                                                    report.unverified.push(
                                                "Retained view type provenance is unavailable"
                                                    .into(),
                                            );
                                                }
                                            } else if profile.as_ref().is_some_and(|p| p.matches)
                                                && replay_profile
                                                    .as_ref()
                                                    .is_some_and(|p| p.matches)
                                                && same_pair(&current_pair, &expected_pair)
                                                && same_pair(&replay_pair, &expected_pair)
                                            {
                                                // Material profiles and both sampled pairs
                                                // already match above. Only checked scoped
                                                // origins may bridge different provenance
                                                // envelopes around the same business input.
                                                let equivalent =
                                                    |left: &ScenarioSpec, right: &ScenarioSpec| {
                                                        if scene_equivalence_key(left)?
                                                            == scene_equivalence_key(right)?
                                                        {
                                                            return Ok(true);
                                                        }
                                                        match &policy.retained_history {
                                                            Some(history) => history
                                                                .equivalent_prepared_frames(
                                                                    witness.before_program(),
                                                                    witness.after_program(),
                                                                    left,
                                                                    right,
                                                                    &cancelled,
                                                                ),
                                                            None => Ok(false),
                                                        }
                                                    };
                                                let (direct, frame_available) = match equivalent(
                                                    witness.initial_scenario(),
                                                    &retained_scene,
                                                )
                                                .and_then(|same| {
                                                    if same {
                                                        Ok(true)
                                                    } else {
                                                        equivalent(
                                                            &witness.witness().scenario,
                                                            &retained_scene,
                                                        )
                                                    }
                                                }) {
                                                    Ok(same) => (same, true),
                                                    Err(error) => {
                                                        unavailable = true;
                                                        report.unverified.push(format!("Retained input-frame equivalence is unverified: {error:?}"));
                                                        (false, false)
                                                    }
                                                };
                                                if direct {
                                                    matched.insert(index);
                                                    settled_pairs.insert(pair_key.clone());
                                                } else if frame_available
                                                    && comparisons >= policy.max_comparisons
                                                {
                                                    unavailable = true;
                                                    report.unverified.push("Retained-scene comparison budget exhausted; equivalence is unverified".into());
                                                } else if frame_available {
                                                    comparisons += 1;
                                                    let normalized = engine.minimize(
                                                        witness.before_program(),
                                                        witness.after_program(),
                                                        &retained_scene,
                                                        &request.decisions,
                                                        prior_target,
                                                        hypothesis_search.clone(),
                                                    )?;
                                                    report.runs.extend(normalized.runs);
                                                    if let Some(normalized) = normalized.witness {
                                                        let same_frame = match equivalent(
                                                            &normalized.witness().scenario,
                                                            &witness.witness().scenario,
                                                        ) {
                                                            Ok(same) => same,
                                                            Err(error) => {
                                                                unavailable = true;
                                                                report.unverified.push(format!("Retained reduced-frame equivalence is unverified: {error:?}"));
                                                                false
                                                            }
                                                        };
                                                        if same_frame {
                                                            matched.insert(index);
                                                            settled_pairs.insert(pair_key.clone());
                                                        } else if normalized
                                                            .witness()
                                                            .minimization
                                                            .as_ref()
                                                            .is_some_and(|m| !m.complete)
                                                        {
                                                            unavailable = true;
                                                            report.unverified.push("Retained-scene reduction is incomplete; equivalence is unverified".into());
                                                        }
                                                        report.normalizations.push(normalized);
                                                    } else if normalized.state
                                                        != EvidenceState::NoDifferenceFound
                                                    {
                                                        unavailable = true;
                                                        report
                                                            .unverified
                                                            .extend(normalized.diagnostics);
                                                    }
                                                }
                                            }
                                        } else {
                                            unavailable = true;
                                            report.unverified.push("Retained material observation correspondence is unresolved".into());
                                        }
                                    } else {
                                        unavailable = true;
                                    }
                                }
                            }
                            _ => unavailable = true,
                        }
                        for result in [a, b] {
                            match result {
                                Ok(run) => report.runs.push(run),
                                Err(error) => report
                                    .unverified
                                    .push(format!("Retained scene replay unavailable: {error:?}")),
                            }
                        }
                    }
                } else {
                    unavailable = true;
                }
            }
            if unavailable {
                report.unverified.extend(material_unknowns.into_values());
                entry.disposition = Disposition::Unverified;
                entry.explanation="Saved outcome equivalence is unverified; unavailable execution or correspondence is not evidence of a new choice".into();
                report.unverified.push(entry.explanation.clone());
                report.log.push(entry);
                continue;
            }
            let had_witnesses = !witnesses.is_empty();
            witnesses = witnesses
                .into_iter()
                .enumerate()
                .filter_map(|(i, w)| (!matched.contains(&i)).then_some(w))
                .collect();
            witnesses.extend(
                replacements
                    .into_iter()
                    .filter_map(|(key, w)| (!settled_pairs.contains(&key)).then_some(w)),
            );
            // Only complete independently matched profiles can settle these
            // channels; a supported witness on another pair/scene cannot.
            material_unknowns.retain(|key, _| !settled_pairs.contains(key));
            if had_witnesses && witnesses.is_empty() {
                report.unverified.extend(material_unknowns.into_values());
                if report.unverified.len() > unverified_before || exhausted {
                    entry.disposition = Disposition::Unverified;
                    entry.state = EvidenceState::Inconclusive;
                    entry.explanation="Verified contrasts reproduce saved outcomes; other alternatives remain unverified".into();
                } else {
                    entry.disposition = Disposition::Settled;
                    entry.explanation =
                        "Every proposed contrast reproduces saved outcomes; no repeated prompt"
                            .into();
                }
                report.log.push(entry);
                continue;
            }
        }
        report.unverified.extend(material_unknowns.into_values());
        if witnesses.is_empty() {
            if comparisons == comparisons_before_hypothesis
                && entry.disposition != Disposition::Settled
                && report.unverified.len() == unverified_before
                && !exhausted
            {
                report.unverified.push("Captured executions differ, but no supported target comparison established what changed".into());
            }
            if report.unverified.len() > unverified_before || exhausted {
                entry.disposition = Disposition::Unverified;
                entry.state = EvidenceState::Inconclusive;
                entry.explanation =
                    "No verified new choice; competing implementation checks remain incomplete"
                        .into();
            } else if entry.disposition != Disposition::Settled {
                entry.disposition = Disposition::NoWitness;
                if entry.explanation.is_empty() {
                    entry.explanation =
                        "No valid independently demonstrated contrast was found".into();
                }
            }
            report.log.push(entry);
            continue;
        }
        if hypothesis.kind != HypothesisKind::UnresolvedChoice {
            entry.disposition = if hypothesis.kind == HypothesisKind::PossibleDefect {
                Disposition::PossibleDefect
            } else {
                Disposition::RequestedChange
            };
            entry.explanation =
                "Observed proposed defect/requested change is not an unresolved preference".into();
            report.log.push(entry);
            continue;
        }
        let key = hypothesis_groups[&hypothesis.id].clone();
        let mut connected_contexts = BTreeSet::from([key.clone()]);
        let mut connected_scenes = witnesses
            .iter()
            .map(|w| verified_scene_key(w, &mut witness_scene_cache))
            .collect::<Result<BTreeSet<_>, _>>()?;
        let question_scenes = report
            .questions
            .iter()
            .map(|q| {
                q.witnesses
                    .iter()
                    .map(|w| verified_scene_key(w, &mut witness_scene_cache))
                    .collect::<Result<BTreeSet<_>, _>>()
            })
            .collect::<Result<Vec<_>, _>>()?;
        let question_contexts: Vec<BTreeSet<_>> = report
            .questions
            .iter()
            .map(|q| {
                q.hypotheses
                    .iter()
                    .map(|id| hypothesis_groups[id].clone())
                    .collect()
            })
            .collect();
        let mut matching = BTreeSet::new();
        // A new hypothesis can bridge several existing cards. Close the whole
        // connected component, rather than merging only its first match.
        loop {
            let previous = matching.len();
            for index in 0..report.questions.len() {
                if !matching.contains(&index)
                    && (!question_scenes[index].is_disjoint(&connected_scenes)
                        || !question_contexts[index].is_disjoint(&connected_contexts))
                {
                    matching.insert(index);
                    connected_scenes.extend(question_scenes[index].iter().cloned());
                    connected_contexts.extend(question_contexts[index].iter().cloned());
                }
            }
            if matching.len() == previous {
                break;
            }
        }
        if let Some(&first) = matching.iter().next() {
            let mut groups = vec![];
            let mut remaining = vec![];
            for (index, question) in report.questions.drain(..).enumerate() {
                if matching.contains(&index) {
                    groups.push(question);
                } else {
                    remaining.push(question);
                }
            }
            let mut question = groups.remove(0);
            let mut known = question_scenes[first].clone();
            for other in groups {
                question.hypotheses.extend(other.hypotheses);
                for witness in other.witnesses {
                    if known.insert(verified_scene_key(&witness, &mut witness_scene_cache)?) {
                        question.witnesses.push(witness);
                    }
                }
            }
            question.hypotheses.push(hypothesis.id.clone());
            question
                .hypotheses
                .sort_by_key(|id| hypothesis_positions[id]);
            question.hypotheses.dedup();
            question.unknowns.clear();
            for id in &question.hypotheses {
                for unknown in &result.response.hypotheses[hypothesis_positions[id]].unknowns {
                    if !question.unknowns.contains(unknown) {
                        question.unknowns.push(unknown.clone());
                    }
                }
            }
            for witness in witnesses {
                if known.insert(verified_scene_key(&witness, &mut witness_scene_cache)?) {
                    question.witnesses.push(witness);
                }
            }
            for prior_entry in &mut report.log {
                if prior_entry.disposition == Disposition::Question
                    && prior_entry.hypothesis != question.hypotheses[0]
                    && question.hypotheses.contains(&prior_entry.hypothesis)
                {
                    prior_entry.disposition = Disposition::Grouped;
                }
            }
            remaining.insert(first, question);
            report.questions = remaining;
            entry.disposition = Disposition::Grouped;
        } else {
            report.questions.push(ChoiceQuestion {
                id: format!("choice-{}", key.as_str()),
                action: hypothesis.action.clone(),
                observable: hypothesis.observable.clone(),
                statement: observed_statement(&witnesses[0]),
                hypotheses: vec![hypothesis.id.clone()],
                witnesses,
                producer: result.producer.clone(),
                unknowns: hypothesis.unknowns.clone(),
            });
            entry.disposition = Disposition::Question;
        }
        entry.state = if report.unverified.len() > unverified_before || exhausted {
            EvidenceState::Inconclusive
        } else {
            EvidenceState::Observed
        };
        entry.explanation =
            "New source-bound contrast independently executed and structurally reduced".into();
        if entry.state == EvidenceState::Inconclusive {
            entry
                .explanation
                .push_str("; remaining alternative checks or reduction coverage are unverified");
        }
        report.log.push(entry);
    }
    Ok(report)
}

/// Enumerate declared channels independently of whether one source changed them.
/// Alternative-only declarations still create coverage obligations.
fn comparison_targets(
    programs: &[&CapturedProgram],
    point: &str,
    policy: &DiscoveryPolicy,
) -> Result<Vec<ObservationTarget>, AdapterError> {
    let observables: BTreeSet<_> = programs
        .iter()
        .flat_map(|p| p.program.observables.iter().map(|o| o.id.clone()))
        .collect();
    let mut targets: Vec<_> = observables
        .into_iter()
        .map(|observable| ObservationTarget::Observable {
            point: point.into(),
            observable,
        })
        .collect();
    let mut outputs = BTreeMap::<Id, BTreeSet<Id>>::new();
    let mut columns = BTreeSet::new();
    for program in programs {
        let provenance = history::comparison_columns(policy, program)?;
        for output in &program.program.outputs {
            outputs.entry(output.id.clone()).or_default().extend(
                output
                    .columns
                    .iter()
                    .filter(|column| !provenance.output(&output.id, &column.id))
                    .map(|c| c.id.clone()),
            );
        }
        for view in &program.program.views {
            if let Some(schema) = program.program.view_schema(&view.id)? {
                columns.extend(
                    schema
                        .columns
                        .keys()
                        .filter(|column| !provenance.view(&view.id, column))
                        .cloned(),
                );
            }
        }
    }
    for (output, columns) in outputs {
        targets.push(ObservationTarget::OutputCount {
            point: point.into(),
            output: output.clone(),
        });
        for column in columns {
            targets.push(ObservationTarget::OutputColumn {
                point: point.into(),
                output: output.clone(),
                column,
            });
        }
    }
    targets.push(ObservationTarget::ViewRows {
        point: point.into(),
    });
    for column in columns {
        targets.push(ObservationTarget::ViewColumn {
            point: point.into(),
            column,
        });
    }
    Ok(targets)
}

fn verified_scene_key(
    witness: &VerifiedWitness,
    cache: &mut BTreeMap<Digest, Digest>,
) -> Result<(Digest, Digest, Digest), AdapterError> {
    let bound_scene = &witness.witness().before.binding.scenario_digest;
    let scene = if let Some(key) = cache.get(bound_scene) {
        key.clone()
    } else {
        let key = scene_equivalence_key(&witness.witness().scenario)?;
        cache.insert(bound_scene.clone(), key.clone());
        key
    };
    Ok((
        witness.before_program().binding.identity()?,
        witness.after_program().binding.identity()?,
        scene,
    ))
}

fn comparable_outcomes(actual: &RunEvidence, prior: &RunEvidence) -> bool {
    actual.state == EvidenceState::Observed
        && prior.state == EvidenceState::Observed
        && actual.binding.runtime_version == prior.binding.runtime_version
        && actual.binding.driver_version == prior.binding.driver_version
        && actual.binding.input_digest == prior.binding.input_digest
        && actual.observations.len() == prior.observations.len()
        && actual
            .observations
            .iter()
            .zip(&prior.observations)
            .all(|(a, b)| {
                if a.point != b.point
                    || a.view_schema != b.view_schema
                    || a.value_types != b.value_types
                {
                    return false;
                }
                let mut targets: Vec<_> = a
                    .value_types
                    .keys()
                    .map(|observable| ObservationTarget::Observable {
                        point: a.point.clone(),
                        observable: observable.clone(),
                    })
                    .collect();
                let mut outputs = BTreeMap::<Id, BTreeSet<Id>>::new();
                for artifact in a.outputs.iter().chain(&b.outputs) {
                    outputs
                        .entry(artifact.output.clone())
                        .or_default()
                        .extend(artifact.columns.iter().map(|column| column.id.clone()));
                }
                for (output, columns) in outputs {
                    targets.push(ObservationTarget::OutputCount {
                        point: a.point.clone(),
                        output: output.clone(),
                    });
                    for column in columns {
                        targets.push(ObservationTarget::OutputColumn {
                            point: a.point.clone(),
                            output: output.clone(),
                            column,
                        });
                    }
                }
                if let Some(schema) = &a.view_schema {
                    targets.push(ObservationTarget::ViewRows {
                        point: a.point.clone(),
                    });
                    for column in schema.columns.keys() {
                        targets.push(ObservationTarget::ViewColumn {
                            point: a.point.clone(),
                            column: column.clone(),
                        });
                    }
                }
                targets.iter().all(|target| {
                    target
                        .differs(std::slice::from_ref(a), std::slice::from_ref(b))
                        .is_some()
                })
            })
}
fn same_outcomes(actual: &RunEvidence, prior: &RunEvidence) -> bool {
    actual.state == EvidenceState::Observed
        && prior.state == EvidenceState::Observed
        && actual.binding.input_digest == prior.binding.input_digest
        && actual.observations.len() == prior.observations.len()
        && actual
            .observations
            .iter()
            .zip(&prior.observations)
            .all(|(a, b)| {
                a.point == b.point
                    && a.values == b.values
                    && a.value_types == b.value_types
                    && a.view == b.view
                    && a.view_schema == b.view_schema
                    && a.outputs == b.outputs
            })
}

fn observed_statement(witness: &VerifiedWitness) -> String {
    let property = &witness.witness().distinguishing_properties[0];
    let PropertyPredicate::Equal { left, .. } = &property.predicate else {
        return "The independently observed outcomes differ".into();
    };
    match left {
        PropertyTerm::Observed { observable, .. } => {
            let label = witness
                .after_program()
                .program
                .observables
                .iter()
                .find(|o| &o.id == observable)
                .map(|o| o.label.as_str())
                .unwrap_or(observable);
            format!("The observed {label} values differ")
        }
        PropertyTerm::OutputCount { output, .. } => {
            format!("The local output row count differs for {output}")
        }
        PropertyTerm::OutputColumn { output, column, .. } => {
            format!("Local output values differ in {output}, column {column}")
        }
        PropertyTerm::ViewRows { .. } => "The displayed records or their order differ".into(),
        PropertyTerm::ViewColumn { column, .. } => {
            format!("Displayed values differ in column {column}")
        }
        _ => "The independently observed outcomes differ".into(),
    }
}

fn witness_target(witness: &DifferentialWitness) -> Option<ObservationTarget> {
    witness.distinguishing_properties.iter().find_map(|p| {
        let PropertyPredicate::Equal { left, .. } = &p.predicate else {
            return None;
        };
        Some(match left {
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
            _ => return None,
        })
    })
}
type SampledOutcome = Option<(PropertyPredicate, Type, DataValue)>;
fn same_pair(a: &(SampledOutcome, SampledOutcome), b: &(SampledOutcome, SampledOutcome)) -> bool {
    let eq = |a: &SampledOutcome, b: &SampledOutcome| matches!((a,b),(Some((_,at,av)),Some((_,bt,bv))) if at==bt&&av==bv);
    (eq(&a.0, &b.0) && eq(&a.1, &b.1)) || (eq(&a.0, &b.1) && eq(&a.1, &b.0))
}
/// Point names locate observations only inside their own run. Cross-run
/// correspondence comes from actual trace positions or unique execution context.
fn retained_target(
    target: &ObservationTarget,
    current: &RunEvidence,
    prior: &RunEvidence,
) -> Option<ObservationTarget> {
    let point = if current.trace.len() == prior.trace.len()
        && current.trace.iter().zip(&prior.trace).all(|(a, b)| {
            matches!(
                (&a.input, &b.input),
                (SemanticInput::Observe { .. }, SemanticInput::Observe { .. })
            ) || a.input == b.input
        }) {
        // Complete independently executed semantic traces differ only in
        // observation annotations, so their ordered points correspond exactly.
        let index = current
            .observations
            .iter()
            .position(|o| o.point == target.point())?;
        &prior.observations.get(index)?.point
    } else {
        if !current
            .trace
            .iter()
            .filter(|step| action_bearing(&step.input))
            .map(|step| &step.input)
            .eq(prior
                .trace
                .iter()
                .filter(|step| action_bearing(&step.input))
                .map(|step| &step.input))
        {
            return None;
        }
        let current_groups = observation_groups(current);
        let prior_groups = observation_groups(prior);
        let current = current_groups
            .iter()
            .find(|group| group.points.contains(&target.point()))?;
        let mut candidates = prior_groups
            .iter()
            .filter(|group| group.mutation == current.mutation && group.context == current.context);
        let group = candidates.next()?;
        if candidates.next().is_some() {
            return None;
        }
        *group.points.first()?
    };
    let mut target = target.clone();
    match &mut target {
        ObservationTarget::Observable { point: p, .. }
        | ObservationTarget::OutputCount { point: p, .. }
        | ObservationTarget::OutputColumn { point: p, .. }
        | ObservationTarget::ViewRows { point: p }
        | ObservationTarget::ViewColumn { point: p, .. } => *p = point.into(),
        ObservationTarget::Property { .. } => return None,
    };
    Some(target)
}
/// History lookup ignores Observe instrumentation but preserves every effective
/// input and initial condition. A key match is not execution/settlement proof.
fn history_execution_key(scene: &ScenarioSpec) -> Result<Digest, AdapterError> {
    let inputs: Vec<_> = scene
        .inputs
        .iter()
        .filter(|input| !matches!(input, SemanticInput::Observe { .. }))
        .collect();
    Ok(canonical_digest(
        IdentityDomain::Input,
        &(
            &scene.seed,
            &scene.session,
            scene.clock_day,
            scene.random_seed,
            inputs,
        ),
    )?)
}

/// This key is only for independently replayed/reduced scene equivalence, never
/// an execution binding. Observation point names are annotations, not effects.
fn scene_equivalence_key(scene: &ScenarioSpec) -> Result<Digest, AdapterError> {
    let mut inputs = scene.inputs.clone();
    let mut index = 0;
    for input in &mut inputs {
        if let SemanticInput::Observe { point } = input {
            *point = format!("observation-{index}");
            index += 1;
        }
    }
    Ok(canonical_digest(
        IdentityDomain::Input,
        &(
            &scene.seed,
            &scene.session,
            scene.clock_day,
            scene.random_seed,
            inputs,
        ),
    )?)
}

/// Only adjacent successful Observe trace entries with identical material state
/// can alias. A clock advance or any action terminates the group, even if its
/// record/session hashes happen not to change.
fn action_bearing(input: &SemanticInput) -> bool {
    matches!(
        input,
        SemanticInput::Invoke { .. }
            | SemanticInput::Control { .. }
            | SemanticInput::Activate { .. }
            | SemanticInput::Submit { .. }
    )
}
struct ObservationGroup<'a> {
    mutation: usize,
    context: Vec<&'a SemanticInput>,
    points: Vec<&'a str>,
}
fn observation_groups(run: &RunEvidence) -> Vec<ObservationGroup<'_>> {
    let mut groups: Vec<ObservationGroup<'_>> = vec![];
    let mut previous: Option<(&TraceStep, &Observation)> = None;
    let mut mutation = 0usize;
    let mut context = vec![];
    for step in &run.trace {
        let SemanticInput::Observe { point } = &step.input else {
            if action_bearing(&step.input) {
                mutation += 1;
                context.clear();
            } else {
                context.push(&step.input);
            }
            previous = None;
            continue;
        };
        let Some(observation) = run.observations.iter().find(|o| &o.point == point) else {
            previous = None;
            continue;
        };
        let duplicate = previous.is_some_and(|(prior, old)| {
            prior.outcome == StepOutcome::Applied
                && step.outcome == StepOutcome::Applied
                && prior.after_data == step.before_data
                && step.before_data == step.after_data
                && prior.after_session == step.before_session
                && step.before_session == step.after_session
                && old.values == observation.values
                && old.value_types == observation.value_types
                && old.view == observation.view
                && old.view_schema == observation.view_schema
                && old.outputs == observation.outputs
        });
        if duplicate {
            groups.last_mut().unwrap().points.push(&observation.point);
        } else {
            groups.push(ObservationGroup {
                mutation,
                context: context.clone(),
                points: vec![&observation.point],
            });
        }
        previous = Some((step, observation));
    }
    groups
}

/// Material channels without a property term still constrain settlement. They
/// may establish an unknown boundary, never a fabricated executable predicate.
fn unrepresented_material(observation: &Observation) -> Option<BTreeMap<String, Digest>> {
    fn add(
        values: &mut BTreeMap<String, Digest>,
        key: String,
        value: &impl serde::Serialize,
    ) -> Option<()> {
        values.insert(
            key,
            canonical_digest(IdentityDomain::Observation, value).ok()?,
        );
        Some(())
    }
    let mut values = BTreeMap::new();
    add(&mut values, "view".into(), &observation.view.view)?;
    add(&mut values, "selection".into(), &observation.view.selected)?;
    add(
        &mut values,
        "actions".into(),
        &observation.view.enabled_actions,
    )?;
    for (id, value) in &observation.view.controls {
        add(&mut values, format!("control/{id}"), value)?;
    }
    for (id, value) in &observation.view.form_values {
        add(&mut values, format!("form/{id}"), value)?;
    }
    let mut row_actions: Vec<_> = observation
        .view
        .rows
        .iter()
        .filter(|row| !row.enabled_actions.is_empty())
        .map(|row| (&row.record.entity, &row.record.record, &row.enabled_actions))
        .collect();
    row_actions.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    add(&mut values, "row_actions".into(), &row_actions)?;
    for (index, artifact) in observation.outputs.iter().enumerate() {
        add(&mut values, format!("output/{index}/id"), &artifact.output)?;
        add(
            &mut values,
            format!("output/{index}/format"),
            &artifact.format,
        )?;
        add(
            &mut values,
            format!("output/{index}/columns"),
            &artifact.columns,
        )?;
        add(
            &mut values,
            format!("output/{index}/rows"),
            &artifact.rows.len(),
        )?;
    }
    Some(values)
}

/// A supported scalar contrast does not cover simultaneous controls, actions,
/// selections or output layout changes. Retain that boundary even without saved
/// decisions and before reductions can erase the original extra consequence.
fn unrepresented_differences(before: &[Observation], after: &[Observation]) -> Vec<String> {
    fn channels(observation: &Observation) -> Option<BTreeMap<String, Digest>> {
        let mut channels = unrepresented_material(observation)?;
        let mut counts = BTreeMap::<&Id, usize>::new();
        for artifact in &observation.outputs {
            *counts.entry(&artifact.output).or_default() += 1;
        }
        for (index, artifact) in observation.outputs.iter().enumerate() {
            // For one artifact, OutputCount represents its row count exactly.
            // Multiple artifact partitions/order are not represented by totals.
            if counts[&artifact.output] == 1 {
                channels.remove(&format!("output/{index}/rows"));
            }
        }
        Some(channels)
    }
    let mut differences = BTreeSet::new();
    if before.len() != after.len() {
        differences.insert("observation correspondence".into());
    }
    for (a, b) in before.iter().zip(after) {
        if a.point != b.point {
            differences.insert("observation correspondence".into());
        }
        match (channels(a), channels(b)) {
            (Some(left), Some(right)) => {
                for key in left.keys().chain(right.keys()) {
                    if left.get(key) != right.get(key) {
                        differences.insert(format!("{}/{key}", a.point));
                    }
                }
            }
            _ => {
                differences.insert("material channel extraction unavailable".into());
            }
        }
    }
    differences.into_iter().collect()
}

struct RetainedProfile {
    matches: bool,
    novel: Option<AcceptedProperty>,
}
/// Compare complete ordered material vectors, with one consistent retained side
/// for the whole trace. Coordinate-wise mixing cannot count as a saved outcome.
fn retained_profile(
    before: &RunEvidence,
    _after: &RunEvidence,
    expected_before: &RetainedRun,
    expected_after: &RetainedRun,
    compared_before: &[Observation],
    compared_after: &[Observation],
) -> Option<RetainedProfile> {
    // Include all typed channels, including ones absent from the first side.
    let observations: Vec<_> = compared_before.iter().chain(compared_after).collect();
    let mut observables: BTreeSet<_> = observations
        .iter()
        .flat_map(|o| o.value_types.keys().cloned())
        .collect();
    let mut outputs: BTreeSet<_> = observations
        .iter()
        .flat_map(|o| o.outputs.iter().map(|a| a.output.clone()))
        .collect();
    let mut output_columns: BTreeSet<_> = observations
        .iter()
        .flat_map(|o| o.outputs.iter())
        .flat_map(|a| a.columns.iter().map(|c| (a.output.clone(), c.id.clone())))
        .collect();
    let mut view_columns: BTreeSet<_> = observations
        .iter()
        .flat_map(|o| o.view_schema.iter().flat_map(|s| s.columns.keys().cloned()))
        .collect();
    for historical in [expected_before, expected_after] {
        for observation in &historical.comparison {
            for id in observation.value_types.keys() {
                observables.insert(historical.forward(SemanticKind::Observable, id));
            }
            for output in &observation.outputs {
                let id = historical.forward(SemanticKind::Output, &output.output);
                outputs.insert(id.clone());
                output_columns.extend(
                    output
                        .columns
                        .iter()
                        .map(|column| (id.clone(), column.id.clone())),
                );
            }
            view_columns.extend(
                observation
                    .view_schema
                    .iter()
                    .flat_map(|schema| schema.columns.keys().cloned()),
            );
        }
    }
    let mut coordinates = vec![];
    let mut opaque_coordinates = vec![];
    for observation in compared_before {
        let point = &observation.point;
        let current_channels = [
            unrepresented_material(observation)?,
            unrepresented_material(compared_after.iter().find(|o| &o.point == point)?)?,
        ];
        let point_target = ObservationTarget::ViewRows {
            point: point.clone(),
        };
        if let (Some(old_before), Some(old_after)) = (
            expected_before.target(&point_target, before),
            expected_after.target(&point_target, before),
        ) {
            let old_channels = [
                expected_before.material(
                    expected_before
                        .comparison
                        .iter()
                        .find(|o| o.point == old_before.point())?,
                )?,
                expected_after.material(
                    expected_after
                        .comparison
                        .iter()
                        .find(|o| o.point == old_after.point())?,
                )?,
            ];
            let channels = [
                &current_channels[0],
                &current_channels[1],
                &old_channels[0],
                &old_channels[1],
            ];
            let keys: BTreeSet<_> = channels.iter().flat_map(|m| m.keys()).collect();
            for key in keys {
                let values = channels.map(|m| m.get(key).cloned());
                // A common context change on both sides is not a new contrast.
                if values[0] != values[1] || values[2] != values[3] {
                    opaque_coordinates.push(values);
                }
            }
        } else if current_channels[0] != current_channels[1] {
            return None;
        }

        let mut targets: Vec<_> = observables
            .iter()
            .map(|observable| ObservationTarget::Observable {
                point: point.clone(),
                observable: observable.clone(),
            })
            .collect();
        targets.push(ObservationTarget::ViewRows {
            point: point.clone(),
        });
        targets.extend(outputs.iter().map(|output| ObservationTarget::OutputCount {
            point: point.clone(),
            output: output.clone(),
        }));
        targets.extend(output_columns.iter().map(|(output, column)| {
            ObservationTarget::OutputColumn {
                point: point.clone(),
                output: output.clone(),
                column: column.clone(),
            }
        }));
        targets.extend(
            view_columns
                .iter()
                .map(|column| ObservationTarget::ViewColumn {
                    point: point.clone(),
                    column: column.clone(),
                }),
        );
        for target in targets {
            let current_before = target.sample_term(compared_before);
            let current_after = target.sample_term(compared_after);
            let (old_before, old_after) = match (
                expected_before.target(&target, before),
                expected_after.target(&target, before),
            ) {
                (Some(a), Some(b)) => (a, b),
                _ if matches!((&current_before,&current_after),(Some((_,a,x)),Some((_,b,y))) if a==b&&x==y)
                    || (current_before.is_none() && current_after.is_none()) =>
                {
                    continue
                }
                _ => return None,
            };
            let values = [
                current_before,
                current_after,
                old_before.sample_term(&expected_before.comparison),
                old_after.sample_term(&expected_after.comparison),
            ];
            if values.iter().all(Option::is_none) {
                continue;
            }
            let values = values.into_iter().collect::<Option<Vec<_>>>()?;
            if values.iter().any(|value| value.1 != values[0].1) {
                return None;
            }
            // Common context cannot distinguish either pair of implementations.
            if values[0].2 == values[1].2 && values[2].2 == values[3].2 {
                continue;
            }
            coordinates.push(values);
        }
    }
    if coordinates.is_empty() {
        return None;
    }
    let equal = |a: usize, b: usize| {
        coordinates
            .iter()
            .all(|c| c[a].1 == c[b].1 && c[a].2 == c[b].2)
            && opaque_coordinates.iter().all(|c| c[a] == c[b])
    };
    let matches = (equal(0, 2) && equal(1, 3)) || (equal(0, 3) && equal(1, 2));
    let mut novel = None;
    for side in [0, 1] {
        if equal(side, 2) || equal(side, 3) || equal(side, 1 - side) {
            continue;
        }
        // Smallest conjunction separating this actual vector from both saved
        // vectors and the other implementation. Three excluded vectors require
        // at most three coordinates; this search has only eight mask states.
        let mut best: Vec<Option<Vec<usize>>> = vec![None; 8];
        best[0] = Some(vec![]);
        for (index, c) in coordinates.iter().enumerate() {
            let mut mask = 0;
            for (bit, other) in [2, 3, 1 - side].into_iter().enumerate() {
                if c[side].2 != c[other].2 {
                    mask |= 1 << bit;
                }
            }
            let prior_best = best.clone();
            for (old, path) in prior_best.into_iter().enumerate() {
                if let Some(mut path) = path {
                    path.push(index);
                    let next = old | mask;
                    if best[next]
                        .as_ref()
                        .is_none_or(|existing| path.len() < existing.len())
                    {
                        best[next] = Some(path);
                    }
                }
            }
        }
        let indices = best[7].take()?;
        let values = indices
            .into_iter()
            .map(|index| {
                let (term, typ, value) = &coordinates[index][side];
                PropertyPredicate::Equal {
                    left: term.clone(),
                    right: PropertyTerm::Literal {
                        value_type: typ.clone(),
                        value: value.clone(),
                    },
                }
            })
            .collect();
        novel = Some(AcceptedProperty {
            id: "new-outcome-combination".into(),
            description: "The observed combination differs from both saved outcomes".into(),
            predicate: PropertyPredicate::And { values },
        });
        break;
    }
    Some(RetainedProfile { matches, novel })
}
