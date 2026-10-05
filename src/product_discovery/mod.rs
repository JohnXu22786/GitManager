//! Source-bound hypotheses become questions only after independent execution.
//! This module cannot adopt programs or mutate a daily-work store.
mod provider;
mod source;
use crate::product_contract::*;
use crate::product_runtime::LocalRuntime;
use crate::product_scenarios::*;
pub use provider::*;
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
    /// Previously accepted witnesses, addressed by the saved decision digest.
    pub retained_witnesses: Vec<DifferentialWitness>,
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
            retained_witnesses: vec![],
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
    s.inputs
        .iter()
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
struct ReplayBudget {
    remaining: Cell<usize>,
    cancelled: Arc<AtomicBool>,
}
impl ReplayBudget {
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
            &requirement.scenario,
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
) -> Result<(RunEvidence, CheckState, Digest), AdapterError> {
    let expected = chosen_evidence(decision, policy)?;
    if scene.input_identity()? != expected.binding.input_digest {
        return Err(AdapterError::Unsupported(format!(
            "Decision {} has no chosen concrete outcome for this additional scene",
            decision.id
        )));
    }
    let run = budget.replay(
        runtime,
        program,
        scene,
        &request.decisions,
        policy.search.runtime.clone(),
        id,
    )?;
    let comparable = run.binding.runtime_version == expected.binding.runtime_version
        && run.binding.driver_version == expected.binding.driver_version
        && run
            .observations
            .iter()
            .zip(&expected.observations)
            .all(|(a, b)| a.view_schema == b.view_schema);
    let state = if run.state != EvidenceState::Observed {
        CheckState::Failed
    } else if !comparable {
        CheckState::Unknown
    } else if same_outcomes(&run, expected) {
        CheckState::Satisfied
    } else {
        CheckState::Violated
    };
    let obligation = canonical_digest(
        IdentityDomain::Decision,
        &(&decision.witness, &expected.binding.artifact.program_digest),
    )?;
    Ok((run, state, obligation))
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
    let delta = analyze_delta(before, candidate)?;
    let mut report = DiscoveryReport {
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
    let runtime = LocalRuntime::with_cancellation(cancelled.clone());
    let replay_budget = ReplayBudget {
        remaining: Cell::new(policy.max_precheck_replays),
        cancelled: cancelled.clone(),
    };
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
    let mut blocked = BTreeSet::new();
    let mut pending = BTreeMap::<Id, Vec<&ScopedDecision>>::new();
    'decision_checks: for decision in request
        .decisions
        .decisions
        .iter()
        .filter(|d| matches!(d.status, DecisionStatus::Active | DecisionStatus::Pending))
    {
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
            if decision.obligations.is_empty() && is_chosen(decision) {
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
                    ) {
                        Ok((run, state, property_digest)) => {
                            if state == CheckState::Violated {
                                report.defects.push(format!(
                                    "Active decision {} violates its chosen concrete outcome",
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
                                explanation: "Independently replayed the accepted concrete outcome"
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
                    if &h.action == action {
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
                    &scene.scenario,
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
        let captured = if let Some(source) = request
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
    let mut comparisons = 0usize;
    for hypothesis in &result.response.hypotheses {
        let unverified_before = report.unverified.len();
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
        if !source::relevant(&report.delta, before, candidate, hypothesis)? {
            entry.disposition = Disposition::Irrelevant;
            entry.explanation =
                "Hypothesis has no changed reachable source locus for this action/observation"
                    .into();
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
        let mut scene = hypothesis.scenario()?;
        let mut hypothesis_search = policy.search.clone();
        hypothesis_search.retain_initial_outcomes |= pending.contains_key(&hypothesis.id);
        scene.validity = policy
            .workflow_validity
            .get(&hypothesis.action)
            .cloned()
            .unwrap_or_default();
        if !scenario_operations(&scene, &candidate.program).contains(&hypothesis.action) {
            entry.disposition = Disposition::Irrelevant;
            entry.explanation = "Scenario never performs the hypothesized action".into();
            report.log.push(entry);
            continue;
        }
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
            "captured-baseline",
        );
        let candidate_run = replay_budget.replay(
            &runtime,
            candidate,
            &scene,
            &request.decisions,
            policy.search.runtime.clone(),
            "captured-candidate",
        );
        let anchored = matches!((&baseline_run,&candidate_run),(Ok(a),Ok(b)) if a.state==EvidenceState::Observed&&b.state==EvidenceState::Observed&&!same_outcomes(a,b));
        let unchanged =
            matches!((&baseline_run,&candidate_run),(Ok(a),Ok(b)) if same_outcomes(a,b));
        let current_ran = matches!(&candidate_run,Ok(b) if b.state==EvidenceState::Observed);
        let captured_observations = match (&baseline_run, &candidate_run) {
            (Ok(a), Ok(b))
                if a.state == EvidenceState::Observed && b.state == EvidenceState::Observed =>
            {
                Some((a.observations.clone(), b.observations.clone()))
            }
            _ => None,
        };
        for result in [baseline_run, candidate_run] {
            match result {
                Ok(run) => report.runs.push(run),
                Err(e) => entry.explanation = format!("Captured-source replay unavailable: {e:?}"),
            }
        }
        if unchanged {
            entry.disposition = Disposition::NoWitness;
            entry.state = EvidenceState::NoDifferenceFound;
            entry.explanation="Actual captured edit has identical outcomes in this scenario; generated alternatives cannot invent a choice".into();
            report.log.push(entry);
            continue;
        }
        if !anchored && !(new_feature && current_ran) {
            entry.disposition = Disposition::Unverified;
            entry.explanation="Actual edited behavior could not be linked to a valid source comparison or independently required new feature".into();
            report.unverified.push(entry.explanation.clone());
            report.log.push(entry);
            continue;
        }
        let points: Vec<_> = scene
            .inputs
            .iter()
            .filter_map(|i| match i {
                SemanticInput::Observe { point } => Some(point.clone()),
                _ => None,
            })
            .collect();
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
                d.status == DecisionStatus::Active && (!d.obligations.is_empty() || is_chosen(d))
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
                        if decision.obligations.is_empty() && is_chosen(decision) {
                            match check_chosen(
                                alternative,
                                &example.scenario,
                                decision,
                                request,
                                policy,
                                &runtime,
                                &replay_budget,
                                "alternative-chosen-outcome",
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
                            &example.scenario,
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
            'points: for point in &points {
                let mut targets: Vec<_> = candidate
                    .program
                    .observables
                    .iter()
                    .map(|observable| ObservationTarget::Observable {
                        point: point.clone(),
                        observable: observable.id.clone(),
                    })
                    .collect();
                for output in &candidate.program.outputs {
                    targets.push(ObservationTarget::OutputCount {
                        point: point.clone(),
                        output: output.id.clone(),
                    });
                    for column in &output.columns {
                        targets.push(ObservationTarget::OutputColumn {
                            point: point.clone(),
                            output: output.id.clone(),
                            column: column.id.clone(),
                        });
                    }
                }
                targets.push(ObservationTarget::ViewRows {
                    point: point.clone(),
                });
                let mut columns = BTreeSet::new();
                for view in &candidate.program.views {
                    if let Some(schema) = candidate.program.view_schema(&view.id)? {
                        columns.extend(schema.columns.keys().cloned());
                    }
                }
                for column in columns {
                    targets.push(ObservationTarget::ViewColumn {
                        point: point.clone(),
                        column,
                    });
                }
                for target in targets {
                    if !new_feature
                        && !captured_observations
                            .as_ref()
                            .is_some_and(|(a, b)| target.differs(a, b) == Some(true))
                    {
                        continue;
                    }
                    if comparisons >= policy.max_comparisons {
                        exhausted = true;
                        break 'points;
                    }
                    comparisons += 1;
                    let comparison = engine.minimize(
                        alternative,
                        candidate,
                        &scene,
                        &request.decisions,
                        target,
                        hypothesis_search.clone(),
                    )?;
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
                if let Some(prior) = policy
                    .retained_witnesses
                    .iter()
                    .find(|w| w.identity().ok().as_ref() == Some(&decision.witness))
                {
                    for (index, witness) in witnesses.iter().enumerate() {
                        let a = replay_budget.replay(
                            &runtime,
                            witness.before_program(),
                            &prior.scenario,
                            &request.decisions,
                            policy.search.runtime.clone(),
                            "retained-before",
                        );
                        let b = replay_budget.replay(
                            &runtime,
                            witness.after_program(),
                            &prior.scenario,
                            &request.decisions,
                            policy.search.runtime.clone(),
                            "retained-after",
                        );
                        match (&a, &b) {
                            (Ok(a), Ok(b))
                                if comparable_outcomes(a, &prior.before)
                                    && comparable_outcomes(b, &prior.after) =>
                            {
                                let pair_key = (
                                    witness.before_program().binding.identity()?,
                                    witness.after_program().binding.identity()?,
                                    scene_equivalence_key(witness.initial_scenario())?,
                                );
                                let retained_key = (
                                    pair_key.0.clone(),
                                    pair_key.1.clone(),
                                    scene_equivalence_key(&prior.scenario)?,
                                );
                                let (original_before, original_after) = witness.initial_runs();
                                let profile = retained_profile(
                                    original_before,
                                    original_after,
                                    &prior.before,
                                    &prior.after,
                                );
                                // The provider's observation subset cannot hide a changed
                                // point already observed by our complete retained replay.
                                let replay_profile =
                                    retained_profile(a, b, &prior.before, &prior.after);
                                if profile.is_none() || replay_profile.is_none() {
                                    unavailable = true;
                                    report.unverified.push("Retained material observation correspondence is unresolved".into());
                                }
                                if replay_profile.as_ref().is_some_and(|p| p.matches) {
                                    settled_pairs.insert(retained_key.clone());
                                }
                                for (key, initial_scene, profile) in [
                                    (&pair_key, witness.initial_scenario(), profile.as_ref()),
                                    (&retained_key, &prior.scenario, replay_profile.as_ref()),
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
                                        if let Some(prior_target) = retained_target(
                                            &current_target,
                                            original_before,
                                            &prior.before,
                                        ) {
                                            let current_pair = (
                                                current_target
                                                    .sample(&original_before.observations),
                                                current_target.sample(&original_after.observations),
                                            );
                                            let expected_pair = (
                                                prior_target.sample(&prior.before.observations),
                                                prior_target.sample(&prior.after.observations),
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
                                                ) && prior
                                                    .before
                                                    .observations
                                                    .iter()
                                                    .chain(&prior.after.observations)
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
                                                let direct = scene_equivalence_key(&scene)?
                                                    == scene_equivalence_key(&prior.scenario)?
                                                    || scene_equivalence_key(
                                                        &witness.witness().scenario,
                                                    )? == scene_equivalence_key(
                                                        &prior.scenario,
                                                    )?;
                                                if direct {
                                                    matched.insert(index);
                                                    settled_pairs.insert(pair_key.clone());
                                                } else if comparisons >= policy.max_comparisons {
                                                    unavailable = true;
                                                    report.unverified.push("Retained-scene comparison budget exhausted; equivalence is unverified".into());
                                                } else {
                                                    comparisons += 1;
                                                    let normalized = engine.minimize(
                                                        witness.before_program(),
                                                        witness.after_program(),
                                                        &prior.scenario,
                                                        &request.decisions,
                                                        prior_target,
                                                        hypothesis_search.clone(),
                                                    )?;
                                                    report.runs.extend(normalized.runs);
                                                    if let Some(normalized) = normalized.witness {
                                                        if scene_equivalence_key(
                                                            &normalized.witness().scenario,
                                                        )? == scene_equivalence_key(
                                                            &witness.witness().scenario,
                                                        )? {
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
            if had_witnesses && witnesses.is_empty() {
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
        if witnesses.is_empty() {
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
        let key = canonical_digest(
            IdentityDomain::Decision,
            &(&hypothesis.action, &hypothesis.observable),
        )?;
        if let Some(question) = report.questions.iter_mut().find(|q| {
            q.id == format!("choice-{}", key.as_str())
                || (q.action == hypothesis.action
                    && q.witnesses
                        .iter()
                        .any(|known| witnesses.iter().any(|new| same_executed_scene(known, new))))
        }) {
            question.hypotheses.push(hypothesis.id.clone());
            for unknown in &hypothesis.unknowns {
                if !question.unknowns.contains(unknown) {
                    question.unknowns.push(unknown.clone());
                }
            }
            for witness in witnesses {
                if !question
                    .witnesses
                    .iter()
                    .any(|w| same_executed_scene(w, &witness))
                {
                    question.witnesses.push(witness);
                }
            }
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

fn same_executed_scene(a: &VerifiedWitness, b: &VerifiedWitness) -> bool {
    a.before_program() == b.before_program()
        && a.after_program() == b.after_program()
        && matches!((scene_equivalence_key(&a.witness().scenario),scene_equivalence_key(&b.witness().scenario)),(Ok(a),Ok(b)) if a==b)
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
            .all(|(a, b)| a.view_schema == b.view_schema)
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

struct RetainedProfile {
    matches: bool,
    novel: Option<AcceptedProperty>,
}
/// Compare complete ordered material vectors, with one consistent retained side
/// for the whole trace. Coordinate-wise mixing cannot count as a saved outcome.
fn retained_profile(
    before: &RunEvidence,
    after: &RunEvidence,
    expected_before: &RunEvidence,
    expected_after: &RunEvidence,
) -> Option<RetainedProfile> {
    // Include all typed channels, including ones absent from the first side.
    let observations: Vec<_> = [before, after, expected_before, expected_after]
        .into_iter()
        .flat_map(|run| run.observations.iter())
        .collect();
    let observables: BTreeSet<_> = observations
        .iter()
        .flat_map(|o| o.value_types.keys().cloned())
        .collect();
    let outputs: BTreeSet<_> = observations
        .iter()
        .flat_map(|o| o.outputs.iter().map(|a| a.output.clone()))
        .collect();
    let output_columns: BTreeSet<_> = observations
        .iter()
        .flat_map(|o| o.outputs.iter())
        .flat_map(|a| a.columns.iter().map(|c| (a.output.clone(), c.id.clone())))
        .collect();
    let view_columns: BTreeSet<_> = observations
        .iter()
        .flat_map(|o| o.view_schema.iter().flat_map(|s| s.columns.keys().cloned()))
        .collect();
    let mut coordinates = vec![];
    for observation in &before.observations {
        let point = &observation.point;
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
            let current_before = target.sample_term(&before.observations);
            let current_after = target.sample_term(&after.observations);
            let old = match retained_target(&target, before, expected_before) {
                Some(old) => old,
                None if matches!((&current_before,&current_after),(Some((_,a,x)),Some((_,b,y))) if a==b&&x==y)
                    || (current_before.is_none() && current_after.is_none()) =>
                {
                    continue
                }
                None => return None,
            };
            let values = [
                current_before,
                current_after,
                old.sample_term(&expected_before.observations),
                old.sample_term(&expected_after.observations),
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
